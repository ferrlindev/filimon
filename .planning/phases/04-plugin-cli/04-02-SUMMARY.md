---
phase: 04-plugin-cli
plan: 02
subsystem: plugin-host
tags: [tokio, joinset, graceful-shutdown, paused-time, wasmtime-host]

requires:
  - phase: 03-component-host-runtime
    provides: Host, run_plugin, InstanceFactory, ItemSink, CursorStore, FakeFactory test helpers
provides:
  - "Host shutdown bounded by a 10 s grace, then abort of the remaining plugin tasks (WR-10)"
  - "Generic supervise() over InstanceFactory and ItemSink, testable without wasm"
  - "Host::run_with_abort, Host::with_shutdown_grace, Host::loaded_sources, loaded_fetchers, load_failures"
affects: [04-plugin-cli run command (plan 04-06), README shutdown wording (plan 04-06)]

actuals:
  tokens: 12500
  tasks: 2
  commits: 4
plan_head_before: e4c493836756b868ccc610eea77e39d832fd487d
plan_head_after: 6e9b157975109160168fdbdc3d0a26ec4a0a8fed

tech-stack:
  added: []
  patterns:
    - "Supervisor loop: biased select over JoinSet join, shutdown token, abort token and a lazily armed grace sleep"
    - "Paused-time supervisor tests with a never-returning sink"

key-files:
  created: []
  modified:
    - src/plugins/host/mod.rs

key-decisions:
  - "Grace is the constant DEFAULT_SHUTDOWN_GRACE (10 s), set through Host::with_shutdown_grace; HostOptions is unchanged (OQ-8)"
  - "The abort token aborts at any time, including before shutdown: a forced stop needs no grace"
  - "Aborted plugins are reported as disabled: Some(\"aborted at shutdown\") and logged by name; panics keep plugin task panicked"
  - "The join branch is first in the biased select so a task that already finished keeps its real report"

patterns-established:
  - "Distinguish JoinError::is_cancelled (abort) from a panic when filing a task report"

requirements-completed: [CLI-04]

coverage:
  - id: D1
    description: "Host stops within the shutdown grace behind a stuck sink and names every aborted plugin"
    requirement: "CLI-04"
    verification:
      - kind: unit
        ref: "src/plugins/host/mod.rs#grace_returns_after_the_deadline_and_reports_aborted_plugins"
        status: pass
    human_judgment: false
  - id: D2
    description: "A second stop request aborts before the grace ends, also when it arrives before shutdown"
    requirement: "CLI-04"
    verification:
      - kind: unit
        ref: "src/plugins/host/mod.rs#a_second_signal_aborts_before_the_grace_ends"
        status: pass
      - kind: unit
        ref: "src/plugins/host/mod.rs#a_forced_stop_before_shutdown_aborts_without_waiting_for_the_grace"
        status: pass
    human_judgment: false
  - id: D3
    description: "Healthy shutdown is not delayed; panics are told apart from aborts"
    requirement: "CLI-04"
    verification:
      - kind: unit
        ref: "src/plugins/host/mod.rs#healthy_plugins_stop_at_once_with_their_real_reports"
        status: pass
      - kind: unit
        ref: "src/plugins/host/mod.rs#a_panicking_task_is_reported_as_a_panic_not_as_an_abort"
        status: pass
    human_judgment: false
  - id: D4
    description: "Same-instant plugins stay independent; per-plugin batch order and cursor are preserved"
    requirement: "CLI-04"
    verification:
      - kind: unit
        ref: "src/plugins/host/mod.rs#two_plugins_with_the_same_interval_poll_independently"
        status: pass
      - kind: unit
        ref: "src/plugins/host/mod.rs#per_plugin_batches_reach_the_sink_in_poll_order"
        status: pass
    human_judgment: false
  - id: D5
    description: "Host exposes what loaded and what failed before polling starts"
    requirement: "CLI-04"
    verification:
      - kind: unit
        ref: "src/plugins/host/mod.rs#host_accessors_report_what_loaded_and_what_failed"
        status: pass
      - kind: unit
        ref: "src/plugins/host/mod.rs#a_host_with_nothing_loaded_returns_an_empty_report"
        status: pass
    human_judgment: false

duration: 45min
completed: 2026-11-10
status: complete
---

# Phase 4 Plan 02: Bounded Host Shutdown Summary

**Host::run now stops within a 10 s grace after shutdown (second signal aborts at once), reporting each cut-off plugin as `aborted at shutdown`, via a generic `supervise` tested under paused time.**

## Performance

- **Duration:** about 45 min (most of it cargo compile time)
- **Started:** 2026-11-10T05:30:00Z (approx.)
- **Completed:** 2026-11-10T06:17:00Z
- **Tasks:** 2
- **Files modified:** 1 (`src/plugins/host/mod.rs`)

