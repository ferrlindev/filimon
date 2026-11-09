# Phase 3: Component Host Runtime - Pattern Map

**Mapped:** 2026-11-09
**Files analyzed:** 14 new/modified
**Analogs found:** 11 / 14 (all analog paths verified git-tracked via `git ls-files`)

Note: `plugins/stub-source/src/bindings.rs` and `/target/` are gitignored generated output. Do not cite them.

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match |
|---|---|---|---|---|
| `src/plugins/mod.rs` (modify: `pub mod host;`) | config/module root | n/a | itself, lines 10-20 | exact |
| `src/plugins/host/mod.rs` (Host, HostOptions, HostReport) | service | event-driven | `src/plugins/config.rs` (typed loaded structs, lines 125-192) | role-match |
| `src/plugins/host/engine.rs` (Engine, bindgen!, HostState, log/fetch imports) | service | request-response | no in-repo analog; use RESEARCH Patterns 1 and 5 | none |
| `src/plugins/host/loader.rs` (from_file, import preflight, `*Pre`) | service | file-I/O | `src/plugins/config.rs` `load_registry` plus `src/plugins/error.rs` | role-match |
| `src/plugins/host/instance.rs` | service | request-response | none; RESEARCH "Trap vs guest error classification" | none |
| `src/plugins/host/scheduler.rs` | service | event-driven | none; RESEARCH Pattern 2 | none |
| `src/plugins/host/backoff.rs` | utility | transform | `BackoffConfig` in `src/plugins/config.rs` (inputs); RESEARCH Pattern 4 | partial |
| `src/plugins/host/cursor.rs` | service | file-I/O | `src/plugins/error.rs` (path-naming diagnostics); `tempfile` already a dep | partial |
| `src/plugins/host/sink.rs` (ItemSink, FluvioSink) | service | streaming | `src/streaming.rs` `TypedProducer` (lines 209-244) and `Arc<Mutex<TypedProducer>>` (lines 322-343) | role-match |
| `src/plugins/host/map.rs` | utility | transform | `SourceItem::social` in `src/models.rs` (lines ~160-190) | exact |
| `src/streaming.rs` (modify: add `TypedProducer::clear_errors`) | service | streaming | `TypedProducer::flush` (same file) | exact |
| `src/cli.rs` (modify: `--state-dir` / `FILI_STATE_DIR`) | config | request-response | `--registry` arg (cli.rs:39-41) | exact |
| `src/main.rs` (modify: `init_logging()`) | config | n/a | `main()` entry (main.rs:54-60) | partial |
| `plugins/fault-source/` (new fixture crate) | test fixture | request-response | `plugins/stub-source/` | exact |

## Pattern Assignments

### `src/plugins/host/*` module wiring (`src/plugins/mod.rs`)

Analog: `src/plugins/mod.rs` lines 10-20. The existing module-level allow already covers the new `host` submodule while nothing in `main` calls it.
```rust
#![cfg_attr(
    not(test),
    allow(dead_code, reason = "consumed by the plugin host and CLI in Phases 3-4")
)]

pub mod config;
pub mod error;
pub mod secrets;
// add: pub mod host;
```
Tests: co-located `#[cfg(test)] mod tests` (mod.rs line ~24) using `tempfile::TempDir`, `load_registry_with_env`. The crate is bin-only, so there is no `tests/` dir.

### `host/mod.rs`, `host/loader.rs` (consume the registry)

Analog: `src/plugins/config.rs` lines 125-192. The host consumes only these types; open only enabled entries' `component` paths (HOST-01).
```rust
pub struct LoadedRegistry { pub path, pub base_dir, pub sources: BTreeMap<String, LoadedSource>, pub fetchers: BTreeMap<String, LoadedFetcher> }
pub struct LoadedSource { pub name, pub component: PathBuf, pub config_path, pub enabled: bool, pub fetcher: Option<String>, pub plugin: Option<PluginConfig> }
pub struct PluginConfig { pub host: HostConfig, pub settings_toml: String, pub secrets: ResolvedSecrets }
pub struct HostConfig { pub poll_interval: Duration, pub timeout: Duration, pub allowlist: Vec<String>, pub backoff: BackoffConfig }
pub struct BackoffConfig { pub initial: Duration, pub max: Duration, pub multiplier: f64 }
pub struct LoadedFetcher { pub name, pub component: PathBuf, pub enabled: bool }
```
`plugin` is `Some` exactly when the source is enabled. Secrets are the redacting `Secret` newtype (`src/plugins/secrets.rs:20-37`); call `.expose()` only when handing values to the owning plugin's `init`.

