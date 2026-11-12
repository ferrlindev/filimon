# Phase 8: Integration, Live E2E & Authoring - Context

**Gathered:** 2026-11-12
**Status:** Ready for planning

<domain>
## Phase Boundary

Prove the whole system end to end: compiled Reddit/YouTube `.wasm` components run inside the host against replayed fixtures (CI-safe) and live APIs (credential-gated), items land in a local Fluvio topic, a human-run live E2E produces NER-enriched social JSON alongside news, and a third party can build, register and run a source plugin (and a fetcher) from a template and guide using only the published WIT. Requirements: TEST-02, TEST-03, TEST-04, WIT-03, DOCS-01, DOCS-02.

</domain>

<decisions>
## Implementation Decisions

### Replay & Fluvio harness (TEST-02)
- **D-01:** Fixtures are replayed by a **replay fetcher plugin** (a `fetcher`-world component serving fixtures by URL). The Reddit/YouTube source instances are assigned to it via the Phase 5 fetcher assignment. This exercises the real host fetch path, allowlist and fetcher assignment with no network. — **Reversibility:** reversible — test infrastructure.
- **D-02:** Integration tests need a **local Fluvio cluster** and **skip cleanly with a clear message** if none is reachable. They create a unique topic per run and assert the expected `SourceItem`s land in it. The harness does not start a cluster, and a missing cluster is not a failure.
- **D-03:** The test helper builds `plugins/reddit` and `plugins/youtube` (and the replay fetcher) with `cargo component build --release` **when the `.wasm` is missing or stale**, so `cargo test` is self-sufficient. No built `.wasm` is committed.

### Live mode gating (TEST-03)
- **D-04:** Live tests use the **same test binary** and **skip at runtime** when the plugin's required `.env` keys are absent, printing `skipped: missing <KEY>`. No extra opt-in flag, no `#[ignore]`.
- **D-05:** Live tests cover the **app-only instances only** (`reddit`: client_id/secret; `youtube`: API key). `reddit-user` / `youtube-user` (PKCE, interactive login) are excluded from automated live runs.
- **D-06:** **One test body, two fetch backends.** Replay uses the replay fetcher and asserts exact items; live uses the `http` fetcher and asserts structure only (non-empty, `platform`, ids, `published` range).

### Live E2E run & evidence (TEST-04)
- **D-07:** TEST-04 is a **human-run checkpoint with scripted steps**: a runbook/script (start Fluvio, news crawl, run `fili` over Reddit + YouTube, consume enriched output) executed by the user with real keys. Not an automated test.
- **D-08:** Evidence committed under the phase directory: a **sanitised, trimmed enriched JSON sample** (social + news, tokens/PII stripped) plus the **run transcript**, referenced from VERIFICATION.md.
- **D-09:** SC3 requires **both Reddit and YouTube**. If Reddit credentials cannot be obtained, **stop and ask the user** whether to waive; never silently drop Reddit. (Replay-only Reddit coverage with synthetic fixtures per 07 D-21 does not satisfy TEST-04.)

