---
last_mapped_commit: ee408de9974ea0cee802f332ee2386ef13b0428b
last_mapped_at: 2026-11-22
---
# Codebase Concerns

**Analysis Date:** 2026-11-22

## Tech Debt

**Oversized modules:**
- Issue: Several files exceed 2000 lines and mix concerns (config parsing, CLI handling, scheduling, fetching).
- Files: `src/plugins/config.rs` (3981), `src/commands/consume.rs` (2809), `src/commands/plugins.rs` (2686), `src/plugins/host/scheduler.rs` (2594), `src/commands/auth.rs` (2068), `src/plugins/host/fetch.rs` (1967)
- Impact: Hard to review, high merge-conflict risk, slow incremental compile.
- Fix approach: Split into submodules (parsing / validation / types / tests). Move inline test blocks into `tests/` or sibling `*_tests.rs` files, as already done for `src/plugins/host/integration_tests.rs`.

**Panicking config load in CLI args:**
- Issue: `Args::produce` calls `load_config(&self.config).unwrap()` and `self.ls.as_ref().unwrap()`. A missing or malformed `config.json` panics instead of returning an error.
- Files: `src/args.rs` (lines 15-20)
- Impact: Poor UX on bad config; the untracked `test_config_panic.json` at repo root (contents: `not json`) is a leftover from the panic test.
- Fix approach: Return `Result` from `produce_links`, propagate via `anyhow`/`Box<dyn Error>`, and delete `test_config_panic.json`.

**Unfinished async hook in watch command:**
- Issue: `// TODO: We can't easily await here in the sync closure`.
- Files: `src/commands/watch.rs` (line 112)
- Impact: Some watch behavior is deferred or incomplete.
- Fix approach: Use a channel or `tokio::spawn`/`block_on` handle from the sync callback.

**Per-plugin workspace duplication:**
- Issue: Each plugin (`plugins/reddit`, `plugins/rss`, `plugins/http-fetcher`, `plugins/fault-source`, `plugins/replay-fetcher`, `plugins/stub-fetcher`) is a separate crate with its own `Cargo.lock`, `rust-toolchain.toml`, and `.cargo/config.toml`. `plugins/reddit/src/{cursor,http,poll,settings}.rs` and `plugins/rss/src/{cursor,http,poll,settings}.rs` have parallel structure.
- Impact: Dependency and toolchain drift; duplicated logic (cursor, poll, http wrappers) must be fixed in multiple places.
- Fix approach: Extract a shared plugin-support crate; consider a plugins workspace.

## Known Bugs

No confirmed bugs detected from static review. Verify `src/commands/watch.rs` TODO behavior.

## Security Considerations

**Secrets handling:**
- Risk: `.env` is gitignored, and `.env.example` is tracked (currently modified, uncommitted). Auth providers store/handle tokens (`src/plugins/auth/client.rs`, `src/plugins/auth/provider.rs`, `src/commands/auth.rs`).
- Current mitigation: `.gitignore` excludes `.env` and `.env.*`.
- Recommendations: Confirm token storage uses restrictive file permissions and that logs never print credentials. Review the `.env.example` diff before commit so no real values land in it.

**Untrusted WASM plugins and network content:**
- Risk: Plugins run in wasmtime (`wasmtime = 48`, component-model) and fetch arbitrary external content (HTML, RSS, Reddit JSON).
- Files: `src/plugins/host/fetch.rs`, `src/plugins/host/scheduler.rs`, `plugins/*/src/http.rs`
- Current mitigation: Component sandbox plus host-mediated fetch.
- Recommendations: Enforce host-side limits on response size, redirects, timeouts, and allowed hosts, and verify fuel/memory limits on plugin instances.

## Performance Bottlenecks

**Large model and build artifacts:**
- Problem: `models/` (591M, gitignored) holds GLiNER weights, loaded for extraction. `target/` is 3.5G.
- Improvement path: Document model download in the README; keep model loading lazy and shared across calls.

**Wasmtime compile cost:**
- Problem: `cranelift` plus `parallel-compilation` makes plugin load and builds heavy.
- Improvement path: Cache compiled components.

## Fragile Areas

**Vendored `fluvio/` checkout:**
- Files: `fluvio/` (1.6G, untracked build tree; `fluvio/target/` is large)
- Why fragile: A local Fluvio clone coexists with `fluvio = "0.50"` in `Cargo.toml`. It is unclear which one the build uses. `git ls-files fluvio` returns nothing, so it is not committed.
- Safe modification: Do not edit; confirm whether it is needed and add it to `.gitignore` if it is local only.

**Plugin scheduler:**
- Files: `src/plugins/host/scheduler.rs`, `src/plugins/config.rs`
- Why fragile: Large stateful code (retries, cursors, rate limits) with timing-dependent behavior.
- Safe modification: Change only with `src/plugins/host/integration_tests.rs` and `src/plugins/host/wasm_tests.rs` run.
- Test coverage: Tests exist; WASM tests need prebuilt plugin artifacts.

**Fixture/WIT coupling:**
- Files: `wit/types.wit`, `src/models.rs` (test parses the WIT text by string search at lines ~433)
- Why fragile: Tests find record bodies via `find('}')`, so a WIT formatting change breaks them.
- Safe modification: Edit WIT and the model tests together.

## Scaling Limits

Not measured. Single-process scheduler with in-memory state; no horizontal scaling documented.

## Dependencies at Risk

**Pinned toolchain per crate:**
- Risk: `rust-toolchain.toml` in root and in each plugin can diverge.
- Impact: Build failures for wasm targets when versions differ.
- Migration plan: Single shared toolchain file.

## Missing Critical Features

Not detected.

## Test Coverage Gaps

**Test hygiene:**
- What's not tested: Panic paths in `src/args.rs` are tested with fixed temp filenames in the working directory (`fs::write(config_path, ...)` then `remove_file`), which can race under parallel test runs and leave files such as `test_config_panic.json`.
- Files: `src/args.rs` (lines 80-141)
- Risk: Flaky tests, stray files committed accidentally.
- Priority: Medium. Use `tempfile::TempDir` as `src/output.rs` already does.

**Network-dependent behavior:**
- Replay fixtures exist for Reddit (`plugins/reddit/tests/fixtures/`) but the RSS plugin (`plugins/rss/`) has no fixtures directory.
- Priority: Low to Medium.

## Repository Hygiene

- Working tree has deleted tracked `.planning/codebase/*.md` files, untracked tooling dirs (`.agents/`, `.bg-shell/`, `.claude/`, `.cursor/`, `.gsd-path/`), and untracked `AGENTS.md`/`WORKFLOW.md`. Decide which to commit or gitignore.

---

*Concerns audit: 2026-11-22*
