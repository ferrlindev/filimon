// ─────────────────────────────────────────────────────────────────────────────
// plugins/host/wasm_tests.rs - Host tests against the prebuilt wasm fixtures
//
// Ignored by default: they need the stub components built per
// plugins/README.md. Time is paused, so the poll interval costs nothing.
// ─────────────────────────────────────────────────────────────────────────────

use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

use super::sink::{ItemSink, SinkError, SinkRecord, VecSink};
use super::testing::{LogCapture, TestSource, fixture_wasm, load_test_registry, write_registry};
use super::{Host, HostOptions, HostReport};
use crate::models::SourceType;

const GREETING: &str = "hello from the stub source";

fn stub_source(interval_secs: u64) -> TestSource {
    TestSource {
        name: "stub".to_string(),
        component: fixture_wasm("stub-source", "stub_source.wasm"),
        enabled: true,
        interval_secs,
        backoff: None,
        settings: format!("greeting = \"{GREETING}\""),
    }
}

#[tokio::test(start_paused = true)]
#[ignore = "needs plugin fixtures: see plugins/README.md"]
async fn wasm_tracer_stub_polls_and_persists_cursor() {
    let tmp = TempDir::new().expect("tempdir");
    let registry_path = write_registry(tmp.path(), &[stub_source(60)]);
    let registry = load_test_registry(&registry_path);
    let state_dir = tmp.path().join(".fili-state");
    let (logs, _guard) = LogCapture::start();

    let host = Host::load(
        &registry,
        HostOptions {
            state_dir: state_dir.clone(),
        },
    )
    .expect("host loads");

    let sink = Arc::new(VecSink::default());
    let token = CancellationToken::new();
    let handle = tokio::spawn(host.run(Arc::clone(&sink), token.clone()));
    tokio::time::sleep(Duration::from_secs(1)).await;
    token.cancel();
    let report = handle.await.expect("host task");

    assert_eq!(report.loaded_sources, vec!["stub".to_string()]);
    assert!(report.load_failures.is_empty(), "{:?}", report.load_failures);
    let plugin = &report.plugins["stub"];
    assert_eq!(plugin.polls, 1);
    assert_eq!(plugin.items_sent, 1);
    assert_eq!(plugin.disabled, None);

    let records = sink.records();
    assert_eq!(records.len(), 1);
    let record = &records[0];
    assert_eq!(record.key, "stub:stub-0");
    assert_eq!(record.item.source_type, SourceType::Social);
    assert_eq!(record.item.platform, "stub");
    assert_eq!(
        record.item.published.as_deref(),
        Some("2026-01-01T00:00:00.000Z")
    );
    assert_eq!(record.item.raw, Some(serde_json::json!({"n": 0})));

    let state_text =
        std::fs::read_to_string(state_dir.join("stub.json")).expect("state file exists");
    let state: serde_json::Value = serde_json::from_str(&state_text).expect("state json");
    assert_eq!(state["version"], 1);
    assert_eq!(state["plugin"], "stub");
    assert_eq!(state["cursor"], "AQ==");

    let captured = logs.text();
    assert!(captured.contains("plugin=stub"), "{captured}");
    assert!(captured.contains(GREETING), "{captured}");
}

