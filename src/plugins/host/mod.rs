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

    use self::sink::{SinkError, SinkRecord};
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
