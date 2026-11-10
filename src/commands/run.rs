// ─────────────────────────────────────────────────────────────────────────────
// commands/run.rs - `fili run`
//
// Polls every enabled source plugin on its own interval and produces the items
// to Fluvio until the operator stops it. `run` is the only place that connects
// the production sink; `run_with_sink` is generic over `ItemSink` and takes
// both stop tokens, so everything else is tested with an in-memory sink.
//
// Startup is honest: a registry with nothing enabled, a state directory held by
// another process and a run in which no source component could be loaded are
// all errors before any polling starts, and every component that failed to load
// is printed naming its plugin and path. Nothing printed here carries settings,
// secrets or item content.
// ─────────────────────────────────────────────────────────────────────────────

use std::future::Future;
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

use miette::IntoDiagnostic;
use tokio_util::sync::CancellationToken;

use crate::commands::plugins::state_dir_for;
use crate::output::sanitize_for_terminal;
use crate::plugins::config::{LoadedRegistry, load_registry};
use crate::plugins::host::engine::clean_guest_text;
use crate::plugins::host::loader::PluginKind;
use crate::plugins::host::scheduler::PluginReport;
use crate::plugins::host::sink::{FluvioSink, ItemSink};
use crate::plugins::host::{
    ABORTED_AT_SHUTDOWN, DEFAULT_SHUTDOWN_GRACE, Host, HostOptions, HostReport,
};

/// Longest reason shown in the summary's STATE column, in characters.
const STATE_REASON_CLIP: usize = 80;

/// Longest load failure text printed at startup, in characters.
const LOAD_FAILURE_CLIP: usize = 512;

/// The summary table's column headings, in order.
const SUMMARY_HEADINGS: [&str; 6] = ["NAME", "POLLS", "SENT", "DROPPED", "FAILURES", "STATE"];

// ── Startup Checks ────────────────────────────────────────────────────────────

/// Refuses a registry that has no enabled source, so `fili run` never idles in
/// silence.
///
/// # Errors
/// An error naming the registry file when it has no sources or every source is
/// disabled.
pub fn ensure_enabled_sources(registry: &LoadedRegistry) -> miette::Result<()> {
    if registry.sources.values().any(|source| source.enabled) {
        return Ok(());
    }
    Err(miette::miette!(
        "no source plugin is enabled in {}: register a source and set `enabled = true` for at least one",
        sanitize_for_terminal(&registry.path.display().to_string())
    ))
}

/// The exit rule (OQ-9). An operator-initiated stop is success even when tasks
/// were aborted at shutdown; the run fails only when every plugin ended
/// disabled by a fault of its own.
///
/// # Errors
/// An error when `report` has plugins and every one of them was disabled for a
/// reason other than being aborted at shutdown.
pub fn run_outcome(report: &HostReport) -> miette::Result<()> {
    let faulted = |plugin: &PluginReport| {
        plugin
            .disabled
            .as_deref()
            .is_some_and(|reason| reason != ABORTED_AT_SHUTDOWN)
    };
    if !report.plugins.is_empty() && report.plugins.values().all(faulted) {
        return Err(miette::miette!(
            "every plugin was disabled by a fault; see the summary above"
        ));
    }
    Ok(())
}

// ── Summary ───────────────────────────────────────────────────────────────────

/// The STATE cell of one plugin.
fn state_cell(plugin: &PluginReport) -> String {
    match plugin.disabled.as_deref() {
        None => "stopped".to_string(),
        Some(ABORTED_AT_SHUTDOWN) => ABORTED_AT_SHUTDOWN.to_string(),
        Some(reason) => format!("disabled: {}", clean_guest_text(reason, STATE_REASON_CLIP)),
    }
}

