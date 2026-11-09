// ─────────────────────────────────────────────────────────────────────────────
// plugins/host/loader.rs - Registry-driven component loading (HOST-01)
//
// Only enabled registry entries with a loaded `PluginConfig` are opened. There
// is no directory listing or scanning anywhere on this path, and every opened
// path is recorded in `LoadReport.opened`. A component that fails to open,
// compile or link is reported and never stops the others.
// ─────────────────────────────────────────────────────────────────────────────

use std::collections::BTreeMap;
use std::path::PathBuf;

use wasmtime::component::Component;

use super::engine::{HostEngine, HostState, SourcePre};
use super::error::HostError;
use crate::plugins::config::LoadedRegistry;

/// Which kind of plugin a load failure belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginKind {
    Source,
    Fetcher,
}

/// One component that could not be loaded.
#[derive(Debug)]
pub struct LoadFailure {
    pub kind: PluginKind,
    pub name: String,
    pub error: HostError,
}

/// The outcome of loading a registry's components.
pub struct LoadReport {
    /// Linked, ready-to-instantiate source components by plugin alias.
    pub sources: BTreeMap<String, SourcePre<HostState>>,
    /// Fetcher components are loaded by a later plan.
    pub fetcher_names: Vec<String>,
    pub failures: Vec<LoadFailure>,
    /// Every component path that was opened, in order.
    pub opened: Vec<PathBuf>,
}

/// Opens every enabled, loaded source component in `registry`.
#[must_use]
pub fn load_components(registry: &LoadedRegistry, host_engine: &HostEngine) -> LoadReport {
    let mut report = LoadReport {
        sources: BTreeMap::new(),
        fetcher_names: Vec::new(),
        failures: Vec::new(),
        opened: Vec::new(),
    };

    for source in registry.sources.values() {
        if !(source.enabled && source.plugin.is_some()) {
            continue;
        }
        report.opened.push(source.component.clone());
        match load_source(source.component.as_path(), host_engine) {
            Ok(pre) => {
                report.sources.insert(source.name.clone(), pre);
            }
            Err(e) => report.failures.push(LoadFailure {
                kind: PluginKind::Source,
                name: source.name.clone(),
                error: HostError::ComponentLoad {
                    plugin: source.name.clone(),
                    path: source.component.display().to_string(),
                    reason: format!("{e:#}"),
                },
            }),
        }
    }
    report
}

fn load_source(
    path: &std::path::Path,
    host_engine: &HostEngine,
) -> wasmtime::Result<SourcePre<HostState>> {
    let component = Component::from_file(host_engine.engine(), path)?;
    let pre = host_engine.source_linker().instantiate_pre(&component)?;
    SourcePre::new(pre)
}
