# Phase 3: Component Host Runtime - Research

**Researched:** 2026-11-09
**Domain:** Wasmtime component-model host embedded in an existing Rust/tokio/Fluvio CLI (`fili`)
**Confidence:** HIGH for the wasmtime host mechanics (every one was run in a spike against the real stub components); MEDIUM for the toolchain-change plan (needs a human action) and for a few judgement calls listed in the Assumptions Log.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Rate limiting and backoff (HOST-03)**
- **D-01:** On `rate-limited(Some(retry_after))` the host waits `max(retry_after, current backoff step)`, capped at the plugin's `[host.backoff] max_secs`. With `None` it uses the backoff step alone.
- **D-02:** Backoff is exponential: `initial_secs * multiplier^n` up to `max_secs`, advancing on each consecutive `transient` / `rate-limited` / `auth-required` failure. **One successful poll resets it fully.** After the backoff window the plugin is polled again, then returns to `poll_interval_secs`. No jitter.

**Failure handling (HOST-05)**
- **D-03:** `fatal` and `config` poll/init errors **disable that plugin instance until `fili` restarts** (matches the WIT doc: "host stops polling this instance"). Log loudly with plugin name and error text. `auth-required` is treated as backoff, like `transient`, because Phase 6 can fix it live.
- **D-04:** A **trap** (panic, unreachable, OOM) discards the instance. The host builds a fresh instance and calls `init` again after the backoff window. A trap counts as a transient failure for backoff. It is logged with the plugin name. Other plugins and the `fili` process are unaffected.

**Cursor persistence (HOST-02)**
- **D-05:** One **JSON file per plugin alias** in a state directory, written atomically (temp file + rename). The cursor is stored as the opaque bytes from Phase 1 D-01 (encoded as hex or base64 in the file). — **Reversibility:** costly — the state file format and layout is what restarts depend on.
- **D-06:** State dir defaults to **`.fili-state/` next to the registry file** (same relative-to-registry convention as component paths). Overridable by a `--state-dir` flag / `FILI_STATE_DIR` env var.
- **D-07:** The cursor is committed **only after the items are produced to Fluvio and flushed**. Delivery is at-least-once: a crash between flush and persist replays items on restart. No dedupe in this phase (v2 HOST-07).
- **D-08:** A corrupt or unreadable cursor file **disables only that plugin** with an error that names the plugin and the file path. Other plugins keep polling. The operator deletes the file to reset. The host never silently starts over from `none`.

### Claude's Discretion (not discussed — researcher/planner decide)
- **WASI policy (open blocker).** Phase 1 plugins target `wasm32-unknown-unknown` (no `wasi:*` imports). Research must decide whether the host rejects wasip1-built components with a clear error or links a locked-down WASI context. Default lean: reject with a clear error (smallest attack surface, matches PROJECT.md "every capability is an explicit WIT import").
- **Host entry point.** How Phase 3 is driven before Phase 4's run command exists (library API plus integration test harness vs. a minimal internal subcommand). Must not pre-empt the Phase 4 CLI surface.
- **Interim imports.** `fetch` returns `fetch-error::not-configured` until Phase 5; `log` maps to `tracing` with the plugin name. Access token is passed as `none` until Phase 6. What "loading" a fetcher component means before Phase 5 (validate it instantiates against the `fetcher` world, no routing) is left to research.
- Wasmtime configuration, async vs sync host, scheduler structure (per-plugin tasks), item validation (missing id/url) and the WIT item to `SourceItem` mapping (published epoch-ms to RFC 3339 per Phase 2 D-09), Fluvio message key choice.

### Deferred Ideas (OUT OF SCOPE)
- Per-plugin CPU/fuel and memory limits — v2 HOST-06
- Cross-poll item dedupe (needed to remove at-least-once duplicates) — v2 HOST-07
- Backoff jitter and gradual backoff decay — not needed yet; revisit if flapping APIs show up in Phase 8 live runs
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| HOST-01 | `fili` loads source and fetcher components listed in an explicit registry (`filimon.toml`); nothing unlisted is loaded | Loader consumes only `LoadedRegistry` enabled entries; `Component::from_file` is called per listed path only; `instantiate_pre` against the matching world validates without running guest code (spike-verified). Section "Discretion Resolutions" 1, 3 |
| HOST-02 | Polls each enabled source on its interval, persists the returned cursor, resumes from it after restart | Per-plugin tokio task with fixed-delay `sleep`; JSON-per-alias cursor store with atomic temp+rename; commit-after-flush ordering. Patterns 2, 3; stub-source cursor behaviour makes the restart test deterministic |
| HOST-03 | Per-plugin TOML sets poll interval and backoff on errors / HTTP 429 | `BackoffConfig`/`HostConfig` already validated (multiplier finite and >= 1.0, max >= initial); pure `backoff` function; Pattern 4 |
| HOST-04 | Items returned by plugins are produced to Fluvio as `SourceItem` records | `SourceItem::social(...)` is the 1:1 WIT-item mapping; `TypedProducer::send` + `flush`; sticky-producer-error behaviour of Fluvio 0.50.1 found and handled. Discretion Resolutions 6 |
| HOST-05 | A plugin that traps or returns an error is logged and backed off without crashing `fili` or stopping other plugins | Trap detection via `downcast_ref::<wasmtime::Trap>`, poisoned-instance behaviour (`CannotEnterComponent`) verified; fresh-instance rebuild; epoch-yield so a spinning guest cannot wedge a tokio worker; `tracing` has **no subscriber installed today** (must fix). Pitfalls 1-3 |
</phase_requirements>

## Summary

The host is a small, well-understood wasmtime embedding. I built throwaway hosts in `/tmp` (outside the repo) against the repo's real `stub-source` and `stub-fetcher` components and a purpose-built fault-injection component, on wasmtime **36.0.20** and **48.0.8**. Everything the plan needs worked: `bindgen!` for both the `source` and `fetcher` worlds (sharing the `types`/`log` interfaces through `with:`), `instantiate_pre` validation without running guest code, async `init`/`poll` calls from spawned tokio tasks, trap detection, the "instance is poisoned after a trap" behaviour that makes D-04 mandatory, and cooperative cancellation of a spinning guest via epoch interruption. An unlinked WASI import already produces a clear wasmtime error, so "reject WASI" costs nothing: do not link WASI, preflight the import list, and wrap the error with a build hint.

Three findings change the plan and the planner must not miss them. (1) **Toolchain:** the repo is pinned to Rust 1.85 by a local `rustup override`, and under an edition-2024 resolver-3 crate with `rust-version = "1.85"` cargo silently resolves `wasmtime` to **34.0.2**, which is out of support and predates a run of 2026 wasmtime security advisories. Any supported wasmtime needs Rust >= 1.86 (36 LTS) or >= 1.95 (48 LTS). The root `fili` crate already compiles cleanly with wasmtime 48.0.8 on rustc 1.96.0 (checked in a copy of the repo); the 1.85 pin is only needed for the SmartModule crates, which can be given their own nested `rust-toolchain.toml`. This needs a human action (`rustup override unset`) and is a checkpoint. (2) **Logging:** `fili` never installs a `tracing` subscriber (`grep -rn "subscriber\|EnvFilter" src` returns nothing), so every `tracing::warn!` the host emits for D-03/D-04 would go nowhere today. (3) **Fluvio producer errors are sticky:** in fluvio 0.50.1, after a produce failure `send` keeps returning the stored error until `TopicProducer::clear_errors()` is called, and `TypedProducer` does not expose it.

**Primary recommendation:** Build `src/plugins/host/` as a library-style module (no new subcommand), async on wasmtime 48.0.x LTS with epoch-yield enabled, one tokio task per enabled source, a `ItemSink` trait so all scheduler/backoff/cursor behaviour is tested with a fake sink and paused time, one new fault-injection fixture plugin, and gate the toolchain change behind a `checkpoint:human-action` in Wave 0.

