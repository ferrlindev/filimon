// ─────────────────────────────────────────────────────────────────────────────
// plugins/host/loader.rs - Registry-driven component loading (HOST-01)
//
// Only enabled registry entries are opened (sources also need a loaded
// `PluginConfig`). There is no directory listing or scanning anywhere on this
// path, and every opened path is recorded in `LoadReport.opened`.
//
// Each opened component is preflighted: any import outside the
// `filimon:plugin/` package (WASI included) is rejected with a named
// `HostError::ForeignImports` before instantiation. A component that passes
// preflight but does not fit its world is `HostError::WrongWorld`. A component
// that fails to open, compile or link is reported and never stops the others.
// ─────────────────────────────────────────────────────────────────────────────

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use wasmtime::component::Component;

use super::engine::{FetcherPre, HostEngine, HostState, SourcePre, clean_guest_text};
use super::error::HostError;
use crate::plugins::config::LoadedRegistry;

/// The only import package a plugin may use.
const PLUGIN_PACKAGE_PREFIX: &str = "filimon:plugin/";
/// Longest foreign import name kept in a diagnostic, in characters.
const IMPORT_NAME_CLIP: usize = 128;
/// Most foreign import names listed in one diagnostic.
const IMPORT_LIST_CAP: usize = 16;
/// Longest wasmtime reason kept in a diagnostic, in characters.
const REASON_CLIP: usize = 512;

/// Which kind of plugin a load failure belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginKind {
    Source,
    Fetcher,
}

impl PluginKind {
    /// The WIT world a plugin of this kind must fit.
    #[must_use]
    pub fn world(self) -> &'static str {
        match self {
            Self::Source => "source",
            Self::Fetcher => "fetcher",
        }
    }
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
    /// Linked fetcher components by plugin alias. Nothing routes to them
    /// before Phase 5.
    pub fetchers: BTreeMap<String, FetcherPre<HostState>>,
    /// Names of the fetchers in `fetchers`, in registry order.
    pub fetcher_names: Vec<String>,
    pub failures: Vec<LoadFailure>,
    /// Every component path that was opened, in order.
    pub opened: Vec<PathBuf>,
}

/// Opens every enabled source and every enabled fetcher in `registry`. A
/// failed fetcher does not stop sources (RESEARCH A7).
#[must_use]
pub fn load_components(registry: &LoadedRegistry, host_engine: &HostEngine) -> LoadReport {
    let mut report = LoadReport {
        sources: BTreeMap::new(),
        fetchers: BTreeMap::new(),
        fetcher_names: Vec::new(),
        failures: Vec::new(),
        opened: Vec::new(),
    };

    for source in registry.sources.values() {
        if !(source.enabled && source.plugin.is_some()) {
            continue;
        }
        report.opened.push(source.component.clone());
        match load_source(&source.name, source.component.as_path(), host_engine) {
            Ok(pre) => {
                report.sources.insert(source.name.clone(), pre);
            }
            Err(error) => report.failures.push(LoadFailure {
                kind: PluginKind::Source,
                name: source.name.clone(),
                error,
            }),
        }
    }

    for fetcher in registry.fetchers.values() {
        if !fetcher.enabled {
            continue;
        }
        report.opened.push(fetcher.component.clone());
        match load_fetcher(&fetcher.name, fetcher.component.as_path(), host_engine) {
            Ok(pre) => {
                report.fetchers.insert(fetcher.name.clone(), pre);
                report.fetcher_names.push(fetcher.name.clone());
            }
            Err(error) => report.failures.push(LoadFailure {
                kind: PluginKind::Fetcher,
                name: fetcher.name.clone(),
                error,
            }),
        }
    }
    report
}

/// Opens and compiles a component, then rejects foreign imports.
fn open_component(
    plugin: &str,
    path: &Path,
    host_engine: &HostEngine,
) -> Result<Component, HostError> {
    let component =
        Component::from_file(host_engine.engine(), path).map_err(|e| HostError::ComponentLoad {
            plugin: plugin.to_string(),
            path: path.display().to_string(),
            reason: clean_guest_text(&format!("{e:#}"), REASON_CLIP),
        })?;
    preflight_imports(plugin, path, host_engine.engine(), &component)?;
    Ok(component)
}

