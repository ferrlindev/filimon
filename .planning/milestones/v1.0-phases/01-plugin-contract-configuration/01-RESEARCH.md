# Phase 1: Plugin Contract & Configuration - Research

**Researched:** 2026-11-03
**Domain:** WebAssembly Component Model (WIT, `cargo component`, `wasm-tools`) + Rust TOML/.env config loading with `miette` diagnostics
**Confidence:** HIGH for toolchain and WIT (probed in a scratch directory this session); MEDIUM-HIGH for config-loader design (crate APIs read from source; design choices are recommendations)

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

#### WIT data shapes
- **D-01:** Poll cursor is `option<list<u8>>` — opaque bytes, `none` on first poll. Host persists it verbatim and passes it back. (User chose bytes over string for flexibility; host-side display of cursors must hex/base64-encode.) — **Reversibility:** one-way — changes the published `poll` signature every plugin compiles against.
- **D-02:** Item record = typed fields mirroring the Phase 2 `SourceItem` (id, url, title, text, author, published, platform) plus `raw: option<string>` holding platform JSON metadata. Host validates required fields. — **Reversibility:** one-way — published record shape; Phase 2 SourceItem must line up with it (Phase 2 SC1).
- **D-03:** `published` encoded as `option<u64>` unix epoch **milliseconds**. Host renders RFC 3339 when building SourceItem JSON.
- **D-04:** Errors are typed variants. `poll` returns `result<..., poll-error>` where `poll-error = variant { rate-limited(option<u32> /* retry-after secs */), auth-required, config(string), transient(string), fatal(string) }`. `fetch` gets an analogous typed `fetch-error` variant (exact cases at Claude's discretion; must distinguish denied-by-policy, timeout, transport, and rate-limited so Phase 3/5 can back off per variant). — **Reversibility:** costly — variant cases are part of the contract; adding cases is a version bump.

#### Config delivery to plugins
- **D-05:** Source `init` receives the plugin's `[settings]` section as a **raw TOML string**; the plugin parses it itself (guest needs a TOML parser, e.g. `toml` crate).
- **D-06:** Secrets are **pushed in `init`** alongside settings: `init(settings: string, secrets: list<tuple<string, string>>)` (exact param naming at Claude's discretion). Only the secrets the plugin's TOML declares are passed (satisfies CONF-02). Keys are the alias names from D-12, not env var names.
- **D-07:** OAuth access token is **passed into `poll`**: `poll(cursor: option<list<u8>>, access-token: option<string>)`. No token import. Host passes `none` until Phase 6 wires real tokens. Refresh tokens never cross the boundary (AUTH-03).
- **D-08:** Fetcher world has **no init** — fetchers are pure `fetch(request) -> result<response, fetch-error>` with no settings or secrets. (Implication: a configurable headless-browser fetcher (FETCH-05, v2) will need a contract revision — accepted.)
- **D-09:** Resulting `source` world imports only `fetch` and `log` (settings/secrets/token flow via init/poll params). **User approved rewording WIT-01 and Phase 1 SC1** to say the plugin is provided settings, secrets and access tokens by the host (via init/poll parameters) rather than via import functions. REQUIREMENTS.md WIT-01 and ROADMAP.md Phase 1 SC1 updated accordingly in this commit.

#### Registry & TOML layout
- **D-10:** `filimon.toml` uses **named tables**: `[sources.<name>]` with `component`, `config`, `enabled`, `fetcher`; `[fetchers.<name>]` with `component` (and `enabled`). Table key is the plugin name, so duplicates are impossible and errors cite `sources.reddit`.
- **D-11:** Key split — **registry = wiring only** (component path, config path, enabled, fetcher assignment). **Per-plugin TOML = everything else**: a `[host]` table (poll interval, backoff, allowlist, timeout, secrets aliases) and a `[settings]` table passed raw to `init` (D-05). Host treats `[settings]` as opaque: it only checks it is valid TOML; plugin owns schema validation, and `init` returning `config(..)` error surfaces as a config error naming the plugin.
- **D-12:** Secrets declared as an **alias map** in the plugin TOML: `[host.secrets] client_id = "REDDIT_CLIENT_ID"` — plugin sees `client_id`, operator chooses the env var. `.env` is read from the **directory of `filimon.toml`**; real process env vars override `.env` values.
- **D-13:** `filimon.toml` located at `./filimon.toml` by default, overridable with a global `--registry <path>` flag (env `FILI_REGISTRY`). All `component`/`config` paths resolve **relative to the registry file's directory**.
- **D-14:** Config errors (CONF-03): **collect all problems across all plugins**, then print one `miette` diagnostic per problem (plugin name, file path, field/key, TOML span when available) and exit non-zero. Missing file, malformed TOML, missing secret, unknown fetcher reference all covered.

#### Versioning & crate layout
- **D-15:** WIT package id **`filimon:plugin@0.1.0`**, semver. Stay 0.x while the contract settles; the host (Phase 3) checks the component's imported package version and rejects incompatible ones with a clear error. Bump to 1.0 at milestone end once Reddit/YouTube validate it.
- **D-16:** `wit/` at repo root holds the package. Stub crates live in `plugins/stub-source` and `plugins/stub-fetcher`, **excluded from the root Cargo workspace** (root is pinned to Rust 1.85 for SmartModules); they may carry their own `rust-toolchain.toml` if `cargo component` needs newer Rust. They reference only `../../wit` — proving "built from WIT alone".
- **D-17:** Stubs are **minimal but exercising**: stub source parses its TOML settings, calls `log`, returns one hardcoded item and an echoed/advanced cursor; stub fetcher returns a canned response. They double as test fixtures for the Phase 3 host.

### Claude's Discretion
- Exact WIT interface split (e.g. `types`, `host`, `source`, `fetcher` interfaces) and field/param naming.
- `log` import shape (level enum + message is the expected default).
- Fetch request/response record shape (method, url, headers, body, status) and `fetch-error` cases, within D-04's constraint.
- Defaults for interval/backoff/timeout values and their TOML key names under `[host]`.
- Rust module layout for config loading (e.g. `src/plugins/config.rs`), crate choices (`toml`, `dotenvy`), and how TOML spans are threaded into miette.
- Whether `log` should also be imported by the fetcher world.
- Tool installation approach for `cargo-component` / `wasm-tools` (neither is installed; host rustc is 1.85.0) — research must pin versions.

### Deferred Ideas (OUT OF SCOPE)
- Fetcher configuration/secrets (needed by a future headless-browser/CDP fetcher, FETCH-05 v2) — requires adding a fetcher init in a later contract version.
- Host-side schema validation of plugin `[settings]` — plugin owns validation for now.
- "Skip bad plugins and warn" run-mode behaviour — possible later flag on the run command (Phase 4).
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| WIT-01 | Versioned WIT package defines a `source` world: exports `init(settings, secrets)` and `poll(cursor, access-token) -> (items, next-cursor)`; host provides `fetch` and `log` imports; settings, named secrets and access tokens arrive via `init`/`poll` params | Probed WIT package (Code Examples §1) validated with `wasm-tools 1.263.0`; WIT pitfalls (duplicate names, `map` keyword, per-file package agreement) reproduced; `wasm-tools component targets` gives a per-world conformance check |
| WIT-02 | Same package defines a `fetcher` world: exports `fetch(request) -> response`; host provides a low-level outbound-HTTP import | Shared `fetch` interface (imported by `source`, exported by `fetcher`) + `outbound-http` interface; validated in the same probe |
| CONF-01 | Each plugin has its own TOML file (subreddits, channels, intervals, allowlist) referenced from `filimon.toml` | `toml` 1.1.8 (MSRV 1.85, matches root pin) + `serde` structs with `toml::Spanned` for field spans; `[settings]` kept as `toml::Table` and re-serialised for `init` |
| CONF-02 | API keys / client secrets load from `.env`; each plugin receives only the secrets its config names | `dotenvy::from_path_iter` reads `.env` into a map **without** calling `env::set_var` (which is `unsafe` in edition 2024); per-plugin resolution through an injectable env-lookup closure |
| CONF-03 | Invalid config produces a clear error naming plugin and field | `miette` 7.6 derive: `#[source_code] NamedSource<String>`, `#[label] Option<SourceSpan>`, `#[related] Vec<_>` to render every collected problem; `toml::de::Error::span()` supplies byte ranges |
</phase_requirements>

## Project Constraints (from CLAUDE.md)

`.claude/CLAUDE.md` imports `AGENTS.md` and `WORKFLOW.md`. Those govern the separate "GSD Path" pipeline under `.project/`; no `.project/STATE.md` exists, so that pipeline is inactive and this phase runs under `.planning/`. Directives that still apply in spirit:

- Report partial or failed results plainly; never report a command as passing unless it ran.
- Preserve uncertainty (`[ASSUMED]`, open questions) instead of silently resolving it.
- Record user decisions verbatim (done above in `<user_constraints>`).

Project conventions that bind this phase (from `.planning/codebase/CONVENTIONS.md` and root `Cargo.toml`):

- `miette::Result` + contextual errors; no `unwrap()` in production code.
- Clippy `all` + `pedantic` at warn level; `unsafe_code = "warn"` — new code must be clean under both.
- Module header comment with ASCII `// ─────` dividers, `// ── Section ──` sub-headers, `///` docs with `# Errors` sections.
- Do NOT repurpose `src/config.rs` (watch-mode JSON config). Never modify `fluvio/`.
- Root crate is edition 2024, `rust-version = "1.85"`.

## Summary

The toolchain question in STATE.md is settled. `cargo-component` **0.21.1** is still the latest release (published 2025-04-07; repo last pushed 2025-07-14; not archived, labelled "experimental"). It generates bindings with `wit-bindgen` **0.41** and builds for `wasm32-wasip1` by default. It also accepts `wasm32-unknown-unknown`, but it does **not** recognise `wasm32-wasip2`. `wasm-tools` **1.263.0** (2026-10-30) needs Rust 1.88+, so it cannot be built with the root's 1.85 toolchain. Install both with the stable toolchain into `~/.cargo/bin`, or into a scratch `--root`. They are dev tools; they never join the `fili` build.

In this session I built a WIT package in a scratch directory using the exact shapes from D-01..D-08 and validated it with `wasm-tools 1.263.0`. The probe also reproduced three contract-authoring pitfalls with real error output: an interface and a world cannot share a name; `map` is now a WIT keyword; and every `.wit` file in the directory must declare the identical `package filimon:plugin@0.1.0;`. Stub source and fetcher crates built against that package are covered in the probe-results section below. `wasm-tools component targets <wit-dir> -w <world> <component.wasm>` is the tool that proves SC2 ("valid component for their world").

Config loading is plain serde. `toml` 1.1.8 has MSRV 1.85, so it fits the root pin. It exposes `toml::Spanned<T>` and `toml::de::Error::span()`. For `.env` files, use `dotenvy` 0.15.7's `from_path_iter`, which yields `(key, value)` pairs without mutating the process environment. That matters because `std::env::set_var` is `unsafe` in edition 2024 and the crate warns on unsafe code. Errors become `thiserror` + `miette::Diagnostic` types, collected into one parent diagnostic with `#[related]`, which satisfies D-14's collect-all-then-report requirement.

**Primary recommendation:** Put three `.wit` files in `wit/` (`types.wit`, `host.wit`, `worlds.wit`), all declaring `package filimon:plugin@0.1.0;`. Use one shared `fetch` interface that the `source` world imports and the `fetcher` world exports. Each stub crate is its own workspace (`[workspace]` table), pins `rust-toolchain.toml`, and uses `[package.metadata.component.target] path = "../../wit"`. Load config in a new `src/plugins/` module with `toml` + `dotenvy::from_path_iter` + `miette` `#[related]` aggregation.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| WIT contract (`wit/`) | Contract artifact (repo root, versioned) | — | Consumed by guests via `cargo component` and later by the Phase 3 host via `wasmtime::component::bindgen!`; it must not depend on any Rust crate |
| Stub source / fetcher components | Guest (WASM component, `plugins/*`) | — | Prove the contract builds from WIT alone; reused as Phase 3 host fixtures |
| Registry + per-plugin TOML parsing | Host (`fili` binary, new `src/plugins/` module) | — | Host owns wiring, scheduling inputs, and allowlist data; plugins never see the registry |
| `.env` secret resolution | Host | — | Secrets must be filtered per plugin before crossing into a guest (CONF-02); the guest only receives alias→value pairs at `init` |
| `[settings]` schema validation | Guest (plugin's `init`) | Host (TOML syntax only) | D-11: host checks it parses, plugin owns the schema and reports `poll-error::config` |
| Diagnostics rendering | Host (`miette` in `main`) | — | Existing error convention |
| `--registry` / `FILI_REGISTRY` flag | Host CLI (`src/cli.rs`) | — | Global clap arg; consumed by later phases' subcommands |

## Standard Stack

### Core (host, root crate)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `toml` | 1.1.8 (`toml = "1"`) | Parse `filimon.toml` and per-plugin TOML into serde structs; `Spanned<T>`; `de::Error::span()` | De-facto TOML crate (toml-rs); MSRV declared `rust-version: 1.85` [VERIFIED: `cargo info toml` → `version: 1.1.8+spec-1.1.0`, `rust-version: 1.85`] |
| `dotenvy` | 0.15.7 | Read `.env` as an iterator of `(String, String)` without mutating process env | Maintained fork of `dotenv`; `from_path_iter` API [VERIFIED: local source `dotenvy-0.15.7/src/lib.rs:130` `pub fn from_path_iter<P: AsRef<Path>>(path: P) -> Result<Iter<File>>`] |
| `thiserror` | 2.0.21 (`thiserror = "2"`) | `Error` derive for config error types used with `miette::Diagnostic` | Canonical companion to miette derive; already in the lockfile transitively (2.0.17) [VERIFIED: `cargo info thiserror` → `version: 2.0.21`, `rust-version: 1.77`] |
| `miette` | 7.6.0 (existing, `features = ["fancy"]`) | Diagnostics with source snippets, labels, related errors | Already the project's error crate; default `derive` feature is on [VERIFIED: `cargo info miette@7.6.0` → `+default = [derive]`; `Cargo.lock` has `miette 7.6.0`] |
| `serde` | 1 (existing) | Derive `Deserialize` for config structs | Existing |

### Guest (stub crates, NOT in root workspace)
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `wit-bindgen-rt` | `0.41` (`features = ["bitflags"]`) | Runtime support referenced by `cargo component`-generated `src/bindings.rs` | Required by every `cargo component` crate; 0.41 matches cargo-component 0.21.1's internal `wit-bindgen-rust = "0.41.0"` [VERIFIED: cargo-component v0.21.1 `Cargo.toml`; `src/commands/new.rs:25,318-320` adds `wit-bindgen-rt` with `--features bitflags`] |
| `toml` | `1` | Stub source parses its raw `[settings]` string (D-05, D-17) | Stub source only |
| `serde` | `1` (derive) | Settings struct in stub source | Stub source only |

### Dev tools (installed, not dependencies)
| Tool | Version | Purpose | Notes |
|------|---------|---------|-------|
| `cargo-component` | 0.21.1 | Generate bindings from `../../wit` and build a component | Latest release; no prebuilt binaries on the GitHub release, so `cargo install --locked` is required [VERIFIED: GitHub releases API `v0.21.1 2025-04-07`, empty asset list] |
| `wasm-tools` | 1.263.0 | `component wit` (validate WIT), `component targets` (component vs world), `validate` | Needs Rust ≥ 1.88 to build [VERIFIED: `cargo info wasm-tools` → `rust-version: 1.88.0`]; built and run successfully this session with stable 1.98.0 |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `cargo component` | Plain `cargo build --target wasm32-wasip2` + `wit-bindgen` 0.63 `generate!` macro | Actively released, no extra tool. But SC2 and DOCS-01 name `cargo component`; switching is a requirement change. Keep `cargo component` |
| `dotenvy` | Hand-parse `KEY=VALUE` | Quoting, `export` prefixes, escapes, multiline values — see Don't Hand-Roll |
| `thiserror` + derive | Manual `impl Diagnostic` | Far more boilerplate; derive is miette's documented path |
| `toml` 1.x | `toml` 0.8 (already in the lockfile via deps) | 0.8 also works, but 1.x is current and its `Spanned` re-export and error API are what this research verified. Two `toml` versions in the lockfile is harmless |

**Installation:**
```bash
# Host crate (root, Rust 1.85)
cargo add toml@1 dotenvy@0.15 thiserror@2

# Dev tools (any toolchain >= 1.88; use stable)
cargo +stable install cargo-component@0.21.1 --locked
cargo +stable install wasm-tools@1.263.0 --locked
```

**Version verification:** Every version above was checked against crates.io (`cargo info` / crates.io API) on 2026-11-03.

## Package Legitimacy Audit

Run through `gsd-tools query package-legitimacy check --ecosystem crates` this session.

| Package | Registry | Age | Downloads | Source Repo | Verdict | Disposition |
|---------|----------|-----|-----------|-------------|---------|-------------|
| toml | crates.io | since 2014 | ~22.7M/wk | github.com/toml-rs/toml | [OK] | Approved |
| dotenvy | crates.io | since 2022 | ~4.8M/wk | github.com/allan2/dotenvy | [OK] | Approved |
| thiserror | crates.io | since 2019 | ~37.7M/wk | github.com/dtolnay/thiserror | [OK] | Approved |
| wit-bindgen-rt | crates.io | since 2024 | ~1.0M/wk | github.com/bytecodealliance/wit-bindgen | [OK] | Approved (guest only) |
| cargo-component | crates.io | since 2021 | ~3.9K/wk | github.com/bytecodealliance/cargo-component | [OK] | Approved (dev tool) |
| wasm-tools | crates.io | since 2022 | ~10K/wk | github.com/bytecodealliance/wasm-tools | [OK] | Approved (dev tool) |

**Packages removed due to [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

Note: `dotenvy` 0.15.7 is from 2023-03 and there has been no release since. The API needed here (`from_path_iter`) is small and stable, so this is acceptable. Flag it as a watch item, not a blocker.

## Toolchain Probe Results (run this session, scratch dir `/tmp/fili-probe`, nothing written to the repo)

I installed both tools into a scratch `--root` using `rustup run stable cargo install ... --locked`. Stable is rustc 1.98.0; cargo-component took 7m42s to build. Then I built the WIT package from Code Examples §1 and the two stubs from §2–§3, with a stub-local `rust-toolchain.toml` pinning `1.96.0`.

| Probe | Result | Evidence |
|-------|--------|----------|
| `wasm-tools component wit wit/` on the 3-file package | **pass** (exit 0; prints the resolved package) | `wasm-tools 1.263.0` |
| `wasm-tools component wit wit/ --wasm -o pkg.wasm && wasm-tools validate pkg.wasm` | **pass** | printed `validated-binary` |
| `cargo component build --release` (default target `wasm32-wasip1`) for both stubs | **builds**, components `validate` | `Creating component target/wasm32-wasip1/release/stub_source.wasm` |
| `wasm-tools component targets wit -w source <wasip1 stub>` | **FAIL** | `type mismatch for import 'source'` / `missing import named 'wasi:cli/environment@0.2.3'` |
| Imports of the wasip1 stub source | `filimon:plugin/types`, `filimon:plugin/log`, plus `wasi:cli/{environment,exit,stdin,stdout,stderr}`, `wasi:io/{error,streams}`, `wasi:clocks/wall-clock`, `wasi:filesystem/{types,preopens}` (all `@0.2.3`) | `wasm-tools component wit stub_source.wasm` |
| `cargo component build --release --target wasm32-unknown-unknown` for both stubs | **builds**, `validate` passes | — |
| `wasm-tools component targets wit -w source <unknown-unknown stub source>` | **pass** (exit 0) | `targets-source=0` |
| `wasm-tools component targets wit -w fetcher <unknown-unknown stub fetcher>` | **pass** (exit 0) | `targets-fetcher=0` |
| Negative: `targets -w fetcher <stub source>` | **fail, as expected** | `type mismatch for import 'fetcher'` |
| Per-crate `.cargo/config.toml` `[build] target = "wasm32-unknown-unknown"` + plain `cargo component build --release` | **pass**, output under `target/wasm32-unknown-unknown/release/` | cargo-component reads the configured build target (`src/lib.rs:200-208` at v0.21.1) |
| Unused imports | **Dropped from the component.** The stub source imports `fetch` only once its code references `fetch::fetch(..)`; the stub fetcher imports neither `log` nor `outbound-http` when it calls neither | `wasm-tools component wit` before/after adding a settings-gated `fetch` call |
| Config probe on **Rust 1.85.0** (`toml 1.1.8`, `dotenvy 0.15.7`, `thiserror 2`, `miette 7.6 fancy`) | **compiles and runs**; spans, `deny_unknown_fields`, `NonZeroU64`, `#[related]` rendering, and `from_path_iter` all behave as shown in Code Examples §4–§6 | see outputs quoted in §4–§6 |
| Nested crate under the repo's root workspace **without** `[workspace]` | **error** | `current package believes it's in a workspace when it's not` (and the message suggests `workspace.exclude` or an empty `[workspace]` table) |
| Same nested crate **with** an empty `[workspace]` table | **pass** | `cargo metadata` OK |
| `rust-toolchain.toml` in a subdirectory vs the repo's `rustup override` (1.85.0) | **subdir file wins** | `1.96.0-x86_64-apple-darwin (overridden by '.../tc-probe/rust-toolchain.toml')`; without the file: `1.85.0 ... (directory override for '/Users/jferrolino/Devs/portfolio/filimon')` |

**What this means for the plan:**
1. Stubs must build for **`wasm32-unknown-unknown`**. Set it through a per-crate `.cargo/config.toml` so a plain `cargo component build` does the right thing. Only that target passes `wasm-tools component targets`, which is the clean, automatable proof of SC2 ("valid component for their world").
2. The stub source must actually **call** `fetch` (gated by a settings key) and `log`. Otherwise the component never imports `fetch`, and the Phase 3 host fixture would never exercise linking that import.
3. The root's Rust 1.85 pin is a **machine-local `rustup override`**, not a committed `rust-toolchain.toml`. Stubs need their own committed `rust-toolchain.toml` (it beats the override, as verified above) with `targets = ["wasm32-unknown-unknown"]`.

## Architecture Patterns

### System Architecture Diagram

```
                 ┌──────────────── contract (Phase 1 output) ────────────────┐
                 │  wit/  package filimon:plugin@0.1.0                        │
                 │   types.wit  (item, poll-response, poll-error, request,    │
                 │               response, fetch-error, method, headers)      │
                 │   host.wit   (interface log | fetch | outbound-http)       │
                 │   worlds.wit (world source | world fetcher)                │
                 └───────┬──────────────────────────────┬─────────────────────┘
                         │ cargo component (bindgen)    │ wasmtime bindgen! (Phase 3)
                         v                              v
   plugins/stub-source ──> stub_source.wasm      fili host (later phases)
   plugins/stub-fetcher ─> stub_fetcher.wasm            ^
          │                                             │ typed, validated config
          └─ wasm-tools component targets (SC2 gate)    │
                                                        │
  operator files                    fili (Phase 1 code: src/plugins/)
  ─────────────                     ───────────────────────────────────
  --registry / FILI_REGISTRY ─┐
  (default ./filimon.toml)    v
  filimon.toml ──────> parse registry (toml, Spanned) ──┐
     [sources.<n>] component/config/enabled/fetcher     │ for each enabled source:
     [fetchers.<n>] component/enabled                   v
  <dir>/reddit.toml ─> parse plugin file ──> [host] typed (interval, backoff,
                                             timeout, allowlist, secrets map)
                                         └─> [settings] kept as toml::Table ──> to_string ──> init(settings)
  <dir>/.env ──> dotenvy::from_path_iter ─┐
  process env ────────────────────────────┴─> resolve only aliases this plugin names ──> init(secrets)
                                                        │
                      every failure ──> Vec<ConfigError> (keep going) ──> ConfigErrors{#[related]} ──> miette report, exit≠0
```

### Recommended Project Structure
```
wit/
├── types.wit            # interface types (records/variants shared by both worlds)
├── host.wit             # interfaces log, fetch, outbound-http
└── worlds.wit           # world source, world fetcher
plugins/
├── stub-source/
│   ├── .cargo/config.toml     # [build] target = "wasm32-unknown-unknown"
│   ├── rust-toolchain.toml    # channel pin + targets
│   ├── Cargo.toml             # own [workspace]; metadata.component.target.path = "../../wit"
│   ├── Cargo.lock
│   └── src/lib.rs             # src/bindings.rs is generated by cargo component
└── stub-fetcher/              # same layout, world = "fetcher"
src/plugins/
├── mod.rs               # re-exports; load_registry() entry point
├── config.rs            # serde structs: Registry, SourceEntry, FetcherEntry, PluginFile, HostSection
├── secrets.rs           # .env reading + per-plugin alias resolution, redacting Secret type
└── error.rs             # ConfigError enum + ConfigErrors aggregate (thiserror + miette)
examples/plugins/        # sample filimon.toml, reddit.toml, youtube.toml, stub.toml, .env.example
```
Keep the name `src/plugins/` (not `src/config.rs`, which belongs to watch mode). Phase 3 will add a host submodule next to it.

### Pattern 1: One shared `fetch` interface, imported by sources and exported by fetchers
**What:** `interface fetch { fetch: func(request: request) -> result<response, fetch-error>; }`. `world source { import fetch; }` and `world fetcher { export fetch; }`.
**Why:** The source's import and the fetcher's export have the *identical* type. Phase 5 can forward one to the other with no translation, and could even compose them with `wac`. The fetcher's own low-level capability is a separate `outbound-http` interface (`send`), so the two never get confused.
**Verified:** the fetcher component exports `filimon:plugin/fetch@0.1.0` and passes `targets -w fetcher`.

### Pattern 2: World-level exported functions for `init`/`poll`
**What:** `export init: func(...)` and `export poll: func(...)` directly in `world source`. The generated guest trait is `bindings::Guest` with `fn init(settings: String, secrets: Vec<(String, String)>) -> Result<(), PollError>` and `fn poll(cursor: Option<Vec<u8>>, access_token: Option<String>) -> Result<PollResponse, PollError>`. (Verified by compilation. The generated import `fetch(request: &Request) -> Result<Response, FetchError>` and `log(level: Level, message: &str)` appear in `src/bindings.rs`.)
**Why:** This matches SC1's wording literally ("source world exports init … poll"). It also avoids the duplicate-name error you get if you put an interface named `source` inside the world named `source`.

### Pattern 3: Collect-all config validation
**What:** Parsing never returns early on a per-plugin problem. Each plugin is validated independently and pushes `ConfigError`s into a `Vec`. If the vector is non-empty at the end, return `ConfigErrors { #[related] problems }`. Exit non-zero through `miette::Result` in `main` (the existing pattern).
**When it short-circuits:** Only when the registry file itself is missing or unparsable. Nothing can be validated without it.

### Pattern 4: Injectable environment lookup
**What:** `resolve_secrets(plugin, aliases, dotenv: &HashMap<String,String>, env: &dyn Fn(&str) -> Option<String>)`. Production passes `|k| std::env::var(k).ok()`; tests pass a closure over a fixture map.
**Why:** Edition 2024 makes `std::env::set_var`/`remove_var` `unsafe`, and the crate warns on unsafe. Mutating the process env in parallel tests is also racy. Reading (`std::env::var`) stays safe.

### Anti-Patterns to Avoid
- **`dotenvy::dotenv()` / `from_path()` / `Iter::load()`:** these call `env::set_var` (`dotenvy-0.15.7/src/iter.rs:35,52`). They mutate global state, leak every `.env` key to everything in the process, and require `unsafe` on edition 2024. Use `from_path_iter` and collect into a map.
- **Passing env var names to the plugin:** D-06/D-12 require alias keys (`client_id`), not `REDDIT_CLIENT_ID`.
- **`#[serde(flatten)]` around `Spanned` fields or with `deny_unknown_fields`:** serde's flatten buffers values and loses span info, and it does not compose with `deny_unknown_fields`. Keep structs flat. [ASSUMED — known serde behaviour, not probed]
- **Committing a WIT file whose `package` line differs from its siblings:** the whole directory fails to parse (verified error in Pitfall 3).
- **Making the stub crates root-workspace members:** this breaks the 1.85-pinned root build and fails D-16's "WIT alone" proof.
- **Logging or `Debug`-printing secret values:** use a redacting newtype (Code Examples §6).

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| `.env` parsing | `split_once('=')` loop | `dotenvy::from_path_iter` | Quotes, `export` prefix (verified: `export QUOTED="a b"` parsed), escapes, comments, multiline values, line-numbered parse errors (`Error::LineParse(String, usize)`) |
| TOML parsing + error positions | regex / manual line tracking | `toml::from_str` + `toml::de::Error::span()` + `toml::Spanned<T>` | Byte-accurate spans feed directly into `miette::SourceSpan` |
| Rendering errors with snippets | custom formatter | `miette` derive with `#[source_code]`, `#[label]`, `#[related]` | Already a dependency; renders file:line:col snippets (verified output in §5) |
| Component bindings | hand-written canonical ABI glue | `cargo component` (generates `src/bindings.rs` with `wit-bindgen` 0.41) | ABI lifting/lowering, `cabi_realloc`, export macro |
| Checking "component matches world" | parsing component imports yourself | `wasm-tools component targets <wit> -w <world> <wasm>` | Official conformance check; verified pass/fail behaviour |
| Zero-interval validation | manual `if x == 0` | `std::num::NonZeroU64` field | serde error `invalid value: integer '0', expected a nonzero u64` with span (verified) |
| Typos in `[host]` keys | silent ignore | `#[serde(deny_unknown_fields)]` | Verified: `unknown field 'poll_intervall_secs', expected one of ...` with span |

**Key insight:** Every error-quality requirement in CONF-03 is already met by `toml` spans plus `miette` labels. The loader's real job is to carry the right `NamedSource` (file name + text) and the plugin name alongside each span.

## Common Pitfalls

### Pitfall 1: Default `cargo component` target adds WASI imports the world doesn't declare
**What goes wrong:** Built for the default `wasm32-wasip1`, the component imports 10 `wasi:*@0.2.3` interfaces. `wasm-tools component targets` then rejects it against `world source`, and Phase 3 would have to link WASI.
**Why it happens:** cargo-component adapts the wasip1 core module with the WASI preview1 adapter. Rust `std` references env, stdio, clocks, and filesystem.
**How to avoid:** Build with `wasm32-unknown-unknown` through a per-crate `.cargo/config.toml`. The guest then has no ambient filesystem, env, clock, or stdout. Logging goes through the `log` import and HTTP through `fetch`, which fits the host-mediated design.
**Warning signs:** `wasm-tools component wit <stub>.wasm` lists `wasi:` imports. Also watch for `SystemTime::now()` or `std::fs` use in guest code (these panic or fail on unknown-unknown).

### Pitfall 2: Interface and world sharing a name
**What goes wrong:** `world source { export source; }` with `interface source` fails: `duplicate item named 'source'` (verified).
**How to avoid:** Use world-level export functions (Pattern 2), or name interfaces differently from worlds.

### Pitfall 3: Multi-file WIT package with mismatched `package` lines
**What goes wrong:** `package identifier 'filimon:plugin@0.2.0' does not match previous package name of 'filimon:plugin@0.1.0'` (verified).
**How to avoid:** Every file in `wit/` repeats `package filimon:plugin@0.1.0;`. A version bump edits every file. Add a grep check to verification: every `wit/*.wit` contains the same package line.

### Pitfall 4: New WIT keywords as field names
**What goes wrong:** `map` is a keyword in the current WIT grammar. `record r { map: string }` fails with `expected an identifier or string, found keyword 'map'` (verified). The full keyword list also includes `type`, `list`, `option`, `result`, `string`, `stream`, `future`, `async`, `flags`, `enum`, `variant`, `record`, `resource`, `use`, `with`, `as`, `from`, `static`, `include`, `constructor`, `own`, `borrow`, `tuple` [CITED: github.com/WebAssembly/component-model design/mvp/WIT.md].
**How to avoid:** Escape with `%` (e.g. `%type`) or pick another name. None of the D-02 field names (`id, url, title, text, author, published, platform, raw`) are keywords; verified by the probe parse.

### Pitfall 5: Nested stub crates captured by the root workspace
**What goes wrong:** `cargo component build` in `plugins/stub-source` fails with `current package believes it's in a workspace when it's not` (verified inside this repo).
**How to avoid:** Give each stub an empty `[workspace]` table. This is self-contained and survives copying the crate outside the repo (useful for the Phase 8 template). Also add `exclude = ["plugins"]` to the root `[workspace]` (D-16, CONTEXT integration point).

### Pitfall 6: Stub `target/` directories not gitignored
**What goes wrong:** The root `.gitignore` ignores only `/target/` (`.gitignore:2`), so `plugins/stub-source/target/` (with ~250 KB `.wasm` files) would show up as untracked.
**How to avoid:** Add `plugins/*/target/` to `.gitignore`. Decide on `src/bindings.rs` (see Open Question 2).

### Pitfall 7: `.env` fixture files are gitignored everywhere
**What goes wrong:** `.gitignore` patterns `.env` and `.env.*` (no leading slash) match at any depth. A committed `tests/fixtures/.env` silently never gets committed, and tests pass locally but fail in CI.
**How to avoid:** Unit tests write fixtures into `tempfile::TempDir` (`tempfile = "3"` is already a dependency). Ship the sample as `examples/plugins/.env.example`; `!.env.example` is already whitelisted.

### Pitfall 8: Mutating the process environment
**What goes wrong:** `std::env::set_var` is `unsafe` in edition 2024 → `unsafe_code` warnings; parallel tests race.
**How to avoid:** Pattern 4 (injectable lookup) plus `from_path_iter`.

### Pitfall 9: Registry-relative path resolution
**What goes wrong:** Resolving `config = "reddit.toml"` against the CWD instead of the registry's directory (D-13).
**How to avoid:** `let base = registry_path.parent().unwrap_or(Path::new("."));` then `base.join(p)`. `Path::new("filimon.toml").parent()` is `Some("")`, and `"".join("x")` is `"x"`, so it behaves correctly. Absolute paths survive `join` unchanged. The `.env` path is `base.join(".env")`. [ASSUMED — std `Path` semantics, not probed this session]

### Pitfall 10: `[settings]` re-serialisation changes key order and drops comments
**What goes wrong:** `toml::to_string(&toml::Table)` emits keys sorted (verified: `listing` before `subreddits`) and loses comments.
**How to avoid:** This is acceptable. The plugin parses it as TOML, and semantics are preserved. Document that plugins must not rely on key order or comments. An absent `[settings]` becomes `""` (valid empty TOML) through `#[serde(default)]`.

### Pitfall 11: `cargo component new` pulls a newer `wit-bindgen-rt`
**What goes wrong:** `cargo component new` runs `cargo add wit-bindgen-rt --features bitflags` (no version), which today resolves 0.44.0, while generation uses wit-bindgen 0.41. It works (0.44's public API is a superset of 0.41's, per a local source diff), but it is an unpinned drift source.
**How to avoid:** Hand-write the stub `Cargo.toml` with `wit-bindgen-rt = { version = "0.41", features = ["bitflags"] }` and commit `Cargo.lock`.

### Pitfall 12: cargo-component is "experimental" and has not released since 2025-04
**What goes wrong:** Future breakage, or Rust toolchain incompatibility with the old locked dependency tree.
**How to avoid:** Pin `cargo-component@0.21.1 --locked`. Today it builds on stable 1.98.0 and builds stubs on 1.96.0 (verified). Record the exact install commands in the plan and in a short `plugins/README.md`. Fallback if it ever breaks: `cargo build --target wasm32-wasip2` with the `wit-bindgen` macro. That would be a requirement change for SC2/DOCS-01 and needs user sign-off.

## Code Examples

### §1 WIT package (validated with `wasm-tools 1.263.0` this session)
```wit
// wit/types.wit
package filimon:plugin@0.1.0;

/// Shared data types for every filimon plugin world.
interface types {
    /// One item returned by a source plugin. Mirrors the host `SourceItem` (Phase 2).
    record item {
        id: string,
        url: string,
        title: option<string>,
        text: string,
        author: option<string>,
        /// Unix epoch milliseconds (D-03).
        published: option<u64>,
        platform: string,
        /// Platform metadata as a JSON document.
        raw: option<string>,
    }

    /// Result of one successful poll.
    record poll-response {
        items: list<item>,
        /// Opaque cursor (D-01); the host stores it verbatim and passes it back.
        next-cursor: option<list<u8>>,
    }

    /// Why `init` or `poll` failed (D-04). The host backs off per case.
    variant poll-error {
        /// Retry-after in seconds, when known.
        rate-limited(option<u32>),
        auth-required,
        config(string),
        transient(string),
        fatal(string),
    }

    variant method { get, head, post, put, patch, delete, options, other(string) }

    type headers = list<tuple<string, string>>;

    record request { method: method, url: string, headers: headers, body: option<list<u8>> }

    record response { status: u16, headers: headers, body: list<u8> }

    variant fetch-error {
        /// Blocked by host policy (allowlist) — FETCH-02.
        denied(string),
        timeout,
        transport(string),
        rate-limited(option<u32>),
        /// No fetcher is assigned to this source (Phase 3 interim, STATE.md blocker).
        not-configured,
        invalid-request(string),
    }
}
```
```wit
// wit/host.wit
package filimon:plugin@0.1.0;

/// Structured logging into the host's tracing output.
interface log {
    enum level { trace, debug, info, warn, error }
    log: func(level: level, message: string);
}

/// High-level fetch: imported by sources, exported by fetchers.
interface fetch {
    use types.{request, response, fetch-error};
    fetch: func(request: request) -> result<response, fetch-error>;
}

/// Low-level outbound HTTP capability provided by the host to fetchers.
interface outbound-http {
    use types.{request, response, fetch-error};
    send: func(request: request) -> result<response, fetch-error>;
}
```
```wit
// wit/worlds.wit
package filimon:plugin@0.1.0;

world source {
    use types.{poll-response, poll-error};
    import log;
    import fetch;

    export init: func(settings: string, secrets: list<tuple<string, string>>) -> result<_, poll-error>;
    export poll: func(cursor: option<list<u8>>, access-token: option<string>) -> result<poll-response, poll-error>;
}

world fetcher {
    import log;
    import outbound-http;
    export fetch;
}
```
Design notes (Claude's discretion items, recommended):
- `fetch-error` includes `not-configured` now. STATE.md flags that Phase 3 needs an interim "not configured" fetch error, and adding a variant case later is a version bump (D-04).
- `init` reuses `poll-error`, so `config(..)` from `init` maps straight to D-11's "config error naming the plugin".
- `item.url` and `item.text` are required, `title` optional (posts and videos have titles, but generic social posts may not). This must be confirmed against Phase 2 SourceItem (see Assumptions A2).
- The request has no timeout field. The host owns timeout from `[host]` (D-11, FETCH-02) and enforces it inside its `outbound-http`/routing implementation.
- The fetcher imports `log` (recommended). Unused imports cost nothing (they are dropped from the component, verified), and fetchers need diagnostics.

### §2 Stub source crate (built and `targets`-checked this session)
```toml
# plugins/stub-source/Cargo.toml
[package]
name = "stub-source"
version = "0.1.0"
edition = "2024"
publish = false

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

[workspace]
```
```toml
# plugins/stub-source/rust-toolchain.toml
[toolchain]
channel = "1.96.0"
targets = ["wasm32-unknown-unknown"]
```
```toml
# plugins/stub-source/.cargo/config.toml
[build]
target = "wasm32-unknown-unknown"
```
```rust
// plugins/stub-source/src/lib.rs  (compiled with cargo-component 0.21.1 / rustc 1.96.0)
#[allow(warnings)]
mod bindings;

use bindings::filimon::plugin::fetch::fetch;
use bindings::filimon::plugin::log::{log, Level};
use bindings::filimon::plugin::types::{Item, Method, PollError, PollResponse, Request};
use bindings::Guest;
use serde::Deserialize;

#[derive(Deserialize)]
struct Settings {
    greeting: String,
    fetch_url: Option<String>, // when set, init exercises the `fetch` import
}

struct Component;

impl Guest for Component {
    fn init(settings: String, secrets: Vec<(String, String)>) -> Result<(), PollError> {
        let s: Settings = toml::from_str(&settings).map_err(|e| PollError::Config(e.to_string()))?;
        log(Level::Info, &format!("init: {} ({} secrets)", s.greeting, secrets.len()));
        if let Some(url) = s.fetch_url {
            let req = Request { method: Method::Get, url, headers: vec![], body: None };
            match fetch(&req) {
                Ok(r) => log(Level::Debug, &format!("status {}", r.status)),
                Err(e) => log(Level::Warn, &format!("{e:?}")),
            }
        }
        Ok(())
    }

    fn poll(cursor: Option<Vec<u8>>, _access_token: Option<String>) -> Result<PollResponse, PollError> {
        let n = cursor.and_then(|c| c.first().copied()).unwrap_or(0);
        Ok(PollResponse {
            items: vec![Item {
                id: format!("stub-{n}"),
                url: "https://example.invalid/stub".into(),
                title: Some("stub".into()),
                text: "hello".into(),
                author: None,
                published: Some(0),
                platform: "stub".into(),
                raw: None,
            }],
            next_cursor: Some(vec![n.wrapping_add(1)]),
        })
    }
}

bindings::export!(Component with_types_in bindings);
```
(In the probe, the `fetch` call was written with fully qualified paths. The `use` form above is equivalent.)

### §3 Stub fetcher crate (built and `targets`-checked this session)
```rust
// plugins/stub-fetcher/src/lib.rs   (Cargo.toml as §2 but world = "fetcher", no toml/serde deps)
#[allow(warnings)]
mod bindings;

use bindings::exports::filimon::plugin::fetch::Guest;
use bindings::filimon::plugin::types::{FetchError, Request, Response};

struct Component;

impl Guest for Component {
    fn fetch(request: Request) -> Result<Response, FetchError> {
        Ok(Response { status: 200, headers: vec![("x-stub".into(), request.url)], body: b"{}".to_vec() })
    }
}

bindings::export!(Component with_types_in bindings);
```

### §4 Typed config structs (compiled and run on Rust 1.85.0 this session)
```rust
use std::collections::BTreeMap;
use std::num::NonZeroU64;
use std::path::PathBuf;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registry {
    #[serde(default)] pub sources: BTreeMap<String, SourceEntry>,
    #[serde(default)] pub fetchers: BTreeMap<String, FetcherEntry>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceEntry {
    pub component: toml::Spanned<PathBuf>,
    pub config: toml::Spanned<PathBuf>,
    #[serde(default = "default_true")] pub enabled: bool,
    pub fetcher: Option<toml::Spanned<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FetcherEntry {
    pub component: toml::Spanned<PathBuf>,
    #[serde(default = "default_true")] pub enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginFile {
    pub host: HostSection,
    #[serde(default)] pub settings: toml::Table, // opaque; re-serialised for init
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostSection {
    pub poll_interval_secs: NonZeroU64,
    #[serde(default)] pub allowlist: Vec<String>,
    #[serde(default)] pub secrets: BTreeMap<String, toml::Spanned<String>>, // alias -> ENV_VAR
    // + timeout_secs, [host.backoff] {initial_secs, max_secs, multiplier} with defaults
}

fn default_true() -> bool { true }
```
Verified outputs from the probe:
- registry `config` span `60..73` → `"reddit.toml"`, `enabled=true` by default
- `poll_interval_secs = 0` → `span=Some(28..29) msg=invalid value: integer '0', expected a nonzero u64`
- typo → `span=Some(7..26) msg=unknown field 'poll_intervall_secs', expected one of 'poll_interval_secs', 'allowlist', 'secrets'`
- `[host` → `span=Some(5..5) msg=unclosed table, expected ']'`
- `toml::to_string(&settings)` → `listing = "new"\nsubreddits = ["rust"]\n`

### §5 Error types with collect-all rendering (compiled and run on Rust 1.85.0)
```rust
use miette::{Diagnostic, NamedSource, SourceSpan};

#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum ConfigError {
    #[error("plugin `{plugin}`: {message}")]
    #[diagnostic(code(fili::config::malformed_toml))]
    Malformed {
        plugin: String,
        message: String,
        #[source_code] src: NamedSource<String>,
        #[label("here")] span: Option<SourceSpan>,
    },
    #[error("plugin `{plugin}`: secret `{alias}` needs env var `{env_var}`, which is not set in .env or the process environment")]
    #[diagnostic(code(fili::config::missing_secret), help("add `{env_var}=...` to {dotenv}"))]
    MissingSecret {
        plugin: String, alias: String, env_var: String, dotenv: String,
        #[source_code] src: NamedSource<String>,
        #[label("declared here")] span: SourceSpan,
    },
    // + PluginFileMissing { plugin, path, registry src + span of `config` value }
    // + UnknownFetcher { plugin, fetcher, registry src + span of `fetcher` value }
    // + RegistryMissing / RegistryMalformed (short-circuit)
    // + DotenvParse { path, line } (from dotenvy::Error::LineParse)
}

#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("{} problem(s) found in plugin configuration", problems.len())]
pub struct ConfigErrors {
    #[related] pub problems: Vec<ConfigError>,
}
```
Rendered output (verbatim from the probe, path shortened):
```
Error:   × 2 problem(s) found in plugin configuration

Error: fili::config::missing_secret

  × plugin `reddit`: secret `client_id` needs env var `REDDIT_CLIENT_ID`,
  │ which is not set in .env or the process environment
   ╭─[reddit.toml:5:13]
 4 │ [host.secrets]
 5 │ client_id = "REDDIT_CLIENT_ID"
   ·             ─────────┬────────
   ·                      ╰── declared here
 6 │ [settings]
   ╰────
  help: add `REDDIT_CLIENT_ID=...` to .../fili-cfgprobe/.env

Error: fili::config::malformed_toml

  × plugin `youtube`: unclosed table, expected `]`
   ╭─[youtube.toml:1:6]
 1 │ [host
   ·      ▲
   ·      ╰── here
```
Conversions used: `toml::Spanned::span() -> Range<usize>` → `.into()` gives a `SourceSpan`; `toml::de::Error::span() -> Option<Range<usize>>` → `.map(Into::into)` [VERIFIED: docs.rs toml 1.1.8 `de::Error` methods `message`, `span`, `set_input`; serde_spanned 1.1.2 `spanned.rs:91` `pub fn span(&self) -> core::ops::Range<usize>`].

### §6 `.env` resolution without touching the process env (compiled and run on Rust 1.85.0)
```rust
use std::collections::HashMap;
use std::path::Path;

/// # Errors
/// Returns the dotenvy error for unreadable/malformed files; a missing file is not an error.
pub fn read_dotenv(path: &Path) -> Result<HashMap<String, String>, dotenvy::Error> {
    match dotenvy::from_path_iter(path) {
        Ok(iter) => iter.collect(),
        Err(e) if e.not_found() => Ok(HashMap::new()),
        Err(e) => Err(e),
    }
}

/// Redacts on Debug so secrets never reach logs or diagnostics.
pub struct Secret(String);
impl std::fmt::Debug for Secret {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("Secret(***)") }
}

// process env wins over .env (D-12); env lookup is injected for tests
fn lookup(name: &str, dotenv: &HashMap<String, String>, env: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    env(name).or_else(|| dotenv.get(name).cloned())
}
```
Verified: `.env` containing `export QUOTED="a b"` parsed to key `QUOTED`, and afterwards `std::env::var("QUOTED").is_ok()` was `false` (process env untouched). A missing file gives `Error::not_found() == true` [VERIFIED: `dotenvy-0.15.7/src/errors.rs:10-24`, verbatim: `pub enum Error { LineParse(String, usize), Io(io::Error), EnvVar(env::VarError), }` and `pub fn not_found(&self) -> bool`].

### §7 Sample registry and plugin TOML (recommended shape, key names at Claude's discretion)
```toml
# examples/plugins/filimon.toml
[sources.reddit]
component = "components/reddit.wasm"
config = "reddit.toml"
enabled = true
fetcher = "http"

[sources.youtube]
component = "components/youtube.wasm"
config = "youtube.toml"
fetcher = "http"

[fetchers.http]
component = "components/http-fetcher.wasm"
```
```toml
# examples/plugins/reddit.toml
[host]
poll_interval_secs = 300
timeout_secs = 30
allowlist = ["oauth.reddit.com", "www.reddit.com"]

[host.backoff]
initial_secs = 60
max_secs = 3600
multiplier = 2.0

[host.secrets]
client_id = "REDDIT_CLIENT_ID"
client_secret = "REDDIT_CLIENT_SECRET"

[settings]
subreddits = ["rust", "programming"]
listing = "new"
```
Recommended defaults: `timeout_secs = 30`, backoff `60 / 3600 / 2.0`, and `allowlist = []` meaning deny-all. `poll_interval_secs` is required (no default) so operators choose it consciously. [ASSUMED: values are a reasoned default, not sourced]

### §8 Global CLI flag
```rust
// src/cli.rs, inside `pub struct Cli` (src/cli.rs:15)
/// Path to the plugin registry (filimon.toml).
#[arg(long, global = true, env = "FILI_REGISTRY", default_value = "filimon.toml")]
pub registry: PathBuf,
```
[ASSUMED: clap 4 `global = true` + `env` semantics from training knowledge; the `env` feature is already enabled in `Cargo.toml`]

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `wasm32-wasi` target name | `wasm32-wasip1` (and upstream `wasm32-wasip2` since Rust 1.82) | 2024 | cargo-component 0.21.1 still recognises `wasm32-wasi`, `wasm32-wasip1`, `wasm32-unknown-unknown` only (`src/lib.rs:52-53` at v0.21.1, verbatim: `target == "wasm32-wasi" \|\| target == "wasm32-wasip1" \|\| target == "wasm32-unknown-unknown"`) |
| `cargo component` as the only way to build components | Upstream `wasm32-wasip2` + `wit-bindgen` macro is now a common path | 2024–2025 | Locked to `cargo component` by SC2/DOCS-01; fallback noted in Pitfall 12 |
| `dotenv` crate | `dotenvy` (fork) | 2022 | Use `dotenvy` |
| toml 0.5–0.8 | toml 1.x (TOML spec 1.1) | 2025–2026 | `toml::Spanned`, `de::Error::span()` available; MSRV 1.85 |
| `map` usable as a WIT identifier | `map` is a keyword | recent WIT grammar | Avoid as a field name |

**Deprecated/outdated:**
- `dotenvy::dotenv()`-style loaders for this use: they mutate the process env, which is unsafe in edition 2024.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | Stubs (and the documented plugin target) should be `wasm32-unknown-unknown`, so components carry no WASI imports and Phase 3 need not link WASI | Probe Results, Pitfall 1 | Third-party authors following generic `cargo component` docs get wasip1 components with WASI imports; Phase 3 must then either reject them with a clear error or link a locked-down WASI context. Needs user/Phase 3 confirmation |
| A2 | Item record optionality: `id`, `url`, `text`, `platform` required; `title`, `author`, `published`, `raw` optional | Code Examples §1 | One-way contract (D-02). Phase 2 SourceItem must match; changing later is a version bump |
| A3 | `fetch-error` cases `denied, timeout, transport, rate-limited, not-configured, invalid-request`; `method` as a variant with `other(string)` | Code Examples §1 | Costly to change (D-04); adding cases bumps the version |
| A4 | `init` returns `result<_, poll-error>` (reuses poll-error) | Code Examples §1 | Low; a separate `init-error` would be a minor redesign before 0.1.0 is published |
| A5 | `[host]` key names (`poll_interval_secs`, `timeout_secs`, `[host.backoff]`, `allowlist`, `[host.secrets]`) and defaults (30s timeout, 60/3600/2.0 backoff, empty allowlist = deny all, interval required) | Code Examples §7 | Low; Phase 3/5 consume them, so renaming later touches the sample and docs |
| A6 | `serde(flatten)` loses spans / conflicts with `deny_unknown_fields` | Anti-Patterns | Low; just keep structs flat |
| A7 | Path-join semantics for `Path::new("filimon.toml").parent()` | Pitfall 9 | Low; covered by a unit test |
| A8 | clap `global = true` + `env` + `default_value` work together for the `--registry` flag | Code Examples §8 | Low; compile-time and unit-test visible |
| A9 | Phase 1 resolves but does not require existence of `component` `.wasm` paths (Phase 3 HOST-01 loads them; the sample registry's components are not built in Phase 1) | Open Question 1 | Medium; if the user expects a missing `.wasm` to be a Phase 1 config error, add a `ComponentMissing` variant |
| A10 | `src/bindings.rs` in stub crates should be gitignored (regenerated from WIT on each build, which strengthens the "WIT alone" proof) | Open Question 2 | Low |
| A11 | Plugin names must be unique across `sources` and `fetchers`, and limited to `[a-z0-9_-]` | Security / Open Question 3 | Low; avoids ambiguity for Phase 4 `fili plugins check <name>` and cursor-file naming |

## Open Questions (RESOLVED)

All five questions are settled for Phase 1 (2026-11-03, plan revision). Each outcome is a CONTEXT.md "Claude's Discretion" choice or an extension of a locked decision, contradicts no D-NN, needs no user ruling, and is implemented by the plan cited.

1. **Should a missing `component` `.wasm` be a Phase 1 config error?**
   - What we know: D-14 lists missing file (plugin TOML), malformed TOML, missing secret, unknown fetcher. It does not name the component file.
   - What's unclear: whether `load_registry` should stat component paths.
   - Recommendation: resolve the path in Phase 1 but leave the existence check to Phase 3/4 (`fili plugins check`). The sample registry then loads in tests without built `.wasm` files. Alternatively, add the check behind a flag.
   - **RESOLVED:** No. Phase 1 resolves each `component` path relative to the registry directory (D-13) and does not stat it; no `ComponentMissing` variant and no flag. ROADMAP Phase 1 SC4 names the missing plugin TOML file, which is the "missing file" of D-14. Loading components belongs to HOST-01 (Phase 3), and a missing or broken component is reported by `fili plugins check` (CLI-02, Phase 4 SC2). Implemented by 01-02 Task 1 (no existence check, A9) and 01-04 Task 1 (the sample registry loads in tests without built `.wasm` files).
2. **Commit or ignore `plugins/*/src/bindings.rs`?**
   - Recommendation: ignore it, plus `plugins/*/target/`. cargo-component regenerates it on every build, so a committed copy can only go stale.
   - **RESOLVED:** Ignore both. Each stub crate carries its own `.gitignore` with `/target/` and `/src/bindings.rs` (A10), so the root `.gitignore` stays untouched. Implemented by 01-01 Task 1 (stub-source; acceptance runs `git check-ignore` on both paths and confirms `Cargo.lock` stays committable) and Task 2 (stub-fetcher gets the identical `.gitignore`).
3. **Name rules for plugins.**
   - Recommendation: reject names outside `[a-z0-9_-]+` and names used in both `[sources]` and `[fetchers]`. Also reject an enabled source that references a disabled or unknown fetcher. Each is a `ConfigError` with a span.
   - **RESOLVED:** Adopted as recommended (A11, Security V5). `InvalidName` rejects names outside `[a-z0-9_-]+`, `DuplicateName` rejects a name used under both tables, and `UnknownFetcher` / `DisabledFetcher` reject an enabled source pointing at an undefined or disabled fetcher; each names the plugin and field with a span. Implemented by 01-03 Task 1 (`UnknownFetcher`) and Task 2 (`InvalidName`, `DuplicateName`, `DisabledFetcher`). The 01-04 sample satisfies every rule.
4. **Which target is officially supported for third-party plugins (A1)?**
   - Recommendation: Phase 1 documents `wasm32-unknown-unknown` in `plugins/README.md` and the stubs. Flag it for Phase 3 research, which must decide whether to accept wasip1-built components at all.
   - **RESOLVED (Phase 1 scope):** `wasm32-unknown-unknown` is the target Phase 1 builds and documents. Each stub's `rust-toolchain.toml` and `.cargo/config.toml` select it, `plugins/README.md` explains why, and both stub verifies assert no `wasi:` import. Implemented by 01-01 Tasks 1 and 2. Whether the Phase 3 host accepts wasip1-built components (reject with a clear error, or link a locked-down WASI context) is a component-instantiation decision, which CONTEXT.md places outside Phase 1. It is handed to Phase 3 research through STATE.md Blockers/Concerns and places no requirement on Phase 1.
5. **Empty secret values (`KEY=` in `.env`).**
   - Recommendation: treat an empty value as missing, with a distinct message ("set but empty").
   - **RESOLVED:** Adopted as a recorded decision, not an assumption. It extends the "missing secret" case of D-14 and CONF-03: an empty effective value (process env first, then `.env`, per D-12) is an `EmptySecret` error naming the plugin, alias and env var, never the value. Implemented by 01-02 Task 2 (`empty_secret_value_is_error`) and ordered with the other secret problems by 01-03 Task 3. Reversible: dropping the check removes one variant.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| rustc / cargo 1.85.0 (root, via `rustup override`) | host config code | ✓ | 1.85.0 | — |
| Rust toolchain for stubs | stub builds | ✓ | 1.96.0 (installed, has `wasm32-unknown-unknown` and `wasm32-wasip1`); stable 1.98.0 also present | `rust-toolchain.toml` `targets` auto-installs the target |
| `wasm32-unknown-unknown` target | stub builds | ✓ on 1.85.0, 1.96.0, stable; ✗ on 1.93.0 | — | auto-install through `rust-toolchain.toml` |
| `cargo-component` | WIT-01/02 SC2 | ✗ (not on PATH; was built into scratch `/tmp/fili-probe/tools` for this research only) | 0.21.1 | `cargo +stable install cargo-component@0.21.1 --locked` (~8 min build; no prebuilt release asset) |
| `wasm-tools` | WIT validation, `component targets` | ✗ (same scratch build only) | 1.263.0 | `cargo +stable install wasm-tools@1.263.0 --locked` (needs Rust ≥ 1.88) |
| `rustup` | toolchain file handling | ✓ | 1.29.0 | — |
| Network to crates.io | first stub build (deps), tool install | ✓ (used this session) | — | — |

**Missing dependencies with no fallback:** none.
**Missing dependencies with fallback:** `cargo-component` and `wasm-tools` need a Wave 0 install task. A prebuilt `wasm-tools` binary download was blocked by this environment's permission policy, so install from crates.io source.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in `#[test]` (unit tests inside the `fili` bin crate; there is no lib target and no `tests/` dir) |
| Config file | none |
| Quick run command | `cargo test --bin fili plugins::` (baseline `cargo test --bin fili` ran in ~15 s this session, 6 tests pass) |
| Full suite command | `cargo test --bin fili && cargo clippy --bin fili -- -D warnings` plus the WIT/stub script below |
| WIT/stub gate | `wasm-tools component wit wit/ >/dev/null && (cd plugins/stub-source && cargo component build --release) && (cd plugins/stub-fetcher && cargo component build --release) && wasm-tools component targets wit -w source plugins/stub-source/target/wasm32-unknown-unknown/release/stub_source.wasm && wasm-tools component targets wit -w fetcher plugins/stub-fetcher/target/wasm32-unknown-unknown/release/stub_fetcher.wasm` |

Note: root clippy is `warn`, not deny. Use `-D warnings` only scoped to verification, or check that the new module adds no warnings. Phase 1 code is not called from `main` until later phases, so expect `dead_code` warnings. Use a module-level `#![cfg_attr(not(test), allow(dead_code, reason = "consumed by Phase 3/4"))]` or wire `--registry` minimally.

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| WIT-01 | `wit/` parses; `source` world has exports `init`,`poll` and imports `log`,`fetch` | smoke | `wasm-tools component wit wit/ \| grep -E 'export (init\|poll)\|import (log\|fetch)'` | ❌ Wave 0 (`wit/`) |
| WIT-01 | stub source builds from `../../wit` and conforms to `source` | integration | `cd plugins/stub-source && cargo component build --release && wasm-tools component targets ../../wit -w source target/wasm32-unknown-unknown/release/stub_source.wasm` | ❌ Wave 0 |
| WIT-02 | `fetcher` world exports `fetch`, imports `outbound-http`; stub conforms | integration | same as above with `-w fetcher` | ❌ Wave 0 |
| WIT-01/02 | negative: stub source does NOT satisfy `fetcher` | smoke | `! wasm-tools component targets wit -w fetcher <stub_source.wasm>` | ❌ |
| CONF-01 | sample registry + plugin TOMLs load into typed config; paths resolved relative to registry dir | unit | `cargo test --bin fili plugins::config::tests::loads_sample_registry` | ❌ Wave 0 |
| CONF-02 | plugin receives only its aliased secrets; process env overrides `.env`; alias keys, not env names | unit | `cargo test --bin fili plugins::secrets::tests` | ❌ Wave 0 |
| CONF-03 | missing plugin TOML → error names plugin + `config` field | unit | `cargo test --bin fili plugins::config::tests::missing_plugin_file` | ❌ |
| CONF-03 | malformed TOML → error names plugin + span | unit | `...::malformed_plugin_toml` | ❌ |
| CONF-03 | missing secret → error names plugin + alias + env var | unit | `...::missing_secret` | ❌ |
| CONF-03 | unknown fetcher → error names plugin + `fetcher` | unit | `...::unknown_fetcher` | ❌ |
| CONF-03 | multiple problems across plugins all reported (D-14) | unit | `...::collects_all_problems` (assert `problems.len() == N`; render with `miette::GraphicalReportHandler::new_themed(GraphicalTheme::unicode_nocolor())` into a `String` and assert it contains plugin names) | ❌ |

### Sampling Rate
- **Per task commit:** `cargo test --bin fili plugins::` (config tasks) or the WIT/stub gate (WIT tasks)
- **Per wave merge:** full suite + WIT/stub gate
- **Phase gate:** all green before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] Install `cargo-component@0.21.1` and `wasm-tools@1.263.0` (`cargo +stable install ... --locked`)
- [ ] `wit/{types,host,worlds}.wit`
- [ ] `plugins/stub-source`, `plugins/stub-fetcher` (Cargo.toml with `[workspace]`, `rust-toolchain.toml`, `.cargo/config.toml`)
- [ ] Root `Cargo.toml` `[workspace] exclude = ["plugins"]`; `.gitignore` adds `plugins/*/target/` (and the `bindings.rs` decision)
- [ ] `src/plugins/` module with `#[cfg(test)]` tests using `tempfile::TempDir` fixtures (never committed `.env` files)
- [ ] `examples/plugins/` sample `filimon.toml`, plugin TOMLs, `.env.example`; a test loads this exact sample (SC3)

## Security Domain

`security_enforcement: true`, ASVS level 1.

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no (OAuth is Phase 6) | — |
| V3 Session Management | no | — |
| V4 Access Control | yes (capability scoping) | Per-plugin secret filtering (only aliased keys reach `init`); no WASI imports in stub components, so no ambient fs/env access |
| V5 Input Validation | yes | `serde` typed structs + `deny_unknown_fields`, `NonZeroU64`, allowlist host-format validation, plugin-name charset check |
| V6 Cryptography | no | — |
| V8 Data Protection | yes | `Secret` newtype with redacting `Debug`; error messages name env var **names** only, never values; `.env` stays gitignored |
| V14 Configuration | yes | Explicit registry only (nothing auto-discovered); errors fail closed (non-zero exit) |

### Known Threat Patterns for this stack

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Secret value leaked through `Debug`, `tracing`, or miette output | Information disclosure | Redacting newtype; diagnostics print alias + env var name only; unit test asserts the rendered report does not contain the fixture secret value |
| Plugin TOML aliases an unrelated sensitive env var (e.g. `AWS_SECRET_ACCESS_KEY`), because process env overrides `.env` (D-12) | Information disclosure | The operator owns plugin TOMLs. Log at `info` which env var **names** each plugin receives so the mapping is auditable. A prefix restriction is a possible future hardening (not decided) |
| Over-broad secret delivery (all `.env` keys to every plugin) | Elevation of privilege | Per-plugin resolution from `[host.secrets]` only (CONF-02); test asserts plugin B's resolved map lacks plugin A's keys |
| Guest gains ambient capabilities through WASI imports | Elevation of privilege | Build target `wasm32-unknown-unknown`; Phase 3 can refuse components importing `wasi:*` (see A1) |
| Malformed or huge config files | Denial of service | Operator-supplied local files; low risk at ASVS L1. TOML parse errors fail closed |
| Inheriting the crawler's disabled TLS validation (CONCERNS.md) | Tampering | Not touched in Phase 1. The WIT has no "insecure" flag in `request`, and none should be added |

## Sources

### Primary (HIGH confidence, tool-verified this session)
- Scratch probes in `/tmp/fili-probe` with `wasm-tools 1.263.0`, `cargo-component 0.21.1`, rustc 1.96.0 / 1.85.0. Outputs are quoted above.
- crates.io via `cargo info` / API: toml 1.1.8 (rust-version 1.85), dotenvy 0.15.7, thiserror 2.0.21, miette 7.6.0, wit-bindgen-rt 0.44.0/0.41.0, wasm-tools 1.263.0 (rust-version 1.88.0), cargo-component 0.21.1
- GitHub API: cargo-component latest release v0.21.1 (2025-04-07, no assets), repo `archived: false`, pushed 2025-07-14; wasm-tools v1.263.0 (2026-10-30)
- cargo-component v0.21.1 source: `Cargo.toml` (wit-bindgen-* 0.41.0, wit-component 0.227.0), `src/lib.rs:52-53,200-208`, `src/commands/new.rs:25,264,314-320`, `src/metadata.rs` (target table keys `path`, `world`, `dependencies`, `package`, `version`, `registry`)
- Local crate sources: `dotenvy-0.15.7/src/{lib.rs,iter.rs,errors.rs}`, `toml-1.1.8/src/lib.rs` (re-exports `from_str`, `to_string`, `Spanned`, `Table`), `serde_spanned-1.1.2/src/spanned.rs`, wit-bindgen-rt 0.41 vs 0.44 `lib.rs` diff
- Repo files read: `Cargo.toml` (lines 4-5, 8, 12, 31, 41-46), `src/cli.rs`, `src/main.rs`, `src/config.rs`, `src/models.rs`, `.gitignore`, `.planning/*`

### Secondary (MEDIUM)
- https://github.com/bytecodealliance/cargo-component README (experimental status, targets, install)
- https://raw.githubusercontent.com/WebAssembly/component-model/main/design/mvp/WIT.md (keywords, package syntax, multi-file packages, `use` in worlds)
- https://docs.rs/toml/1.1.8/toml/de/struct.Error.html, https://docs.rs/miette/7.6.0 (derive attributes, `#[related]`, `NamedSource`, `SourceSpan` conversions)
- wasm-tools `src/bin/wasm-tools/component.rs` (`component wit` inputs/flags, `component targets` usage)

### Tertiary (LOW)
- Training knowledge for clap `global = true` details and serde `flatten` caveats (tagged `[ASSUMED]`)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH. Versions come from the registry; builds were run on the actual target toolchains.
- Architecture (WIT shape, stub layout): HIGH for "it parses, builds, and conforms" (probed). MEDIUM for contract field choices, which are recommendations within discretion (A2–A4).
- Config loader: HIGH for crate behaviour (compiled and run on 1.85). MEDIUM for key names and defaults (A5).
- Pitfalls: HIGH. Pitfalls 1–5, 8, 10, and 11 were reproduced or verified this session.

**Research date:** 2026-11-03
**Valid until:** 2026-12-03. cargo-component is stagnant and wasm-tools moves weekly, but the pinned versions stay valid.