## Project Constraints (from CLAUDE.md)

`./.claude/CLAUDE.md` only `@`-includes `AGENTS.md` and `WORKFLOW.md` (GSD Path pipeline, inactive here: no `.project/`). Rules that still apply to this run: claims need checked sources; report partial or failed results plainly; record uncertainty with confidence levels; never report a command as passing unless it ran. Project conventions from `.planning/PROJECT.md` and the code, treated as binding:

- Never modify the `fluvio/` reference checkout; do not fix existing `fluvio-wise` gaps (watch mode, silent drops, batch-only NER) [CITED: .planning/PROJECT.md "Out of Scope"].
- Plugins have no direct network; capabilities are explicit WIT imports [CITED: .planning/PROJECT.md "Constraints"].
- `miette` diagnostics name plugin and field/key; secrets never in Debug or logs (`Secret` newtype, `.expose()` only where handed to the owning plugin) [VERIFIED: src/plugins/secrets.rs:20-37, "Debug output is redacted to `Secret(***)`"].
- Diagnostics never carry record content; stderr text goes through `output::sanitize_for_terminal` [VERIFIED: src/output.rs:49 `pub fn sanitize_for_terminal(s: &str) -> String`].
- Lints: `unsafe_code = "warn"`, clippy `all` and `pedantic` = warn [VERIFIED: Cargo.toml:8].
- Tests are co-located `#[cfg(test)] mod tests`; the crate is **bin-only** (`[[bin]] name = "fili"`), so there is no `tests/` integration directory that can import it. Cluster-dependent tests are `#[ignore = "..."]` [VERIFIED: src/commands/consume.rs:2477 `#[ignore = "needs a local Fluvio cluster with the filimon SmartModules loaded"]`].
- `tokio` with `test-util` is available for paused-time tests [VERIFIED: Cargo.toml:46].

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Registry-driven component loading, WIT-world validation | Host process (`fili`) | — | HOST-01: only `LoadedRegistry` enabled entries are opened |
| WASI/unknown-import rejection | Host process (wasmtime Linker + import preflight) | — | The Linker is the capability boundary; no WASI is linked |
| Scheduling (interval, backoff, per-plugin isolation) | Host process (tokio tasks) | — | Plugins are stateless and never schedule themselves |
| Cursor persistence | Host filesystem (`.fili-state/`) | — | Host owns cursor (PROJECT.md key decision) |
| Item validation and WIT-item to `SourceItem` mapping | Host process | — | Plugin output is untrusted input |
| Produce to `filimon-social-items` | Fluvio client (`TypedProducer`) | Fluvio cluster | Existing typed producer; topic created by operators |
| Guest `log` import | Host process (`tracing`) | — | Sanitize guest text, tag with plugin name |
| Guest `fetch` import | Host process (interim `not-configured`) | Phase 5 fetcher layer | Real routing is Phase 5 |
| Poll-time cancellation / anti-wedge | Wasmtime engine (epoch interruption) | tokio | Cooperative yield makes guests cancellable |

## Standard Stack

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `wasmtime` | `48` (resolved 48.0.8; MSRV 1.95.0, published 2026-08-20 for 48.0.0) | Component-model host | Reference runtime; 48 is an LTS line (every major divisible by 12), supported 24 months, security fixes backported [VERIFIED: crates.io API + CITED: docs.wasmtime.dev/stability-release.html] |
| `tokio` | already `1` (`full`, dev: `test-util`) | Per-plugin tasks, `sleep`, `select!`, signals | Already a dependency |
| `base64` | `0.22` (already in Cargo.lock as 0.22.1 transitively) | Cursor bytes <-> JSON string | Standard, 38M weekly downloads |
| `tokio-util` | `0.7` (already in Cargo.lock as 0.7.18 transitively) | `CancellationToken` for host shutdown | Standard tokio companion; avoids hand-built shutdown plumbing |
| `tempfile` | already `3` (regular dependency) | `NamedTempFile::new_in(dir).persist(path)` for atomic cursor writes | Already used |
| `serde`/`serde_json`, `tracing`, `miette`, `thiserror`, `url`, `chrono` | already present | State file, logs, diagnostics, URL check, timestamps | Existing conventions |

wasmtime feature set that was compiled and run in the spike: `default-features = false, features = ["component-model", "cranelift", "runtime", "async", "std", "parallel-compilation"]`. Do not enable `wat` (text components would then load), `cache`, or `pooling-allocator`.

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| wasmtime 48 LTS (MSRV 1.95) | wasmtime 36 LTS (36.0.20, MSRV 1.86, supported until about 2027-08) | Smaller toolchain jump but ends in about 9 months. Also needs `Config::async_support(true)`, which 48 deprecates as a no-op. Fall back to this only if the user refuses toolchain >= 1.95. |
| wasmtime 34.0.2 (what cargo picks under rust-version 1.85) | — | **Rejected:** not a supported release; Debian's tracker lists multiple 2026 wasmtime RUSTSEC advisories. Do not let the resolver pick it. |
| `tokio_util::CancellationToken` | `tokio::sync::watch<bool>` | Zero new direct dependency, slightly clumsier; either is acceptable. |
| Link a locked-down WASI context | Reject WASI imports | See Discretion Resolutions 1. |

**Installation:**
```bash
# Cargo.toml [dependencies] additions (after the toolchain checkpoint)
wasmtime = { version = "48", default-features = false, features = ["component-model", "cranelift", "runtime", "async", "std", "parallel-compilation"] }
base64 = "0.22"
tokio-util = "0.7"
# Cargo.toml: rust-version = "1.95"   (was "1.85")
```

**Version verification (this session):** `crates.io` API: wasmtime `48.0.8` (2026-10-29, rust_version 1.95.0), `36.0.20` (1.86.0), `34.0.2` (2025-07-18, 1.85.0), `50.0.1` (1.97.0, newest stable, above the installed 1.96 toolchain). `cargo +1.85.0 generate-lockfile` on an edition-2024 crate with `rust-version = "1.85"` printed `Adding wasmtime v34.0.2 (available: v50.0.1, requires Rust 1.97.0)`. In a copy of this repo, `cargo +1.96.0 check` with the three additions above finished with exit 0 and `Cargo.lock` containing `wasmtime 48.0.8` next to `fluvio 0.50.1` (no dependency conflict).

## Package Legitimacy Audit

| Package | Registry | Age | Downloads | Source Repo | Verdict | Disposition |
|---------|----------|-----|-----------|-------------|---------|-------------|
| wasmtime | crates.io | created 2018-10-30 | about 982k/wk (43.9M total) | github.com/bytecodealliance/wasmtime | OK (second run; the first run returned SUS with all-null signals, a lookup failure) | Approved. Name confirmed by official docs (docs.wasmtime.dev) and the crates.io API; `rust_version` and release dates read from the API. |
| base64 | crates.io | 2015-12-04 | about 38.7M/wk | github.com/marshallpierce/rust-base64 | OK | Approved (already in Cargo.lock) |
| tokio-util | crates.io | 2018-02-01 | about 17.8M/wk | github.com/tokio-rs/tokio | OK | Approved (already in Cargo.lock) |

