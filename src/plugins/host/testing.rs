// ─────────────────────────────────────────────────────────────────────────────
// plugins/host/testing.rs - Shared test helpers (cfg(test) only)
// ─────────────────────────────────────────────────────────────────────────────

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::Write;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use super::cursor::CursorStore;
use super::engine::{PollError, PollResponse, WitItem};
use super::instance::{CallOutcome, InstanceBuildError, InstanceFactory, PluginInstance};
use super::scheduler::{PluginReport, PluginSpec, run_plugin};
use super::sink::{ItemSink, SinkError, SinkRecord, VecSink};
use crate::plugins::config::{
    BackoffConfig, HostConfig, LoadedRegistry, load_registry_with_env,
};
use crate::plugins::secrets::ResolvedSecrets;

/// The repository root.
pub fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// A prebuilt fixture component, or a panic naming how to build it.
pub fn fixture_wasm(crate_dir: &str, file: &str) -> PathBuf {
    let path = repo_root()
        .join("plugins")
        .join(crate_dir)
        .join("target/wasm32-unknown-unknown/release")
        .join(file);
    assert!(
        path.is_file(),
        "needs plugin fixtures: see plugins/README.md ({} is missing)",
        path.display()
    );
    path
}

/// `(component (import "wasi:cli/environment@0.2.0" (instance)))`, built
/// with `wasm-tools parse`.
pub(crate) const WASI_IMPORT_COMPONENT: &[u8] = &[
    0x00, 0x61, 0x73, 0x6d, 0x0d, 0x00, 0x01, 0x00, 0x07, 0x03, 0x01, 0x42, 0x00, 0x0a, 0x1f, 0x01,
    0x00, 0x1a, 0x77, 0x61, 0x73, 0x69, 0x3a, 0x63, 0x6c, 0x69, 0x2f, 0x65, 0x6e, 0x76, 0x69, 0x72,
    0x6f, 0x6e, 0x6d, 0x65, 0x6e, 0x74, 0x40, 0x30, 0x2e, 0x32, 0x2e, 0x30, 0x05, 0x00,
];

/// One source for `write_registry`.
pub struct TestSource {
    pub name: String,
    pub component: PathBuf,
    pub enabled: bool,
    pub interval_secs: u64,
    /// `(initial_secs, max_secs, multiplier)`.
    pub backoff: Option<(u64, u64, f64)>,
    /// Text placed under `[settings]`.
    pub settings: String,
}

/// Writes `filimon.toml` and one `<name>.toml` per source into `dir`.
pub fn write_registry(dir: &Path, sources: &[TestSource]) -> PathBuf {
    let mut registry = String::new();
    for source in sources {
        registry.push_str(&format!(
            "[sources.{name}]\ncomponent = {component:?}\nconfig = \"{name}.toml\"\nenabled = {enabled}\n\n",
            name = source.name,
            component = source.component.display().to_string(),
            enabled = source.enabled,
        ));

        let mut plugin = format!("[host]\npoll_interval_secs = {}\n", source.interval_secs);
        if let Some((initial, max, multiplier)) = source.backoff {
            plugin.push_str(&format!(
                "\n[host.backoff]\ninitial_secs = {initial}\nmax_secs = {max}\nmultiplier = {multiplier:?}\n"
            ));
        }
        plugin.push_str(&format!("\n[settings]\n{}\n", source.settings));
        std::fs::write(dir.join(format!("{}.toml", source.name)), plugin)
            .expect("write plugin toml");
    }
    let path = dir.join("filimon.toml");
    std::fs::write(&path, registry).expect("write registry");
    path
}

/// Loads a test registry, panicking with the rendered error on failure.
pub fn load_test_registry(path: &Path) -> LoadedRegistry {
    load_registry_with_env(path, &|_| None).unwrap_or_else(|e| {
        panic!("test registry failed to load: {:?}", miette::Report::new(e));
    })
}

type CaptureBuf = Arc<Mutex<Vec<u8>>>;

thread_local! {
    /// The buffer that receives this thread's tracing output, set while a
    /// `LogCapture` guard is live on the thread. `None` discards the output.
    static THREAD_CAPTURE: RefCell<Option<CaptureBuf>> = const { RefCell::new(None) };
}

/// The writer of the process-wide capture subscriber: appends each write to
/// the capture buffer of the writing thread, or drops it when that thread has
/// none. `fmt` formats and writes an event on the thread that emitted it, so
/// the routing key is the emitting thread.
struct ThreadRoutedWriter;

