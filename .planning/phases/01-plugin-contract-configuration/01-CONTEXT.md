# Phase 1: Plugin Contract & Configuration - Context

**Gathered:** 2026-11-03
**Status:** Ready for planning

<domain>
## Phase Boundary

Deliver the stable, versioned contract plugin authors and operators write against:

1. A versioned WIT package (`wit/`) defining a `source` world and a `fetcher` world, validated with `wasm-tools`.
2. Stub source + stub fetcher crates that generate bindings from the WIT package alone (`cargo component`) and compile to valid components.
3. Typed loading of the `filimon.toml` registry, per-plugin TOML files, and `.env` secrets, with clear errors naming plugin + field/key.

Requirements: WIT-01, WIT-02, CONF-01, CONF-02, CONF-03.

NOT in this phase: instantiating/running components (Phase 3), SourceItem Rust envelope and pipeline routing (Phase 2), CLI subcommands (Phase 4), fetch routing/allowlist enforcement (Phase 5), OAuth flows (Phase 6). Phase 1 only *defines and loads/validates* the formats those phases consume.

</domain>

<decisions>
## Implementation Decisions

### WIT data shapes
- **D-01:** Poll cursor is `option<list<u8>>` — opaque bytes, `none` on first poll. Host persists it verbatim and passes it back. (User chose bytes over string for flexibility; host-side display of cursors must hex/base64-encode.) — **Reversibility:** one-way — changes the published `poll` signature every plugin compiles against.
- **D-02:** Item record = typed fields mirroring the Phase 2 `SourceItem` (id, url, title, text, author, published, platform) plus `raw: option<string>` holding platform JSON metadata. Host validates required fields. — **Reversibility:** one-way — published record shape; Phase 2 SourceItem must line up with it (Phase 2 SC1).
- **D-03:** `published` encoded as `option<u64>` unix epoch **milliseconds**. Host renders RFC 3339 when building SourceItem JSON.
- **D-04:** Errors are typed variants. `poll` returns `result<..., poll-error>` where `poll-error = variant { rate-limited(option<u32> /* retry-after secs */), auth-required, config(string), transient(string), fatal(string) }`. `fetch` gets an analogous typed `fetch-error` variant (exact cases at Claude's discretion; must distinguish denied-by-policy, timeout, transport, and rate-limited so Phase 3/5 can back off per variant). — **Reversibility:** costly — variant cases are part of the contract; adding cases is a version bump.

### Config delivery to plugins
- **D-05:** Source `init` receives the plugin's `[settings]` section as a **raw TOML string**; the plugin parses it itself (guest needs a TOML parser, e.g. `toml` crate).
- **D-06:** Secrets are **pushed in `init`** alongside settings: `init(settings: string, secrets: list<tuple<string, string>>)` (exact param naming at Claude's discretion). Only the secrets the plugin's TOML declares are passed (satisfies CONF-02). Keys are the alias names from D-12, not env var names.
- **D-07:** OAuth access token is **passed into `poll`**: `poll(cursor: option<list<u8>>, access-token: option<string>)`. No token import. Host passes `none` until Phase 6 wires real tokens. Refresh tokens never cross the boundary (AUTH-03).
- **D-08:** Fetcher world has **no init** — fetchers are pure `fetch(request) -> result<response, fetch-error>` with no settings or secrets. (Implication: a configurable headless-browser fetcher (FETCH-05, v2) will need a contract revision — accepted.)
- **D-09:** Resulting `source` world imports only `fetch` and `log` (settings/secrets/token flow via init/poll params). **User approved rewording WIT-01 and Phase 1 SC1** to say the plugin is provided settings, secrets and access tokens by the host (via init/poll parameters) rather than via import functions. REQUIREMENTS.md WIT-01 and ROADMAP.md Phase 1 SC1 updated accordingly in this commit.

### Registry & TOML layout
- **D-10:** `filimon.toml` uses **named tables**: `[sources.<name>]` with `component`, `config`, `enabled`, `fetcher`; `[fetchers.<name>]` with `component` (and `enabled`). Table key is the plugin name, so duplicates are impossible and errors cite `sources.reddit`.
- **D-11:** Key split — **registry = wiring only** (component path, config path, enabled, fetcher assignment). **Per-plugin TOML = everything else**: a `[host]` table (poll interval, backoff, allowlist, timeout, secrets aliases) and a `[settings]` table passed raw to `init` (D-05). Host treats `[settings]` as opaque: it only checks it is valid TOML; plugin owns schema validation, and `init` returning `config(..)` error surfaces as a config error naming the plugin.
- **D-12:** Secrets declared as an **alias map** in the plugin TOML: `[host.secrets] client_id = "REDDIT_CLIENT_ID"` — plugin sees `client_id`, operator chooses the env var. `.env` is read from the **directory of `filimon.toml`**; real process env vars override `.env` values.
- **D-13:** `filimon.toml` located at `./filimon.toml` by default, overridable with a global `--registry <path>` flag (env `FILI_REGISTRY`). All `component`/`config` paths resolve **relative to the registry file's directory**.
- **D-14:** Config errors (CONF-03): **collect all problems across all plugins**, then print one `miette` diagnostic per problem (plugin name, file path, field/key, TOML span when available) and exit non-zero. Missing file, malformed TOML, missing secret, unknown fetcher reference all covered.

### Versioning & crate layout
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

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Project scope & requirements
- `.planning/PROJECT.md` — constraints (WASM component model, host-mediated fetch, secrets in `.env`, official APIs only) and Key Decisions
- `.planning/REQUIREMENTS.md` — WIT-01, WIT-02, CONF-01..03 (WIT-01 reworded per D-09)
- `.planning/ROADMAP.md` §Phase 1 — success criteria (SC1 reworded per D-09); §Phase 2 SC1 requires SourceItem to line up with the WIT item record; §Phase 3 blocker note on interim "not configured" fetch
- `.planning/STATE.md` — research flag: Phase 1 WIT/`cargo component` toolchain versions

### Codebase
- `.planning/codebase/STACK.md` — Rust 1.85 pin (SmartModule linker issue), workspace members
- `.planning/codebase/CONVENTIONS.md` — miette errors, tracing, module/doc style, clippy pedantic
- `.planning/codebase/CONCERNS.md` — crawler disables TLS validation (must not leak into plugin fetch)

No external specs/ADRs — requirements fully captured in decisions above.

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `miette` (with `fancy`) already a dependency — use for CONF-03 diagnostics (supports source spans).
- `serde` derive already used for Fluvio records — reuse for typed config structs.
- `clap` derive with `env` feature — `--registry` / `FILI_REGISTRY` global flag fits `src/cli.rs` `Cli` struct.

### Established Patterns
- Errors: `miette::Result` + `miette!()` with contextual messages; avoid `unwrap()`.
- Module headers with ASCII section dividers; `///` docs with `# Errors` sections.
- Clippy `all` + `pedantic` warn level — new code must be clean.

### Integration Points
- `src/config.rs` currently loads the unrelated watch-mode `config.json` (`{ "ls": [...] }`). Do NOT repurpose it; put plugin config in a new module.
- `src/cli.rs` `Cli`/`Command` — global `--registry` flag lands here (subcommands come in Phase 4).
- Root `Cargo.toml` `[workspace] members` — must add `exclude = ["plugins/*"]` (or equivalent) so stub crates stay out.
- `fluvio/` is a reference checkout — never modify; `.gitignore` already ignores it.

</code_context>

<specifics>
## Specific Ideas

- Cursor bytes: when the host later shows cursors (`fili plugins list`, logs), render as hex/base64.
- Stub source should demonstrate the full init contract (raw TOML settings + pushed secrets) so it serves as a living example for the Phase 8 authoring guide.

</specifics>

<deferred>
## Deferred Ideas

- Fetcher configuration/secrets (needed by a future headless-browser/CDP fetcher, FETCH-05 v2) — requires adding a fetcher init in a later contract version.
- Host-side schema validation of plugin `[settings]` — plugin owns validation for now.
- "Skip bad plugins and warn" run-mode behaviour — possible later flag on the run command (Phase 4).

</deferred>

---

*Phase: 01-plugin-contract-configuration*
*Context gathered: 2026-11-03*