/// Writes the per-plugin summary: a header line, then one line per plugin in
/// name order. Columns are left-aligned and padded to the widest cell with two
/// spaces between them; the last column is not padded. Plugins cut off at the
/// shutdown deadline are named again in one warning line.
///
/// # Errors
/// Returns the write error of `out`.
pub fn render_run_summary(report: &HostReport, out: &mut impl Write) -> std::io::Result<()> {
    if report.plugins.is_empty() {
        return writeln!(out, "[Run] no plugin ran");
    }

    let table: Vec<[String; 6]> = report
        .plugins
        .iter()
        .map(|(name, plugin)| {
            [
                sanitize_for_terminal(name),
                plugin.polls.to_string(),
                plugin.items_sent.to_string(),
                plugin.dropped.to_string(),
                plugin.failures.to_string(),
                state_cell(plugin),
            ]
        })
        .collect();
    let mut widths = SUMMARY_HEADINGS.map(|heading| heading.chars().count());
    for row in &table {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }

    let line = |row: [&str; 6]| {
        let mut text = String::new();
        for (index, cell) in row.iter().enumerate() {
            text.push_str(cell);
            if index + 1 < row.len() {
                let padding = widths[index] - cell.chars().count();
                text.push_str(&" ".repeat(padding + 2));
            }
        }
        text
    };

    writeln!(out, "{}", line(SUMMARY_HEADINGS))?;
    for row in &table {
        writeln!(out, "{}", line(row.each_ref().map(String::as_str)))?;
    }

    let aborted: Vec<String> = report
        .plugins
        .iter()
        .filter(|(_, plugin)| plugin.disabled.as_deref() == Some(ABORTED_AT_SHUTDOWN))
        .map(|(name, _)| sanitize_for_terminal(name))
        .collect();
    if !aborted.is_empty() {
        writeln!(
            out,
            "[Run] warning: aborted at shutdown: {}; an aborted send may replay its last batch on the next start",
            aborted.join(", ")
        )?;
    }
    Ok(())
}

// ── The Run ───────────────────────────────────────────────────────────────────