impl Write for ThreadRoutedWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = THREAD_CAPTURE.try_with(|slot| {
            if let Some(target) = slot.borrow().as_ref() {
                target.lock().expect("log buffer").extend_from_slice(buf);
            }
        });
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Installs, once per process, the one subscriber every test thread shares.
///
/// A per-test `tracing::subscriber::set_default` cannot be made reliable under
/// the parallel test runner. `tracing` caches an `Interest` per callsite, and
/// with a single live scoped dispatcher it computes that interest from the
/// registering thread's own default (`tracing-core`'s `has_just_one` path). A
/// sibling test thread with no subscriber that reaches a callsite first caches
/// `never` for it, and the capturing thread's events from that callsite are
/// then silently dropped until some unrelated `set_default` rebuilds the cache.
/// `rebuild_interest_cache` right after `set_default` does not help, because
/// the callsite may register after it.
///
/// One global dispatcher at `TRACE` is every thread's fallback, so every
/// callsite's interest is always or sometimes, never `never`; which buffer
/// (if any) an event lands in is decided at write time, per thread.
fn install_capture_subscriber() {
    static INSTALLED: OnceLock<()> = OnceLock::new();
    INSTALLED.get_or_init(|| {
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .with_target(true)
            .with_writer(|| ThreadRoutedWriter)
            .finish();
        tracing::subscriber::set_global_default(subscriber).expect(
            "LogCapture needs to own the process-wide tracing subscriber, \
             but another one is already installed",
        );
    });
}

/// Captures `tracing` output emitted on the current thread into a string.
///
/// Capture is per thread: tests using it stay on the current-thread runtime,
/// and output from other threads (or from tasks on other threads) is not seen.
pub struct LogCapture {
    buf: CaptureBuf,
}

/// Ends the capture on the thread that started it when dropped, restoring
/// the capture (if any) that was live before it. Not `Send`: it must be
/// dropped on the thread that created it.
#[must_use = "dropping the guard ends the capture"]
pub struct CaptureGuard {
    previous: Option<CaptureBuf>,
    _same_thread: PhantomData<*const ()>,
}

impl Drop for CaptureGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        let _ = THREAD_CAPTURE.try_with(|slot| *slot.borrow_mut() = previous);
    }
}

impl LogCapture {
    pub fn start() -> (Self, CaptureGuard) {
        install_capture_subscriber();
        let buf: CaptureBuf = Arc::new(Mutex::new(Vec::new()));
        let previous = THREAD_CAPTURE.with(|slot| slot.replace(Some(Arc::clone(&buf))));
        (
            Self { buf },
            CaptureGuard {
                previous,
                _same_thread: PhantomData,
            },
        )
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.buf.lock().expect("log buffer")).into_owned()
    }
}

// ── Loopback HTTP Server ──────────────────────────────────────────────────────

/// One request a `TestHttpServer` received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct SeenRequest {
    pub(crate) method: String,
    /// The request target as sent: path plus query.
    pub(crate) target: String,
    /// Header names are lowercased.
    pub(crate) headers: Vec<(String, String)>,
    pub(crate) body: Vec<u8>,
}

impl SeenRequest {
    /// The first header named `name` (lowercase), if any.
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(header, _)| header == name)
            .map(|(_, value)| value.as_str())
    }
}

/// What a `TestHttpServer` answers one request with.
#[derive(Debug, Clone)]
pub(crate) struct Reply {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    delay: Duration,
    omit_length: bool,
}

impl Reply {
    /// A 200 answer with `body`.
    pub(crate) fn ok(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            headers: vec![],
            body: body.into(),
            delay: Duration::ZERO,
            omit_length: false,
        }
    }

    /// A redirect with this status and a `Location` header.
    pub(crate) fn redirect(status: u16, location: &str) -> Self {
        Self {
            status,
            headers: vec![("location".to_string(), location.to_string())],
            body: vec![],
            delay: Duration::ZERO,
            omit_length: false,
        }
    }

    /// An answer with this status and no body.
    pub(crate) fn status(status: u16) -> Self {
        Self {
            status,
            ..Self::ok("")
        }
    }

    /// Waits `delay` after reading the request before it answers.
    pub(crate) fn delayed(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    /// An answer with this status and header, for example an `x-test` header.
    pub(crate) fn with_header(mut self, name: &str, value: &str) -> Self {
        self.headers.push((name.to_string(), value.to_string()));
        self
    }

    /// Leaves out `Content-Length`; the close of the socket ends the body.
    pub(crate) fn without_length(mut self) -> Self {
        self.omit_length = true;
        self
    }
}