#[tokio::test(start_paused = true)]
#[ignore = "needs plugin fixtures: see plugins/README.md"]
async fn wasm_restart_resumes_from_persisted_cursor() {
    use base64::Engine as _;

    let tmp = TempDir::new().expect("tempdir");
    let registry_path = write_registry(tmp.path(), &[stub_source(60)]);
    let registry = load_test_registry(&registry_path);
    let state_dir = tmp.path().join(".fili-state");

    // Run 1: polls land at t=0, 60 and 120.
    {
        let host = Host::load(
            &registry,
            HostOptions {
                state_dir: state_dir.clone(),
            },
        )
        .expect("host loads");
        let sink = Arc::new(VecSink::default());
        let token = CancellationToken::new();
        let handle = tokio::spawn(host.run(Arc::clone(&sink), token.clone()));
        tokio::time::sleep(Duration::from_secs(150)).await;
        token.cancel();
        let report = handle.await.expect("host task");

        let ids: Vec<String> = sink.records().into_iter().map(|r| r.item.id).collect();
        assert_eq!(ids, vec!["stub-0", "stub-1", "stub-2"]);
        assert_eq!(report.plugins["stub"].polls, 3);
        assert_eq!(report.plugins["stub"].disabled, None);

        let text = std::fs::read_to_string(state_dir.join("stub.json")).expect("state file");
        let state: serde_json::Value = serde_json::from_str(&text).expect("state json");
        let cursor = base64::engine::general_purpose::STANDARD
            .decode(state["cursor"].as_str().expect("cursor is a string"))
            .expect("cursor is base64");
        assert_eq!(cursor, vec![3]);
    }

    // Run 2: a fresh host on the same state directory resumes at stub-3.
    let host = Host::load(&registry, HostOptions { state_dir }).expect("host reloads");
    let sink = Arc::new(VecSink::default());
    let token = CancellationToken::new();
    let handle = tokio::spawn(host.run(Arc::clone(&sink), token.clone()));
    tokio::time::sleep(Duration::from_secs(1)).await;
    token.cancel();
    let report = handle.await.expect("host task");

    let records = sink.records();
    assert_eq!(records.first().map(|r| r.item.id.as_str()), Some("stub-3"));
    assert_eq!(report.plugins["stub"].polls, 1);
}

// ── Fault-Injection Tests ─────────────────────────────────────────────────────
//
// The `fault-source` fixture misbehaves on purpose (settings `mode`,
// `after_polls`, `retry_after_secs`). These tests show the scheduler's rules
// hold on real wasm: isolation, backoff, disabling and anti-wedge.

/// A fault-source plugin. `settings` is the text placed under `[settings]`.
fn fault_source(
    name: &str,
    interval_secs: u64,
    backoff: Option<(u64, u64, f64)>,
    settings: &str,
) -> TestSource {
    TestSource {
        name: name.to_string(),
        component: fixture_wasm("fault-source", "fault_source.wasm"),
        enabled: true,
        interval_secs,
        backoff,
        settings: settings.to_string(),
    }
}

/// A stub-source plugin under another alias.
fn stub_named(name: &str, interval_secs: u64) -> TestSource {
    TestSource {
        name: name.to_string(),
        ..stub_source(interval_secs)
    }
}

/// Loads a registry of `sources` into a fresh host.
fn load_host(tmp: &TempDir, sources: &[TestSource]) -> Host {
    let registry_path = write_registry(tmp.path(), sources);
    let registry = load_test_registry(&registry_path);
    Host::load(
        &registry,
        HostOptions {
            state_dir: tmp.path().join(".fili-state"),
        },
    )
    .expect("host loads")
}

/// Runs `host` for `duration`, then cancels and joins.
async fn run_host<S: ItemSink>(host: Host, sink: Arc<S>, duration: Duration) -> HostReport {
    let token = CancellationToken::new();
    let handle = tokio::spawn(host.run(sink, token.clone()));
    tokio::time::sleep(duration).await;
    token.cancel();
    handle.await.expect("host task")
}

fn ids_of(sink: &VecSink, plugin: &str) -> Vec<String> {
    sink.plugin_records(plugin)
        .into_iter()
        .map(|r| r.item.id)
        .collect()
}

/// Delegates to a `VecSink`, but panics inside `send_batch` for one plugin.
struct PanickingSink {
    panic_for: String,
    inner: VecSink,
}

impl ItemSink for PanickingSink {
    async fn send_batch(&self, plugin: &str, records: Vec<SinkRecord>) -> Result<(), SinkError> {
        assert!(plugin != self.panic_for, "injected sink panic for {plugin}");
        self.inner.send_batch(plugin, records).await
    }
}

