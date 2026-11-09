# Phase 4: Plugin CLI - Pattern Map

**Mapped:** 2026-11-10
**Files analyzed:** 14 (new or modified)
**Analogs found:** 13 / 14

All analog paths are git-tracked (checked with `git ls-files src/plugins src/commands`). `tests/` does not exist yet.

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match |
|---|---|---|---|---|
| `src/cli.rs` (add `Plugins`, `Run`; drop dead_code allow) | config/CLI | request-response | itself (`Command` enum, lines ~70-120) | exact |
| `src/main.rs` (dispatch; skip banner) | entry/dispatch | request-response | itself (lines 84-122) | exact |
| `src/commands/mod.rs` (+ `plugins`, `run`) | module index | n/a | itself (7 lines) | exact |
| `src/commands/plugins.rs` (list/check/run_once, render fns) | command | request-response, file-I/O | `src/commands/produce.rs` (+ `output.rs` sanitize) | role-match |
| `src/commands/run.rs` (`run_with_sink` + FluvioSink wiring) | command | event-driven/streaming | `src/commands/watch.rs`, `Host::run` in `src/plugins/host/mod.rs` | role-match |
| `src/plugins/host/cursor.rs` (+ `PollStatus`, `write_status`, lock-free `read_status`, free cursor read) | service/store | file-I/O | itself (`CursorStore::read/write`, 100-170) | exact |
| `src/plugins/host/loader.rs` (+ `pub check_component`, one-source load) | service | transform | itself (`load_source`/`load_fetcher`, 146-172) | exact |
| `src/plugins/host/once.rs` (single-cycle poll) | service | request-response | `run_plugin` in `scheduler.rs` (297-431) | role-match |
| `src/plugins/host/scheduler.rs` (+ status writes) | service | event-driven | itself | exact |
| `src/plugins/host/mod.rs` (accessors, grace, generic supervisor, `HostOptions` field, empty state dir reject) | service | event-driven | itself (`Host::run`, 129-184) | exact |
| `src/plugins/config.rs` + `error.rs` (registry listing, `ConfigError::plugin()`) | config loader | transform | itself (`load_registry_with_env`) | exact |
| `src/plugins/mod.rs` (remove module-level dead_code allow) | config | n/a | itself (lines 10-16) | exact |
| `tests/plugins_cli.rs` (new) | test | request-response | none in repo | no analog |
| `plugins/README.md` (shutdown wording) | docs | n/a | itself (~287-295) | exact |

## Pattern Assignments

### `src/plugins/host/cursor.rs` (store, file-I/O)

**Analog:** itself. Header banner style (lines 1-11) is `// ───` rule, file name, description; copy for every new file. Imports are std, external, then `super::`/`crate::` (lines 13-22).

**Atomic write to copy for `write_status`** (lines 150-175):
```rust
let mut temp = tempfile::NamedTempFile::new_in(&self.dir).map_err(|e| fail(e.to_string()))?;
serde_json::to_writer(&mut temp, &state).map_err(|e| fail(e.to_string()))?;
temp.as_file().sync_all().map_err(|e| fail(e.to_string()))?;
temp.persist(&path).map_err(|e| fail(e.error.to_string()))?;
if let Ok(dir) = File::open(&self.dir) { let _ = dir.sync_all(); }
```
**Read/validation to split into a free fn over `(dir, alias)`** (lines 100-141): NotFound gives `Ok(None)`; other io gives `CursorUnreadable`; parse failure, `version > STATE_VERSION` and `plugin != alias` give `CursorCorrupt`; foreign plugin name goes through `clean_guest_text(&state.plugin, 64)`. `CursorStore::read` then calls the free fn with `&self.dir`. Never write or delete a bad file (D-08).
**Lock (only for `--produce`/`run`)** lines 59-84: `create_dir_all`, open `.lock`, `try_lock()`, `HostError::StateDirLocked`. `read_status` must not use it and must not `create_dir_all`.
**Tolerant struct** lines 30-37: `StateFile {version, plugin, cursor}` has no `deny_unknown_fields`; model `PollStatus` the same. Do not touch `StateFile`; the key-pin test (~line 367) stays valid.
**Tests** (lines 175+): `TempDir::new()`, `CursorStore::open(dir.path())`, write then read round trip, assertions on JSON fields via `serde_json::Value`; corrupt-file test writes `"{not json"` and asserts the error names plugin and path. Mirror for status (garbage, oversize 64 KiB cap, round trip).

### `src/plugins/host/loader.rs` (service, transform)

