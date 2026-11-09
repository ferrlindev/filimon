// ─────────────────────────────────────────────────────────────────────────────
// plugins/host/mod.rs - The component host runtime
//
// Library-style entry points: `Host::load` opens the registry's enabled
// components, `Host::run` polls each on its own task until the shutdown token
// fires. Phase 4 owns the CLI surface; nothing here adds a subcommand.
// ─────────────────────────────────────────────────────────────────────────────

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

use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use self::cursor::CursorStore;
use self::engine::HostEngine;
use self::error::HostError;
use self::instance::WasmtimeFactory;
use self::loader::{LoadFailure, load_components};
use self::scheduler::{PluginReport, PluginSpec, run_plugin};
use self::sink::ItemSink;
use crate::plugins::config::LoadedRegistry;

/// The state directory next to the registry file (D-06).
pub const DEFAULT_STATE_DIR_NAME: &str = ".fili-state";

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
        })
    }

    /// Runs one task per loaded source until `shutdown` is cancelled or the
    /// task disables itself. A panic in one task is reported for that plugin
    /// only.
    pub async fn run<S: ItemSink>(self, sink: Arc<S>, shutdown: CancellationToken) -> HostReport {
        let Self {
            runtime,
            cursors,
            plugins,
            loaded_sources,
            loaded_fetchers,
            load_failures,
        } = self;

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
        while let Some(joined) = tasks.join_next_with_id().await {
            match joined {
                Ok((id, report)) => {
                    if let Some(name) = names.remove(&id) {
                        reports.insert(name, report);
                    }
                }
                Err(e) => {
                    if let Some(name) = names.remove(&e.id()) {
                        tracing::error!(plugin = %name, "plugin task panicked");
                        reports.insert(
                            name,
                            PluginReport {
                                disabled: Some("plugin task panicked".to_string()),
                                ..PluginReport::default()
                            },
                        );
                    }
                }
            }
        }

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

#[cfg(test)]
mod tests {
    use super::*;

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