type Handler = Arc<dyn Fn(&SeenRequest) -> Reply + Send + Sync>;

/// A loopback HTTP/1.1 server for egress tests. It answers every request
/// with `Connection: close`; dropping it aborts its tasks.
pub(crate) struct TestHttpServer {
    port: u16,
    seen: Arc<Mutex<Vec<SeenRequest>>>,
    accept_loop: tokio::task::JoinHandle<()>,
}

impl TestHttpServer {
    /// Binds `127.0.0.1:0` and answers each request with `handler`.
    pub(crate) async fn start(
        handler: impl Fn(&SeenRequest) -> Reply + Send + Sync + 'static,
    ) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind loopback");
        let port = listener.local_addr().expect("local address").port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let handler: Handler = Arc::new(handler);
        let recorded = Arc::clone(&seen);
        let accept_loop = tokio::spawn(async move {
            // Connection tasks live in the set, so aborting this task ends them.
            let mut connections = tokio::task::JoinSet::new();
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    break;
                };
                let handler = Arc::clone(&handler);
                let recorded = Arc::clone(&recorded);
                connections.spawn(serve_connection(stream, handler, recorded));
            }
        });
        Self {
            port,
            seen,
            accept_loop,
        }
    }

    pub(crate) fn port(&self) -> u16 {
        self.port
    }

    /// `http://127.0.0.1:<port><path>`.
    pub(crate) fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port())
    }

    /// `http://localhost:<port><path>`: a different name for the same server.
    pub(crate) fn localhost_url(&self, path: &str) -> String {
        format!("http://localhost:{}{path}", self.port())
    }

    /// Every request received so far, in arrival order.
    pub(crate) fn seen(&self) -> Vec<SeenRequest> {
        self.seen.lock().expect("seen requests").clone()
    }
}

impl Drop for TestHttpServer {
    fn drop(&mut self) {
        self.accept_loop.abort();
    }
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        404 => "Not Found",
        429 => "Too Many Requests",
        _ => "Status",
    }
}

async fn serve_connection(
    stream: tokio::net::TcpStream,
    handler: Handler,
    seen: Arc<Mutex<Vec<SeenRequest>>>,
) {
    use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
        return;
    }
    let mut parts = line.split_whitespace();
    let method = parts.next().unwrap_or_default().to_string();
    let target = parts.next().unwrap_or_default().to_string();

    let mut headers = Vec::new();
    loop {
        line.clear();
        if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
            return;
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some((name, value)) = trimmed.split_once(':') {
            headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
        }
    }
    let length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .and_then(|(_, value)| value.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0_u8; length];
    if reader.read_exact(&mut body).await.is_err() {
        return;
    }

    let request = SeenRequest {
        method,
        target,
        headers,
        body,
    };
    seen.lock().expect("seen requests").push(request.clone());
    let reply = handler(&request);
    if !reply.delay.is_zero() {
        tokio::time::sleep(reply.delay).await;
    }

    let mut head = format!(
        "HTTP/1.1 {} {}\r\n",
        reply.status,
        reason_phrase(reply.status)
    );
    let mut lines: Vec<String> = reply
        .headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .collect();
    if !reply.omit_length {
        lines.push(format!("content-length: {}\r\n", reply.body.len()));
    }
    lines.push("connection: close\r\n\r\n".to_string());
    head.push_str(&lines.concat());

    // The client may hang up early (a body over the cap, a timeout); that is
    // not a test failure.
    let mut stream = reader.into_inner();
    if stream.write_all(head.as_bytes()).await.is_ok() {
        let _ = stream.write_all(&reply.body).await;
    }
    let _ = stream.shutdown().await;
}

// ── Scripted Fakes For Scheduler Tests ────────────────────────────────────────

/// One scripted answer. Every `init` and `poll` call of every instance a
/// `FakeFactory` builds pops the next step, so a rebuilt instance continues
/// the same script.
pub(crate) enum FakeStep {
    InitOk,
    InitErr(PollError),
    PollOk {
        items: Vec<WitItem>,
        next_cursor: Option<Vec<u8>>,
        /// Virtual time the poll takes.
        delay: Duration,
    },
    PollErr(PollError),
    /// A wasmtime-level failure of whichever call pops it.
    Trap(String),
}

impl FakeStep {
    /// A poll answer with no items, no delay and the given next cursor.
    pub(crate) fn poll_ok(next_cursor: Option<Vec<u8>>) -> Self {
        Self::PollOk {
            items: vec![],
            next_cursor,
            delay: Duration::ZERO,
        }
    }
}

