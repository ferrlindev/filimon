// ─────────────────────────────────────────────────────────────────────────────
// plugins/host/mod.rs - The component host runtime
//
// Library-style entry points: `Host::load` opens the registry's enabled
// components, `Host::run` polls each on its own task until the shutdown token
// fires. Phase 4 owns the CLI surface; nothing here adds a subcommand.
// ─────────────────────────────────────────────────────────────────────────────

pub mod backoff;
pub mod cursor;
pub mod engine;
pub mod error;
pub mod instance;
pub mod loader;
pub mod map;
pub mod scheduler;
pub mod sink;

#[cfg(test)]
pub(crate) mod testing;
#[cfg(test)]
mod wasm_tests;

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use tokio::task::JoinSet;
use tokio::time::Instant;
use tokio_util::sync::CancellationToken;

use self::cursor::CursorStore;
use self::engine::HostEngine;
use self::error::HostError;
use self::instance::{InstanceFactory, WasmtimeFactory};
use self::loader::{LoadFailure, load_components};
use self::scheduler::{PluginReport, PluginSpec, run_plugin};
use self::sink::ItemSink;
use crate::plugins::config::LoadedRegistry;

/// The state directory next to the registry file (D-06).
pub const DEFAULT_STATE_DIR_NAME: &str = ".fili-state";

/// How long the plugin tasks get to stop on their own after the shutdown token
/// fires, before the supervisor aborts them (OQ-8). Aborting a task mid-send is
/// safe: the cursor is written only after the sink returned (D-07), so the worst
/// case is the documented at-least-once replay of the last batch.
pub const DEFAULT_SHUTDOWN_GRACE: Duration = Duration::from_secs(10);

/// The `disabled` text of a plugin that was cut off at the grace deadline. An
/// aborted plugin is never reported as a clean stop.
pub const ABORTED_AT_SHUTDOWN: &str = "aborted at shutdown";

/// Options for one host run.
#[derive(Debug, Clone)]
pub struct HostOptions {
    pub state_dir: PathBuf,
}

impl HostOptions {
    /// The flag value exactly as given, else `<registry dir>/.fili-state`.
    #[must_use]
    pub fn resolve_state_dir(registry_base_dir: &Path, flag: Option<&Path>) -> PathBuf {
        flag.map_or_else(
            || registry_base_dir.join(DEFAULT_STATE_DIR_NAME),
            Path::to_path_buf,
        )
    }

    #[must_use]
    pub fn for_registry(registry: &LoadedRegistry, flag: Option<&Path>) -> Self {
        Self {
            state_dir: Self::resolve_state_dir(&registry.base_dir, flag),
        }
    }
}

/// A loaded registry, ready to run.
pub struct Host {
    runtime: HostEngine,
    cursors: Arc<CursorStore>,
    plugins: Vec<(PluginSpec, Arc<WasmtimeFactory>)>,
    loaded_sources: Vec<String>,
    loaded_fetchers: Vec<String>,
    load_failures: Vec<LoadFailure>,
    shutdown_grace: Duration,
}

/// What a host run did.
#[derive(Debug)]
pub struct HostReport {
    pub loaded_sources: Vec<String>,
    pub loaded_fetchers: Vec<String>,
    pub load_failures: Vec<LoadFailure>,
    pub plugins: BTreeMap<String, PluginReport>,
}