#[tokio::test(start_paused = true)]
#[ignore = "needs plugin fixtures: see plugins/README.md"]
async fn wasm_trap_is_isolated_logged_and_instance_rebuilt() {
    let tmp = TempDir::new().expect("tempdir");
    let host = load_host(
        &tmp,
        &[
            stub_source(60),
            fault_source(
                "trapper",
                60,
                Some((10, 100, 2.0)),
                "mode = \"trap\"\nafter_polls = 1",
            ),
        ],
    );
    let (logs, _guard) = LogCapture::start();

    let sink = Arc::new(VecSink::default());
    let report = run_host(host, Arc::clone(&sink), Duration::from_secs(150)).await;

    // The trapper is polled OK at 0 s, traps at 60 s, is rebuilt after the
    // 10 s backoff (70 s) and its per-instance counter restarts, so `f1`
    // appears again; the cycle repeats at 140 s.
    let trapper_ids = ids_of(&sink, "trapper");
    assert!(
        trapper_ids.iter().filter(|id| id.as_str() == "f1").count() >= 2,
        "{trapper_ids:?}"
    );
    assert!(trapper_ids.iter().all(|id| id == "f1"), "{trapper_ids:?}");
    let trapper = &report.plugins["trapper"];
    assert!(trapper.failures >= 2, "{trapper:?}");
    assert_eq!(trapper.disabled, None);

    // The neighbour keeps its own schedule.
    assert_eq!(ids_of(&sink, "stub"), vec!["stub-0", "stub-1", "stub-2"]);
    let stub = &report.plugins["stub"];
    assert_eq!(stub.polls, 3);
    assert_eq!(stub.disabled, None);

    let captured = logs.text();
    assert!(captured.contains("plugin=trapper"), "{captured}");
    assert!(captured.to_lowercase().contains("trap"), "{captured}");
    assert!(!captured.contains('\u{1b}'), "{captured:?}");
}

#[tokio::test(start_paused = true)]
#[ignore = "needs plugin fixtures: see plugins/README.md"]
async fn wasm_fatal_disables_only_that_plugin() {
    let tmp = TempDir::new().expect("tempdir");
    let host = load_host(
        &tmp,
        &[
            stub_source(60),
            fault_source("bad", 60, None, "mode = \"fatal\""),
        ],
    );
    let (logs, _guard) = LogCapture::start();

    let sink = Arc::new(VecSink::default());
    let report = run_host(host, Arc::clone(&sink), Duration::from_secs(150)).await;

    let bad = &report.plugins["bad"];
    let reason = bad.disabled.as_deref().expect("bad is disabled");
    assert!(reason.contains("fatal"), "{reason}");
    assert_eq!(bad.polls, 0);

    let stub = &report.plugins["stub"];
    assert_eq!(stub.polls, 3);
    assert_eq!(stub.disabled, None);

    let captured = logs.text();
    assert!(captured.contains("plugin=bad"), "{captured}");
    assert!(
        captured.contains("disabled until fili restarts"),
        "{captured}"
    );
}

#[tokio::test(start_paused = true)]
#[ignore = "needs plugin fixtures: see plugins/README.md"]
async fn wasm_rate_limited_is_backed_off_with_retry_after_not_disabled() {
    let tmp = TempDir::new().expect("tempdir");
    let host = load_host(
        &tmp,
        &[fault_source(
            "limited",
            60,
            Some((10, 3600, 2.0)),
            "mode = \"rate-limited\"\nretry_after_secs = 25",
        )],
    );

    let sink = Arc::new(VecSink::default());
    let report = run_host(host, sink, Duration::from_secs(60)).await;

    // retry-after 25 against steps 10, 20, 40 gives waits 25, 25, 40: polls
    // land at 0, 25 and 50 s.
    let limited = &report.plugins["limited"];
    assert_eq!(limited.failures, 3, "{limited:?}");
    assert_eq!(limited.disabled, None);
}

#[tokio::test(start_paused = true)]
#[ignore = "needs plugin fixtures: see plugins/README.md"]
async fn wasm_transient_and_auth_required_are_backed_off() {
    let tmp = TempDir::new().expect("tempdir");
    let host = load_host(
        &tmp,
        &[
            fault_source("t", 60, Some((10, 3600, 2.0)), "mode = \"transient\""),
            fault_source("a", 60, Some((10, 3600, 2.0)), "mode = \"auth-required\""),
        ],
    );

    let sink = Arc::new(VecSink::default());
    let report = run_host(host, sink, Duration::from_secs(100)).await;

    // Polls at 0, 10, 30 and 70 s (waits 10, 20, 40, 80).
    for name in ["t", "a"] {
        let plugin = &report.plugins[name];
        assert_eq!(plugin.failures, 4, "{name}: {plugin:?}");
        assert_eq!(plugin.disabled, None, "{name}");
        assert_eq!(plugin.polls, 0, "{name}");
    }
}