**Analog:** itself. Expose `check_component` over these (lines 121-172):
```rust
fn open_component(plugin, path, host_engine) -> Result<Component, HostError> // Component::from_file + preflight_imports, ComponentLoad error with clean_guest_text(.., REASON_CLIP)
fn load_source(plugin, path, host_engine) -> Result<SourcePre<HostState>, HostError> {
    let component = open_component(plugin, path, host_engine)?;
    host_engine.source_linker().instantiate_pre(&component).and_then(SourcePre::new)
        .map_err(|e| wrong_world(plugin, PluginKind::Source, path, &e))
}
```
`load_fetcher` is the same with `fetcher_linker`/`FetcherPre`. Make `load_source` `pub(crate)`/`pub` for `once.rs` and add `pub fn check_component(kind, name, path, &HostEngine) -> Result<(), HostError>` that matches on `PluginKind` and drops the `Pre`. Existing loader tests use garbage bytes and `WASI_IMPORT_COMPONENT` bytes: reuse for check tests.

### `src/plugins/host/scheduler.rs` (status writes)

**Analog:** `run_plugin` (297-431). Cursor-read early return (lines 305-314) is the corrupt-cursor disable path that also needs a status write:
```rust
Err(e) => { report.failures += 1;
    let reason = clean_guest_text(&e.to_string(), REPORT_TEXT_CLIP);
    disable(&mut report, &spec.name, reason); return report; }
```
Secrets build (319-323), the only place a value is exposed; copy verbatim into `once.rs`:
```rust
spec.secrets.iter().map(|(alias, secret)| (alias.clone(), secret.expose().to_string())).collect()
```
Oversize cursor rule lives at ~371-384 (`MAX_CURSOR_BYTES`, message "next-cursor is {len} bytes, over the {MAX_CURSOR_BYTES} byte cap"); extract a shared helper rather than copying. `describe(&CycleOutcome)` is `pub` at line 181; use it for status `error` and run-once failure text. Status write failure: `tracing::warn!` with cleaned text, never fail the cycle. Tests: `run_for` and `FakeFactory`/`FakeStep` in `testing.rs` (428-440, 216-284) under `#[tokio::test(start_paused = true)]`; assert `ok/error/items`, never the timestamp.

### `src/plugins/host/mod.rs` (supervisor, grace, accessors)

**Analog:** itself. `Host::run` (129-184) JoinSet loop to wrap with a grace deadline:
```rust
let mut tasks = JoinSet::new();
let mut names: HashMap<tokio::task::Id, String> = HashMap::new();
for (spec, factory) in plugins { let handle = tasks.spawn(run_plugin(spec, factory, Arc::clone(&sink), Arc::clone(&cursors), shutdown.clone())); names.insert(handle.id(), name); }
while let Some(joined) = tasks.join_next_with_id().await { match joined { Ok((id, report)) => ..., Err(e) => /* panic: PluginReport{disabled: Some("plugin task panicked"), ..default} */ } }
```
After `shutdown` fires, `select!` the join loop against `sleep(grace)`, then `tasks.abort_all()` and report remaining names with `disabled: Some("aborted at shutdown")`. Extract the loop to a generic fn over `InstanceFactory` so `FakeFactory` can test it. `HostOptions { state_dir }` (lines 44-47) gets the grace field; update every literal (`wasm_tests.rs` lines 38-41 and others, grep `HostOptions {`). `resolve_state_dir` (51-57): add the empty/whitespace reject returning `HostError::StateDir`. Existing tests at the file end (`state_dir_defaults_next_to_registry`) show test style. Add `load_failures()` and `loaded_sources()` accessors on `Host` (fields already exist, lines ~70-76).

### `src/plugins/host/once.rs` (new, single cycle)

**Analog:** `run_plugin` plus the traits in `instance.rs` lines 56-77:
```rust
pub trait PluginInstance { fn init(&mut self, settings: &str, secrets: &[(String,String)]) -> impl Future<Output=CallOutcome<()>>+Send;
                           fn poll(&mut self, cursor: Option<&[u8]>) -> impl Future<Output=CallOutcome<PollResponse>>+Send; }
pub trait InstanceFactory { type Instance; fn instantiate(&self, plugin:&str) -> impl Future<Output=Result<Self::Instance, InstanceBuildError>>+Send; }
```
Order: `instantiate`, `init(&spec.settings_toml, &secrets)`, `poll(cursor)`, `map_items(name, items)`; with produce: `sink.send_batch` then `cursors.write` (D-07). Dry run uses lock-free cursor read and takes no `CursorStore`. Make the function generic over `F: InstanceFactory, S: ItemSink`, test with `FakeFactory` + `VecSink`/failing sink (`testing.rs` 361-385). Select the cycle against Ctrl-C at the command level.

### `src/plugins/config.rs` / `error.rs`

**Analog:** itself. `load_registry_with_env(path, &|name| env)` is the testable entry (used by `testing.rs::load_test_registry`, lines 86-91). Add a listing fn parsing only `filimon.toml` (names, kind, enabled, component); `is_valid_name` (config.rs ~386-393) is the name rule. Add `ConfigError::plugin(&self) -> Option<&str>` as a match over variants in `error.rs` (None for `RegistryUnreadable`, `RegistryMalformed`, `DotenvInvalid`). `finish_plugin` info log at ~348 is names only. Test fixtures: `write_registry(dir, &[TestSource{..}])` in `testing.rs` (60-83).