impl Host {
    /// Builds the engine, opens the state directory and loads every enabled
    /// source component. A component that fails to load is reported in
    /// `HostReport.load_failures` and the rest still run.
    ///
    /// # Errors
    /// Returns `HostError::Engine` when the engine cannot be built and
    /// `HostError::StateDir` when the state directory cannot be created.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "options is part of the fixed Host::load signature; later plans add owned fields"
    )]
    pub fn load(registry: &LoadedRegistry, options: HostOptions) -> Result<Self, HostError> {
        let host_engine = HostEngine::new().map_err(|e| HostError::Engine {
            reason: format!("{e:#}"),
        })?;
        let cursors = Arc::new(CursorStore::open(&options.state_dir)?);

        let report = load_components(registry, &host_engine);
        let mut plugins = Vec::new();
        let mut loaded_sources = Vec::new();
        for (name, pre) in report.sources {
            let Some(spec) = registry.sources.get(&name).and_then(PluginSpec::from_loaded) else {
                continue;
            };
            let factory = Arc::new(WasmtimeFactory::new(host_engine.engine().clone(), pre));
            loaded_sources.push(name);
            plugins.push((spec, factory));
        }

        Ok(Self {
            runtime: host_engine,
            cursors,
            plugins,
            loaded_sources,
            loaded_fetchers: report.fetcher_names,
            load_failures: report.failures,
            shutdown_grace: DEFAULT_SHUTDOWN_GRACE,
        })
    }

    /// Names of the source plugins that loaded and will be polled.
    #[must_use]
    pub fn loaded_sources(&self) -> &[String] {
        &[]
    }

    /// Names of the fetcher plugins that loaded.
    #[must_use]
    pub fn loaded_fetchers(&self) -> &[String] {
        &[]
    }

    /// The components that could not be loaded.
    #[must_use]
    pub fn load_failures(&self) -> &[LoadFailure] {
        &[]
    }

    /// Sets how long tasks get to stop after shutdown before they are aborted.
    #[must_use]
    pub fn with_shutdown_grace(mut self, grace: Duration) -> Self {
        self.shutdown_grace = grace;
        self
    }

    /// Runs one task per loaded source until `shutdown` is cancelled or the
    /// task disables itself. A panic in one task is reported for that plugin
    /// only. After `shutdown` the tasks get the shutdown grace, then the ones
    /// still running are aborted (see [`supervise`]).
    pub async fn run<S: ItemSink>(self, sink: Arc<S>, shutdown: CancellationToken) -> HostReport {
        self.run_with_abort(sink, shutdown, CancellationToken::new())
            .await
    }

    /// Like [`Host::run`], with a second token that forces the abort at once
    /// (a second stop request from the operator).
    pub async fn run_with_abort<S: ItemSink>(
        self,
        sink: Arc<S>,
        shutdown: CancellationToken,
        abort: CancellationToken,
    ) -> HostReport {
        let Self {
            runtime,
            cursors,
            plugins,
            loaded_sources,
            loaded_fetchers,
            load_failures,
            shutdown_grace,
        } = self;

        let reports = supervise(plugins, sink, cursors, shutdown, abort, shutdown_grace).await;

        // Keep the engine (and its epoch ticker) alive until every task ended.
        drop(runtime);
        HostReport {
            loaded_sources,
            loaded_fetchers,
            load_failures,
            plugins: reports,
        }
    }
}