#[tokio::test(start_paused = true)]
#[ignore = "needs plugin fixtures: see plugins/README.md"]
async fn wasm_init_fatal_disables_plugin_before_any_poll() {
    let tmp = TempDir::new().expect("tempdir");
    let host = load_host(
        &tmp,
        &[
            stub_source(60),
            fault_source("doomed", 60, None, "mode = \"init-fatal\""),
        ],
    );

    let sink = Arc::new(VecSink::default());
    let report = run_host(host, Arc::clone(&sink), Duration::from_secs(150)).await;

    let doomed = &report.plugins["doomed"];
    assert!(doomed.disabled.is_some(), "{doomed:?}");
    assert_eq!(doomed.polls, 0);
    assert_eq!(report.plugins["stub"].polls, 3);
    assert_eq!(report.plugins["stub"].disabled, None);
}

#[tokio::test(start_paused = true)]
#[ignore = "needs plugin fixtures: see plugins/README.md"]
async fn wasm_bad_settings_disable_with_a_config_error() {
    let tmp = TempDir::new().expect("tempdir");
    let host = load_host(
        &tmp,
        &[
            stub_source(60),
            fault_source("picky", 60, None, "mode = \"ok\"\nbogus = 1"),
        ],
    );

    let sink = Arc::new(VecSink::default());
    let report = run_host(host, Arc::clone(&sink), Duration::from_secs(150)).await;

    let picky = &report.plugins["picky"];
    let reason = picky.disabled.as_deref().expect("picky is disabled");
    assert!(reason.contains("config"), "{reason}");
    assert_eq!(picky.polls, 0);
    assert_eq!(report.plugins["stub"].polls, 3);
}

#[tokio::test(start_paused = true)]
#[ignore = "needs plugin fixtures: see plugins/README.md"]
async fn wasm_host_task_panic_is_isolated_and_reported() {
    let tmp = TempDir::new().expect("tempdir");
    let host = load_host(&tmp, &[stub_named("good", 60), stub_named("bad", 60)]);
    let (logs, _guard) = LogCapture::start();

    let sink = Arc::new(PanickingSink {
        panic_for: "bad".to_string(),
        inner: VecSink::default(),
    });
    // `run` returning at all is the proof that a task panic does not propagate.
    let report = run_host(host, Arc::clone(&sink), Duration::from_secs(150)).await;

    let bad = &report.plugins["bad"];
    assert_eq!(bad.disabled.as_deref(), Some("plugin task panicked"));
    let good = &report.plugins["good"];
    assert_eq!(good.polls, 3);
    assert_eq!(good.disabled, None);
    assert_eq!(
        ids_of(&sink.inner, "good"),
        vec!["stub-0", "stub-1", "stub-2"]
    );

    let captured = logs.text();
    assert!(captured.contains("plugin=bad"), "{captured}");
    assert!(captured.contains("ERROR"), "{captured}");
}

/// Real clock on purpose: a guest that never finishes keeps the runtime busy,
/// so tokio's paused clock would never auto-advance.
#[tokio::test]
#[ignore = "needs plugin fixtures: see plugins/README.md"]
async fn wasm_spinning_guest_is_cancelled_on_shutdown_and_does_not_block_other_plugins() {
    let tmp = TempDir::new().expect("tempdir");
    let host = load_host(
        &tmp,
        &[
            stub_source(1),
            fault_source("spinner", 1, None, "mode = \"spin\""),
        ],
    );

    let sink = Arc::new(VecSink::default());
    let token = CancellationToken::new();
    let handle = tokio::spawn(host.run(Arc::clone(&sink), token.clone()));
    tokio::time::sleep(Duration::from_millis(2500)).await;

    // The spinner is still inside its first poll; the stub kept its schedule.
    let seen = ids_of(&sink, "stub").len();
    assert!(seen >= 2, "stub polled {seen} times beside the spinner");

    token.cancel();
    let report = tokio::time::timeout(Duration::from_secs(10), handle)
        .await
        .expect("Host::run returns within 10 s of cancellation")
        .expect("host task");

    let spinner = &report.plugins["spinner"];
    assert_eq!(spinner.polls, 0, "{spinner:?}");
    assert_eq!(spinner.disabled, None);
    assert!(report.plugins["stub"].polls >= 2);
    assert_eq!(report.plugins["stub"].disabled, None);
}
