---
phase: "4"
slug: "plugin-cli"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: validated
nyquist_compliant: true
wave_0_complete: true
created: "2026-11-10"
---

# Phase 4 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Seeded from `04-RESEARCH.md` § Validation Architecture. Task IDs are filled in once plans exist.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in harness; `#[tokio::test(start_paused = true)]` for scheduler and shutdown tests; new `tests/plugins_cli.rs` drives the binary through `CARGO_BIN_EXE_fili` |
| **Config file** | none (Cargo.toml); `tests/` does not exist yet |
| **Quick run command** | `cargo test -p filimon plugins::` (plus `cli::tests` or `commands::` for the module under change) |
| **Full suite command** | `cargo test -p filimon` (baseline at phase start: 255 passed, 29 ignored) |
| **Wasm group** | `cargo test -p filimon -- --ignored wasm_` (needs the stub and fault fixtures built; see plugins/README.md) |
| **Live group** | `cargo test -p filimon -- --ignored live_` (needs the local cluster; scratch `filimon-test-*` topics only) |
| **Estimated runtime** | ~60 seconds quick/full; ignored groups extra |

---

## Sampling Rate

- **After every task commit:** Run the quick command for the module under change
- **After every plan wave:** Run `cargo test -p filimon` and `cargo clippy --all-targets`
- **Before `/gsd-verify-work`:** Full suite green, then `-- --ignored wasm_`, then (cluster up) `-- --ignored live_`. Report any group that could not run as NOT RUN; plain `cargo test` does not exercise real wasm
- **Max feedback latency:** 120 seconds

---

## Per-Task Verification Map

Requirement-level map (task rows are added when plans are written).

| Requirement | Behavior | Test Type | Automated Command | File Exists | Status |
|-------------|----------|-----------|-------------------|-------------|--------|
| CLI-01 | `list` rows for source, fetcher, disabled, never-polled, ok, error and unreadable status; works with a broken plugin TOML and while the state-dir lock is held; creates nothing | unit | `cargo test -p filimon commands::plugins` | ✅ | ✅ green |
| CLI-01 | scheduler writes status after PollOk, guest error, trap, sink failure, disable and corrupt cursor; failure leaves the cursor file byte-identical | unit (fakes, paused time) | `cargo test -p filimon plugins::host::scheduler` | ✅ | ✅ green |
| CLI-01 | `read_status` / `write_status` round trip, atomic, tolerant of garbage, size cap, no collision with cursor names | unit | `cargo test -p filimon plugins::host::cursor` | ✅ | ✅ green |
| CLI-01 | `fili plugins list` and `--json`: exit 0, stdout is exactly the table or valid JSON (no banner) | integration (binary) | `cargo test -p filimon --test plugins_cli list` | ✅ | ✅ green |
| CLI-02 | `check` passes a good source and fetcher component | wasm | `cargo test -p filimon -- --ignored wasm_check` | ✅ | ✅ green |
| CLI-02 | wrong world, garbage bytes and a WASI import each fail and name the plugin and the problem | unit | `cargo test -p filimon commands::plugins::tests::check` | ✅ | ✅ green |
| CLI-02 | missing secret, bad TOML and bad `[host]` value fail naming plugin and field; other plugins' problems are not reported | unit | same | ✅ | ✅ green |
| CLI-02 | `check` never polls and never creates the state dir; exit code non-zero on failure; secret value never printed | unit + integration | `cargo test -p filimon --test plugins_cli check` | ✅ | ✅ green |
| CLI-03 | single poll prints mapped items; dropped count shown; sanitized and clipped; `--json` is one `SourceItem` per line | unit (`FakeFactory`) | `cargo test -p filimon plugins::host::once` | ✅ | ✅ green |
| CLI-03 | without `--produce` the sink is never called and the state dir is untouched; with it, send then cursor write, cursor unchanged on sink failure | unit (`VecSink`, failing sink) | same | ✅ | ✅ green |
| CLI-03 | real stub component: `run-once stub` prints `stub-0` without Fluvio | wasm | `cargo test -p filimon -- --ignored wasm_run_once` | ✅ | ✅ green |
| CLI-04 | `Host::run` returns within the shutdown grace and reports aborted plugins when the sink never returns | unit (paused time) | `cargo test -p filimon plugins::host::tests` | ✅ | ✅ green |
| CLI-04 | `run_with_sink` polls all enabled sources, skips disabled, prints load failures, errors on zero sources | unit (`VecSink`) | `cargo test -p filimon commands::run` | ✅ | ✅ green |
| CLI-04 | run against a scratch topic produces records; stop; restart resumes | live | `cargo test -p filimon -- --ignored live_run` | ✅ | ✅ green (run by the 04-06 executor against the local cluster; not re-run at validation) |
| WR-07 | empty `--state-dir` rejected by clap and by `resolve_state_dir` / `CursorStore::open` | unit | `cargo test -p filimon cli::tests` | ✅ | ✅ green |
| CLI surface | new subcommands and flags parse; `--state-dir` and `--registry` work before and after the subcommand; `plugins` requires a subcommand | unit | `cargo test -p filimon cli::tests` | ✅ | ✅ green |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [x] `tests/plugins_cli.rs` — binary-level exit-code and stdout tests for CLI-01 and CLI-02 (the `tests/` directory is new)
- [x] registry-fixture helpers for `commands/plugins.rs` tests (reuse `testing.rs::write_registry` and `load_test_registry`)
- [x] a generic supervisor in `host/mod.rs` that `FakeFactory` can drive (precondition for the paused-time shutdown test)
- [x] update every `HostOptions { state_dir }` literal if a field is added

No framework install is needed.

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| The real `fili run` binary produces to `filimon-social-items` and stops cleanly on Ctrl-C | CLI-04 | The production sink is pinned to the production topic, so no automated test may run it | With the cluster up and a registry that lists the stub plugin, run `fili run`, confirm records on the topic, press Ctrl-C and confirm it exits within the shutdown grace |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 120s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** validated 2026-11-10 (all 16 requirement rows covered; manual-only item remains manual)

## Validation Audit 2026-11-10

| Metric | Count |
|---|---|
| Gaps found | 0 |
| Resolved | 0 |
| Escalated | 0 |
