# Phase 1: Plugin Contract & Configuration - Pattern Map

**Mapped:** 2026-11-03
**Files analyzed:** 20 (new + modified)
**Analogs found:** 12 / 20 (WIT files, stub toolchain files, and examples have no in-repo analog; use RESEARCH.md Code Examples)

Tracked-source note: `src/config.rs`, `src/streaming.rs`, `src/commands/`, and `smartmodules/` are **untracked working-tree files** (not gitignored mirrors). Per the orchestrator note, the working tree is current, so they are valid analogs. Tracked analogs: `src/cli.rs`, `src/main.rs`, `src/models.rs`, `src/check.rs`, `src/args.rs`, `Cargo.toml`, `.gitignore`. Never use `fluvio/` as an analog.

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|---|---|---|---|---|
| `wit/types.wit` | contract | n/a (type defs) | none in repo; RESEARCH §1 | no analog |
| `wit/host.wit` | contract | request-response | none; RESEARCH §1 | no analog |
| `wit/worlds.wit` | contract | n/a | none; RESEARCH §1 | no analog |
| `plugins/stub-source/Cargo.toml` | config (guest crate) | build | `smartmodules/filter-relevance/Cargo.toml` | role-match |
| `plugins/stub-source/src/lib.rs` | guest component | request-response | `smartmodules/filter-relevance/src/lib.rs` | role-match (WASM guest, header style) |
| `plugins/stub-source/rust-toolchain.toml`, `.cargo/config.toml` | config | build | none; RESEARCH §2 | no analog |
| `plugins/stub-fetcher/Cargo.toml` | config | build | `smartmodules/filter-relevance/Cargo.toml` | role-match |
| `plugins/stub-fetcher/src/lib.rs` | guest component | request-response | `smartmodules/filter-relevance/src/lib.rs` | role-match |
| `plugins/stub-fetcher/rust-toolchain.toml`, `.cargo/config.toml` | config | build | none; RESEARCH §2 | no analog |
| `plugins/README.md` | docs | n/a | none | no analog |
| `src/plugins/mod.rs` | module root / service entry | file-I/O | `src/commands/mod.rs` | role-match |
| `src/plugins/config.rs` | model (serde structs) + loader | file-I/O, transform | `src/config.rs` (load) + `src/models.rs` (structs/docs) | exact (serde file load) |
| `src/plugins/secrets.rs` | utility | file-I/O | `src/config.rs` | partial |
| `src/plugins/error.rs` | error types | collect-all validation | `src/check.rs` (`ItemError`/`ProcessingResult` partition) | role-match |
| `src/cli.rs` (modify: `--registry`) | config/CLI | request-response | itself (`tokenizer` arg, lines 16-18) | exact |
| `src/main.rs` (modify: `mod plugins;`) | entry | n/a | itself lines 18-26 | exact |
| `Cargo.toml` (modify: deps + `exclude`) | config | n/a | itself lines 18-47 | exact |
| `.gitignore` (modify: `plugins/*/target/`) | config | n/a | itself lines 1-3 | exact |
| `examples/plugins/{filimon.toml,stub.toml,reddit.toml,youtube.toml,.env.example}` | fixture/config | n/a | none | no analog |
| Unit tests in `src/plugins/*.rs` | test | file-I/O | `src/check.rs` lines 59-147, `src/args.rs` tests | role-match |

## Pattern Assignments

### `src/plugins/config.rs` (model + loader, file-I/O)

**Analog:** `src/config.rs` (lines 1-12) for read-then-deserialize; `src/models.rs` (lines 1-27) for header + struct docs.

Load pattern to replace (do NOT edit `src/config.rs`; copy shape, upgrade errors):
```rust
pub fn load_config(path: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let raw = std::fs::read_to_string(path)?;
    let config: Config = serde_json::from_str(&raw)?;
    Ok(config.ls)
}
```
Change: `toml::from_str` instead of `serde_json`; keep the raw `String` so it becomes `NamedSource::new(path, raw)` for diagnostics; return `ConfigError`, not `Box<dyn Error>`. Struct shapes: RESEARCH §4 (`Registry`, `SourceEntry`, `FetcherEntry`, `PluginFile`, `HostSection`, `toml::Spanned`, `deny_unknown_fields`, `NonZeroU64`).

