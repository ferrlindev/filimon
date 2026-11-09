// ─────────────────────────────────────────────────────────────────────────────
// plugins/host/testing.rs - Shared test helpers (cfg(test) only)
// ─────────────────────────────────────────────────────────────────────────────

use std::collections::VecDeque;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::time::Instant;
use tokio_util::sync::CancellationToken;
use tracing::subscriber::DefaultGuard;

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

#[derive(Clone)]
struct SharedBuf(Arc<Mutex<Vec<u8>>>);

impl Write for SharedBuf {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log buffer").extend_from_slice(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// Captures `tracing` output into a string. The default subscriber is
/// thread-local, so tests using it stay on the current-thread runtime.
pub struct LogCapture {
    buf: Arc<Mutex<Vec<u8>>>,
}

impl LogCapture {
    pub fn start() -> (Self, DefaultGuard) {
        let buf = Arc::new(Mutex::new(Vec::new()));
        let writer = SharedBuf(Arc::clone(&buf));
        let subscriber = tracing_subscriber::fmt()
            .with_ansi(false)
            .with_max_level(tracing::Level::TRACE)
            .with_target(true)
            .with_writer(move || writer.clone())
            .finish();
        let guard = tracing::subscriber::set_default(subscriber);
        (Self { buf }, guard)
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.buf.lock().expect("log buffer")).into_owned()
    }
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