## Accomplishments

- WR-10 closed: shutdown behind a stuck broker is bounded by `DEFAULT_SHUTDOWN_GRACE` instead of N x 60 s; the remaining tasks are aborted, and the cursor of an aborted send is never written (D-07), so the worst case is the documented at-least-once replay.
- `supervise` extracted from `Host::run` and made generic over `InstanceFactory` and `ItemSink`; `Host::run` keeps its signature and delegates through `run_with_abort`.
- `Host` accessors (`loaded_sources`, `loaded_fetchers`, `load_failures`) let the run command detect zero loaded sources before polling.
- 11 new tests in `plugins::host::tests`; unit-test binary went from 255 to 265 passed, 0 failed.

## Task Commits

1. **Task 1 (tracer): generic supervisor with grace and abort** - `854467a` (feat)
2. **Task 2 RED: failing tests** - `209c9b9` (test)
3. **Task 2 GREEN: forced-stop abort and accessors** - `d8dc8fe` (feat)
4. **Task 2 REFACTOR: `Duration::from_mins` in tests** - `6e9b157` (refactor)

## Files Created/Modified

- `src/plugins/host/mod.rs` - `DEFAULT_SHUTDOWN_GRACE`, `ABORTED_AT_SHUTDOWN`, `supervise`, `record_joined`, `Host::run_with_abort`, `Host::with_shutdown_grace`, accessors, tests.

## TDD Notes (Task 2)

- **RED:** `a_forced_stop_before_shutdown_aborts_without_waiting_for_the_grace` failed on its `expect` ("the abort token must stop the supervisor without a shutdown: Elapsed") because task 1's tracer gated the abort branch on an armed deadline; `host_accessors_report_what_loaded_and_what_failed` failed on `assertion left == right: left 0, right 1` against empty accessor stubs. The first RED run hung (an unbounded await); the test was bounded with `tokio::time::timeout` so RED is an assertion failure, not a hang. Semantic assessment: both targets executed and failed on the planned assertion for the intended reason.
- The other five supervisor tests passed at RED because the tracer already implements that behaviour (second signal after shutdown, healthy stop, panic vs abort, same-instant, poll order); they are regression guards, not RED targets. `gsd_run check tdd-red-evidence` was not run (tdd_mode not enabled in config; failure output is plain libtest text, not a supported report format).
- **GREEN:** real accessors; abort branch no longer requires a deadline.
- **REFACTOR:** two `from_secs(60)` literals changed to `from_mins(1)` to clear new clippy lints.

## Decisions Made

- Tracer scope: the abort branch was first gated on the armed deadline (one path: hanging sink, shutdown, grace, abort); task 2 relaxed it so the abort token works at any time, as the plan's task 2 step 2 requires.
- Accessor stubs were committed with the RED tests so the RED commit compiles and fails on an assertion.

## Deviations from Plan

None - plan executed exactly as written. (One extra test beyond the nine named ones, `a_forced_stop_before_shutdown_aborts_without_waiting_for_the_grace`, was added to cover task 2 step 2; it contains neither `wasm_` nor `live_`.)

## Verification

- `cargo test -p filimon plugins::host::tests`: 12 passed, all nine named tests plus both `state_dir_*` tests report ok.
- `cargo test -p filimon plugins::host`: 125 passed, 0 failed, 21 ignored.
- `cargo test -p filimon`: 265 passed (> 255 baseline), 0 failed, 29 ignored.
- `cargo clippy -p filimon --all-targets`: the only `src/plugins/host/mod.rs` warning is the baseline `for_registry is never used` (plan 04-06 resolves it).
- `git diff` of `Cargo.toml`, `Cargo.lock`, `wasm_tests.rs`, `sink.rs`, `loader.rs` against the plan base is empty; no test name contains `wasm_` or `live_`.

## Issues Encountered

- The first RED run left a test process hanging (paused-time supervisor awaiting an abort it ignored); the permission layer refused `pkill`, so that stray test process may still be alive on the machine and can be killed manually (`fili-f9f00fae1fe89728`). The test was then bounded with a virtual-time timeout.

## Known Stubs

None.

## Threat Flags

None - no new network, auth, file or schema surface; T-4-06 and T-4-08 are mitigated and tested, T-4-07 is accepted as planned.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Plan 04-06 can call `Host::run_with_abort` with a second-signal token, print the three accessors before polling, and warn about plugins reported as `aborted at shutdown` while exiting 0 on an operator stop.
- README sentence claiming shutdown returns within seconds is still to be corrected in plan 04-06.

---
*Phase: 04-plugin-cli*
*Completed: 2026-11-10*

## Self-Check: PASSED

- FOUND: src/plugins/host/mod.rs
- FOUND commits: 854467a, 209c9b9, d8dc8fe, 6e9b157
