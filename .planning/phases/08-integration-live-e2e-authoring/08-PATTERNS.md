# Phase 8: Integration, Live E2E & Authoring - Pattern Map

**Mapped:** 2026-11-12
**Files analyzed:** 14 new/modified
**Analogs found:** 12 / 14 (all analog paths verified git-tracked via `git ls-files`)

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|---|---|---|---|---|
| `plugins/replay-fetcher/` (Cargo.toml, .cargo/config.toml, rust-toolchain.toml, .gitignore, Cargo.lock) | config/crate scaffold | build | `plugins/stub-fetcher/*` | exact |
| `plugins/replay-fetcher/src/lib.rs` | plugin (fetcher world) | request-response | `plugins/stub-fetcher/src/lib.rs` | exact |
| `plugins/replay-fetcher/src/fixtures.rs` | data table | transform | none (RESEARCH Code Examples) | no analog |
| `plugins/template/` (scaffold + `src/lib.rs`) | plugin (source world) | request-response | `plugins/stub-source/*` | exact |
| `src/plugins/host/integration_tests.rs` | test | event-driven / streaming (Fluvio) | `src/plugins/host/sink.rs` `live_support` + `live_host_end_to_end_stub_to_fluvio_with_restart` | exact |
| `src/plugins/host/mod.rs` (add `#[cfg(test)] mod integration_tests;`) | config | n/a | existing cfg(test) mod lines 22-27 | exact |
| `src/plugins/host/testing.rs` (extend `write_registry`) | test utility | file-I/O | itself, `TestSource`/`write_registry` | exact |
| `scripts/verify-authoring.sh` | script | batch | none in repo (`scripts/` absent); drive pattern from `tests/plugins_cli.rs` | partial |
| `scripts/live-e2e.sh` | script | batch | none; `registry_assigning` in wasm_tests.rs for registry shape | partial |
| `docs/plugin-guide.md` | docs | n/a | `plugins/README.md` | role-match |
| `examples/plugins/.env.example` | config | n/a | itself | exact |
| `plugins/README.md` (test group, plugin list) | docs | n/a | itself | exact |
| `.gitignore` (live-E2E work dir) | config | n/a | existing `.fili-state/` entry | exact |
| `.planning/phases/08-.../evidence/*` | evidence | file-I/O | none | no analog |

## Pattern Assignments

### `plugins/replay-fetcher/` scaffold (copy from `plugins/stub-fetcher`)

**Cargo.toml** (stub-fetcher/Cargo.toml lines 9-24; change name/package/description):
```toml
[lib]
crate-type = ["cdylib"]

[dependencies]
wit-bindgen-rt = { version = "0.41", features = ["bitflags"] }

[package.metadata.component]
package = "filimon:stub-fetcher"      # -> "filimon:replay-fetcher"

[package.metadata.component.target]
path = "../../wit"
world = "fetcher"

# Own workspace, so the root filimon workspace does not capture this crate.
[workspace]
```
Also copy verbatim: `.cargo/config.toml` (`[build] target = "wasm32-unknown-unknown"`), `rust-toolchain.toml` (`channel = "1.96.0"`, `targets = ["wasm32-unknown-unknown"]`), `.gitignore` (`/target/`, `/src/bindings.rs`), and `Cargo.lock` (regenerate; commit). Same set for `plugins/template` from `plugins/stub-source` (its Cargo.toml adds `serde` and `toml`; world = "source"; package = `filimon:template`).

### `plugins/replay-fetcher/src/lib.rs` (fetcher, request-response)

**Analog:** `plugins/stub-fetcher/src/lib.rs`

**Imports + export shape** (lines 16-23, 29-33; export at file end):
```rust
#[allow(warnings)]
mod bindings;

use bindings::exports::filimon::plugin::fetch::Guest;
use bindings::filimon::plugin::log::{Level, log};
use bindings::filimon::plugin::types::{FetchError, Method, Request, Response};

struct Component;
impl Guest for Component {
    fn fetch(request: Request) -> Result<Response, FetchError> { ... }
}
bindings::export!(Component with_types_in bindings);
```
**Core pattern:** table lookup per RESEARCH "Replay fetcher glue" (substring needle over `fixtures::TABLE`, 404 fixed body on miss). Differences from analog: do NOT log `request.url` (stub logs it at line 38 via `log(Level::Debug, &format!("stub fetch: {}", request.url))`); log host+path only. Do not import `outbound_http`. Split `mod fixtures;` into its own file so guide readers edit one file.

### `plugins/template/src/lib.rs` (source, request-response)

**Analog:** `plugins/stub-source/src/lib.rs` (143 lines). It already has the `fetch_url` pattern the research wants (A6), so the template is a trimmed copy.