/// One thing a fake instance did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FakeEvent {
    Build,
    Init,
    Poll {
        cursor: Option<Vec<u8>>,
        started: Instant,
        ended: Instant,
    },
}

/// Builds `FakeInstance`s that share one script and one event log.
pub(crate) struct FakeFactory {
    script: Arc<Mutex<VecDeque<FakeStep>>>,
    events: Arc<Mutex<Vec<FakeEvent>>>,
}

impl FakeFactory {
    pub(crate) fn new(steps: Vec<FakeStep>) -> Self {
        Self {
            script: Arc::new(Mutex::new(steps.into())),
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub(crate) fn events(&self) -> Vec<FakeEvent> {
        self.events.lock().expect("events").clone()
    }

    pub(crate) fn builds(&self) -> usize {
        self.events()
            .iter()
            .filter(|e| matches!(e, FakeEvent::Build))
            .count()
    }

    pub(crate) fn inits(&self) -> usize {
        self.events()
            .iter()
            .filter(|e| matches!(e, FakeEvent::Init))
            .count()
    }

    /// Whole seconds from `start` to each completed poll's start.
    pub(crate) fn poll_offsets(&self, start: Instant) -> Vec<u64> {
        self.events()
            .iter()
            .filter_map(|e| match e {
                FakeEvent::Poll { started, .. } => Some(started.duration_since(start).as_secs()),
                _ => None,
            })
            .collect()
    }

    /// The cursor each completed poll received.
    pub(crate) fn poll_cursors(&self) -> Vec<Option<Vec<u8>>> {
        self.events()
            .iter()
            .filter_map(|e| match e {
                FakeEvent::Poll { cursor, .. } => Some(cursor.clone()),
                _ => None,
            })
            .collect()
    }
}

impl InstanceFactory for FakeFactory {
    type Instance = FakeInstance;

    async fn instantiate(&self, _plugin: &str) -> Result<FakeInstance, InstanceBuildError> {
        self.events.lock().expect("events").push(FakeEvent::Build);
        Ok(FakeInstance {
            script: Arc::clone(&self.script),
            events: Arc::clone(&self.events),
        })
    }
}

/// A scripted instance.
pub(crate) struct FakeInstance {
    script: Arc<Mutex<VecDeque<FakeStep>>>,
    events: Arc<Mutex<Vec<FakeEvent>>>,
}

impl PluginInstance for FakeInstance {
    async fn init(&mut self, _settings: &str, _secrets: &[(String, String)]) -> CallOutcome<()> {
        self.events.lock().expect("events").push(FakeEvent::Init);
        let step = self.script.lock().expect("script").pop_front();
        match step {
            None | Some(FakeStep::InitOk) => CallOutcome::Ok(()),
            Some(FakeStep::InitErr(error)) => CallOutcome::Guest(error),
            Some(FakeStep::Trap(text)) => CallOutcome::InstanceFatal(text),
            Some(_) => panic!("fake script: init popped a poll step"),
        }
    }

    async fn poll(&mut self, cursor: Option<&[u8]>) -> CallOutcome<PollResponse> {
        let started = Instant::now();
        let step = self.script.lock().expect("script").pop_front();
        let outcome = match step {
            None => CallOutcome::Ok(PollResponse {
                items: vec![],
                next_cursor: cursor.map(<[u8]>::to_vec),
            }),
            Some(FakeStep::PollOk {
                items,
                next_cursor,
                delay,
            }) => {
                if !delay.is_zero() {
                    tokio::time::sleep(delay).await;
                }
                CallOutcome::Ok(PollResponse { items, next_cursor })
            }
            Some(FakeStep::PollErr(error)) => CallOutcome::Guest(error),
            Some(FakeStep::Trap(text)) => CallOutcome::InstanceFatal(text),
            Some(_) => panic!("fake script: poll popped an init step"),
        };
        self.events.lock().expect("events").push(FakeEvent::Poll {
            cursor: cursor.map(<[u8]>::to_vec),
            started,
            ended: Instant::now(),
        });
        outcome
    }
}

/// A `VecSink` whose first `fail_first` `send_batch` calls fail.
pub(crate) struct FlakySink {
    inner: VecSink,
    fail_first: usize,
    calls: AtomicUsize,
}

impl FlakySink {
    pub(crate) fn new(fail_first: usize) -> Self {
        Self {
            inner: VecSink::default(),
            fail_first,
            calls: AtomicUsize::new(0),
        }
    }

    pub(crate) fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    pub(crate) fn records(&self) -> Vec<SinkRecord> {
        self.inner.records()
    }
}

impl ItemSink for FlakySink {
    async fn send_batch(&self, plugin: &str, records: Vec<SinkRecord>) -> Result<(), SinkError> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        if call < self.fail_first {
            return Err(SinkError("broker unavailable".to_string()));
        }
        self.inner.send_batch(plugin, records).await
    }
}

/// A plugin spec built without a registry.
pub(crate) fn spec(
    name: &str,
    interval_secs: u64,
    initial_secs: u64,
    max_secs: u64,
    multiplier: f64,
) -> PluginSpec {
    PluginSpec {
        name: name.to_string(),
        host: HostConfig {
            poll_interval: Duration::from_secs(interval_secs),
            timeout: Duration::from_secs(30),
            allowlist: vec![],
            backoff: BackoffConfig {
                initial: Duration::from_secs(initial_secs),
                max: Duration::from_secs(max_secs),
                multiplier,
            },
        },
        settings_toml: String::new(),
        secrets: ResolvedSecrets::new(),
    }
}

/// A valid item with the given id.
pub(crate) fn wit_item(id: &str) -> WitItem {
    WitItem {
        id: id.to_string(),
        url: format!("https://example.invalid/fake/{id}"),
        title: None,
        text: format!("item {id}"),
        author: None,
        published: None,
        platform: "fake".to_string(),
        raw: None,
    }
}

/// Runs `run_plugin`, lets `duration` pass, cancels and joins.
pub(crate) async fn run_for<F: InstanceFactory, S: ItemSink>(
    spec: PluginSpec,
    factory: Arc<F>,
    sink: Arc<S>,
    cursors: Arc<CursorStore>,
    duration: Duration,
) -> PluginReport {
    let token = CancellationToken::new();
    let handle = tokio::spawn(run_plugin(spec, factory, sink, cursors, token.clone()));
    tokio::time::sleep(duration).await;
    token.cancel();
    handle.await.expect("plugin task")
}

#[cfg(test)]
mod tests {
    use std::sync::Barrier;