Header + struct doc style (`src/models.rs` lines 1-27):
```rust
// ─────────────────────────────────────────────────────────────────────────────
// models.rs - Shared data structures for the WISE pipeline
//
// All structures are serializable for Fluvio streaming. ...
// ─────────────────────────────────────────────────────────────────────────────

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Raw HTML fetched from a single URL.
///
/// This is the output of Stage 1 (Crawler) and input to Stage 2 (Preprocessor).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawPage { pub url: String, pub html: String }
```

### `src/plugins/error.rs` (error types, collect-all)

**Analog:** `src/check.rs` lines 3-30 — existing "collect valid + invalid, keep going" pattern:
```rust
#[derive(Debug)]
pub struct ItemError { pub item: String, pub message: String }

#[derive(Debug)]
pub struct ProcessingResult { pub invalid: Vec<ItemError>, pub valid: Vec<String> }
```
Keep the "accumulate into `Vec<Error>`, never early-return per item" idea (D-14), but implement with `thiserror` + `miette::Diagnostic` (`#[source_code] NamedSource<String>`, `#[label] Option<SourceSpan>`, aggregate `ConfigErrors { #[related] problems }`) per RESEARCH §5. Do not hand-write `impl fmt::Display` like `check.rs` lines 53-57; use `#[error(...)]`.

### `src/plugins/secrets.rs` (utility, file-I/O)

**Analog:** `src/config.rs` (read file → map). Implement per RESEARCH §6: `dotenvy::from_path_iter`, missing file = empty map, injectable `env: &dyn Fn(&str) -> Option<String>`, redacting `Secret` newtype. No `set_var` (Cargo.toml `unsafe_code = "warn"`, line 8).

### `src/plugins/mod.rs` (module root)

**Analog:** `src/commands/mod.rs` lines 1-7:
```rust
pub mod consume;
pub mod produce;
pub mod watch;

pub use consume::run as run_consume;
```
Apply: `pub mod config; pub mod error; pub mod secrets;` + `pub use` of the `load_registry` entry point. Add the module header divider block (see Shared Patterns).

### `src/cli.rs` (modify: global `--registry`)

**Analog:** same file lines 16-18 (path arg with env):
```rust
/// Path to GLiNER tokenizer.json (for NER on consume)
#[arg(long, env = "TOKENIZER_PATH", default_value = DEFAULT_TOKENIZER)]
pub tokenizer: PathBuf,
```
Add: `const DEFAULT_REGISTRY: &str = "filimon.toml";` next to lines 6-7, and
`#[arg(long, global = true, env = "FILI_REGISTRY", default_value = DEFAULT_REGISTRY)] pub registry: PathBuf,` in `Cli`. Leave `config` (line 28-30, watch-mode JSON) untouched.

### `src/main.rs` (modify)

**Analog:** lines 18-26 (alphabetical `mod` list) — insert `mod plugins;` between `output` and `scorer`. Errors propagate via `async fn main() -> miette::Result<()>` (line 51); a `ConfigErrors` returned as `miette::Report` renders all related problems and exits non-zero. Phase 1 adds no subcommand.

### `Cargo.toml` (modify)

**Analog:** lines 18-47. Add to `[dependencies]` (after `miette`, line 31): `toml = "1"`, `dotenvy = "0.15"`, `thiserror = "2"`. Add to `[workspace]` (line 41): `exclude = ["plugins"]`. Do not add stubs to `members`.

### `.gitignore` (modify)

**Analog:** lines 1-3 section style:
```
# ── Rust build artefacts ───────────────────────────────────────────────────────
/target/
```
Append `plugins/*/target/` in that section. Note line 14-16 (`.env`, `.env.*`, `!.env.example`) means `.env` fixtures must be written to `tempfile::TempDir` in tests (RESEARCH Pitfall 7). Note line 3 ignores `Cargo.lock` at any depth — stub `Cargo.lock` commit (RESEARCH Pitfall 11) requires a `!plugins/*/Cargo.lock` negation or `git add -f`; planner must decide.

### `plugins/stub-source/Cargo.toml`, `plugins/stub-fetcher/Cargo.toml` (guest crate config)

**Analog:** `smartmodules/filter-relevance/Cargo.toml` lines 1-15:
```toml
[package]
name = "filter-relevance"
version = "0.1.0"
edition = "2024"
rust-version = "1.85"
description = "Fluvio SmartModule: ..."
authors = ["filimon"]

[lib]
crate-type = ['cdylib']
```
Copy package/`[lib] cdylib` shape and `description`/`authors` fields; differ by: NO `rust-version = "1.85"` (stubs pin 1.96.0 via `rust-toolchain.toml`), add `[package.metadata.component]` + `.target { path = "../../wit", world = ... }` and an empty `[workspace]` table (RESEARCH §2, Pitfall 5), `wit-bindgen-rt = { version = "0.41", features = ["bitflags"] }`.