**Imports** (lines 22-30):
```rust
#[allow(warnings)]
mod bindings;
use bindings::Guest;
use bindings::filimon::plugin::fetch::fetch;
use bindings::filimon::plugin::log::{Level, log};
use bindings::filimon::plugin::types::{Item, Method, PollError, PollResponse, Request};
use serde::Deserialize;
```
**Settings parsing / error mapping** (lines 33-47, 55-60): `#[derive(Deserialize)] #[serde(deny_unknown_fields)] struct Settings`; `toml::from_str(&raw).map_err(|e| PollError::Config(e.to_string()))?`.

**Fetch call** (lines 76-96): build `Request { method: Method::Get, url, headers: vec![], body: None }`; match `fetch(&request)` `Ok(response)`/`Err(error)` and log at Debug/Warn. In the template, move this into `poll` and put `response.status` plus a bounded body excerpt in `Item.text`; degrade to a hardcoded item or `PollError::Config` when `fetch_url` is absent. Log secret alias names only, never values (lines 62-71).

**Item and cursor** (lines 108-128): `Item { id, url, title: Some(..), text, author, published: Some(ms), platform, raw }`, `PollResponse { items, next_cursor: Some(vec![n.wrapping_add(1)]) }`. Drop `record_inputs`/`RECORDED_INIT` (test-fixture-only). Keep `bindings::export!(Component with_types_in bindings);`. Keep `path = "../../wit"` in Cargo.toml (builds in repo); guide edits it to `path = "wit"`.

### `src/plugins/host/integration_tests.rs` (test, Fluvio event-driven)

**Analog:** `src/plugins/host/sink.rs` lines 180-243 (`live_support`) and 455-641 (`CountingSink`, `run_host_until`, the live end-to-end test). `live_support` is `pub(crate)` under `#[cfg(test)]`, so reuse directly.

**Imports** (sink.rs 249-252 pattern):
```rust
use crate::plugins::host::sink::live_support::{delete_topic, ready_sink, scratch_topic};
use crate::plugins::host::testing::{repo_root, load_test_registry, write_registry, TestSource};
use crate::plugins::auth::testing::{write_tokens, unix_now};
use futures_util::{FutureExt, StreamExt};
use fluvio::Offset;
```
**Scratch topic with panic-safe cleanup** (sink.rs 455-464 and 574-583 shape):
```rust
let topic = scratch_topic("host").await?;
let body = std::panic::AssertUnwindSafe(async { ... Ok::<(), miette::Report>(()) })
    .catch_unwind()
    .await;
let deleted = delete_topic(&topic).await;
match body { Ok(outcome) => { outcome?; deleted } Err(panic) => std::panic::resume_unwind(panic) }
```
**Run host to N records** (copy `CountingSink` impl `ItemSink::send_batch` and `run_host_until`, sink.rs 466-545): `Host::load(registry, HostOptions { state_dir })`, `tokio::spawn(host.run(Arc::clone(&sink), token.clone()))`, 50 ms poll loop with 30 s deadline, `token.cancel()`, join, check `report.plugins[..].disabled`. Generalise the hard-coded `"stub"` report lookup to check each plugin.

**Read back** (sink.rs 560-580): `FluvioStream::connect().await?.topic_stream(&topic, Offset::beginning(), Vec::new())`, `tokio::time::timeout(15 s, records.next())`, `serde_json::from_slice::<SourceItem>(got.value())`, key assertion `format!("{platform}:{id}")`.

**Real clock:** plain `#[tokio::test]`, NOT `start_paused` (sink.rs doc comment, "Real clock: ..."). Do NOT use `#[ignore]` (D-04); instead return early after printing `skipped: ...` (RESEARCH Pattern 4 `fluvio_or_skip`, `missing_key`).

**Reddit token trick** (auth/testing.rs line 76): `write_tokens(&state_dir, "reddit", "replay-token", unix_now() + 86_400)` before `Host::load`; state dir layout as in wasm_tests.rs `auth_registry` (state_dir = `dir.join(".fili-state")`).

**Replay registry shape** (wasm_tests.rs 522-524 and 902-910): sources/fetchers in `filimon.toml`; plugin TOML with `[host] poll_interval_secs`, `allowlist = [...]`, `[host.oauth] authorize_url/token_url/scopes/client_id`, `[settings]`. Source entry carries `fetcher = "replay"`; `[fetchers.replay] component = ...`.

**Env closure instead of secret files** (testing.rs `load_test_registry` uses `load_registry_with_env(path, &|_| None)`): call `load_registry_with_env(&path, &|k| map.get(k).cloned())` directly for the YouTube fake key / live keys.

**Build helper** (replaces `fixture_wasm` panic in testing.rs lines 33-46): new `ensure_built` per RESEARCH Code Examples; keep the same output path `plugins/<dir>/target/wasm32-unknown-unknown/release/<file>`.

### `src/plugins/host/testing.rs` (extend)

