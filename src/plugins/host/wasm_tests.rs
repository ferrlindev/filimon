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

use super::sink::VecSink;
use super::testing::{LogCapture, TestSource, fixture_wasm, load_test_registry, write_registry};
use super::{Host, HostOptions};
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
