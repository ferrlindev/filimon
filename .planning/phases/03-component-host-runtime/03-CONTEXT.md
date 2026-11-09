# Phase 3: Component Host Runtime - Context

**Gathered:** 2026-11-09
**Status:** Ready for planning

<domain>
## Phase Boundary

`fili` hosts WASM components. It loads only what the registry (`filimon.toml`) lists and enables, polls each source on its interval, persists cursors across restarts, isolates failing plugins, and produces returned items to Fluvio as `SourceItem` records (to `filimon-social-items`, Phase 2 D-01).

Not in this phase: the plugin CLI (Phase 4), real fetch routing and allowlist/timeout enforcement (Phase 5), OAuth tokens (Phase 6), real Reddit/YouTube plugins (Phase 7), per-plugin CPU/memory limits (v2 HOST-06), cross-poll dedupe (v2 HOST-07).

Requirements: HOST-01..HOST-05.

</domain>

<decisions>
## Implementation Decisions

### Rate limiting and backoff (HOST-03)
- **D-01:** On `rate-limited(Some(retry_after))` the host waits `max(retry_after, current backoff step)`, capped at the plugin's `[host.backoff] max_secs`. With `None` it uses the backoff step alone.
- **D-02:** Backoff is exponential: `initial_secs * multiplier^n` up to `max_secs`, advancing on each consecutive `transient` / `rate-limited` / `auth-required` failure. **One successful poll resets it fully.** After the backoff window the plugin is polled again, then returns to `poll_interval_secs`. No jitter.

### Failure handling (HOST-05)
- **D-03:** `fatal` and `config` poll/init errors **disable that plugin instance until `fili` restarts** (matches the WIT doc: "host stops polling this instance"). Log loudly with plugin name and error text. `auth-required` is treated as backoff, like `transient`, because Phase 6 can fix it live.
- **D-04:** A **trap** (panic, unreachable, OOM) discards the instance. The host builds a fresh instance and calls `init` again after the backoff window. A trap counts as a transient failure for backoff. It is logged with the plugin name. Other plugins and the `fili` process are unaffected.

### Cursor persistence (HOST-02)
- **D-05:** One **JSON file per plugin alias** in a state directory, written atomically (temp file + rename). The cursor is stored as the opaque bytes from Phase 1 D-01 (encoded as hex or base64 in the file). — **Reversibility:** costly — the state file format and layout is what restarts depend on.
- **D-06:** State dir defaults to **`.fili-state/` next to the registry file** (same relative-to-registry convention as component paths). Overridable by a `--state-dir` flag / `FILI_STATE_DIR` env var.
- **D-07:** The cursor is committed **only after the items are produced to Fluvio and flushed**. Delivery is at-least-once: a crash between flush and persist replays items on restart. No dedupe in this phase (v2 HOST-07).
- **D-08:** A corrupt or unreadable cursor file **disables only that plugin** with an error that names the plugin and the file path. Other plugins keep polling. The operator deletes the file to reset. The host never silently starts over from `none`.

### Claude's Discretion (not discussed — researcher/planner decide)
- **WASI policy (open blocker).** Phase 1 plugins target `wasm32-unknown-unknown` (no `wasi:*` imports). Research must decide whether the host rejects wasip1-built components with a clear error or links a locked-down WASI context. Default lean: reject with a clear error (smallest attack surface, matches PROJECT.md "every capability is an explicit WIT import").
- **Host entry point.** How Phase 3 is driven before Phase 4's run command exists (library API plus integration test harness vs. a minimal internal subcommand). Must not pre-empt the Phase 4 CLI surface.
- **Interim imports.** `fetch` returns `fetch-error::not-configured` until Phase 5; `log` maps to `tracing` with the plugin name. Access token is passed as `none` until Phase 6. What "loading" a fetcher component means before Phase 5 (validate it instantiates against the `fetcher` world, no routing) is left to research.
- Wasmtime configuration, async vs sync host, scheduler structure (per-plugin tasks), item validation (missing id/url) and the WIT item to `SourceItem` mapping (published epoch-ms to RFC 3339 per Phase 2 D-09), Fluvio message key choice.