    use super::*;

    fn probe(who: &str) {
        tracing::warn!(plugin = %who, "log-capture-probe");
    }

    /// The scheduler flake: a callsite first reached by a thread that has no
    /// capture must still deliver events to a thread that does.
    #[test]
    fn capture_sees_a_callsite_first_registered_by_a_thread_without_one() {
        let (logs, _guard) = LogCapture::start();
        std::thread::spawn(|| probe("uncaptured"))
            .join()
            .expect("probe thread");
        probe("captured");

        let text = logs.text();
        assert!(text.contains("plugin=captured"), "{text:?}");
        assert!(!text.contains("plugin=uncaptured"), "{text:?}");
    }

    #[test]
    fn concurrent_captures_only_see_their_own_threads_output() {
        let barrier = Arc::new(Barrier::new(2));
        let handles: Vec<_> = ["one", "two"]
            .into_iter()
            .map(|who| {
                let barrier = Arc::clone(&barrier);
                std::thread::spawn(move || {
                    let (logs, _guard) = LogCapture::start();
                    barrier.wait();
                    probe(who);
                    barrier.wait();
                    logs.text()
                })
            })
            .collect();
        let texts: Vec<String> = handles
            .into_iter()
            .map(|h| h.join().expect("capture thread"))
            .collect();

        assert!(texts[0].contains("plugin=one") && !texts[0].contains("plugin=two"));
        assert!(texts[1].contains("plugin=two") && !texts[1].contains("plugin=one"));
    }

    #[test]
    fn nested_capture_restores_the_outer_one_and_no_capture_discards() {
        probe("before");
        let (outer, outer_guard) = LogCapture::start();
        probe("outer-1");
        {
            let (inner, _inner_guard) = LogCapture::start();
            probe("inner");
            assert!(inner.text().contains("plugin=inner"));
            assert!(!inner.text().contains("plugin=outer-1"));
        }
        probe("outer-2");
        drop(outer_guard);
        probe("after");

        let text = outer.text();
        assert!(text.contains("plugin=outer-1"), "{text:?}");
        assert!(text.contains("plugin=outer-2"), "{text:?}");
        for absent in ["plugin=inner", "plugin=before", "plugin=after"] {
            assert!(!text.contains(absent), "{text:?}");
        }
    }
}