**Packages removed due to [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none after re-run. If the planner re-runs `gsd_run query package-legitimacy check --ecosystem crates wasmtime` and gets the null-signal SUS result again, retry once; it is a transient lookup failure, not a property of the crate.

## Discretion Resolutions

Each item gives a recommendation, confidence, and evidence. Locked decisions D-01..D-08 were not revisited.

### 1. WASI policy: **reject**, do not link WASI (HIGH)
- Do not add `wasmtime-wasi`. Link only the `filimon:plugin/*` interfaces. A component importing anything else fails `instantiate_pre`.
- Spike: a component importing `wasi:cli/environment@0.2.0` fails with `component imports instance \`wasi:cli/environment@0.2.0\`, but a matching implementation was not found in the linker` [VERIFIED: spike run, wasmtime 36.0.20 and 48.0.8].
- Improve the message by preflighting `component.component_type().imports(&engine)` (iteration verified: it yields `filimon:plugin/types@0.1.0`, `filimon:plugin/log@0.1.0`, `filimon:plugin/fetch@0.1.0` for stub-source) and collecting every import whose name does not start with `filimon:plugin/`. Report all of them in one miette error naming the plugin, plus the hint "build for wasm32-unknown-unknown; see plugins/README.md".
- Rationale: matches PROJECT.md "Plugin components target wasm32-unknown-unknown (no WASI imports) - Every capability is an explicit WIT import" and the Phase 1 plugin README ("Guest code on this target has no filesystem, environment, clock, or stdout").
- Reversible: allowing a locked-down WASI later is additive; rejecting later would be breaking. That asymmetry favours starting strict.

### 2. Host entry point: library-style module, **no new subcommand** (HIGH)
- Expose `plugins::host::Host::load(&LoadedRegistry, HostOptions) -> Result<Host>` and `Host::run(self, sink, shutdown: CancellationToken) -> HostReport`. Phase 4's `fili plugins run`/run command calls `run`; its `check` calls the same loader.
- Drive Phase 3 only from in-module tests (the crate is bin-only; there is no `tests/` target). Ignored `live_` tests cover the real Fluvio topic.
- Do add the `--state-dir` / `FILI_STATE_DIR` global flag next to `--registry` (D-06 asks for it; the pattern is `#[arg(long, global = true, env = "FILI_REGISTRY", default_value = DEFAULT_REGISTRY)]` at src/cli.rs:40). That is a flag, not a subcommand, so it does not pre-empt Phase 4's command surface.
- `src/plugins/mod.rs` already has `#![cfg_attr(not(test), allow(dead_code, reason = "consumed by the plugin host and CLI in Phases 3-4"))]` (lines 10-16), which covers the new `host` submodule while nothing in `main` calls it.

### 3. Interim imports and what "loading" means (HIGH)
- `fetch::Host::fetch` returns `Ok(Err(FetchError::NotConfigured))`. [VERIFIED: wit/types.wit:80 `not-configured,` under "/// No fetcher is assigned to this source (Phase 3 interim)."] Spike: stub-source's `init` with `fetch_url` set logged `init fetch failed: FetchError::NotConfigured` and `init` still returned `Ok(())`.
- `log::Host::log` maps the WIT level to a `tracing` event with a `plugin` field (see Pattern 5).
- `access-token`: always `None` (D-07 of Phase 1, Phase 6 replaces it).
- **Loading a fetcher** = `Component::from_file`, import preflight, then `FetcherPre::new(linker.instantiate_pre(&component)?)`. This checks that every import is provided and that the `fetch` export exists with the right type, runs no guest code, and keeps the `FetcherPre` (cheap, reusable) in a `LoadedFetchers` map for Phase 5. Spike: stub-fetcher passes; stub-source loaded as a fetcher is rejected (`component imports instance \`filimon:plugin/fetch@0.1.0\` ... wrong type`); stub-fetcher loaded as a source fails `no export \`init\` found`. The fetcher linker needs `outbound-http::send` defined (return `Err(FetchError::NotConfigured)`), otherwise `instantiate_pre` fails.
- Loading **sources**: same, with `SourcePre`; instantiate later per plugin task.
- Load every **enabled** fetcher in the registry (SC1: "Source and fetcher components listed and enabled ... are loaded"). A fetcher that fails to load is reported by name in the `HostReport` and logged; it does not stop sources in this phase (no routing exists yet). Phase 5 should decide to disable dependents. [A7]

### 4. Wasmtime configuration and async vs sync (HIGH)
- **Async.** Phase 5's `fetch` import must do async HTTP and call a fetcher component from inside a host import; sync would force `block_on` inside a guest call. Spike: `SourcePre::instantiate_async` plus `call_init`/`call_poll` inside `tokio::spawn`ed tasks (futures are `Send`), two plugins running concurrently.
- `bindgen!` syntax valid on both 36 and 48: `imports: { default: async | trappable }, exports: { default: async }` (the older `async: true` form is a compile error on 36.0.20).
- On 48 do **not** call `Config::async_support` (deprecated, "no longer has any effect"). On 36 it is required. Write for 48.
- **Epoch interruption: enable it, as an anti-wedge measure, not a resource limit.** `cfg.epoch_interruption(true)`; per Store `store.set_epoch_deadline(1); store.epoch_deadline_async_yield_and_update(1);`; one engine-level ticker calling `engine.increment_epoch()` every 10 ms **on a dedicated `std::thread`** (not a tokio task, so paused-time tests are unaffected).
  - Evidence (wasmtime 36.0.20 and 48.0.8): a guest stuck in `loop {}` was cancelled by `tokio::time::timeout(300ms, call_poll)` after 300.29 ms / 309.69 ms while the other plugin kept polling in microseconds. With the epoch lines removed, the same program **hung until killed** (`exit=124`), the timeout never fired, and the worker thread stayed wedged.
  - This does not add a CPU limit or kill a looping plugin (that stays HOST-06). It only makes every `call_*` future yield, so shutdown can drop it and other plugins get CPU. A looping plugin still occupies its own task forever; record that as a known gap (Open Question 3).
- No `StoreLimits`/fuel/memory caps (HOST-06). Record as an accepted risk in the security section.
- One `Engine`, one `Linker<HostState>`, one `*Pre` per component, one `Store<HostState>` per live instance. `HostState` holds the plugin name (`Arc<str>`); no `ResourceTable` and no WASI ctx are needed.
- Compile cost: stub-source compiles and runs in about 1 s wall in a debug host. Tests should build the `Engine` and compile components once per test binary (`OnceLock`).

### 5. Scheduler structure (HIGH)
One `tokio::spawn` per enabled source; fixed-delay timing; first poll immediately at start. Details in Pattern 2 and the outcome table below. Fixed-delay (`sleep(interval)` after the previous cycle completes) guarantees "never call poll concurrently on one instance" structurally and makes paused-time tests exact.

| Outcome of a cycle | Host action |
|---|---|
| `Ok(items)` | validate/map, send all, flush, **then** persist `next-cursor`, reset backoff, wait `poll_interval` |
| `Err(rate-limited(Some(n)))` | wait `min(max(n, step), max)`; failure count + 1 |
| `Err(rate-limited(None))`, `Err(transient)`, `Err(auth-required)` | wait `step`; failure count + 1 |
| `Err(config)`, `Err(fatal)` from `init` or `poll` | plugin **disabled until restart**; error-level log with plugin name and (sanitized) text |
| Any `Err` from the wasmtime call itself (trap, `CannotEnterComponent`, host error) | drop the Store/instance; wait `step`; failure count + 1; rebuild instance and call `init` again after the wait |
| `init` returns transient/rate-limited/auth-required | discard that instance (init is once per instance), back off, rebuild and re-init |
| Fluvio send or flush fails (host-side) | cursor **not** committed; `clear_errors()`; log as a Fluvio failure (not blamed on the plugin); wait `step` with the plugin's backoff; same cursor re-polled |
| Cursor file corrupt/unreadable at start | plugin disabled with an error naming plugin and file path (D-08) |

`step` = `backoff(initial, multiplier, max, failures_before_this_one)`. D-02 as written means the backoff window can be shorter than `poll_interval_secs` (for example the sample reddit.toml has initial 60 s vs interval 300 s). Implement D-02 literally and note it in the code comment. [A10]

### 6. Item validation, mapping and Fluvio key (MEDIUM-HIGH)
- **Mapping:** call `SourceItem::social(id, url, title, text, author, published_ms, platform, raw)`. It already converts epoch ms to RFC 3339 via `published: published_ms.and_then(epoch_ms_to_rfc3339)` (src/models.rs:184, out-of-range gives `None`) and parses `raw` JSON, falling back to a JSON string when it is not valid JSON. [VERIFIED: src/models.rs:166-190] Remove the `#[cfg_attr(not(test), allow(dead_code ...))]` on `social` only if the host module is reachable from `main`; otherwise leave it (the `plugins` module-level allow does not cover `models.rs`).
- **Validation (host side, per item, drop-with-warning, never fail the poll):** reject empty/whitespace-only `id` or `url`; require `url` to parse with `url::Url::parse`; reject items whose serialized JSON exceeds a safe record size. The Fluvio producer default `max_request_size` is `1_048_576` and a record over the limit returns `ProducerError::RecordTooLarge` from `send` [VERIFIED: fluvio-0.50.1/src/producer/config.rs:25 `const DEFAULT_MAX_REQUEST_SIZE: usize = 1_048_576;`, memory_batch.rs:59-65]. Use a cap of 1_000_000 bytes of JSON. Empty `platform`: fall back to the registry plugin name. Log one warn per dropped item naming plugin and reason, never record content (Phase 2 convention). The consumer does no id/url validation [VERIFIED: grep of src/commands/consume.rs found none], so the host is the only gate. [A5]
- Dropped items do not block the cursor: the poll still counts as success. Surface a per-plugin `dropped` counter in the `HostReport`.
- **Fluvio key:** `format!("{platform}:{id}")`. Rationale: stable per item, platform-scoped so two plugins on one platform produce equal keys for the same item (helps v2 dedupe/compaction), and the default topic has one partition so per-poll order is preserved regardless of key. Alternative (key = plugin alias) preserves per-plugin order on multi-partition topics; choose it only if operators will create multi-partition topics. [A4]

### 7. Test fixtures (HIGH on need, MEDIUM on naming)
Stub-source cannot fail, trap, or rate-limit, and plugins are stateless by contract, so SC3/SC4 need a fault-injection component. Create `plugins/fault-source/` (own `[workspace]`, own `rust-toolchain.toml` 1.96.0, own `.cargo/config.toml` targeting `wasm32-unknown-unknown`, `.gitignore` for `/target/` and `/src/bindings.rs`, exactly like stub-source). Its `[settings]` TOML selects a `mode`: `ok` (echo cursor, one item per poll with a per-instance counter id), `rate-limited` (with `retry_after`), `transient`, `auth-required`, `fatal`, `config`, `init-fatal`, `trap` (panic), `spin`. A guest `thread_local` counter persists across calls on one instance (verified: ids `f1`,`f2`,`f3`), which is enough for "fail the first N polls of this instance". After a trap the counter resets with the fresh instance, so "trap then recover" is tested in the scheduler unit tests with a fake instance factory, and the wasm-level test asserts trap detection, poisoning, isolation and logging only. A prototype of this crate was built and run in `/tmp/fault-source` (not in the repo) and produced every behaviour listed above.

`.wasm` files are gitignored (`plugins/stub-source/.gitignore: /target/`; verified with `git check-ignore`), so wasm-backed tests need the fixtures built first. Mark them `#[ignore = "needs plugin fixtures: see plugins/README.md"]` with a `wasm_` name prefix, and Fluvio ones `live_`, consistent with the existing `#[ignore]` pattern. Everything else (backoff math, scheduler, cursor store, mapping, load-set) must run in plain `cargo test` with no wasm and no cluster.

## Architecture Patterns

### System Architecture Diagram

```
 filimon.toml + per-plugin TOML + .env
          |  load_registry (Phase 1)
          v
   LoadedRegistry ----------------------------------------------+
          | enabled sources / enabled fetchers only (HOST-01)    |
          v                                                      |
 +--------------------+   import preflight    +----------------+ |
 | ComponentLoader    |--- non-filimon:plugin |  load report   | |
 | Engine (epoch on)  |    import => reject ->|  (per plugin   | |
 | from_file + *Pre   |                       |  ok / error)   | |
 +--------------------+                       +----------------+ |
          | SourcePre (per source)   FetcherPre (kept for Ph.5)   |
          v                                                       |
 +-------------------------- Host::run (shutdown token) --------------------------+
 |  per enabled source: tokio task                                                  |
 |   [read cursor file] --corrupt--> DISABLED(plugin, path)                          |
 |        v                                                                         |
 |   build Store + instantiate + init(settings, secrets) --fatal/config--> DISABLED |
 |        v                    ^---------- trap / any wasmtime Err: drop instance --+|
 |   +--> wait (interval | backoff step | retry-after) --select-- shutdown          ||
 |   |        v                                                                     ||
 |   |   poll(cursor, None) ----Err(transient/rl/auth)----> failures+1, backoff ----+|
 |   |        v Ok(items, next)                                                      |
 |   |   validate + map -> SourceItem (drop bad items, warn)                         |
 |   |        v                                                                      |
 |   |   ItemSink.send_batch (send each, flush) --Err--> clear_errors, backoff       |
 |   |        v Ok                                                                   |
 |   |   persist next-cursor (tmp+rename) ; reset backoff                            |
 |   +-------+                                                                       |
 +-----------------------------------------------------------------------------------+
          |                                           guest imports (host side):
          v                                            log   -> tracing (plugin=..)
   Fluvio topic filimon-social-items                  fetch -> Err(NotConfigured)
```

### Recommended Project Structure
```
src/plugins/
├── config.rs, error.rs, secrets.rs     # existing (Phase 1)
├── mod.rs                              # add `pub mod host;`
└── host/
    ├── mod.rs        # Host, HostOptions, HostReport, public entry points
    ├── engine.rs     # Config/Engine, epoch ticker thread, bindgen! modules, HostState, log+fetch imports
    ├── loader.rs     # ComponentLoader: from_file, import preflight, SourcePre/FetcherPre; records opened paths
    ├── instance.rs   # LiveSource (Store + Source), call classification, InstanceFactory trait for tests
    ├── scheduler.rs  # per-plugin task state machine
    ├── backoff.rs    # pure functions (no clock, no IO)
    ├── cursor.rs     # CursorStore: read/write JSON, atomic write, dir lock
    ├── sink.rs       # ItemSink trait, FluvioSink, test VecSink
    └── map.rs        # validate + WIT item -> SourceItem
plugins/fault-source/   # new fixture crate (Discretion Resolutions 7)
```
Add `.fili-state/` to the repo `.gitignore` (the default state dir sits next to the registry, e.g. `examples/plugins/.fili-state/`). [A11]

### Pattern 1: Two bindgen modules sharing types
**What:** One module per world; the second reuses the first's `types` and `log` so Host impls are written once.
**When to use:** Always here; `source` and `fetcher` worlds share interfaces.
**Example:**
```rust
// Source: spike verified on wasmtime 36.0.20 and 48.0.8 against wit/ at repo root.
pub mod source_world {
    wasmtime::component::bindgen!({
        path: "wit",                    // relative to CARGO_MANIFEST_DIR
        world: "source",
        imports: { default: async | trappable },
        exports: { default: async },
    });
}
pub mod fetcher_world {
    wasmtime::component::bindgen!({
        path: "wit",
        world: "fetcher",
        imports: { default: async | trappable },
        exports: { default: async },
        with: {
            "filimon:plugin/types": super::source_world::filimon::plugin::types,
            "filimon:plugin/log": super::source_world::filimon::plugin::log,
        },
    });
}
// Loading (no guest code runs):
let mut linker: Linker<HostState> = Linker::new(&engine);
source_world::Source::add_to_linker::<HostState, HasSelf<HostState>>(&mut linker, |h| h)?;
let pre = source_world::SourcePre::new(linker.instantiate_pre(&component)?)?;
// Per live instance:
let mut store = Store::new(&engine, HostState::new(name));
store.set_epoch_deadline(1);
store.epoch_deadline_async_yield_and_update(1);
let src = pre.instantiate_async(&mut store).await?;
let r = src.call_init(&mut store, &settings_toml, &secrets).await?;   // outer Err = wasmtime/trap, inner = PollError
```
The `wit/` directory's three files all declare `package filimon:plugin@0.1.0;`. Wrap the generated modules in `#[allow(clippy::all, clippy::pedantic)]` (generated code and `pedantic = "warn"`). [A13: clippy interaction not run.]

### Pattern 2: Per-plugin task with a pure state machine
**What:** `async fn run_plugin(ctx, shutdown)` loops `wait -> poll -> classify -> (produce+commit | backoff | disable)`. Make the classification a pure function `fn next_action(state: &mut PluginState, outcome: CallOutcome) -> Action` so D-01..D-04 are table-testable with no clock and no wasm.
**Cancellation rule:** `select!` (biased toward `shutdown`) only around the *wait* and the *guest call*. Once `poll` returned `Ok`, run send, flush and cursor persist to completion (bounded by a host timeout), so a stop cannot split "flushed" from "persisted" more than D-07 already permits.
**Fixed-delay + immediate first poll:** first cycle has zero wait; later waits are `poll_interval` after success, the backoff `step` after failure.

### Pattern 3: Cursor store
```rust
// State file: <state_dir>/<alias>.json ; alias is [a-z0-9_-]+ (path-safe, see below)
// {"version":1,"plugin":"stub","cursor":"AQ=="}   // cursor: base64 of the opaque bytes, or null
//   no file            => first poll (cursor = none)
//   cursor: null       => plugin returned next-cursor none; next poll is a first poll
//   unreadable / bad JSON / bad base64 / "plugin" mismatch => plugin DISABLED (D-08), error names plugin + path
fn write_cursor(dir: &Path, alias: &str, cursor: Option<&[u8]>) -> std::io::Result<()> {
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;       // same filesystem as the target
    serde_json::to_writer(&mut tmp, &StateFile::new(alias, cursor))?;
    tmp.as_file().sync_all()?;                                  // durability before rename
    tmp.persist(dir.join(format!("{alias}.json"))).map_err(|e| e.error)?;
    // best effort on unix: open `dir` and sync_all() it so the rename is durable
    Ok(())
}
```
- Plugin names are path-safe by construction: `.all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')` over a non-empty name [VERIFIED: src/plugins/config.rs:388-393]. Still build the path with `join(format!("{alias}.json"))` only; never accept an alias from anywhere but `LoadedRegistry`.
- Use plain `std::fs` (tiny files). tokio documents that a running `spawn_blocking` task inhibits paused-clock auto-advance, so `spawn_blocking` would also be safe in paused-time tests; plain `std::fs` is simpler. [CITED: docs.rs/tokio time::pause]
- Version the file (`"version": 1`) and do not use `deny_unknown_fields` on read, so Phase 4 can add fields such as a last-poll status (CLI-01) without breaking restarts. [A15]
- Optional single-instance guard: `File::try_lock` on `<state_dir>/.lock` (std, stable on the 1.95+ toolchain; verified on 1.96.0: a second handle's `try_lock` fails). Refuse to start with a clear error if held. Two `fili` processes on one state dir would double-poll and clobber cursors. [A8]
- Persist failure (disk full, permissions): log error naming plugin and path, keep the in-memory cursor so the running host does not re-poll the same items, keep going. Restart would replay (consistent with at-least-once). [A6]

### Pattern 4: Backoff as a pure function
```rust
// Config guarantees (verified): multiplier finite and >= 1.0; max >= initial; both non-zero.
fn step(cfg: &BackoffConfig, failures_before: u32) -> Duration {
    let exp = i32::try_from(failures_before).unwrap_or(i32::MAX);
    let secs = cfg.initial.as_secs_f64() * cfg.multiplier.powi(exp);   // may be +inf; fine
    Duration::from_secs_f64(secs.min(cfg.max.as_secs_f64()))            // +inf.min(max) == max
}
fn rate_limited_wait(retry_after_secs: Option<u32>, step: Duration, cfg: &BackoffConfig) -> Duration {
    retry_after_secs
        .map_or(step, |n| step.max(Duration::from_secs(u64::from(n))))
        .min(cfg.max)                                                   // D-01
}
```

### Pattern 5: Guest `log` import to `tracing`
```rust
// tracing macros need a const level, so match:
impl p::log::Host for HostState {
    async fn log(&mut self, level: p::log::Level, message: String) -> wasmtime::Result<()> {
        let msg = clip(&crate::output::sanitize_for_terminal(&message), 4096);   // guest text is untrusted
        let plugin = &*self.plugin;
        match level {
            Level::Trace => tracing::trace!(%plugin, "{msg}"),
            Level::Debug => tracing::debug!(%plugin, "{msg}"),
            Level::Info  => tracing::info!(%plugin, "{msg}"),
            Level::Warn  => tracing::warn!(%plugin, "{msg}"),
            Level::Error => tracing::error!(%plugin, "{msg}"),
        }
        Ok(())
    }
}
```
Level names are `trace, debug, info, warn, error` [VERIFIED: wit/host.wit `enum level { trace, debug, info, warn, error }`].

### Anti-Patterns to Avoid
- **Reusing an instance after any wasmtime `Err`.** After a trap every later call returns `CannotEnterComponent` (verified). Always rebuild.
- **Calling `init` again on the same instance** after a transient `init` error; the contract says once per instance.
- **Discovering `.wasm` files by directory scan** (HOST-01, PROJECT.md out of scope). Open only `LoadedSource.component` / `LoadedFetcher.component` of enabled entries.
- **One shared Fluvio producer with unsynchronised send+flush from many tasks.** Sticky errors and flush-waits-for-everything make failures misattributed. Use one `Arc<tokio::sync::Mutex<TypedProducer>>` and hold the lock across "send all + flush" (the same `Arc<Mutex<TypedProducer>>` shape as `PipelineCoordinator::raw_pages_producer`). Polls are minutes apart, so contention is negligible.
- **Persisting the cursor before flush** (violates D-07).
- **Logging `settings`, `secrets`, `init` arguments, or item content.**
- **Using `tokio::time::interval` for scheduling**: its catch-up semantics fight with backoff; fixed-delay `sleep` is simpler and test-exact.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| WIT bindings and canonical ABI | Manual lifting/lowering | `wasmtime::component::bindgen!` | Verified end-to-end; free type safety against `wit/` |
| Verifying a component fits a world without running it | Parsing the binary | `linker.instantiate_pre` + `SourcePre::new` / `FetcherPre::new` | Checks imports and exports (spike: right/wrong world both detected) |
| Component import listing | Custom wasm parser | `component.component_type().imports(&engine)` | Verified to enumerate names |
| Atomic file replace | Manual write-then-rename | `tempfile::NamedTempFile::new_in(dir).persist()` | Already a dependency; same-fs temp |
| Base64 | Custom hex/base64 | `base64` 0.22 | Standard |
| Shutdown signalling | Ad-hoc `AtomicBool` polling | `tokio_util::sync::CancellationToken` | `select!`-friendly |
| Cooperative guest yield | Fuel/own watchdog thread that kills | epoch interruption (`epoch_deadline_async_yield_and_update`) | Engine feature; verified |
| Terminal-safe text | New sanitizer | `output::sanitize_for_terminal` | Existing; Phase 2 convention |
| WIT item to `SourceItem` | New struct mapping | `SourceItem::social(...)` | Already does epoch-ms to RFC 3339 and raw JSON handling |
| Single-instance lock | PID files | `std::fs::File::try_lock` | In std on the new toolchain |

**Key insight:** every hard part (ABI, validation against a world, atomic persistence, time conversion) already exists in wasmtime, the repo or std. The only genuinely new logic is the outcome-to-action state machine, and it should be a pure function.

## Runtime State Inventory

Not a rename/refactor/migration phase. Omitted. The one piece of new persistent state (`.fili-state/<alias>.json`) is greenfield; there is no pre-existing state to migrate.

## Common Pitfalls

### Pitfall 1: Cargo silently picks an unsupported wasmtime
**What goes wrong:** `cargo add wasmtime` under `rust-version = "1.85"` resolves to 34.0.2.
**Why:** edition 2024 uses the MSRV-aware resolver; 34.0.2 is the newest wasmtime with MSRV <= 1.85 (verified).
**How to avoid:** Wave-0 checkpoint raises `rust-version` and the toolchain first; then pin `wasmtime = "48"`. Add a check (CI/grep) that `Cargo.lock` has wasmtime >= 48.
**Warning signs:** `Cargo.lock` shows `wasmtime 34.x`; any `Adding wasmtime vNN (available ...)` line.

### Pitfall 2: A trapped instance looks alive
**What goes wrong:** Reusing the Store after a trap yields `wasm trap: cannot enter component instance` forever (verified on 36 and 48).
**How to avoid:** Treat **any** `Err` from a `call_*` (as opposed to `Ok(Err(PollError))`) as instance-fatal. Detect trap kind with `e.downcast_ref::<wasmtime::Trap>()` (`UnreachableCodeReached` for a Rust panic on wasm32-unknown-unknown) for the log line only.
**Warning signs:** repeated `CannotEnterComponent` lines for one plugin.

### Pitfall 3: No tracing subscriber, so "logged" is a lie
**What goes wrong:** D-03/D-04/HOST-05 require logging with plugin name, but `fili` installs no subscriber. [VERIFIED: `grep -rn "subscriber\|EnvFilter" src` returned no matches; `tracing-subscriber` with `env-filter` is already in Cargo.toml.]
**How to avoid:** add `fn init_logging()` (use `try_init`, stderr writer, `EnvFilter` from `RUST_LOG`, default directive `filimon::plugins=info`) and call it from `main()` before dispatch. The default keeps other modules silent, so existing commands' output does not change. [A3]
**Tests:** capture with a scoped subscriber (`tracing::subscriber::with_default` over a `fmt` subscriber writing into `Arc<Mutex<Vec<u8>>>`) and assert the text contains `plugin=<alias>`. `with_default` is thread-local, so those tests must stay on the default current-thread `#[tokio::test]` runtime (paused time requires current_thread anyway). No new dependency.

### Pitfall 4: Fluvio producer errors are sticky
**What goes wrong:** after one failed batch, `TopicProducer::send` returns the stored error until `clear_errors()` runs; flush also keeps returning it. [VERIFIED: fluvio-0.50.1/src/producer/mod.rs:281-283 `if let Some(error) = producer_pool.last_error(partition).await { return Err(error.into()); }` and :693 `pub async fn clear_errors`; `flush_all_batches` returns the stored error at :184-188.] `TypedProducer` (src/streaming.rs:210-213) holds the producer privately.
**How to avoid:** add `pub async fn clear_errors(&self)` to `TypedProducer`; call it on the failure path. Pre-check item size so `RecordTooLarge` never happens mid-poll.
**Also:** `flush()` returning `Ok` is the delivery guarantee for D-07 (it returns the stored error, so Ok means no recorded error for those partitions). The existing `TypedProducer::send` discards the `ProduceOutput` future; that is acceptable only because `flush` is always called before the cursor commit.

### Pitfall 5: Producer hangs when the cluster is down
Fluvio's producer retries up to 300 s by default (`DEFAULT_RETRIES_TIMEOUT: Duration = Duration::from_secs(300)`, config.rs:27). Wrap "send all + flush" in a host `tokio::time::timeout` (constant, not plugin TOML), treat expiry as a host-side failure (no commit, backoff). Run `FluvioStream::preflight(&[TOPIC_SOCIAL_ITEMS], &[])` once at start (src/streaming.rs:155, produces "Create it with: fluvio topic create ..." text) and fail fast.

### Pitfall 6: Paused-time tests and wasm
Time pausing requires a `current_thread` runtime [CITED: docs.rs/tokio time::pause]. Epoch ticker must be a std thread (a tokio-timer ticker would be auto-advanced in 10 ms hops). Use `tokio::time::advance` plus `yield_now` loops to step the scheduler, and assert on a fake clock-independent log of `(plugin, call, cursor)` events.

### Pitfall 7: Guest strings in diagnostics
`PollError::{config,transient,fatal}(string)` and trap backtraces are guest-controlled. Sanitize (`sanitize_for_terminal`), clip length, and never put them in a miette `#[source_code]` block. Trap `Display` includes a multi-line wasm backtrace with symbol names; log the first line at `warn`/`error` and the full chain at `debug`.

### Pitfall 8: Fetcher/Source `*Pre` and Store `Send`
`Store<HostState>` futures were `Send` in `tokio::spawn` because `HostState` is `Send`. Do not put `Rc`/`RefCell` or a non-`Send` guard in `HostState`.

### Pitfall 9: Toolchain split breaks SmartModule builds
The root `rustup override` (1.85.0) is a local setting and shadows `rust-toolchain.toml` at the root directory (verified: an override at a directory beat a toolchain file in the same directory, but a nested `rust-toolchain.toml` in a subdirectory beat the parent's override). README states 1.96+ breaks SmartModule linking (`undefined symbol: copy_records` with fluvio-smartmodule 0.50.1). The SmartModules are built with `cd smartmodules/<name> && smdk build`, so nested pins work (see Open Question 1 for the unverified part).

## Code Examples

### Trap vs guest error classification
```rust
// Source: spike verified on wasmtime 36.0.20 and 48.0.8.
enum CallOutcome {
    Ok(PollResponse),
    Guest(PollError),          // Ok(Err(e)) from call_poll/call_init: instance still usable
    InstanceFatal(String),     // outer Err: trap or host error; drop the instance
}
fn classify(r: wasmtime::Result<Result<PollResponse, PollError>>) -> CallOutcome {
    match r {
        Ok(Ok(resp)) => CallOutcome::Ok(resp),
        Ok(Err(e)) => CallOutcome::Guest(e),
        Err(e) => {
            let kind = e.downcast_ref::<wasmtime::Trap>().map(|t| format!("{t:?}"));
            CallOutcome::InstanceFatal(kind.unwrap_or_else(|| format!("{:#}", e.root_cause())))
        }
    }
}
```
`PollError` variants, quoted: `rate-limited(option<u32>)`, `auth-required`, `config(string)`, `transient(string)`, `fatal(string)` [VERIFIED: wit/types.wit `variant poll-error`]. The generated Rust enum is `PollError::{RateLimited(Option<u32>), AuthRequired, Config(String), Transient(String), Fatal(String)}`; stub-source constructs `PollError::Config(...)` and the prototype fixture constructs `PollError::RateLimited(Some(7))` and `PollError::Fatal(...)`.

### Import preflight (clear WASI rejection)
```rust
// Source: names verified via component_type().imports(&engine) in the spike.
let ty = component.component_type();
let foreign: Vec<String> = ty.imports(&engine)
    .map(|(name, _)| name.to_string())
    .filter(|n| !n.starts_with("filimon:plugin/"))
    .collect();
if !foreign.is_empty() {
    return Err(/* miette: plugin `{name}` imports {foreign:?}, which fili does not provide;
                  build for wasm32-unknown-unknown (plugins/README.md) */);
}
```

### Restart test shape (SC2)
```rust
// stub-source: poll(cursor) -> item id "stub-{cursor[0]}", next_cursor = [cursor[0]+1] (plugins/stub-source/src/lib.rs)
// 1. tempdir registry -> stub.wasm, interval 60s; start_paused = true
// 2. host.run(VecSink) ; advance 120s => sink ids [stub-0, stub-1, stub-2] (first poll immediate)
// 3. cancel; assert .fili-state/stub.json cursor decodes to [3]
// 4. new Host::load + run ; advance 0s => first item id == "stub-3"
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `bindgen!({ async: true, trappable_imports: true })` | `imports: { default: async \| trappable }, exports: { default: async }` | by wasmtime 36 (old form fails to compile there) | Use the new form |
| `Config::async_support(true)` required | deprecated no-op ("no longer has any effect") | by wasmtime 48 | Do not call on 48 |
| `wasmtime::Error` = `anyhow::Error` | own type, not `std::error::Error` | by wasmtime 48 | `?` into `anyhow`/`miette` fails; use `wasmtime::Result` and `format!("{e:#}")` |
| 1.85 pin for everything | per-directory pins | this phase | root on >= 1.95, SmartModules stay 1.85 |

**Deprecated/outdated:** wasmtime 34.x and below on the repo's current pin (unsupported; see Pitfall 1).

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Prefer wasmtime 48 LTS over 36 LTS because of the longer support window; the user may prefer the smaller toolchain jump | Standard Stack | Toolchain checkpoint needs >= 1.95 instead of >= 1.86; code differs only in the `async_support` call |
| A2 | Nested `smartmodules/*/rust-toolchain.toml` (1.85.0) works with `smdk build` (verified only that rustup honours nested toolchain files; `smdk build` itself was not run) | Pitfall 9, Open Question 1 | SmartModule builds fail until a different split is used |
| A3 | Install the subscriber in `main()` with default directive `filimon::plugins=info` | Pitfall 3 | Slight extra stderr output for existing commands if filter is broader than intended |
| A4 | Fluvio key `"{platform}:{id}"` | Discretion Resolutions 6 | Ordering/compaction expectations differ on multi-partition topics |
| A5 | Drop-with-warning rules: empty id/url, unparsable url, JSON > 1_000_000 bytes; empty platform falls back to plugin name | Discretion Resolutions 6 | Over- or under-filtering of plugin output; thresholds are judgement derived from the verified 1_048_576 limit |
| A6 | On cursor persist failure keep the in-memory cursor and continue | Pattern 3 | Restart replays more items than needed (still at-least-once) |
| A7 | A fetcher that fails to load does not stop sources in Phase 3 | Discretion Resolutions 3 | Phase 5 must add dependent-disable logic |
| A8 | Optional `File::try_lock` state-dir guard | Pattern 3 | A second `fili` on the same state dir double-polls |
| A9 | Fluvio send/flush failures reuse the plugin's backoff settings | Discretion Resolutions 5 | Retry cadence differs from what operators expect |
| A10 | D-02 literal: backoff may be shorter than `poll_interval_secs` | Discretion Resolutions 5 | Surprise cadence; locked decision, so only documentation |
| A11 | Add `.fili-state/` to `.gitignore` | Project Structure | Accidental commit of cursor files |
| A12 | A local Fluvio cluster was not confirmed running (`fluvio cluster status` printed a header and no result in 15 s) | Environment Availability | `live_` tests cannot run until a cluster is started |
| A13 | Generated bindgen code under clippy pedantic needs an `#[allow]`; clippy was not run on it | Pattern 1 | Extra warnings only |
| A14 | First poll runs immediately at host start | Pattern 2 | If planners want a first wait, SC2 timing changes by one interval |
| A15 | Version the state file and tolerate unknown fields so Phase 4 can add last-poll status | Pattern 3 | State-file format is "costly" per D-05; a later breaking change would force migration |
| A16 | Epoch tick of 10 ms and deadline of 1 tick | Discretion Resolutions 4 | Slightly more or less yield overhead; values were only exercised in the spike |

## Open Questions

1. **Toolchain change authorisation (blocking, human action).**
   - What we know: the local `rustup override` pins `/Users/jferrolino/Devs/portfolio/filimon` to 1.85.0; wasmtime 36 needs >= 1.86 and 48 needs >= 1.95; the root crate checks cleanly on 1.96.0 with wasmtime 48.0.8; 1.96.0 is already installed and already pinned by the plugin crates; the Dockerfile uses `rust:1-slim-bookworm` (unpinned), so it follows stable.
   - What's unclear: the user's consent to change a machine-local setting, and whether `smdk build` works under a nested 1.85.0 pin.
   - Recommendation: Wave 0 contains a `checkpoint:human-action`: run `rustup override unset` in the repo; the plan then adds a root `rust-toolchain.toml` (`channel = "1.96.0"`), sets `rust-version = "1.95"`, adds `smartmodules/{filter-relevance,map-preprocess,map-extract}/rust-toolchain.toml` (`channel = "1.85.0"`, targets as the README lists), updates the README prerequisites (it currently says 1.85 is required for the whole project), and verifies by running `smdk build` in one SmartModule directory.
2. **Should the Phase 3 default log filter change existing commands' output?** Recommendation in Pitfall 3 limits it to `filimon::plugins`. Confirm.
3. **Known gap:** a guest that loops forever stays "polling" forever (no deadline). With epoch yield, shutdown and other plugins are unaffected. A host-wide generous poll deadline would be HOST-06 territory; leave out unless the user wants it now.
4. **Phase 4 coordination:** CLI-01 wants "last poll status (time and ok/error)". Persisting it is not required by D-05; the versioned, extensible state file leaves room. Phase 4 decides.
5. **Multi-partition topic expectation** affects the key choice (A4).

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| rustc/cargo 1.96.0 | wasmtime 48 build | yes (installed toolchain, not active) | 1.96.0 (default active in the repo: 1.85.0 via override) | None below 1.95 for wasmtime 48 (36 LTS needs 1.86) |
| `cargo-component` | building `fault-source` fixture | yes | 0.21.1 | — |
| `wasm-tools` | conformance check, WAT-to-component for ad-hoc fixtures | yes | 1.263.0 | — |
| `wasm32-unknown-unknown` target | fixtures | yes (1.85.0 toolchain lists it; plugin crates pin 1.96.0 with the target) | — | — |
| Prebuilt `stub_source.wasm`, `stub_fetcher.wasm` | wasm-backed tests | yes, built 2026-11-03 under `plugins/*/target/wasm32-unknown-unknown/release/` | — | rebuild per plugins/README.md |
| Fluvio CLI | live tests | yes | 0.18.1 | — |
| Local Fluvio cluster | `live_` tests | not confirmed (see A12) | — | run `fluvio cluster start`; unit tests use `VecSink` |
| Docker | optional | yes | 29.8.2 | — |

**Missing dependencies with no fallback:** none confirmed, except the toolchain checkpoint (Open Question 1).
**Missing dependencies with fallback:** running Fluvio cluster (live tests ignored by default).

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in harness, tokio `#[tokio::test(start_paused = true)]` (`test-util` present) |
| Config file | none (Cargo.toml) |
| Quick run command | `cargo test plugins::host` |
| Full suite command | `cargo test` |
| Wasm fixture tests | `cargo test plugins::host -- --ignored wasm_` (needs built fixtures) |
| Live Fluvio tests | `cargo test plugins::host -- --ignored live_` (needs cluster + topic `filimon-social-items`) |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| HOST-01 | Only enabled registry entries are opened; an unlisted (even invalid) `.wasm` beside them and a disabled source are never read; WASI-importing component rejected with named imports; wrong-world component rejected | unit (loader records opened paths; WAT-built wasi component via `wasm-tools`-free inline bytes or a checked-in tiny `.wasm` test constant) + `wasm_` | `cargo test plugins::host::loader` | Wave 0 |
| HOST-02 | Poll at interval; cursor persisted atomically, commit only after flush; restart resumes from persisted cursor; corrupt file disables only that plugin | unit with fake instance + `wasm_` restart test (stub-source) | `cargo test plugins::host::scheduler`, `plugins::host::cursor` | Wave 0 |
| HOST-03 | Backoff sequence, cap, reset on success, `retry_after` max rule, back to interval | pure unit + paused-time scheduler test | `cargo test plugins::host::backoff` | Wave 0 |
| HOST-04 | WIT item maps to `SourceItem` (epoch ms to RFC 3339, raw JSON), bad id/url dropped, size cap, key format; items arrive in sink in order; `live_` test sees records on `filimon-social-items` | unit + `live_` | `cargo test plugins::host::map`, `plugins::host::sink` | Wave 0 |
| HOST-05 | Trap/error logged with plugin name; trap rebuilds instance and re-inits; fatal/config disables; other plugin keeps polling; process alive; spinning guest cancellable | unit (fake factory, log capture) + `wasm_` (fault-source) | `cargo test plugins::host::scheduler`; `-- --ignored wasm_` | Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test plugins::host`
- **Per wave merge:** `cargo test`
- **Phase gate:** full `cargo test`, then `-- --ignored wasm_`, then (cluster up) `-- --ignored live_`, all green before `/gsd-verify-work`. Plain `cargo test` alone does not exercise real wasm; the verifier must run the ignored groups and report them separately.

### Wave 0 Gaps
- [ ] Toolchain checkpoint and Cargo.toml changes (Open Question 1)
- [ ] `plugins/fault-source/` fixture crate and README entry; build both stubs and the fixture
- [ ] `src/plugins/host/` skeleton with `ItemSink` trait, `VecSink`, fake `InstanceFactory`
- [ ] Log-capture helper (scoped `fmt` subscriber into a shared buffer)
- [ ] `TypedProducer::clear_errors`, `init_logging`, `--state-dir` flag, `.gitignore` entry

## Security Domain

`security_enforcement` is enabled in `.planning/config.json` (ASVS level 1, block on high) [VERIFIED: config.json `"security_enforcement": true, "security_asvs_level": 1`].

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no (OAuth is Phase 6) | — |
| V3 Session Management | no | — |
| V4 Access Control | yes (capability boundary) | Linker exposes only `filimon:plugin/*`; no WASI; import preflight; only registry-listed components opened |
| V5 Input Validation | yes | Treat all guest output as untrusted: item field validation, URL parse, size cap, sanitize log/error text, base64/JSON strict parse of state files |
| V6 Cryptography | no | — |
| V7 Error Handling and Logging | yes | `Secret` stays redacted; never log init args/settings/secrets/item content; sanitize and clip guest text |
| V12 Files and Resources | yes | State dir files named only from validated aliases; atomic writes; temp files in same dir; optional dir lock |
| V14 Configuration | yes | State dir flag/env; secrets only via `Secret::expose()` into the owning plugin's `init` |

### Known Threat Patterns for this stack

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Plugin imports a capability fili does not intend (WASI fs/env/sockets) | Elevation of Privilege | Do not link WASI; import preflight with named rejection |
| Unlisted `.wasm` gets loaded | Elevation of Privilege / Tampering | Registry-driven loading only; test with an invalid unlisted file |
| Plugin spins forever / starves runtime | Denial of Service | Epoch yield makes calls cancellable and fair (verified); real CPU/memory limits deferred (HOST-06, accepted risk) |
| Plugin allocates unbounded memory | Denial of Service | **Not mitigated this phase** (HOST-06); record as accepted risk |
| ANSI/control bytes in guest log lines or error strings | Tampering (terminal injection) | `sanitize_for_terminal`, length clip |
| Secret leakage to logs/state | Information Disclosure | `Secret` redaction; log alias/env names only; state file holds only the cursor and plugin name |
| Oversized or malformed items poison the Fluvio producer | Denial of Service | Pre-validate size; `clear_errors` on failure |
| State-file corruption/tampering silently resets a source | Tampering | D-08: disable plugin with named path; never default to `none` |
| Two processes share one state dir | Tampering | Optional `File::try_lock` guard |
| Path traversal via plugin name | Tampering | Name rule `[a-z0-9_-]+` (verified); build paths only from `LoadedRegistry` names |

## Sources

### Primary (HIGH confidence)
- Spike hosts and fixture run this session (wasmtime 36.0.20 and 48.0.8, rustc 1.96.0) against `plugins/stub-source`, `plugins/stub-fetcher` and a prototype fault-injection component: instantiate, init/poll, `instantiate_pre` for both worlds, WASI rejection text, trap and `CannotEnterComponent`, epoch yield cancellation, un-yielded hang.
- crates.io API (`https://crates.io/api/v1/crates/wasmtime`): versions, dates, `rust_version`, repository, downloads.
- `cargo +1.85.0 generate-lockfile` output (resolver picks 34.0.2); `cargo +1.96.0 check` on a copy of this repo with wasmtime 48.0.8 added (exit 0).
- Local source reads: `wit/*.wit`, `src/plugins/config.rs`, `src/plugins/secrets.rs`, `src/plugins/error.rs`, `src/streaming.rs`, `src/models.rs`, `src/cli.rs`, `src/main.rs`, `plugins/README.md`, `plugins/stub-source/src/lib.rs`, `plugins/stub-fetcher/src/lib.rs`, `examples/plugins/*`, `Cargo.toml`, `.planning/*`.
- `~/.cargo/registry/src/*/fluvio-0.50.1/src/producer/{mod.rs,output.rs,config.rs,memory_batch.rs}` (sticky errors, flush semantics, 1_048_576 limit).

### Secondary (MEDIUM confidence)
- docs.wasmtime.dev/stability-release.html: LTS cadence (multiples of 12), 24-month support, security backports, MSRV stance.
- docs.rs/tokio `time::pause`: auto-advance, `spawn_blocking` inhibits advance, requires `current_thread`.
- Web search of Debian security tracker entries naming 2026 wasmtime RUSTSEC advisories (RUSTSEC-2026-0222 fixed in 36.0.13; others listed). The search did not return advisory pages and nothing specific to 34.x, so the claim used is "34.x is not a supported release", not a specific CVE list.

### Tertiary (LOW confidence)
- None used as a basis for a recommendation. The research-plan / Context7 seam was not used: all findings came from running code and reading the repo, and no provider was configured (`exa`, `brave`, `firecrawl`, `tavily`, `ref`, `perplexity`, `jina` are all `false` in config).

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH, versions read from the registry and the root crate compiled with the chosen set; the LTS choice itself is judgement (A1).
- Architecture: HIGH for wasmtime mechanics (spiked), MEDIUM for the scheduler details that are design choices (A4-A10, A14-A16).
- Pitfalls: HIGH for the six verified by running code or reading Fluvio/tokio source; MEDIUM for the toolchain split (Open Question 1).

**Research date:** 2026-11-09
**Valid until:** 2026-12-09 (wasmtime ships monthly majors, but 48.x is an LTS line; re-check the patch version and advisories at planning time)