### Carried forward
- Poll cursor is `option<list<u8>>`, opaque; host stores it verbatim (Phase 1 D-01).
- Item record fields, epoch-ms `published`; typed `poll-error` variants (Phase 1 D-02..D-04).
- Per-plugin `[host]` table carries `poll_interval_secs`, `timeout_secs`, `allowlist`, `[host.backoff]`, `[host.secrets]` (Phase 1 D-11); `[settings]` is passed raw to `init` (D-05); secrets pushed in `init` (D-06).
- Social `SourceItem`s go to `filimon-social-items` and skip the SmartModule chain (Phase 2 D-01); the topic is created by hand by operators (Phase 2 D-05).
- Plugins are stateless; the host never calls `poll` concurrently on one instance.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Contract
- `wit/types.wit`, `wit/worlds.wit`, `wit/host.wit` — the `item`, `poll-error`, `fetch-error` types and the `source` / `fetcher` worlds the host must implement
- `.planning/phases/01-plugin-contract-configuration/01-CONTEXT.md` — D-01..D-12 (cursor, item, errors, settings/secrets, registry split)
- `.planning/phases/01-plugin-contract-configuration/01-RESEARCH.md` — A1 and Open Question 4 (wasip1 vs wasm32-unknown-unknown)
- `.planning/phases/02-sourceitem-pipeline/02-CONTEXT.md` — D-01, D-05, D-08, D-09 (social topic, SourceItem mapping, published format)

### Requirements and roadmap
- `.planning/REQUIREMENTS.md` — HOST-01..HOST-05 (HOST-06/07 deferred)
- `.planning/ROADMAP.md` §Phase 3 — success criteria SC1-SC4
- `.planning/PROJECT.md` — constraints (no direct plugin network, explicit registry, host polls)
- `.planning/STATE.md` §Blockers/Concerns — Phase 3 notes on the interim "not configured" fetch and the WASI decision

### Code
- `src/plugins/config.rs` — `LoadedRegistry`, `LoadedSource`, `PluginConfig`, `HostConfig`, `BackoffConfig`, `LoadedFetcher`
- `src/plugins/mod.rs`, `src/plugins/error.rs`, `src/plugins/secrets.rs` — config load and diagnostics patterns
- `src/streaming.rs` — `TOPIC_SOCIAL_ITEMS`, `FluvioStream`, `TypedProducer` (`send`, `flush`)
- `src/models.rs` — `SourceItem`
- `examples/plugins/filimon.toml`, `examples/plugins/reddit.toml` — registry and per-plugin TOML shape
- `plugins/stub-source`, `plugins/stub-fetcher` — stub components usable as host test fixtures

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `LoadedRegistry` / `LoadedSource` already carry resolved component path, enabled flag, fetcher assignment, host config (interval, timeout, allowlist, backoff) and redacting `Secret` values.
- `TypedProducer::send` / `flush` produce JSON records, which fits the D-07 commit-after-flush ordering.
- `SourceItem` and `TOPIC_SOCIAL_ITEMS` exist from Phase 2 and are importable by host code.
- Stub source and fetcher crates give real components for tests without network.

### Established Patterns
- `miette` diagnostics that name plugin and field/key; secrets never in Debug or logs.
- Consumer and diagnostics never carry record content in error output (Phase 2).
- Commands live in `src/commands/*.rs`, dispatched from `main.rs`; global `--registry` / `FILI_REGISTRY` flag already exists.
- `tokio` with `test-util` is available for paused-time scheduler tests.

### Integration Points
- New `src/plugins/` host module sits beside the config loader and consumes `LoadedRegistry`.
- Host produces to `filimon-social-items` through the existing typed producer.
- Phase 4's CLI will call into this host; Phase 5 will replace the `not-configured` fetch; Phase 6 will replace the `none` token.

</code_context>

<specifics>
## Specific Ideas

- Test the scheduler with tokio paused time so interval and backoff windows are deterministic.
- Restart test for SC2: poll, stop the host, restart, and assert the next `poll` receives the persisted cursor.

</specifics>

<deferred>
## Deferred Ideas

- Per-plugin CPU/fuel and memory limits — v2 HOST-06
- Cross-poll item dedupe (needed to remove at-least-once duplicates) — v2 HOST-07
- Backoff jitter and gradual backoff decay — not needed yet; revisit if flapping APIs show up in Phase 8 live runs

</deferred>

---

*Phase: 03-component-host-runtime*