Diagnostics pattern (copy for host load errors, e.g. WASI import rejection, corrupt cursor): `src/plugins/error.rs` lines 1-60.
```rust
use miette::{Diagnostic, NamedSource, SourceSpan};
#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum ConfigError {
    #[error("`sources.{plugin}.config`: cannot read `{path}`: {reason}")]
    #[diagnostic(code(fili::config::plugin_file_missing))]
    PluginFileMissing { plugin: String, path: String, reason: String, ... },
}
```
Conventions: `code(fili::<area>::<name>)`, `help(...)` where actionable, error text carries plugin name and path only, never secret values or record content. Define a separate `HostError` enum in the host module, for example `fili::host::wasi_import`, `fili::host::cursor_corrupt` naming plugin and path (D-08). Do not put guest strings in `#[source_code]`.

### `host/sink.rs` (service, streaming)

Analog: `src/streaming.rs` lines 209-244.
```rust
pub struct TypedProducer { producer: fluvio::TopicProducer<SpuSocketPool> }
impl TypedProducer {
    pub async fn send<T: Serialize>(&self, key: &str, value: &T) -> Result<()> {
        let json = serde_json::to_vec(value).map_err(|e| miette::miette!("Failed to serialize: {}", e))?;
        self.producer.send(RecordKey::from(key.to_string()), json).await
            .map_err(|e| miette::miette!("Failed to send record: {}", e))?;
        Ok(())
    }
    pub async fn flush(&self) -> Result<()> {
        self.producer.flush().await.map_err(|e| miette::miette!("Failed to flush producer: {}", e))?;
        Ok(())
    }
}
```
Shared-producer shape: `raw_pages_producer: Arc<Mutex<TypedProducer>>` (streaming.rs:322, built at 331, accessor at 342). Hold the lock across "send all + flush". Producer creation is `FluvioStream::producer(topic)` (streaming.rs:89); use `TOPIC_SOCIAL_ITEMS`. Preflight via `FluvioStream::preflight(&[TOPIC_SOCIAL_ITEMS], &[])` (streaming.rs:155).

Modify `TypedProducer` by adding (same style as `flush`) a `pub async fn clear_errors(&self)` calling `self.producer.clear_errors().await` (fluvio 0.50.1 `TopicProducer::clear_errors`).

Define `ItemSink` as an async trait (or `impl Future` method) with a test `VecSink`. Scheduler tests use the fake sink and need no cluster. Cluster tests are `#[ignore = "..."]` named `live_*`, like `src/commands/consume.rs:2477`.

### `host/map.rs` (utility, transform)

Analog: `src/models.rs` `SourceItem::social` (~lines 154-190). Its `#[cfg_attr(not(test), allow(dead_code, ...))]` can be removed only if reachable from `main`; otherwise leave it.
```rust
pub fn social(id: String, url: String, title: Option<String>, text: String, author: Option<String>,
              published_ms: Option<u64>, platform: String, raw: Option<String>) -> Self {
    Self { source_type: SourceType::Social, platform, id, url, title, text, author,
           published: published_ms.and_then(epoch_ms_to_rfc3339),
           raw: raw.map(|s| serde_json::from_str(&s).unwrap_or(serde_json::Value::String(s))),
           html: None }
}
```
Map the WIT `Item` fields 1:1 into this call. Validation (empty id/url, `url::Url::parse`, JSON over 1_000_000 bytes, empty platform falls back to plugin name) happens before the call. Drop with a warn naming plugin and reason, never content. Sanitize via `crate::output::sanitize_for_terminal` (src/output.rs:49). The message key is `format!("{platform}:{id}")`.

### `host/cursor.rs` (file-I/O)

No tracked analog for atomic JSON state. Use RESEARCH Pattern 3 verbatim (`tempfile::NamedTempFile::new_in(dir)`, `sync_all`, `persist`). Alias safety is guaranteed by config validation (`src/plugins/config.rs:388-393`). Error diagnostics follow the `error.rs` convention (plugin name plus file path). Do not use `deny_unknown_fields` on read.

### `host/backoff.rs` (utility, pure)

Inputs are `BackoffConfig` (config.rs:173). Copy RESEARCH Pattern 4 (`step`, `rate_limited_wait`). Table-test in a co-located `mod tests`; no clock, no IO.

### `host/engine.rs`, `host/instance.rs`, `host/scheduler.rs`

No in-repo analog (first wasmtime code). Use RESEARCH Pattern 1 (two `bindgen!` modules with `with:`), Pattern 2 (pure `next_action` state machine), Pattern 5 (log mapped to tracing), the "Trap vs guest error classification" example and "Import preflight". Wrap generated bindings in `#[allow(clippy::all, clippy::pedantic)]` because the crate lints pedantic as warn (Cargo.toml:8). The scheduler tests use `#[tokio::test(start_paused = true)]` (tokio `test-util` is available, Cargo.toml:46), with the epoch ticker on a `std::thread`.

### `src/cli.rs` (add `--state-dir`)