### `src/cli.rs` (clap)

**Analog:** itself. Subcommand style (lines 76-120): doc-comment help on each variant, `#[arg(...)]` per field.
```rust
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Produce crawled pages ...
    Produce { #[arg(required = true, value_name = "URL")] urls: Vec<String>, #[arg(long, default_value_t = 5)] depth: usize },
    Watch,
    ...
}
```
Add `Plugins { #[command(subcommand)] command: PluginsCommand }` (variants `List { #[arg(long)] json: bool }`, `Check { name: String }`, `RunOnce { name: String, #[arg(long)] produce: bool, #[arg(long)] json: bool }`) and `Run`. Global flags `registry` and `state_dir` already `global = true` (lines 38-49); delete the `cfg_attr(not(test), allow(dead_code ...))` on `state_dir`. Tests (lines 124+): assert on `Cli::command().get_arguments()` metadata, not live env; add `Cli::try_parse_from([...])` cases, including `["fili","--state-dir","","run"]` is_err.

### `src/main.rs`

**Analog:** itself (84-122). Banner is unconditional `println!` at lines 87-89 before `match cli.command`. Guard it: `if !matches!(cli.command, Command::Plugins { .. } | Command::Run) { banner }`. Dispatch arm style: `Command::Produce { ref urls, depth } => { run_produce(urls, depth).await?; }`; re-export new fns in `commands/mod.rs` with `pub use plugins::...; pub use run::run as run_plugin_host;` in the style of lines 4-7. Logging is stderr only, default filter `fili::plugins=info` (lines 38-62), so command-level output uses `println!`/`eprintln!`.

### `src/commands/plugins.rs` and `src/commands/run.rs`

**Analog:** `src/commands/produce.rs` / `watch.rs`: banner header, `use crate::...`, `pub async fn run(...) -> miette::Result<()>` with `println!` progress (e.g. `println!("[Produce] Connecting to Fluvio cluster...")`), errors via `?`/`miette::IntoDiagnostic` (watch.rs line 11). Terminal safety: `crate::output::sanitize_for_terminal` (output.rs:49) plus `clean_guest_text` for every guest- or file-derived string. Sink: `FluvioSink::connect_social()` (sink.rs:72) returns `miette::Result<Self>`; keep `run_with_sink<S: ItemSink>` generic. Signals: `tokio_util::sync::CancellationToken` + `tokio::signal::ctrl_c` (unix SIGTERM branch under `#[cfg(unix)]`). Render fns take `&mut impl Write`; unit tests use `Vec<u8>`.

## Shared Patterns

### Untrusted text
**Source:** `src/plugins/host/engine.rs::clean_guest_text`, `src/output.rs::sanitize_for_terminal`. Apply to every printed guest/status-file string; clip in human output.

### Error types
**Source:** `src/plugins/host/error.rs` (`HostError` variants `ComponentLoad`, `WrongWorld`, `ForeignImports`, `StateDir`, `StateDirLocked`, `CursorCorrupt`, `CursorUnreadable`, `CursorWrite`) and `src/plugins/error.rs` (`ConfigError(s)`); all derive `miette::Diagnostic`. CLI commands return `miette::Result`; exit 1 comes from `main`.

### Test scaffolding
**Source:** `src/plugins/host/testing.rs` (`write_registry`, `load_test_registry`, `FakeFactory`, `FakeStep`, `VecSink`-style sinks, `LogCapture`, `run_for`, `fixture_wasm`). Real-wasm tests named `wasm_*` and `#[ignore]`; cluster tests `live_*` and `#[ignore]` on scratch `filimon-test-*` topics; keep other test names free of those substrings.

### Secrets
Alias and env var names only; `secret.expose()` appears only where `run_plugin` builds the secrets vector (scheduler.rs 319-323).

### Lints and style
`clippy::pedantic = "warn"`; justify any `#[allow(..., reason = "...")]`; Rust 2024; imports std, external, crate.

## No Analog Found

| File | Role | Data Flow | Reason |
|---|---|---|---|
| `tests/plugins_cli.rs` | integration test | request-response | `tests/` does not exist; use `env!("CARGO_BIN_EXE_fili")` + `std::process::Command` per RESEARCH. Reuse registry-writing logic from `src/plugins/host/testing.rs::write_registry` (it is `cfg(test)` in the bin crate, so integration tests must write their own small `filimon.toml` fixtures). Never run `run`/`--produce` here (production topic). |

## Metadata

**Analog search scope:** `src/cli.rs`, `src/main.rs`, `src/commands/`, `src/plugins/` (config, error, host/*)
**Files scanned:** ~12 read (partial ranges for large files)
**Pattern extraction date:** 2026-11-10