**Analog:** `TestSource` and `write_registry`, lines 55-93. Today it writes `component`, `config`, `enabled` and `[host] poll_interval_secs` / `[host.backoff]` / `[settings]` only. Extend `TestSource` with optional `fetcher: Option<String>`, `allowlist: Vec<String>`, `secrets: Vec<(alias, env)>`, `oauth: Option<String>` raw TOML, and a `fetchers: &[(name, PathBuf)]` argument or a sibling fn, keeping existing call sites compiling (add `..Default` or a builder; existing constructors at sink.rs ~540 and others list every field explicitly, so adding fields requires updating each). Prefer a new sibling `write_registry_ext` to avoid touching callers.

### `src/plugins/host/mod.rs`

Add alongside lines 22-27:
```rust
#[cfg(test)]
mod integration_tests;
```

### `scripts/verify-authoring.sh` and `scripts/live-e2e.sh` (no in-repo shell analog)

`scripts/` does not exist; use bash (`just` absent). Take command forms from:
- `tests/plugins_cli.rs` lines 12-19: clear `FILI_REGISTRY`, `FILI_STATE_DIR`, `RUST_LOG` before invoking `fili` (`env -u` in bash). Invocation shape (line ~500): `fili --registry <path> plugins run-once <name> --json`, `plugins check <name>`, `plugins list --json`.
- Registry content: `wasm_tests.rs` `registry_assigning` (522-524), absolute component paths.
- Steps and exit conditions: RESEARCH Pattern 5 (authoring) and Pattern 7 (live E2E, jq sanitise, `grep -F` secret scan, D-09 gate). Use `mktemp -d` outside the repo; `shasum -a 256` of `target/debug/fili` before/after.

### `docs/plugin-guide.md`

**Analog:** `plugins/README.md` for tone, build prerequisites (cargo-component 0.21.1, wasm-tools 1.263.0), "Why wasm32-unknown-unknown", Imports/fetcher layer/Known limits sections to link or repeat (the guide's fetcher section must repeat the "third-party fetcher sees all headers" limit). WIT distribution commands: RESEARCH Code Examples "WIT distribution lines" (tag pending, see below).

### `examples/plugins/.env.example`, `.gitignore`, `plugins/README.md`

Add empty-value keys `REDDIT_CLIENT_ID=` and `REDDIT_USER_AGENT_CONTACT=` following the existing `REDDIT_CLIENT_SECRET=` line style. `.gitignore`: add `.live-e2e/` next to `.fili-state/` (the "Plugin host cursor state" block). `plugins/README.md`: add a fifth `e2e_` group to "Running the host tests" with the `-- --nocapture` note, and list replay-fetcher/template with the "never register it for a real source" wording copied from the stub-fetcher entry.

## Shared Patterns

### Plugin crate scaffold
**Source:** `plugins/stub-source/{Cargo.toml,.cargo/config.toml,rust-toolchain.toml,.gitignore}`
**Apply to:** replay-fetcher, template. Own empty `[workspace]`; `wit-bindgen-rt = { version = "0.41", features = ["bitflags"] }`; `src/bindings.rs` and `target/` gitignored; commit `Cargo.lock`. Root `Cargo.toml` already has `exclude = ["plugins"]`, no edit.

### Test cleanliness / no secrets in output
**Apply to:** integration_tests.rs, replay fetcher, scripts. Secrets only via env closure; never `{:?}` a request/response; stub-source logs alias names only (lib.rs 62-71).

### Scratch topics only
**Source:** `sink.rs` `live_support` (doc: "They never touch `filimon-social-items`"). Never call `FluvioSink::connect_social()` in tests.

### Real clock for Fluvio I/O
**Source:** sink.rs doc on `live_host_end_to_end_stub_to_fluvio_with_restart`.

## No Analog Found

| File | Role | Data Flow | Reason |
|---|---|---|---|
| `plugins/replay-fetcher/src/fixtures.rs` | data table | transform | New seam; use RESEARCH Code Examples (`TABLE: &[(&str, u16, &[u8])]` with `include_bytes!("../../reddit/tests/fixtures/listing_new.json")`, `.../youtube/tests/fixtures/playlist_page.json`) |
| `scripts/*.sh` | script | batch | `scripts/` absent; follow RESEARCH Patterns 5 and 7 |
| `evidence/` sample + transcript | evidence | file-I/O | Produced by human run (D-07/D-08) |

## Planner Notes

- Tag for D-11 does not exist yet (`git tag` empty); needs a human-action checkpoint, name confirmed with user (suggest `wit-v0.1.0`). Script defaults to local `git archive HEAD wit`.
- Reddit credentials (D-09) and GLiNER model files are human prerequisites; plan them first as checkpoints.
- Fixture files to reuse are all tracked: `plugins/reddit/tests/fixtures/listing_new.json`, `plugins/youtube/tests/fixtures/playlist_page.json`.

## Metadata

**Analog search scope:** `plugins/`, `src/plugins/host/`, `src/plugins/auth/testing.rs`, `tests/`, `examples/plugins/`
**Pattern extraction date:** 2026-11-12