fn wrong_world(plugin: &str, kind: PluginKind, path: &Path, error: &wasmtime::Error) -> HostError {
    HostError::WrongWorld {
        plugin: plugin.to_string(),
        kind: kind.world(),
        path: path.display().to_string(),
        // wasmtime's text can echo guest import and export names.
        reason: clean_guest_text(&format!("{error:#}"), REASON_CLIP),
    }
}

pub(crate) fn load_source(
    plugin: &str,
    path: &Path,
    host_engine: &HostEngine,
) -> Result<SourcePre<HostState>, HostError> {
    let component = open_component(plugin, path, host_engine)?;
    let pre = host_engine
        .source_linker()
        .instantiate_pre(&component)
        .and_then(SourcePre::new)
        .map_err(|e| wrong_world(plugin, PluginKind::Source, path, &e))?;
    Ok(pre)
}

fn load_fetcher(
    plugin: &str,
    path: &Path,
    host_engine: &HostEngine,
) -> Result<FetcherPre<HostState>, HostError> {
    let component = open_component(plugin, path, host_engine)?;
    let pre = host_engine
        .fetcher_linker()
        .instantiate_pre(&component)
        .and_then(FetcherPre::new)
        .map_err(|e| wrong_world(plugin, PluginKind::Fetcher, path, &e))?;
    Ok(pre)
}

/// Checks that the component at `path` compiles, imports nothing outside
/// `filimon:plugin/` and fits the world of `kind`, then discards the linked
/// result. Nothing is instantiated and no guest code runs.
///
/// This is the single place the CLI and the host share for "does this
/// component fit its world", so there is no second checker.
///
/// # Errors
/// Returns `ComponentLoad`, `ForeignImports` or `WrongWorld` for the first
/// problem found.
pub fn check_component(
    kind: PluginKind,
    name: &str,
    path: &Path,
    host_engine: &HostEngine,
) -> Result<(), HostError> {
    match kind {
        PluginKind::Source => load_source(name, path, host_engine).map(drop),
        PluginKind::Fetcher => load_fetcher(name, path, host_engine).map(drop),
    }
}

// ── Import Preflight ──────────────────────────────────────────────────────────

/// The names that are not in the `filimon:plugin/` package, in order.
#[must_use]
pub fn foreign_imports(names: &[&str]) -> Vec<String> {
    names
        .iter()
        .filter(|name| !name.starts_with(PLUGIN_PACKAGE_PREFIX))
        .map(|name| (*name).to_string())
        .collect()
}

/// Makes foreign import names printable: each is sanitized and clipped, the
/// list is capped, and a final `…and N more` entry says how many were dropped.
#[must_use]
pub fn describe_foreign(names: &[String]) -> Vec<String> {
    let total = names.len();
    let mut described: Vec<String> = names
        .iter()
        .take(IMPORT_LIST_CAP)
        .map(|name| clean_guest_text(name, IMPORT_NAME_CLIP))
        .collect();
    if total > IMPORT_LIST_CAP {
        described.push(format!("…and {} more", total - IMPORT_LIST_CAP));
    }
    described
}