### `plugins/stub-source/src/lib.rs`, `plugins/stub-fetcher/src/lib.rs` (guest component)

**Analog:** `smartmodules/filter-relevance/src/lib.rs` lines 1-45 — WASM guest with header block, section dividers, doc'd structs, single exported entry:
```rust
// ─────────────────────────────────────────────────────────────────────────────
// filter-relevance SmartModule
//
// This SmartModule filters extracted articles based on their relevance score.
// ─────────────────────────────────────────────────────────────────────────────

use fluvio_smartmodule::{smartmodule, SmartModuleRecord, Result};
use serde::{Deserialize, Serialize};

// ── SmartModule Filter Function ───────────────────────────────────────────────

#[smartmodule(filter)]
pub fn filter(record: &SmartModuleRecord) -> Result<bool> {
    let article: ExtractedArticle = serde_json::from_slice(record.value.as_ref())?;
```
Body comes from RESEARCH §2 (source: `impl Guest`, parse settings with `toml`, call `log` and settings-gated `fetch`, `bindings::export!`) and §3 (fetcher: `exports::filimon::plugin::fetch::Guest`). Errors map into WIT variants (`PollError::Config(e.to_string())`), mirroring the analog's `?` on deserialize.

### Tests in `src/plugins/*.rs`

**Analog:** `src/check.rs` lines 59-147 (inline `#[cfg(test)] mod tests { use super::*; ... }`, `test_` prefixed names, asserting each collected error's fields in order, e.g. lines 93-99) and `src/args.rs` tests (file fixtures via `std::fs`). For this phase use `tempfile::TempDir` (dep at `Cargo.toml` line 26) and the injectable env closure instead of `std::env::set_var`.

## Shared Patterns

### Module header + section dividers
**Source:** `src/streaming.rs` lines 1-26, `src/main.rs` lines 1-16, 36, 48
**Apply to:** every new `.rs` file (host and stubs)
```rust
// ─────────────────────────────────────────────────────────────────────────────
// streaming.rs - Fluvio Streaming Integration
//
// <purpose paragraph>
// ─────────────────────────────────────────────────────────────────────────────

use ...;

// ── Topic Names ───────────────────────────────────────────────────────────────
```

### Error handling
**Source:** `src/streaming.rs` lines 20, 40-45; `src/main.rs` line 51
**Apply to:** host config code at the boundary into `main`
```rust
use miette::Result;
let client = Fluvio::connect().await
    .map_err(|e| miette::miette!("Failed to connect to Fluvio: {}", e))?;
```
Internal config code uses typed `ConfigError`/`ConfigErrors` (Diagnostic) instead of `miette!` strings, so spans survive; convert to `miette::Report` only at the top. No `unwrap()` outside tests.

### Logging
**Source:** `src/streaming.rs` line 24 (`use tracing::{error, info, warn};`), lines 41, 44
**Apply to:** loader progress messages (never log secret values; use redacting `Secret`).

### Lint posture
**Source:** `Cargo.toml` lines 7-12 (`unsafe_code = "warn"`, clippy `all` + `pedantic`). Public fallible fns need `/// # Errors` docs (pedantic `missing_errors_doc`).

## No Analog Found

| File | Role | Data Flow | Reason |
|---|---|---|---|
| `wit/types.wit`, `wit/host.wit`, `wit/worlds.wit` | contract | n/a | No WIT in repo; use RESEARCH §1 verbatim (validated with wasm-tools 1.263.0) |
| `plugins/*/rust-toolchain.toml`, `plugins/*/.cargo/config.toml` | config | build | No committed toolchain files (root pin is a local rustup override); use RESEARCH §2 |
| `plugins/README.md` | docs | n/a | Install commands from RESEARCH "Installation" |
| `examples/plugins/*` | fixtures | n/a | No `examples/` dir; follow RESEARCH Recommended Project Structure and D-10..D-12 key layout |

## Metadata

**Analog search scope:** `src/`, `src/commands/`, `smartmodules/*`, root `Cargo.toml`, `.gitignore` (excluded `fluvio/`)
**Files scanned:** 14
**Pattern extraction date:** 2026-11-03