Analog: `src/cli.rs` lines 39-41.
```rust
/// Path to the plugin registry (filimon.toml); component and config paths inside it resolve relative to its directory.
#[arg(long, global = true, env = "FILI_REGISTRY", default_value = DEFAULT_REGISTRY)]
pub registry: PathBuf,
```
Add `#[arg(long, global = true, env = "FILI_STATE_DIR")] pub state_dir: Option<PathBuf>`. Use `Option` so the default `.fili-state/` is resolved next to the registry (`LoadedRegistry.base_dir`), per D-06.

### `src/main.rs` (add `init_logging()`)

Analog: `main()` at main.rs:54-60 (`let cli = Cli::parse();` then banner `println!`s). Call `init_logging()` before dispatch. Use `tracing_subscriber::fmt().with_env_filter(...).with_writer(std::io::stderr).try_init()` with default directive `filimon::plugins=info` (RESEARCH Pitfall 3). Currently no subscriber is installed anywhere in `src`, and `tracing-subscriber` with `env-filter` is already a dependency.

### `plugins/fault-source/` (test fixture crate)

Analog: `plugins/stub-source/` (tracked files: `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/config.toml`, `.gitignore`, `src/lib.rs`).

`Cargo.toml` shape (copy, rename to `fault-source`, package `filimon:fault-source`, add the same deps):
```toml
[lib]
crate-type = ["cdylib"]
[dependencies]
wit-bindgen-rt = { version = "0.41", features = ["bitflags"] }
serde = { version = "1", features = ["derive"] }
toml = "1"
[package.metadata.component]
package = "filimon:stub-source"
[package.metadata.component.target]
path = "../../wit"
world = "source"
[workspace]   # own workspace
```
Other files, copied verbatim:
- `rust-toolchain.toml`: `channel = "1.96.0"`, `targets = ["wasm32-unknown-unknown"]`
- `.cargo/config.toml`: `[build] target = "wasm32-unknown-unknown"`
- `.gitignore`: `/target/` and `/src/bindings.rs`

Guest code pattern (`plugins/stub-source/src/lib.rs`):
```rust
#[allow(warnings)]
mod bindings;
use bindings::Guest;
use bindings::filimon::plugin::log::{Level, log};
use bindings::filimon::plugin::types::{Item, PollError, PollResponse};
#[derive(Deserialize)] #[serde(deny_unknown_fields)] struct Settings { ... }  // add `mode`
struct Component;
impl Guest for Component {
    fn init(settings: String, secrets: Vec<(String, String)>) -> Result<(), PollError> {
        let settings: Settings = toml::from_str(&settings).map_err(|e| PollError::Config(e.to_string()))?;
        ...
    }
    fn poll(cursor: Option<Vec<u8>>, _access_token: Option<String>) -> Result<PollResponse, PollError> { ... }
}
bindings::export!(Component with_types_in bindings);
```
Modes to implement (RESEARCH Discretion 7): `ok`, `rate-limited`, `transient`, `auth-required`, `fatal`, `config`, `init-fatal`, `trap`, `spin`. Use a `thread_local` counter for per-instance state. The stub-source cursor behaviour (`stub-{cursor[0]}`, `next_cursor = [n+1]`) is the SC2 restart-test oracle.

## Shared Patterns

### Diagnostics and redaction
**Source:** `src/plugins/error.rs`, `src/plugins/secrets.rs:20-37`, `src/output.rs:49`
**Apply to:** every host module that logs or errors. Name plugin and path; sanitize guest text with `sanitize_for_terminal`; never log settings, secrets, or item content.

### Tests
**Source:** `src/plugins/mod.rs` `#[cfg(test)] mod tests` (uses `tempfile::TempDir`, `examples/plugins/` samples via `env!("CARGO_MANIFEST_DIR")`)
**Apply to:** all host modules. Plain `cargo test` covers backoff, scheduler (paused time, fake sink and fake instance factory), cursor, map, and load-set. wasm-backed tests are `#[ignore = "needs plugin fixtures: see plugins/README.md"]` named `wasm_*`. Fluvio tests are `#[ignore]` named `live_*`.

### Registry sample fixtures
**Source:** `examples/plugins/filimon.toml`, `stub.toml`, `reddit.toml`
**Apply to:** host integration tests that need a registry. Build a tempdir registry pointing at `plugins/stub-source` and `plugins/fault-source` builds.

## No Analog Found

| File | Role | Data Flow | Reason |
|---|---|---|---|
| `host/engine.rs` | service | request-response | No wasmtime code in the repo yet; use RESEARCH Patterns 1 and 5 |
| `host/instance.rs` | service | request-response | Same; use RESEARCH classification example |
| `host/scheduler.rs` | service | event-driven | No per-plugin task scheduler exists; use RESEARCH Pattern 2 |

## Metadata

**Analog search scope:** `src/plugins/`, `src/streaming.rs`, `src/models.rs`, `src/cli.rs`, `src/main.rs`, `src/output.rs`, `plugins/`
**Files scanned:** about 10
**Pattern extraction date:** 2026-11-09
**Note:** RESEARCH.md lines 511-615 were not read (Environment Availability and later sections).