/// Rejects a component that imports anything outside `filimon:plugin/`,
/// naming every such import. Runs before instantiation so wasmtime's raw
/// "matching implementation was not found in the linker" text never reaches
/// the user.
fn preflight_imports(
    plugin: &str,
    path: &Path,
    engine: &wasmtime::Engine,
    component: &Component,
) -> Result<(), HostError> {
    let component_type = component.component_type();
    let names: Vec<&str> = component_type
        .imports(engine)
        .map(|(name, _)| name)
        .collect();
    let foreign = foreign_imports(&names);
    if foreign.is_empty() {
        return Ok(());
    }
    Err(HostError::ForeignImports {
        plugin: plugin.to_string(),
        path: path.display().to_string(),
        imports: describe_foreign(&foreign),
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::fmt::Write;
    use std::path::Path;

    use miette::Diagnostic;
    use tempfile::TempDir;

    use super::*;
    use crate::plugins::host::testing::{fixture_wasm, load_test_registry};

    /// `(component (import "wasi:cli/environment@0.2.0" (instance)))`, built
    /// with `wasm-tools parse`.
    const WASI_IMPORT_COMPONENT: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, 0x0d, 0x00, 0x01, 0x00, 0x07, 0x03, 0x01, 0x42, 0x00, 0x0a, 0x1f,
        0x01, 0x00, 0x1a, 0x77, 0x61, 0x73, 0x69, 0x3a, 0x63, 0x6c, 0x69, 0x2f, 0x65, 0x6e, 0x76,
        0x69, 0x72, 0x6f, 0x6e, 0x6d, 0x65, 0x6e, 0x74, 0x40, 0x30, 0x2e, 0x32, 0x2e, 0x30, 0x05,
        0x00,
    ];

    const GARBAGE: &[u8] = b"this is not a wasm component";

    /// One registry entry for `write_inline_registry`.
    struct Entry {
        kind: &'static str,
        name: &'static str,
        component: std::path::PathBuf,
        enabled: bool,
    }

    fn source(name: &'static str, component: &Path, enabled: bool) -> Entry {
        Entry {
            kind: "sources",
            name,
            component: component.to_path_buf(),
            enabled,
        }
    }

    fn fetcher(name: &'static str, component: &Path, enabled: bool) -> Entry {
        Entry {
            kind: "fetchers",
            name,
            component: component.to_path_buf(),
            enabled,
        }
    }

    /// Writes `filimon.toml` plus a minimal `<name>.toml` per source.
    fn write_inline_registry(dir: &Path, entries: &[Entry]) -> std::path::PathBuf {
        let mut registry = String::new();
        for entry in entries {
            let _ = write!(
                registry,
                "[{kind}.{name}]\ncomponent = {component:?}\nenabled = {enabled}\n",
                kind = entry.kind,
                name = entry.name,
                component = entry.component.display().to_string(),
                enabled = entry.enabled,
            );
            if entry.kind == "sources" {
                let _ = writeln!(registry, "config = \"{}.toml\"", entry.name);
                std::fs::write(
                    dir.join(format!("{}.toml", entry.name)),
                    "[host]\npoll_interval_secs = 60\n",
                )
                .expect("write plugin toml");
            }
            registry.push('\n');
        }
        let path = dir.join("filimon.toml");
        std::fs::write(&path, registry).expect("write registry");
        path
    }

    fn load(dir: &Path, entries: &[Entry]) -> LoadReport {
        let registry = load_test_registry(&write_inline_registry(dir, entries));
        let host_engine = HostEngine::new().expect("engine");
        load_components(&registry, &host_engine)
    }

    #[test]
    fn unlisted_and_disabled_components_are_never_opened() {
        let tmp = TempDir::new().expect("tempdir");
        let a = tmp.path().join("a.wasm");
        let b = tmp.path().join("b.wasm");
        let c = tmp.path().join("c.wasm");
        for file in [&a, &b, &c] {
            std::fs::write(file, GARBAGE).expect("write garbage");
        }

        let report = load(tmp.path(), &[source("a", &a, true), source("b", &b, false)]);

        assert_eq!(report.opened, vec![a]);
        assert!(!report.opened.contains(&b));
        assert!(!report.opened.contains(&c));
        assert!(report.sources.is_empty());
        assert_eq!(report.failures.len(), 1);
        let failure = &report.failures[0];
        assert_eq!(failure.kind, PluginKind::Source);
        assert_eq!(failure.name, "a");
        assert!(
            matches!(failure.error, HostError::ComponentLoad { .. }),
            "{:?}",
            failure.error
        );
    }

    #[test]
    fn garbage_component_is_a_component_load_failure_not_a_panic() {
        let tmp = TempDir::new().expect("tempdir");
        let bad = tmp.path().join("bad.wasm");
        std::fs::write(&bad, GARBAGE).expect("write garbage");

        let report = load(tmp.path(), &[source("bad", &bad, true)]);

        let failure = report.failures.first().expect("one failure");
        assert!(matches!(failure.error, HostError::ComponentLoad { .. }));
        let text = failure.error.to_string();
        assert!(text.contains("bad"), "{text}");
        assert!(text.contains(&bad.display().to_string()), "{text}");
    }

    #[test]
    fn component_importing_wasi_is_rejected_with_named_imports() {
        let tmp = TempDir::new().expect("tempdir");
        let wasi = tmp.path().join("wasi.wasm");
        std::fs::write(&wasi, WASI_IMPORT_COMPONENT).expect("write component");

        let report = load(tmp.path(), &[source("wasi", &wasi, true)]);

        assert_eq!(report.opened, vec![wasi]);
        assert!(report.sources.is_empty());
        let failure = report.failures.first().expect("one failure");
        let HostError::ForeignImports { imports, .. } = &failure.error else {
            panic!("expected ForeignImports, got {:?}", failure.error);
        };
        assert_eq!(imports, &vec!["wasi:cli/environment@0.2.0".to_string()]);
        let text = failure.error.to_string();
        assert!(text.contains("wasi"), "{text}");
        assert!(text.contains("wasi:cli/environment@0.2.0"), "{text}");
        let help = failure.error.help().expect("help text").to_string();
        assert!(help.contains("wasm32-unknown-unknown"), "{help}");
    }

    #[test]
    fn plugin_package_imports_are_not_foreign() {
        assert!(
            foreign_imports(&[
                "filimon:plugin/log@0.1.0",
                "filimon:plugin/fetch@0.1.0",
                "filimon:plugin/types@0.1.0",
            ])
            .is_empty()
        );
        assert_eq!(
            foreign_imports(&[
                "filimon:plugin/log@0.1.0",
                "wasi:io/streams@0.2.0",
                "wasi:cli/stdout@0.2.0",
            ]),
            vec![
                "wasi:io/streams@0.2.0".to_string(),
                "wasi:cli/stdout@0.2.0".to_string()
            ]
        );
    }

    #[test]
    fn foreign_import_list_is_sanitized_and_capped() {
        let described = describe_foreign(&["evil\u{1b}[31m\nname".to_string(), "x".repeat(300)]);
        assert_eq!(described[0], "evil[31m name");
        assert!(!described.iter().any(|n| n.chars().any(char::is_control)));
        assert_eq!(described[1].chars().count(), IMPORT_NAME_CLIP + 1);
        assert!(described[1].ends_with('…'));

        let many: Vec<String> = (0..20).map(|i| format!("wasi:x/n{i}")).collect();
        let described = describe_foreign(&many);
        assert_eq!(described.len(), 17);
        assert_eq!(described[15], "wasi:x/n15");
        assert_eq!(described[16], "…and 4 more");
    }

    #[test]
    fn enabled_and_disabled_fetchers_are_opened_only_when_enabled() {
        let tmp = TempDir::new().expect("tempdir");
        let f1 = tmp.path().join("f1.wasm");
        let f2 = tmp.path().join("f2.wasm");
        let f3 = tmp.path().join("f3.wasm");
        for file in [&f1, &f2, &f3] {
            std::fs::write(file, GARBAGE).expect("write garbage");
        }

        let report = load(
            tmp.path(),
            &[fetcher("f1", &f1, true), fetcher("f2", &f2, false)],
        );

        assert!(report.opened.contains(&f1));
        assert!(!report.opened.contains(&f2));
        assert!(!report.opened.contains(&f3));
        assert_eq!(report.opened.len(), 1);
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].kind, PluginKind::Fetcher);
        assert_eq!(report.failures[0].name, "f1");
        assert!(report.fetcher_names.is_empty());
        assert!(report.fetchers.is_empty());
    }

    #[test]
    #[ignore = "needs plugin fixtures: see plugins/README.md"]
    fn wasm_stub_fetcher_loads_as_fetcher() {
        let tmp = TempDir::new().expect("tempdir");
        let stub = fixture_wasm("stub-fetcher", "stub_fetcher.wasm");

        let report = load(tmp.path(), &[fetcher("stub-fetcher", &stub, true)]);

        assert!(report.failures.is_empty(), "{:?}", report.failures);
        assert_eq!(report.fetcher_names, vec!["stub-fetcher".to_string()]);
        assert!(report.fetchers.contains_key("stub-fetcher"));
    }

    #[test]
    #[ignore = "needs plugin fixtures: see plugins/README.md"]
    fn wasm_stub_source_rejected_as_fetcher() {
        let tmp = TempDir::new().expect("tempdir");
        let stub = fixture_wasm("stub-source", "stub_source.wasm");

        let report = load(tmp.path(), &[fetcher("wrong", &stub, true)]);

        assert!(report.fetchers.is_empty());
        let failure = report.failures.first().expect("one failure");
        assert_eq!(failure.kind, PluginKind::Fetcher);
        assert!(
            matches!(
                &failure.error,
                HostError::WrongWorld {
                    kind: "fetcher",
                    ..
                }
            ),
            "{:?}",
            failure.error
        );
    }

    #[test]
    #[ignore = "needs plugin fixtures: see plugins/README.md"]
    fn wasm_stub_fetcher_rejected_as_source() {
        let tmp = TempDir::new().expect("tempdir");
        let stub = fixture_wasm("stub-fetcher", "stub_fetcher.wasm");

        let report = load(tmp.path(), &[source("wrong", &stub, true)]);

        assert!(report.sources.is_empty());
        let failure = report.failures.first().expect("one failure");
        assert_eq!(failure.kind, PluginKind::Source);
        assert!(
            matches!(&failure.error, HostError::WrongWorld { kind: "source", .. }),
            "{:?}",
            failure.error
        );
    }

    #[test]
    #[ignore = "needs plugin fixtures: see plugins/README.md"]
    fn wasm_one_bad_component_does_not_stop_the_others() {
        let tmp = TempDir::new().expect("tempdir");
        let bad = tmp.path().join("bad.wasm");
        std::fs::write(&bad, GARBAGE).expect("write garbage");
        let stub = fixture_wasm("stub-source", "stub_source.wasm");

        let report = load(
            tmp.path(),
            &[source("bad", &bad, true), source("stub", &stub, true)],
        );

        assert!(report.sources.contains_key("stub"));
        assert_eq!(report.failures.len(), 1);
        assert_eq!(report.failures[0].name, "bad");
    }

    #[tokio::test(start_paused = true)]
    #[ignore = "needs plugin fixtures: see plugins/README.md"]
    async fn wasm_host_report_lists_loaded_sources_and_fetchers() {
        use std::sync::Arc;

        use tokio_util::sync::CancellationToken;

        use crate::plugins::host::sink::VecSink;
        use crate::plugins::host::{Host, HostOptions};

        let tmp = TempDir::new().expect("tempdir");
        let stub_source = fixture_wasm("stub-source", "stub_source.wasm");
        let stub_fetcher = fixture_wasm("stub-fetcher", "stub_fetcher.wasm");
        let registry = load_test_registry(&write_inline_registry(
            tmp.path(),
            &[
                source("stub", &stub_source, true),
                fetcher("stub-fetcher", &stub_fetcher, true),
            ],
        ));

        let host = Host::load(
            &registry,
            HostOptions {
                state_dir: tmp.path().join(".fili-state"),
            },
        )
        .expect("host loads");

        let token = CancellationToken::new();
        token.cancel();
        let report = host.run(Arc::new(VecSink::default()), token).await;

        assert_eq!(report.loaded_sources, vec!["stub".to_string()]);
        assert_eq!(report.loaded_fetchers, vec!["stub-fetcher".to_string()]);
        assert!(
            report.load_failures.is_empty(),
            "{:?}",
            report.load_failures
        );
    }
}