/// A comma-separated list of names, or `none`.
fn name_list(names: &[String]) -> String {
    if names.is_empty() {
        "none".to_string()
    } else {
        names
            .iter()
            .map(|name| sanitize_for_terminal(name))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// Loads the host over `registry`, reports what loaded and what did not, runs
/// it into `sink` until `shutdown` (graceful) or `abort` (at once) is cancelled,
/// and prints the summary.
///
/// # Errors
/// Any `HostError` from `Host::load` unchanged (a state directory held by
/// another process is `StateDirLocked`), or an error when no source component
/// could be loaded. Nothing is polled and nothing is sent in either case.
pub async fn run_with_sink<S: ItemSink>(
    registry: &LoadedRegistry,
    state_dir_flag: Option<&Path>,
    sink: Arc<S>,
    shutdown: CancellationToken,
    abort: CancellationToken,
    out: &mut impl Write,
    err: &mut impl Write,
) -> miette::Result<HostReport> {
    let options = HostOptions::for_registry(registry, state_dir_flag);
    let state_dir = options.state_dir.clone();
    let host = Host::load(registry, options)?;

    // Progress lines are best effort: a closed stdout must not stop the run.
    writeln!(
        out,
        "[Run] state directory: {}",
        sanitize_for_terminal(&state_dir.display().to_string())
    )
    .ok();
    writeln!(out, "[Run] loaded sources: {}", name_list(host.loaded_sources())).ok();
    writeln!(out, "[Run] loaded fetchers: {}", name_list(host.loaded_fetchers())).ok();
    for failure in host.load_failures() {
        let kind = match failure.kind {
            PluginKind::Source => "source",
            PluginKind::Fetcher => "fetcher",
        };
        writeln!(
            err,
            "[Run] {kind} `{}` could not be loaded: {}",
            sanitize_for_terminal(&failure.name),
            clean_guest_text(&failure.error.to_string(), LOAD_FAILURE_CLIP)
        )
        .ok();
    }
    if host.loaded_sources().is_empty() {
        return Err(miette::miette!(
            "no source plugin could be loaded; see the errors above"
        ));
    }

    writeln!(
        out,
        "[Run] polling {} source(s); press Ctrl+C to stop",
        host.loaded_sources().len()
    )
    .ok();
    let report = host.run_with_abort(sink, shutdown, abort).await;
    render_run_summary(&report, out).ok();
    Ok(report)
}

/// `fili run`: the strict registry load, the state directory check, the
/// production sink, the host and the exit rule.
///
/// # Errors
/// An error for a registry that does not load, one with nothing enabled, an
/// empty state directory, a missing cluster or topic, any `run_with_sink`
/// error, or a run in which every plugin was disabled by a fault.
pub async fn run(registry_path: &Path, state_dir_flag: Option<&Path>) -> miette::Result<()> {
    let registry = load_registry(registry_path)?;
    ensure_enabled_sources(&registry)?;
    state_dir_for(&registry.base_dir, state_dir_flag)?;

    println!("[Run] Connecting to Fluvio cluster...");
    let sink = Arc::new(FluvioSink::connect_social().await?);

    let shutdown = CancellationToken::new();
    let abort = CancellationToken::new();
    let signals = OsSignals::new().into_diagnostic()?;
    let signal_task = tokio::spawn(forward_signals(
        signals,
        shutdown.clone(),
        abort.clone(),
    ));

    let mut out = std::io::stdout();
    let mut err = std::io::stderr();
    let result = run_with_sink(
        &registry,
        state_dir_flag,
        sink,
        shutdown,
        abort,
        &mut out,
        &mut err,
    )
    .await;
    signal_task.abort();
    run_outcome(&result?)
}

// ── Stop Requests ─────────────────────────────────────────────────────────────

/// Where stop requests come from. Each call to `next` returns when one more
/// request has arrived; the real source is the operating system, tests script
/// it.
pub(crate) trait SignalSource: Send + 'static {
    fn next(&mut self) -> impl Future<Output = ()> + Send;
}

/// Turns stop requests into the two tokens the host understands (OQ-8): the
/// first request starts a graceful stop (the host waits up to the shutdown
/// grace for in-flight sends), the second aborts at once. The task ends after
/// the second request; a caller whose host ended on its own simply aborts it.
pub(crate) async fn forward_signals<S: SignalSource>(
    mut source: S,
    shutdown: CancellationToken,
    abort: CancellationToken,
) {
    source.next().await;
    println!(
        "[Run] stopping: waiting up to {} s for in-flight sends; press Ctrl+C again to stop now",
        DEFAULT_SHUTDOWN_GRACE.as_secs()
    );
    shutdown.cancel();

    source.next().await;
    println!("[Run] stopping now: in-flight sends are aborted");
    abort.cancel();
}

/// The operating system's stop requests: SIGINT (Ctrl+C) and, on Unix, SIGTERM.
/// The streams are created once and kept, so a request that arrives between two
/// calls of `next` is not lost.
pub(crate) struct OsSignals {
    #[cfg(unix)]
    interrupt: tokio::signal::unix::Signal,
    #[cfg(unix)]
    terminate: tokio::signal::unix::Signal,
}

impl OsSignals {
    /// Registers the handlers; must be called inside the runtime.
    ///
    /// # Errors
    /// The io error when a handler cannot be registered.
    pub(crate) fn new() -> std::io::Result<Self> {
        #[cfg(unix)]
        {
            use tokio::signal::unix::{SignalKind, signal};
            Ok(Self {
                interrupt: signal(SignalKind::interrupt())?,
                terminate: signal(SignalKind::terminate())?,
            })
        }
        #[cfg(not(unix))]
        {
            Ok(Self {})
        }
    }
}

impl SignalSource for OsSignals {
    #[cfg(unix)]
    async fn next(&mut self) {
        tokio::select! {
            _ = self.interrupt.recv() => {}
            _ = self.terminate.recv() => {}
        }
    }

    #[cfg(not(unix))]
    async fn next(&mut self) {
        if tokio::signal::ctrl_c().await.is_err() {
            // No handler could be registered: never report a request that did not happen.
            std::future::pending::<()>().await;
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::time::Duration;

    use tempfile::TempDir;

    use super::*;
    use crate::models::SourceItem;
    use crate::plugins::host::cursor::CursorStore;
    use crate::plugins::host::sink::live_support::{delete_topic, ready_sink, scratch_topic};
    use crate::plugins::host::sink::{SinkError, SinkRecord, VecSink};
    use crate::streaming::FluvioStream;
    use crate::plugins::host::testing::{
        TestSource, fixture_wasm, load_test_registry, write_registry,
    };

    fn source(name: &str, component: std::path::PathBuf, enabled: bool) -> TestSource {
        TestSource {
            name: name.to_string(),
            component,
            enabled,
            interval_secs: 60,
            backoff: None,
            // The stub source refuses to start without a greeting.
            settings: "greeting = \"hello from run\"".to_string(),
        }
    }

    fn report_of(plugins: &[(&str, PluginReport)]) -> HostReport {
        HostReport {
            loaded_sources: plugins.iter().map(|(name, _)| (*name).to_string()).collect(),
            loaded_fetchers: Vec::new(),
            load_failures: Vec::new(),
            plugins: plugins
                .iter()
                .map(|(name, report)| ((*name).to_string(), report.clone()))
                .collect::<BTreeMap<_, _>>(),
        }
    }

    fn disabled(reason: &str) -> PluginReport {
        PluginReport {
            disabled: Some(reason.to_string()),
            ..PluginReport::default()
        }
    }

    #[test]
    fn run_refuses_a_registry_with_no_enabled_source() {
        let dir = TempDir::new().expect("tempdir");

        let empty = load_test_registry(&write_registry(dir.path(), &[]));
        let text = ensure_enabled_sources(&empty)
            .expect_err("no sources")
            .to_string();
        assert!(text.contains(&empty.path.display().to_string()), "{text}");
        assert!(text.contains("enabled = true"), "{text}");

        let all_off = load_test_registry(&write_registry(
            dir.path(),
            &[
                source("a", dir.path().join("a.wasm"), false),
                source("b", dir.path().join("b.wasm"), false),
            ],
        ));
        let text = ensure_enabled_sources(&all_off)
            .expect_err("every source disabled")
            .to_string();
        assert!(text.contains("enabled = true"), "{text}");

        let one_on = load_test_registry(&write_registry(
            dir.path(),
            &[
                source("a", dir.path().join("a.wasm"), false),
                source("b", dir.path().join("b.wasm"), true),
            ],
        ));
        ensure_enabled_sources(&one_on).expect("one enabled source is enough");
    }

    #[tokio::test]
    async fn run_with_sink_refuses_a_locked_state_dir() {
        let dir = TempDir::new().expect("tempdir");
        let registry = load_test_registry(&write_registry(
            dir.path(),
            &[source("stub", dir.path().join("stub.wasm"), true)],
        ));
        let state = dir.path().join("state");
        let _held = CursorStore::open(&state).expect("another process's lock");

        let sink = Arc::new(VecSink::default());
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let error = run_with_sink(
            &registry,
            Some(&state),
            Arc::clone(&sink),
            CancellationToken::new(),
            CancellationToken::new(),
            &mut out,
            &mut err,
        )
        .await
        .expect_err("a held state directory is refused");

        let text = error.to_string();
        assert!(text.contains("in use by another fili process"), "{text}");
        assert!(text.contains(&state.display().to_string()), "{text}");
        assert!(sink.records().is_empty(), "nothing may be sent");
        assert!(out.is_empty(), "nothing is announced before the refusal");
    }

    #[tokio::test]
    async fn run_with_sink_prints_load_failures_and_errors_when_nothing_loaded() {
        let dir = TempDir::new().expect("tempdir");
        let garbage = dir.path().join("garbage.wasm");
        std::fs::write(&garbage, b"this is not a wasm component").expect("write garbage");
        let registry = load_test_registry(&write_registry(
            dir.path(),
            &[source("broken", garbage.clone(), true)],
        ));
        let state = dir.path().join("state");

        let sink = Arc::new(VecSink::default());
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let error = run_with_sink(
            &registry,
            Some(&state),
            Arc::clone(&sink),
            CancellationToken::new(),
            CancellationToken::new(),
            &mut out,
            &mut err,
        )
        .await
        .expect_err("nothing could be loaded");

        let text = error.to_string();
        assert!(text.contains("no source plugin could be loaded"), "{text}");
        let printed = String::from_utf8(err).expect("utf8 stderr");
        assert!(printed.contains("broken"), "{printed}");
        assert!(
            printed.contains(&garbage.display().to_string()),
            "the component path is named: {printed}"
        );
        let announced = String::from_utf8(out).expect("utf8 stdout");
        assert!(announced.contains("loaded sources: none"), "{announced}");
        assert!(!announced.contains("polling"), "{announced}");
        assert!(sink.records().is_empty());
    }

    #[test]
    fn run_summary_lists_plugins_sorted_with_counts_and_reasons() {
        let long_reason = format!("boom \u{1b}[31mred\u{1b}[0m {}", "x".repeat(200));
        let report = report_of(&[
            (
                "zeta",
                PluginReport {
                    polls: 7,
                    items_sent: 21,
                    dropped: 1,
                    failures: 2,
                    disabled: None,
                },
            ),
            ("alpha", disabled(&long_reason)),
            ("mid", disabled(ABORTED_AT_SHUTDOWN)),
        ]);

        let mut out = Vec::new();
        render_run_summary(&report, &mut out).expect("render");
        let text = String::from_utf8(out).expect("utf8");
        let lines: Vec<&str> = text.lines().collect();

        assert_eq!(
            lines[0].split_whitespace().collect::<Vec<_>>(),
            ["NAME", "POLLS", "SENT", "DROPPED", "FAILURES", "STATE"]
        );
        let names: Vec<&str> = lines[1..4]
            .iter()
            .filter_map(|line| line.split_whitespace().next())
            .collect();
        assert_eq!(names, ["alpha", "mid", "zeta"], "{text}");

        let zeta = lines[3].split_whitespace().collect::<Vec<_>>();
        assert_eq!(zeta, ["zeta", "7", "21", "1", "2", "stopped"], "{text}");
        assert!(lines[2].ends_with("aborted at shutdown"), "{text}");

        let alpha = lines[1];
        assert!(alpha.contains("disabled: boom"), "{text}");
        assert!(!text.contains('\u{1b}'), "control characters are removed");
        let reason = alpha.split("disabled: ").nth(1).expect("reason cell");
        assert_eq!(reason.chars().count(), STATE_REASON_CLIP + 1, "{reason}");
        assert!(reason.ends_with('…'), "{reason}");
        assert!(lines.iter().all(|line| !line.ends_with(' ')), "{text}");

        let warning = lines.last().expect("warning line");
        assert!(warning.contains("aborted at shutdown: mid"), "{warning}");
        assert!(!warning.contains("alpha") && !warning.contains("zeta"), "{warning}");
    }

    #[test]
    fn run_outcome_is_an_error_only_when_every_plugin_was_disabled_by_a_fault() {
        let ok = PluginReport::default();

        let all_faulted = report_of(&[("a", disabled("boom")), ("b", disabled("trapped"))]);
        let text = run_outcome(&all_faulted)
            .expect_err("every plugin faulted")
            .to_string();
        assert!(text.contains("disabled by a fault"), "{text}");

        let one_running = report_of(&[("a", ok.clone()), ("b", disabled("boom"))]);
        run_outcome(&one_running).expect("one plugin kept running");

        let all_aborted = report_of(&[
            ("a", disabled(ABORTED_AT_SHUTDOWN)),
            ("b", disabled(ABORTED_AT_SHUTDOWN)),
        ]);
        run_outcome(&all_aborted).expect("an operator stop is success");

        let mixed = report_of(&[("a", disabled(ABORTED_AT_SHUTDOWN)), ("b", disabled("boom"))]);
        run_outcome(&mixed).expect("one aborted plus one fault is still an operator stop");

        run_outcome(&report_of(&[])).expect("an empty report is not a fault");
    }

    /// The tracer on the real stub component: both enabled sources are polled,
    /// the disabled one is skipped.
    #[tokio::test(start_paused = true)]
    #[ignore = "needs plugin fixtures: see plugins/README.md"]
    async fn wasm_run_with_sink_polls_every_enabled_source_and_skips_disabled() {
        let tmp = TempDir::new().expect("tempdir");
        let stub = fixture_wasm("stub-source", "stub_source.wasm");
        let registry = load_test_registry(&write_registry(
            tmp.path(),
            &[
                source("stub", stub.clone(), true),
                source("stub2", stub.clone(), true),
                source("off", stub, false),
            ],
        ));
        let state = tmp.path().join(".fili-state");

        let sink = Arc::new(VecSink::default());
        let shutdown = CancellationToken::new();
        let abort = CancellationToken::new();
        let stopper = {
            let shutdown = shutdown.clone();
            async move {
                tokio::time::sleep(Duration::from_secs(1)).await;
                shutdown.cancel();
            }
        };
        let (mut out, mut err) = (Vec::new(), Vec::new());
        let (result, ()) = tokio::join!(
            run_with_sink(
                &registry,
                Some(&state),
                Arc::clone(&sink),
                shutdown.clone(),
                abort,
                &mut out,
                &mut err,
            ),
            stopper
        );
        let report = result.expect("the run succeeds");

        assert_eq!(report.plugins["stub"].polls, 1);
        assert_eq!(report.plugins["stub2"].polls, 1);
        assert!(!report.plugins.contains_key("off"));
        for name in ["stub", "stub2"] {
            let records = sink.plugin_records(name);
            assert_eq!(records.len(), 1, "{name}");
            assert_eq!(records[0].key, "stub:stub-0", "{name}");
        }

        let text = String::from_utf8(out).expect("utf8");
        assert!(text.contains("loaded sources: stub, stub2"), "{text}");
        assert!(text.contains("polling 2 source(s)"), "{text}");
        assert!(state.join("stub.json").is_file());
        assert!(state.join("stub2.json").is_file());
        assert!(!state.join("off.json").exists());
        assert!(err.is_empty(), "{}", String::from_utf8_lossy(&err));
    }

    // ── Stop requests ────────────────────────────────────────────────────────

    /// A signal source scripted through a channel: one message is one request.
    struct ScriptedSignals(tokio::sync::mpsc::UnboundedReceiver<()>);

    impl SignalSource for ScriptedSignals {
        fn next(&mut self) -> impl Future<Output = ()> + Send {
            async move {
                if self.0.recv().await.is_none() {
                    std::future::pending::<()>().await;
                }
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn forward_signals_first_cancels_shutdown_and_second_aborts() {
        let (requests, receiver) = tokio::sync::mpsc::unbounded_channel();
        let shutdown = CancellationToken::new();
        let abort = CancellationToken::new();
        let task = tokio::spawn(forward_signals(
            ScriptedSignals(receiver),
            shutdown.clone(),
            abort.clone(),
        ));

        tokio::time::sleep(Duration::from_secs(5)).await;
        assert!(!shutdown.is_cancelled(), "no request yet");
        assert!(!abort.is_cancelled(), "no request yet");

        requests.send(()).expect("first request");
        tokio::time::sleep(Duration::from_secs(1)).await;
        assert!(shutdown.is_cancelled(), "the first request starts a graceful stop");
        assert!(!abort.is_cancelled(), "the first request must not abort");
        assert!(!task.is_finished(), "the task keeps waiting for a second request");

        requests.send(()).expect("second request");
        tokio::time::timeout(Duration::from_secs(5), task)
            .await
            .expect("the task returns after the second request")
            .expect("join");
        assert!(abort.is_cancelled(), "the second request aborts at once");
    }

    #[tokio::test(start_paused = true)]
    async fn forward_signals_ignores_a_cancelled_shutdown_when_the_host_ended_on_its_own() {
        let (_requests, receiver) = tokio::sync::mpsc::unbounded_channel();
        let shutdown = CancellationToken::new();
        let abort = CancellationToken::new();
        let task = tokio::spawn(forward_signals(
            ScriptedSignals(receiver),
            shutdown.clone(),
            abort.clone(),
        ));

        // The host ended on its own after a minute and the caller drops the task.
        tokio::time::sleep(Duration::from_mins(1)).await;
        task.abort();
        let _ = task.await;

        assert!(!shutdown.is_cancelled());
        assert!(!abort.is_cancelled());
    }

    // ── Cluster-backed run ───────────────────────────────────────────────────

    /// Delegates to a `FluvioSink` and counts the records it delivered.
    struct CountingSink {
        inner: FluvioSink,
        delivered: std::sync::atomic::AtomicUsize,
    }

    impl ItemSink for CountingSink {
        async fn send_batch(
            &self,
            plugin: &str,
            records: Vec<SinkRecord>,
        ) -> Result<(), SinkError> {
            let count = records.len();
            self.inner.send_batch(plugin, records).await?;
            self.delivered
                .fetch_add(count, std::sync::atomic::Ordering::SeqCst);
            Ok(())
        }
    }

    impl CountingSink {
        fn delivered(&self) -> usize {
            self.delivered.load(std::sync::atomic::Ordering::SeqCst)
        }
    }

    /// Runs the command-level function over `registry` into `topic` until at
    /// least `at_least` records were delivered (30 s limit), then stops it the
    /// way an operator would (the shutdown token) and returns the total.
    async fn run_until_delivered(
        registry: &LoadedRegistry,
        state_dir: &Path,
        topic: &str,
        at_least: usize,
    ) -> miette::Result<usize> {
        let sink = Arc::new(CountingSink {
            inner: ready_sink(topic).await?,
            delivered: std::sync::atomic::AtomicUsize::new(0),
        });
        let shutdown = CancellationToken::new();
        let watcher = {
            let sink = Arc::clone(&sink);
            let shutdown = shutdown.clone();
            async move {
                let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
                while sink.delivered() < at_least && tokio::time::Instant::now() < deadline {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                shutdown.cancel();
            }
        };

        let (mut out, mut err) = (Vec::new(), Vec::new());
        let (result, ()) = tokio::join!(
            run_with_sink(
                registry,
                Some(state_dir),
                Arc::clone(&sink),
                shutdown.clone(),
                CancellationToken::new(),
                &mut out,
                &mut err,
            ),
            watcher
        );
        let report = result?;
        if sink.delivered() < at_least {
            return Err(miette::miette!(
                "only {} of {at_least} records were produced within 30 s",
                sink.delivered()
            ));
        }
        let stub = report
            .plugins
            .get("stub")
            .ok_or_else(|| miette::miette!("no report for stub"))?;
        if let Some(reason) = &stub.disabled {
            return Err(miette::miette!("stub was disabled: {reason}"));
        }
        Ok(sink.delivered())
    }

    /// CLI-04 on a real cluster: the command-level run produces the real stub's
    /// records to a scratch topic, and a stop and restart on the same state
    /// directory continues the sequence with no gap and no repeat. Only a
    /// scratch topic is ever named; the production sink is never connected.
    #[tokio::test]
    #[ignore = "needs a local Fluvio cluster and plugin fixtures: see plugins/README.md"]
    async fn live_run_with_sink_produces_to_a_scratch_topic_and_restart_resumes()
    -> miette::Result<()> {
        use futures_util::{FutureExt, StreamExt};

        let topic = scratch_topic("run").await?;
        let body = std::panic::AssertUnwindSafe(async {
            let tmp = TempDir::new().map_err(|e| miette::miette!("tempdir: {e}"))?;
            let mut stub = source("stub", fixture_wasm("stub-source", "stub_source.wasm"), true);
            stub.interval_secs = 1;
            let registry = load_test_registry(&write_registry(tmp.path(), &[stub]));
            let state_dir = tmp.path().join(".fili-state");

            let n1 = run_until_delivered(&registry, &state_dir, &topic, 2).await?;
            let n2 = run_until_delivered(&registry, &state_dir, &topic, 1).await?;
            assert!(n1 >= 2, "run 1 produced {n1}");
            assert!(n2 >= 1, "run 2 produced {n2}");
            let total = n1 + n2;

            let stream = FluvioStream::connect().await?;
            let mut records = stream
                .topic_stream(&topic, fluvio::Offset::beginning(), Vec::new())
                .await?;
            let mut ids = Vec::new();
            for _ in 0..total {
                let got = tokio::time::timeout(Duration::from_secs(15), records.next())
                    .await
                    .map_err(|_| miette::miette!("timed out reading back {total} records"))?
                    .ok_or_else(|| miette::miette!("stream ended early"))?
                    .map_err(|e| miette::miette!("stream error: {e:?}"))?;
                let item: SourceItem =
                    serde_json::from_slice(got.value()).expect("value is a SourceItem");
                assert_eq!(item.platform, "stub");
                let key = got.key().map(|k| String::from_utf8_lossy(k).into_owned());
                assert_eq!(key, Some(format!("stub:{}", item.id)));
                ids.push(item.id);
            }

            // No gap and no repeat across the restart.
            let expected: Vec<String> = (0..total).map(|n| format!("stub-{n}")).collect();
            assert_eq!(ids, expected, "n1 = {n1}, n2 = {n2}");
            Ok::<(), miette::Report>(())
        })
        .catch_unwind()
        .await;
        let deleted = delete_topic(&topic).await;
        match body {
            Ok(outcome) => {
                outcome?;
                deleted
            }
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
}
