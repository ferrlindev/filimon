---
phase: "3"
slug: "component-host-runtime"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: draft
nyquist_compliant: false
wave_0_complete: false
created: "2026-11-09"
---

# Phase 3 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in harness; tokio `#[tokio::test(start_paused = true)]` (`test-util` present) |
| **Config file** | none (Cargo.toml) — plan 03-03 adds the toolchain pins |
| **Quick run command** | `cargo test -p filimon plugins::host` |
| **Full suite command** | `cargo test -p filimon` |
| **Wasm fixture tests** | `cargo test -p filimon -- --ignored wasm_` (needs stub-source, stub-fetcher and fault-source built) |
| **Live Fluvio tests** | `cargo test -p filimon -- --ignored live_` (needs a local cluster; scratch topics only) |
| **Estimated runtime** | ~60 seconds (quick/full); ignored groups extra |

Filter note: libtest ORs several filters, so the ignored groups use a single substring (`wasm_` or `live_`). Adding a path filter such as `plugins::host` beside `--ignored wasm_` would also run the live tests. Live tests are kept out of the `wasm_tests` module for the same reason (the module name contains `wasm_`).

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p filimon plugins::host`
- **After every plan wave:** Run `cargo test -p filimon`
- **Before `/gsd-verify-work`:** Full `cargo test -p filimon`, then `-- --ignored wasm_`, then (cluster up) `-- --ignored live_` — all green. Plain `cargo test` does not exercise real wasm; report ignored groups separately and report a group that could not run as NOT RUN.
- **Max feedback latency:** 90 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 3-01-01 | 03-01 | 1 | HOST-01..05 (prerequisite) | T-3-01 | Machine-local rustup override removed by the user only | manual (human-action) | `! rustup override list \| grep -qF "$(pwd)"` | n/a | ⬜ pending |
| 3-02-01 | 03-02 | 1 | HOST-03, HOST-05 | T-3-02, T-3-03 | Fault fixture builds; never logs secrets | build | `(cd plugins/fault-source && cargo component build --release)` | ❌ W0 | ⬜ pending |
| 3-02-02 | 03-02 | 1 | HOST-03, HOST-05 | T-3-02, T-3-SC | Fixture fits `source` world, no WASI import, same lock set as stub-source | conformance | `wasm-tools component targets ../../wit -w source <wasm>` | ❌ W0 | ⬜ pending |
| 3-03-01 | 03-03 | 2 | HOST-01..05 (prerequisite) | T-3-04, T-3-06, T-3-SC | wasmtime 48.x in Cargo.lock; no WASI crate or extra features | build + grep | `cargo check -p filimon` + Cargo.lock gate | ❌ W0 | ⬜ pending |
| 3-03-02 | 03-03 | 2 | HOST-01..05 (prerequisite) | T-3-05 | SmartModules still build on nested 1.85.0 pins | build | `(cd smartmodules/filter-relevance && smdk build)` | ❌ W0 | ⬜ pending |
| 3-04-01 | 03-04 | 3 | HOST-01, HOST-02, HOST-04 | T-3-07..T-3-13 | Tracer: registry-listed stub polled, item mapped, sink called, cursor written after sink | unit + `wasm_` | `cargo test -p filimon plugins::host` and `plugins::host::wasm_tests::wasm_tracer -- --ignored` | ❌ W0 | ⬜ pending |
| 3-04-02 | 03-04 | 3 | HOST-02 | T-3-11 | Restart resumes from persisted cursor | `wasm_` | `cargo test -p filimon plugins::host::wasm_tests -- --ignored` | ❌ W0 | ⬜ pending |
| 3-05-01 | 03-05 | 3 | HOST-02 | T-3-15 | `--state-dir` / `FILI_STATE_DIR` global flag, no default | unit | `cargo test -p filimon cli::tests` | ✅ | ⬜ pending |
| 3-05-02 | 03-05 | 3 | HOST-05 | T-3-14 | Subscriber on stderr, default `fili::plugins=info` | unit | `cargo test -p filimon default_filter_passes_plugin_events` | ✅ | ⬜ pending |
| 3-06-01 | 03-06 | 4 | HOST-03 | T-3-16 | Backoff sequence, cap, retry-after max rule, no overflow | unit | `cargo test -p filimon plugins::host::backoff` | ❌ W0 | ⬜ pending |
| 3-06-02 | 03-06 | 4 | HOST-03, HOST-05 | T-3-16..T-3-18 | Pure `next_action`: D-01..D-04 table | unit | `cargo test -p filimon plugins::host::scheduler` | ❌ W0 | ⬜ pending |
| 3-06-03 | 03-06 | 4 | HOST-02, HOST-03, HOST-05 | T-3-17, T-3-19 | Interval, backoff, trap rebuild, isolation, logs under paused time (fakes) | unit (paused time) | `cargo test -p filimon plugins::host::scheduler::tests::sched_` | ❌ W0 | ⬜ pending |
| 3-07-01 | 03-07 | 4 | HOST-01 | T-3-20, T-3-21, T-3-23 | Unlisted and disabled files never opened; WASI import rejected with named imports | unit | `cargo test -p filimon plugins::host::loader` | ❌ W0 | ⬜ pending |
| 3-07-02 | 03-07 | 4 | HOST-01 | T-3-22 | Fetchers load against fetcher world; wrong-world rejected | unit + `wasm_` | `cargo test -p filimon plugins::host::loader -- --ignored` | ❌ W0 | ⬜ pending |
| 3-08-01 | 03-08 | 4 | HOST-04 | T-3-24, T-3-25 | Item validation, cap boundary, content-free warnings, key `{platform}:{id}` | unit | `cargo test -p filimon plugins::host::map` | ❌ W0 | ⬜ pending |
| 3-08-02 | 03-08 | 4 | HOST-04 | T-3-24, T-3-26, T-3-27 | FluvioSink send-all-then-flush, `clear_errors` on failure | unit + `live_` | `cargo test -p filimon plugins::host::sink`; `-- --ignored live_` | ❌ W0 | ⬜ pending |
| 3-09-01 | 03-09 | 5 | HOST-02 | T-3-28..T-3-31 | Cursor file shape, mismatch/version checks, lock, atomic write | unit | `cargo test -p filimon plugins::host::cursor` | ❌ W0 | ⬜ pending |
| 3-09-02 | 03-09 | 5 | HOST-02 | T-3-28 | Commit-after-flush, corrupt file disables only that plugin and is never reset, persist-failure policy | unit (paused time) | `cargo test -p filimon plugins::host::scheduler::tests::sched_` | ❌ W0 | ⬜ pending |
| 3-10-01 | 03-10 | 6 | HOST-03, HOST-05 | T-3-32 | Real-wasm trap isolation and rebuild, backoff, fatal/config disable, panic isolation, spinner does not block | `wasm_` | `cargo test -p filimon plugins::host::wasm_tests -- --ignored` | ❌ W0 | ⬜ pending |
| 3-10-02 | 03-10 | 6 | HOST-02, HOST-04 | T-3-34 | Stub to Fluvio scratch topic with restart, no gap, no repeat | `live_` | `cargo test -p filimon -- --ignored live_` | ❌ W0 | ⬜ pending |
| 3-10-03 | 03-10 | 6 | HOST-01..05 | T-3-33 | README states guarantees and limits as implemented | grep | README heading and phrase gate in plan 03-10 Task 3 | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] Toolchain checkpoint (human action, plan 03-01) and Cargo.toml / toolchain-pin changes (plan 03-03)
- [ ] `plugins/fault-source/` fixture crate + README entry; build both stubs and the fixture (plan 03-02)
- [ ] `src/plugins/host/` skeleton with `ItemSink` trait, `VecSink`, instance factory trait (plan 03-04); fake instance factory and flaky sink (plan 03-06)
- [ ] Log-capture helper (scoped `fmt` subscriber into a shared buffer) (plan 03-04, `testing.rs`)
- [ ] `TypedProducer::clear_errors` (plan 03-08), `init_logging` and `--state-dir` flag (plan 03-05), `.gitignore` entry (plan 03-03)

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Toolchain change (`rustup override unset`) | HOST-01..05 (build prerequisite) | Machine-local setting; needs user consent | Run `rustup override unset` in the repo (plan 03-01), then `rustc --version` at the root prints 1.96.0 after plan 03-03 |
| Live Fluvio produce and restart | HOST-02, HOST-04 | Local cluster not confirmed running | `fluvio cluster start`, build the stub fixtures, run `cargo test -p filimon -- --ignored live_`; confirm no `filimon-test-` topics remain |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 90s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