### Authoring: template, WIT distribution, guide (WIT-03, DOCS-01, DOCS-02)
- **D-10:** The template is a **`plugins/template` crate, copy-and-rename**, derived from `plugins/stub-source`, minimal and commented. It must build as-is in CI/tests.
- **D-11:** An outside author obtains the WIT by **copying `wit/` from a tagged release or a git dependency** (git archive / sparse checkout into the plugin's `wit/`). No WIT registry publishing (wkg/warg) this phase. — **Reversibility:** costly — this is the published authoring contract third parties follow.
- **D-12:** The guide lives at **`docs/plugin-guide.md`** (new `docs/` dir). A **script builds the template in a temp dir outside the repo against only the WIT**, registers it in a `filimon.toml`, and runs `fili plugins run-once` with the **unmodified `fili` binary**; the user also runs it once as the SC4 proof.
- **D-13:** The guide's fetcher walkthrough (DOCS-02) **reuses the replay fetcher as the worked example**, so the documented steps are exercised by the integration tests. A built-by-following-the-guide fetcher must serve a source's requests.

### Claude's Discretion
- Exact test file layout, helper names, Fluvio reachability probe, topic naming, and staleness check for the cached build.
- Runbook/script format (shell vs `just`) and location; sanitisation tooling for the evidence sample.
- How the replay fetcher maps URL to fixture file and how Reddit/YouTube base URLs are routed to it.
- Guide structure and section order; pinning the WIT tag name used in examples.

### Carried forward
- WIT unchanged: one-token `poll(cursor, access-token)` (Phase 1 D-07, Phase 6 D-10, Phase 7 D-02).
- Plugins target `wasm32-unknown-unknown`, built with `cargo component`, own `[workspace]` and `Cargo.lock` (Phase 1 D-16, Phase 7 D-16).
- Fixtures under each crate's `tests/fixtures/`; synthetic ones labelled per Phase 7 D-21.
- Instance pattern: `reddit`, `reddit-user`, `youtube`, `youtube-user`, one `.wasm` per platform (Phase 7 D-01, D-07).
- Host does TLS, allowlist, timeout and size limits; `http-fetcher` is a normal registry entry (Phase 5).
- Items pass to `filimon-social-items` unchanged; NER is consumer-side (Phase 2).
- Existing `fluvio-wise` gaps remain out of scope (PROJECT.md).

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Requirements and scope
- `.planning/ROADMAP.md` §Phase 8 — goal and 5 success criteria
- `.planning/REQUIREMENTS.md` — TEST-02, TEST-03, TEST-04, WIT-03, DOCS-01, DOCS-02
- `.planning/PROJECT.md` — constraints, out-of-scope, key decisions

### Prior phase decisions
- `.planning/phases/07-reddit-youtube-plugins/07-CONTEXT.md` — instance pattern, fixtures (D-18, D-21), crate layout (D-16)
- `.planning/phases/07-reddit-youtube-plugins/07-RESEARCH.md` — fixture capture recipes
- `.planning/phases/06-host-managed-oauth/06-CONTEXT.md` — OAuth and token delivery
- `.planning/phases/03-component-host-runtime/03-CONTEXT.md` — host polling, backoff, cursor

### Code and examples
- `wit/` — published WIT package `filimon:plugin@0.1.0`
- `plugins/README.md` — existing plugin crate descriptions
- `examples/plugins/` — registry and per-plugin TOML examples
- `.planning/codebase/TESTING.md`, `.planning/codebase/CONVENTIONS.md` — test conventions

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `plugins/stub-source`: base for `plugins/template` (D-10).
- `plugins/stub-fetcher`, `plugins/http-fetcher`: patterns for the replay fetcher.
- `plugins/reddit/tests/replay.rs`, `plugins/youtube/tests`: native fixture replay and `tests/fixtures/` to reuse.
- `tests/plugins_cli.rs`, `tests/auth_cli.rs`: existing CLI integration tests (patterns for driving `fili`).
- `src/commands/run.rs`, `src/commands/plugins.rs`: `run-once` and the run command.

### Established Patterns
- Plugins are separate crates with own `[workspace]`/`Cargo.lock`, built with `cargo component`.
- No `.github/` CI exists; "CI-safe" means no network and no keys.
- No `docs/` directory exists yet.

### Integration Points
- Host fetcher assignment (Phase 5) routes a source's fetch to the replay fetcher.
- Fluvio topics consumed by `src/commands/consume.rs` for the enriched output.

</code_context>

<specifics>
## Specific Ideas

No specific requirements beyond the recommended options selected for every question.

</specifics>

<deferred>
## Deferred Ideas

- Publishing the WIT to a registry (wkg/warg) — future work once third-party adoption warrants it.
- Automated live E2E run including news crawling — deferred in favour of a human-run checkpoint.
- Live tests for PKCE instances (`reddit-user`, `youtube-user`).
- Adding CI (`.github/workflows`) — not requested; tests are written to be CI-safe.

</deferred>

---

*Phase: 8-Integration, Live E2E & Authoring*
*Context gathered: 2026-11-12*
