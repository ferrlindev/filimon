---
phase: 03-component-host-runtime
plan: 10
subsystem: plugin-host
tags: [wasmtime, fault-injection, fluvio, live-test, readme, isolation, backoff]
requires:
  - phase: 03-component-host-runtime
    provides: "03-02 fault-source fixture, 03-04 tracer + wasm tests, 03-06 scheduler/backoff, 03-07 loader, 03-08 FluvioSink + live_support, 03-09 cursor hardening"
provides:
  - "Eight real-wasm tests (fault-source) proving trap isolation and rebuild, rate-limit/transient/auth-required backoff, fatal/init-fatal/config disabling, host-task panic isolation and the spinning-guest anti-wedge guarantee"
  - "Live SC2 test: stub component to a Fluvio scratch topic with a stop and a restart on the same state directory"
  - "plugins/README.md `## Host behavior` (loading, polling and backoff, failures, cursor state, items, interim imports, known limits, test commands)"
affects: [phase-04]
tech-stack:
  added: []
  patterns:
    - "PanickingSink test double to inject a host-task panic for one plugin name"
    - "CountingSink wrapper around FluvioSink; scratch topic deleted via catch_unwind pattern"
    - "Spinner test on a real clock (paused time cannot advance while a guest spins)"
key-files:
  created: []
  modified:
    - src/plugins/host/wasm_tests.rs
    - src/plugins/host/sink.rs
    - plugins/README.md
key-decisions:
  - "No production code change: every new test passed on its first run, so the host already satisfied the isolation, backoff and anti-wedge behaviour"
  - "Spinning-guest gap stays accepted (RESEARCH Open Question 3, v2 HOST-06) and is stated in the README"
requirements-completed: [HOST-02, HOST-03, HOST-04]
requirements-advanced: [HOST-05]
status: complete
commits: 4
plan_head_before: 38b041191293dcf3d4178dd2feb6d4ec60535d36
plan_head_after: 780fb75
actuals:
  tokens: 14000
  tasks: 3
  commits: 4
duration: 45 min
completed: 2026-11-09
---

# Phase 3 Plan 10: Real-wasm isolation tests, live SC2 test and host documentation Summary

The scheduler rules proved with fakes in 03-06/03-09 now hold on real wasmtime with the fault-injection component, SC2 holds on a real Fluvio topic across a restart, and the README states exactly what the host guarantees and what it does not.

## Accomplishments

- `wasm_tests.rs`: eight new ignored tests with their planned names, plus helpers `fault_source`, `stub_named`, `load_host`, `run_host`, `ids_of` and `PanickingSink`. Trap test: trapper yields `f1` at 0, 70 and 140 s (counter restarts on a rebuilt instance), stub keeps `stub-0..2`, log has `plugin=trapper` and `trap`, no ESC byte. Rate-limited: exactly 3 failures at 60 s (waits 25, 25, 40). Transient and auth-required: 4 failures at 100 s each. Fatal, init-fatal and bad settings disable only their plugin. A panic in `send_batch` for `bad` is reported as `plugin task panicked` while `good` polls 3 times. The spinner test runs on a real clock: the stub polls at least twice beside the spinner and `Host::run` returns within the 10 s timeout after cancel.
- `sink.rs`: `live_host_end_to_end_stub_to_fluvio_with_restart` (real clock, `CountingSink`, `run_host_until`). Two host runs on one registry and state directory write to a scratch `filimon-test-host-...` topic; read-back asserts platform `stub`, social type, key `stub:<id>` and ids exactly `stub-0..stub-(n1+n2-1)` with no gap or repeat. The topic is deleted even on panic.
- `plugins/README.md`: `## Host behavior` with the eight planned subsections, placed before `## Contract version`; 130 added lines, nothing deleted.

## Task Commits

1. Task 1 - real-wasm fault tests: `a6c3786`
2. Task 2 - live end-to-end test: `9b6eb99`
3. Task 3 - README host behavior: `3cbc5e0`
4. Lint follow-up (`Duration::from_mins` in a test): `780fb75`

## Test Group Results (reported separately)

| Group | Command | Result |
| ----- | ------- | ------ |
| Plain | `cargo test -p filimon` | RAN, PASSED: 238 passed, 0 failed, 26 ignored |
| Wasm | `cargo test -p filimon -- --ignored wasm_` | RAN, PASSED: 15 passed (the 8 new tests, the 2 plan 03-04 tests and the 5 loader wasm tests from 03-07), 0 failed; no `live_` test selected |
| Live | `cargo test -p filimon -- --ignored live_` | RAN, PASSED: 3 passed (`live_host_end_to_end_stub_to_fluvio_with_restart`, `live_fluvio_sink_produces_records_in_order_with_keys`, `live_fluvio_sink_connect_names_a_missing_topic`) against the local Fluvio cluster |
| Clippy | `cargo clippy -p filimon --message-format=short` | no line starting with `src/plugins/host/` |

After the live run `fluvio topic list` showed only `filimon-articles`, `filimon-raw-pages` and `filimon-social-items`; no `filimon-test-` topic was left. `"filimon-social-items"` does not appear as a string literal in `sink.rs`.

Fixtures (`stub-source`, `stub-fetcher`, `fault-source`) were built in this worktree with `cargo component build --release`; outputs are gitignored and nothing machine-local was touched.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Lint] `Duration::from_secs(60)` in the rate-limit test**
- clippy (all-targets) asked for `Duration::from_mins(1)`; changed, test re-run and passing. Commit `780fb75`.

### Notes

- Task 1 is `tdd="true"` but all eight tests passed on the first run with no production change (the host from 03-04/03-06/03-09 already behaves as specified), so there is no RED commit; the tests are regression proofs, not drivers of new code.
- The worktree shell refused heredocs and compound git commands, so files were written with Edit/Write and git commands were issued one at a time. The commit ledger file was not written; `plan_head_before` is the dispatch base `38b0411` and `commits: 4` was measured with `git rev-list --count 38b0411..HEAD` before this SUMMARY commit.
- Remaining `--all-targets` clippy warnings in `src/plugins/host/` (for example `connect_social` unused) are pre-existing and out of scope.

## Known Stubs

None. The `not-configured` fetch and absent access token are deliberate interim imports documented in the README and resolved by the fetcher and OAuth phases.

## Flagged Items For Review

- HOST-05 stays flagged (probe category `unclassified`): real-wasm evidence now exists for trap, rate-limit, transient and fatal isolation, but a spinning guest is never terminated while the host runs (accepted gap, v2 HOST-06). It is advanced here, not marked complete.
- The Fluvio key `{platform}:{id}` still assumes a single-partition topic (RESEARCH A4, unconfirmed with the user); the live test uses a one-partition scratch topic.
- The sticky-producer-error recovery path (`clear_errors` after a real produce failure) is still not exercised by any test.

## Threat Flags

None. T-3-32 (accepted, stated in the README Known limits), T-3-33 (README written from the code, states at-least-once, no dedupe, no CPU/memory limits, spinning-guest gap) and T-3-34 (live test uses only its own scratch topic) are addressed as planned.

## Self-Check: PASSED

- FOUND: `src/plugins/host/wasm_tests.rs` (eight new test names and `PanickingSink`), `src/plugins/host/sink.rs` (`live_host_end_to_end_stub_to_fluvio_with_restart`), `plugins/README.md` (`## Host behavior`)
- FOUND commits: a6c3786, 9b6eb99, 3cbc5e0, 780fb75 (ancestors of HEAD)