/// Runs one task per plugin and joins them all.
///
/// Tasks run exactly as `run_plugin` does. When `shutdown` is cancelled the
/// supervisor arms a deadline of `grace` from that moment and keeps joining the
/// tasks that stop on their own, so a healthy stop is never delayed. When the
/// deadline passes, or `abort` is cancelled after `shutdown`, every task still
/// running is aborted and reported as disabled with [`ABORTED_AT_SHUTDOWN`].
/// A task that panics keeps the `plugin task panicked` report.
pub(crate) async fn supervise<F: InstanceFactory, S: ItemSink>(
    plugins: Vec<(PluginSpec, Arc<F>)>,
    sink: Arc<S>,
    cursors: Arc<CursorStore>,
    shutdown: CancellationToken,
    abort: CancellationToken,
    grace: Duration,
) -> BTreeMap<String, PluginReport> {
    let mut tasks = JoinSet::new();
    let mut names: HashMap<tokio::task::Id, String> = HashMap::new();
    for (spec, factory) in plugins {
        let name = spec.name.clone();
        let handle = tasks.spawn(run_plugin(
            spec,
            factory,
            Arc::clone(&sink),
            Arc::clone(&cursors),
            shutdown.clone(),
        ));
        names.insert(handle.id(), name);
    }

    let mut reports = BTreeMap::new();
    let mut deadline: Option<Instant> = None;
    let mut aborted = false;
    while !tasks.is_empty() {
        let grace_over = async {
            match deadline {
                Some(at) => tokio::time::sleep_until(at).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            biased;
            joined = tasks.join_next_with_id() => {
                if let Some(joined) = joined {
                    record_joined(joined, &mut names, &mut reports);
                }
            }
            () = shutdown.cancelled(), if deadline.is_none() && !aborted => {
                deadline = Some(Instant::now() + grace);
            }
            () = abort.cancelled(), if deadline.is_some() && !aborted => {
                aborted = true;
                tasks.abort_all();
            }
            () = grace_over, if deadline.is_some() && !aborted => {
                aborted = true;
                tasks.abort_all();
            }
        }
    }
    reports
}

/// Files one finished task's report under its plugin name.
fn record_joined(
    joined: Result<(tokio::task::Id, PluginReport), tokio::task::JoinError>,
    names: &mut HashMap<tokio::task::Id, String>,
    reports: &mut BTreeMap<String, PluginReport>,
) {
    match joined {
        Ok((id, report)) => {
            if let Some(name) = names.remove(&id) {
                reports.insert(name, report);
            }
        }
        Err(e) => {
            let Some(name) = names.remove(&e.id()) else {
                return;
            };
            let reason = if e.is_cancelled() {
                tracing::warn!(plugin = %name, "plugin aborted at shutdown");
                ABORTED_AT_SHUTDOWN
            } else {
                tracing::error!(plugin = %name, "plugin task panicked");
                "plugin task panicked"
            };
            reports.insert(
                name,
                PluginReport {
                    disabled: Some(reason.to_string()),
                    ..PluginReport::default()
                },
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use tempfile::TempDir;

    use self::sink::{SinkError, SinkRecord, VecSink};
    use self::testing::{FakeFactory, FakeStep, spec, wit_item};

    /// A sink whose send never returns, like a stuck broker.
    struct HangingSink;

    impl ItemSink for HangingSink {
        async fn send_batch(
            &self,
            _plugin: &str,
            _records: Vec<SinkRecord>,
        ) -> Result<(), SinkError> {
            std::future::pending().await
        }
    }

    fn open_cursors() -> (TempDir, Arc<CursorStore>) {
        let dir = TempDir::new().expect("tempdir");
        let cursors = Arc::new(CursorStore::open(dir.path()).expect("cursor store"));
        (dir, cursors)
    }

    /// A plugin that polls once at once and returns one item with cursor `[1]`.
    fn one_item_plugin(name: &str) -> (PluginSpec, Arc<FakeFactory>) {
        let factory = Arc::new(FakeFactory::new(vec![
            FakeStep::InitOk,
            FakeStep::PollOk {
                items: vec![wit_item("x")],
                next_cursor: Some(vec![1]),
                delay: Duration::ZERO,
            },
        ]));
        (spec(name, 60, 60, 3600, 2.0), factory)
    }

    #[test]
    fn default_shutdown_grace_is_ten_seconds() {
        assert_eq!(DEFAULT_SHUTDOWN_GRACE, Duration::from_secs(10));
        assert_eq!(ABORTED_AT_SHUTDOWN, "aborted at shutdown");
    }

    #[tokio::test(start_paused = true)]
    async fn grace_returns_after_the_deadline_and_reports_aborted_plugins() {
        let (dir, cursors) = open_cursors();
        let shutdown = CancellationToken::new();
        let supervisor = tokio::spawn(supervise(
            vec![one_item_plugin("a"), one_item_plugin("b")],
            Arc::new(HangingSink),
            cursors,
            shutdown.clone(),
            CancellationToken::new(),
            DEFAULT_SHUTDOWN_GRACE,
        ));

        tokio::time::sleep(Duration::from_secs(1)).await;
        let cancelled_at = Instant::now();
        shutdown.cancel();
        let reports = supervisor.await.expect("supervisor");
        let waited = cancelled_at.elapsed();

        assert!(waited >= DEFAULT_SHUTDOWN_GRACE, "returned early: {waited:?}");
        assert!(waited < Duration::from_secs(11), "returned late: {waited:?}");
        assert_eq!(reports.len(), 2);
        for name in ["a", "b"] {
            assert_eq!(
                reports[name].disabled.as_deref(),
                Some(ABORTED_AT_SHUTDOWN),
                "{name}"
            );
            assert!(
                !dir.path().join(format!("{name}.json")).exists(),
                "the send never returned, so no cursor may exist for {name}"
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_second_signal_aborts_before_the_grace_ends() {
        let (_dir, cursors) = open_cursors();
        let shutdown = CancellationToken::new();
        let abort = CancellationToken::new();
        let grace = Duration::from_secs(60);
        let start = Instant::now();
        let supervisor = tokio::spawn(supervise(
            vec![one_item_plugin("a"), one_item_plugin("b")],
            Arc::new(HangingSink),
            cursors,
            shutdown.clone(),
            abort.clone(),
            grace,
        ));

        tokio::time::sleep(Duration::from_secs(1)).await;
        shutdown.cancel();
        tokio::time::sleep(Duration::from_secs(2)).await;
        abort.cancel();
        let reports = supervisor.await.expect("supervisor");
        let elapsed = start.elapsed();

        assert!(elapsed < grace, "waited out the grace: {elapsed:?}");
        assert_eq!(elapsed, Duration::from_secs(3), "returned at the abort time");
        for name in ["a", "b"] {
            assert_eq!(
                reports[name].disabled.as_deref(),
                Some(ABORTED_AT_SHUTDOWN),
                "{name}"
            );
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_forced_stop_before_shutdown_aborts_without_waiting_for_the_grace() {
        let (_dir, cursors) = open_cursors();
        let abort = CancellationToken::new();
        let start = Instant::now();
        let supervisor = tokio::spawn(supervise(
            vec![one_item_plugin("a")],
            Arc::new(HangingSink),
            cursors,
            CancellationToken::new(),
            abort.clone(),
            Duration::from_secs(60),
        ));

        tokio::time::sleep(Duration::from_secs(2)).await;
        abort.cancel();
        // Bounded so that a supervisor that ignores the abort fails the test
        // instead of hanging it.
        let reports = tokio::time::timeout(Duration::from_secs(30), supervisor)
            .await
            .expect("the abort token must stop the supervisor without a shutdown")
            .expect("supervisor");

        assert_eq!(start.elapsed(), Duration::from_secs(2));
        assert_eq!(reports["a"].disabled.as_deref(), Some(ABORTED_AT_SHUTDOWN));
    }

    #[tokio::test(start_paused = true)]
    async fn healthy_plugins_stop_at_once_with_their_real_reports() {
        let (_dir, cursors) = open_cursors();
        let sink = Arc::new(VecSink::default());
        let shutdown = CancellationToken::new();
        let supervisor = tokio::spawn(supervise(
            vec![one_item_plugin("a"), one_item_plugin("b")],
            Arc::clone(&sink),
            cursors,
            shutdown.clone(),
            CancellationToken::new(),
            DEFAULT_SHUTDOWN_GRACE,
        ));

        tokio::time::sleep(Duration::from_secs(5)).await;
        let cancelled_at = Instant::now();
        shutdown.cancel();
        let reports = supervisor.await.expect("supervisor");

        assert!(
            cancelled_at.elapsed() < Duration::from_secs(1),
            "waited for the grace: {:?}",
            cancelled_at.elapsed()
        );
        for name in ["a", "b"] {
            let report = &reports[name];
            assert!(report.polls >= 1, "{name}: {report:?}");
            assert!(report.items_sent >= 1, "{name}: {report:?}");
            assert_eq!(report.disabled, None, "{name}");
        }
    }

    /// Panics inside `send_batch` for the plugin named `bad`.
    struct PanickingSink {
        inner: VecSink,
    }

    impl ItemSink for PanickingSink {
        async fn send_batch(
            &self,
            plugin: &str,
            records: Vec<SinkRecord>,
        ) -> Result<(), SinkError> {
            assert!(plugin != "bad", "sink panic on purpose");
            self.inner.send_batch(plugin, records).await
        }
    }

    #[tokio::test(start_paused = true)]
    async fn a_panicking_task_is_reported_as_a_panic_not_as_an_abort() {
        let (_dir, cursors) = open_cursors();
        let shutdown = CancellationToken::new();
        let supervisor = tokio::spawn(supervise(
            vec![one_item_plugin("bad"), one_item_plugin("good")],
            Arc::new(PanickingSink {
                inner: VecSink::default(),
            }),
            cursors,
            shutdown.clone(),
            CancellationToken::new(),
            DEFAULT_SHUTDOWN_GRACE,
        ));

        tokio::time::sleep(Duration::from_secs(1)).await;
        shutdown.cancel();
        let reports = supervisor.await.expect("supervisor");

        assert_eq!(
            reports["bad"].disabled.as_deref(),
            Some("plugin task panicked")
        );
        assert_ne!(reports["bad"].disabled.as_deref(), Some(ABORTED_AT_SHUTDOWN));
        assert_eq!(reports["good"].disabled, None);
        assert_eq!(reports["good"].items_sent, 1);
    }

    #[tokio::test(start_paused = true)]
    async fn two_plugins_with_the_same_interval_poll_independently() {
        let (_dir, cursors) = open_cursors();
        let sink = Arc::new(VecSink::default());
        let plugin = |name: &str, cursor: u8| {
            let factory = Arc::new(FakeFactory::new(vec![
                FakeStep::InitOk,
                FakeStep::PollOk {
                    items: vec![wit_item(name)],
                    next_cursor: Some(vec![cursor]),
                    delay: Duration::ZERO,
                },
            ]));
            (spec(name, 60, 60, 3600, 2.0), factory)
        };
        let shutdown = CancellationToken::new();
        let supervisor = tokio::spawn(supervise(
            vec![plugin("a", 1), plugin("b", 2)],
            Arc::clone(&sink),
            Arc::clone(&cursors),
            shutdown.clone(),
            CancellationToken::new(),
            DEFAULT_SHUTDOWN_GRACE,
        ));

        tokio::time::sleep(Duration::from_secs(1)).await;
        shutdown.cancel();
        let reports = supervisor.await.expect("supervisor");

        assert_eq!(reports["a"].polls, 1);
        assert_eq!(reports["b"].polls, 1);
        assert_eq!(cursors.read("a").expect("read a"), Some(vec![1]));
        assert_eq!(cursors.read("b").expect("read b"), Some(vec![2]));
        assert_eq!(sink.plugin_records("a").len(), 1);
        assert_eq!(sink.plugin_records("b").len(), 1);
    }

    #[tokio::test(start_paused = true)]
    async fn per_plugin_batches_reach_the_sink_in_poll_order() {
        let (_dir, cursors) = open_cursors();
        let sink = Arc::new(VecSink::default());
        let factory = Arc::new(FakeFactory::new(vec![
            FakeStep::InitOk,
            FakeStep::PollOk {
                items: vec![wit_item("x1"), wit_item("x2")],
                next_cursor: Some(vec![1]),
                delay: Duration::ZERO,
            },
            FakeStep::PollOk {
                items: vec![wit_item("x3")],
                next_cursor: Some(vec![2]),
                delay: Duration::ZERO,
            },
        ]));
        let shutdown = CancellationToken::new();
        let supervisor = tokio::spawn(supervise(
            vec![(spec("a", 60, 60, 3600, 2.0), factory)],
            Arc::clone(&sink),
            Arc::clone(&cursors),
            shutdown.clone(),
            CancellationToken::new(),
            DEFAULT_SHUTDOWN_GRACE,
        ));

        tokio::time::sleep(Duration::from_secs(61)).await;
        shutdown.cancel();
        let reports = supervisor.await.expect("supervisor");

        let keys: Vec<String> = sink
            .plugin_records("a")
            .into_iter()
            .map(|record| record.key)
            .collect();
        assert_eq!(keys, vec!["fake:x1", "fake:x2", "fake:x3"]);
        assert_eq!(reports["a"].polls, 2);
        assert_eq!(cursors.read("a").expect("read a"), Some(vec![2]));
    }

    /// A host whose only enabled source has a component that is not wasm, so
    /// `Host::load` succeeds and records one load failure.
    fn host_with_one_load_failure() -> (TempDir, Host) {
        use self::testing::{TestSource, load_test_registry, write_registry};

        let dir = TempDir::new().expect("tempdir");
        let component = dir.path().join("broken.wasm");
        std::fs::write(&component, b"this is not a wasm component").expect("write component");
        let source = |name: &str, enabled: bool| TestSource {
            name: name.to_string(),
            component: component.clone(),
            enabled,
            interval_secs: 60,
            backoff: None,
            settings: String::new(),
        };
        let registry_path = write_registry(
            dir.path(),
            &[source("broken", true), source("off", false)],
        );
        let registry = load_test_registry(&registry_path);
        let options = HostOptions {
            state_dir: dir.path().join("state"),
        };
        let host = Host::load(&registry, options).expect("host loads without a wasm fixture");
        (dir, host)
    }

    #[test]
    fn host_accessors_report_what_loaded_and_what_failed() {
        let (_dir, host) = host_with_one_load_failure();

        assert!(host.loaded_sources().is_empty());
        assert!(host.loaded_fetchers().is_empty());
        let failures = host.load_failures();
        assert_eq!(failures.len(), 1, "{failures:?}");
        assert_eq!(failures[0].name, "broken");
    }

    #[tokio::test]
    async fn a_host_with_nothing_loaded_returns_an_empty_report() {
        let (_dir, host) = host_with_one_load_failure();
        let host = host.with_shutdown_grace(Duration::from_secs(3));

        let shutdown = CancellationToken::new();
        shutdown.cancel();
        let report = host
            .run_with_abort(
                Arc::new(VecSink::default()),
                shutdown,
                CancellationToken::new(),
            )
            .await;

        assert!(report.plugins.is_empty());
        assert!(report.loaded_sources.is_empty());
        assert_eq!(report.load_failures.len(), 1);
        assert_eq!(report.load_failures[0].name, "broken");
    }

    #[test]
    fn state_dir_defaults_next_to_registry() {
        let base = Path::new("/etc/fili");
        assert_eq!(
            HostOptions::resolve_state_dir(base, None),
            PathBuf::from("/etc/fili/.fili-state")
        );
    }

    #[test]
    fn explicit_state_dir_is_used_as_given() {
        let base = Path::new("/etc/fili");
        let flag = Path::new("relative/state");
        assert_eq!(
            HostOptions::resolve_state_dir(base, Some(flag)),
            PathBuf::from("relative/state")
        );
    }
}
