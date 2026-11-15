---
phase: "3"
slug: "component-host-runtime"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: validated
nyquist_compliant: false
wave_0_complete: true
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
| 3-01-01 | 03-01 | 1 | HOST-01..05 (prerequisite) | T-3-01 | Machine-local rustup override removed by the user only | manual (human-action) | `! rustup override list \| grep -qF "$(pwd)"` | n/a | ✅ green |
| 3-02-01 | 03-02 | 1 | HOST-03, HOST-05 | T-3-02, T-3-03 | Fault fixture builds; never logs secrets | build | `(cd plugins/fault-source && cargo component build --release)` | ✅ | ✅ green |
| 3-02-02 | 03-02 | 1 | HOST-03, HOST-05 | T-3-02, T-3-SC | Fixture fits `source` world, no WASI import, same lock set as stub-source | conformance | `wasm-tools component targets ../../wit -w source <wasm>` | ✅ | ✅ green |
| 3-03-01 | 03-03 | 2 | HOST-01..05 (prerequisite) | T-3-04, T-3-06, T-3-SC | wasmtime 48.x in Cargo.lock; no WASI crate or extra features | build + grep | `cargo check -p filimon` + Cargo.lock gate | ✅ | ✅ green |
| 3-03-02 | 03-03 | 2 | HOST-01..05 (prerequisite) | T-3-05 | SmartModules still build on nested 1.85.0 pins | build | `(cd smartmodules/filter-relevance && smdk build)` | ✅ | ✅ green |
| 3-04-01 | 03-04 | 3 | HOST-01, HOST-02, HOST-04 | T-3-07..T-3-13 | Tracer: registry-listed stub polled, item mapped, sink called, cursor written after sink | unit + `wasm_` | `cargo test -p filimon plugins::host` and `plugins::host::wasm_tests::wasm_tracer -- --ignored` | ✅ | ✅ green |
| 3-04-02 | 03-04 | 3 | HOST-02 | T-3-11 | Restart resumes from persisted cursor | `wasm_` | `cargo test -p filimon plugins::host::wasm_tests -- --ignored` | ✅ | ✅ green |
| 3-05-01 | 03-05 | 3 | HOST-02 | T-3-15 | `--state-dir` / `FILI_STATE_DIR` global flag, no default | unit | `cargo test -p filimon cli::tests` | ✅ | ✅ green |
| 3-05-02 | 03-05 | 3 | HOST-05 | T-3-14 | Subscriber on stderr, default `fili::plugins=info` | unit | `cargo test -p filimon default_filter_passes_plugin_events` | ✅ | ✅ green |
| 3-06-01 | 03-06 | 4 | HOST-03 | T-3-16 | Backoff sequence, cap, retry-after max rule, no overflow | unit | `cargo test -p filimon plugins::host::backoff` | ✅ | ✅ green |
| 3-06-02 | 03-06 | 4 | HOST-03, HOST-05 | T-3-16..T-3-18 | Pure `next_action`: D-01..D-04 table | unit | `cargo test -p filimon plugins::host::scheduler` | ✅ | ✅ green |
| 3-06-03 | 03-06 | 4 | HOST-02, HOST-03, HOST-05 | T-3-17, T-3-19 | Interval, backoff, trap rebuild, isolation, logs under paused time (fakes) | unit (paused time) | `cargo test -p filimon plugins::host::scheduler::tests::sched_` | ✅ | ✅ green |
| 3-07-01 | 03-07 | 4 | HOST-01 | T-3-20, T-3-21, T-3-23 | Unlisted and disabled files never opened; WASI import rejected with named imports | unit | `cargo test -p filimon plugins::host::loader` | ✅ | ✅ green |
| 3-07-02 | 03-07 | 4 | HOST-01 | T-3-22 | Fetchers load against fetcher world; wrong-world rejected | unit + `wasm_` | `cargo test -p filimon plugins::host::loader -- --ignored` | ✅ | ✅ green |
| 3-08-01 | 03-08 | 4 | HOST-04 | T-3-24, T-3-25 | Item validation, cap boundary, content-free warnings, key `{platform}:{id}` | unit | `cargo test -p filimon plugins::host::map` | ✅ | ✅ green |
| 3-08-02 | 03-08 | 4 | HOST-04 | T-3-24, T-3-26, T-3-27 | FluvioSink send-all-then-flush, `clear_errors` on failure | unit + `live_` | `cargo test -p filimon plugins::host::sink`; `-- --ignored live_` | ✅ | ✅ green |
| 3-09-01 | 03-09 | 5 | HOST-02 | T-3-28..T-3-31 | Cursor file shape, mismatch/version checks, lock, atomic write | unit | `cargo test -p filimon plugins::host::cursor` | ✅ | ✅ green |
| 3-09-02 | 03-09 | 5 | HOST-02 | T-3-28 | Commit-after-flush, corrupt file disables only that plugin and is never reset, persist-failure policy | unit (paused time) | `cargo test -p filimon plugins::host::scheduler::tests::sched_` | ✅ | ✅ green |
| 3-10-01 | 03-10 | 6 | HOST-03, HOST-05 | T-3-32 | Real-wasm trap isolation and rebuild, backoff, fatal/config disable, panic isolation, spinner does not block | `wasm_` | `cargo test -p filimon plugins::host::wasm_tests -- --ignored` | ✅ | ✅ green |
| 3-10-02 | 03-10 | 6 | HOST-02, HOST-04 | T-3-34 | Stub to Fluvio scratch topic with restart, no gap, no repeat | `live_` | `cargo test -p filimon -- --ignored live_` | ✅ | ✅ green |
| 3-10-03 | 03-10 | 6 | HOST-01..05 | T-3-33 | README states guarantees and limits as implemented | grep | README heading and phrase gate in plan 03-10 Task 3 | ✅ | ✅ green |
| 3-F-01 | fix b7c2324 | post-6 | HOST-04 | T-3-24 | Record key bounded at 512 B and counted with the value in the 1,000,000-byte cap; fixed-label, content-free drops | unit | `cargo test -p filimon plugins::host::map` | ✅ | ✅ green |
| 3-F-02 | fix 60d65ec | post-6 | HOST-04 | T-3-24 | Sink timeout inside `send_batch` after the lock; `clear_errors` on expiry and failure | unit + `live_` | `cargo test -p filimon plugins::host::sink`; `-- --ignored live_` | ✅ | ✅ green |
| 3-F-03 | fix 2e7c091 | post-6 | HOST-05 | T-3-13 | 64 MiB `StoreLimits` + trap_on_grow_failure; 1000 items/poll; 4096 B cursor cap, oversize never persisted | unit | `cargo test -p filimon plugins::host::engine plugins::host::scheduler plugins::host::map` | ✅ | ✅ green |
| 3-V-01 | validate 7cade0b | post-6 | HOST-05 | T-3-13 | Real component growing past the cap through `WasmtimeFactory` is refused, backed off, rebuilt, not disabled; host and sibling unaffected. Mutation-checked: removing `store.limiter(..)` fails it | `wasm_` | `cargo test -p filimon -- --ignored wasm_` (`wasm_guest_memory_growth_past_the_cap_is_refused_and_isolated`) | ✅ | ✅ green |
| 3-V-02 | validate 7cade0b | post-6 | HOST-04 | T-3-24 | Time waiting for the producer lock does not count against the 60 s sink timeout (real time ~62 s) | `live_` | `cargo test -p filimon -- --ignored live_` (`live_fluvio_sink_lock_wait_does_not_count_against_the_timeout`) | ✅ | ✅ green |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [x] Toolchain checkpoint (human action, plan 03-01) and Cargo.toml / toolchain-pin changes (plan 03-03)
- [x] `plugins/fault-source/` fixture crate + README entry; build both stubs and the fixture (plan 03-02)
- [x] `src/plugins/host/` skeleton with `ItemSink` trait, `VecSink`, instance factory trait (plan 03-04); fake instance factory and flaky sink (plan 03-06)
- [x] Log-capture helper (scoped `fmt` subscriber into a shared buffer) (plan 03-04, `testing.rs`)
- [x] `TypedProducer::clear_errors` (plan 03-08), `init_logging` and `--state-dir` flag (plan 03-05), `.gitignore` entry (plan 03-03)

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Toolchain change (`rustup override unset`) | HOST-01..05 (build prerequisite) | Machine-local setting; needs user consent | Run `rustup override unset` in the repo (plan 03-01), then `rustc --version` at the root prints 1.96.0 after plan 03-03 |
| Live Fluvio produce and restart | HOST-02, HOST-04 | Local cluster not confirmed running | `fluvio cluster start`, build the stub fixtures, run `cargo test -p filimon -- --ignored live_`; confirm no `filimon-test-` topics remain |
| Timeout EXPIRY through `FluvioSink::send_batch` runs `clear_errors` on the real producer | HOST-04 (T-3-24) | `FluvioSink` holds a concrete `TypedProducer`; a test cannot make send/flush hang without stopping the cluster. Needs a seam (e.g. a small `SinkProducer` trait, or a `#[cfg(test)]` constructor taking a fake producer). Expiry itself is unit-tested via `send_bounded`; failure-path clearing is covered live | On a DISPOSABLE local cluster only: run a sink against a scratch topic, stop the SPU, call `send_batch`; expect `send timed out` after ~60 s and a later batch to succeed after the SPU restarts |
| Real-wasm and live groups are `#[ignore]`d with no CI job (review IN-04) | HOST-01..05 | Fixtures must be built and a cluster is needed | Before verify-work run: `cargo test -p filimon`, then `-- --ignored wasm_`, then (cluster up) `-- --ignored live_`; report a group that could not run as NOT RUN |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 90s (plain suite ~3 s warm; wasm ~27 s; live ~62 s because of the real-time lock-wait test)
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** validated (PARTIAL) 2026-11-09 - `nyquist_compliant` stays false until the `send_batch` timeout-expiry seam exists; every requirement HOST-01..05 has automated verification.

## Validation Audit 2026-11-09

| Metric | Count |
|---|---|
| Gaps found | 2 |
| Resolved | 1 |
| Escalated | 1 |
