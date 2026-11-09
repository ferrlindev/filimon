---
phase: 03-component-host-runtime
plan: 06
subsystem: plugin-host
tags: [backoff, scheduler, state-machine, tokio-paused-time, fake-instance-factory]
requires:
  - phase: 03-component-host-runtime
    provides: "03-04: scheduler single-path loop, InstanceFactory / PluginInstance traits, CursorStore, VecSink, LogCapture"
provides:
  - "backoff::step and backoff::rate_limited_wait (pure, capped, overflow-safe)"
  - "scheduler::{Phase, CycleOutcome, Action, PluginState, next_action, describe}: D-01..D-04 as one pure state machine"
  - "run_plugin as an instance-lifecycle loop: backoff, discard-rebuild-reinit after wasmtime errors, disable on fatal/config"
  - "testing::{FakeFactory, FakeInstance, FakeStep, FakeEvent, FlakySink, spec, wit_item, run_for}: scripted scheduler tests with no wasm and no cluster"
affects: [03-09, 03-10]
tech-stack:
  added: []
  patterns:
    - "pure outcome-to-action function (no clock, no IO, no logging) with the async loop as a thin driver"
    - "scripted fake InstanceFactory sharing one script and one event log across rebuilt instances; assertions on whole-second poll offsets under start_paused"
key-files:
  created:
    - src/plugins/host/backoff.rs
  modified:
    - src/plugins/host/scheduler.rs
    - src/plugins/host/testing.rs
    - src/plugins/host/mod.rs
key-decisions:
  - "RESEARCH A10 taken literally: a backoff window can be shorter than poll_interval_secs; documented on next_action and backoff.rs, not adjusted"
  - "RESEARCH A9: host-side sink failures reuse the plugin's backoff, keep the instance, repoll the same cursor, and the log says the item sink failed and the plugin is not at fault"
  - "A disable counts as a failure in PluginReport.failures; polls counts only polls the guest answered successfully"
  - "PluginReport.dropped is added only after a batch is sent, so a repolled batch is not double counted"
requirements-completed: [HOST-03]
requirements-advanced: [HOST-02, HOST-05]
status: complete
commits: 3
plan_head_before: 0eff1cb38e8613d7ba6488e657314c6e84cb8afb
plan_head_after: ac3479e91c5f1d48a7c0f4c15f31140b189faf03
actuals:
  tokens: 21000
  tasks: 3
  commits: 3
duration: 30 min
completed: 2026-11-09
---

# Phase 3 Plan 06: Backoff, disable and rebuild state machine Summary

D-01..D-04 live in one pure `next_action` over `CycleOutcome`: transient, rate-limited and auth-required back off exponentially (with `retry_after`), fatal and config disable only that plugin, and a trap, build failure or failed `init` discards the instance so the loop builds and initialises a fresh one; all of it is proven under paused time with a scripted fake factory.

## Accomplishments

- `backoff.rs`: `step` is `initial * multiplier^n` clamped as a float against `max` before `Duration::from_secs_f64` (so `u32::MAX` failures give `max`, never a panic); `rate_limited_wait` is `max(retry_after, step)` capped at `max`. Twelve table tests.
- `scheduler.rs`: `next_action` and `describe` (guest text through `clean_guest_text(.., 512)`), failure counter with `saturating_add(1)`, and `run_plugin` rewritten as an outer build/init loop around an inner poll loop. Cursor read error still ends only that plugin (D-08); the success path (map, `send_batch` under the 60 s timeout run to completion, then cursor write, A6) is unchanged.
- A sink failure leaves the cursor unwritten and unchanged and repolls the same cursor after the plugin's backoff step.
- `testing.rs`: scripted fakes shared by all `sched_*` tests; a rebuilt instance continues the same script.

## Task Commits

1. Task 1 - pure backoff arithmetic: `4c73b02`
2. Task 2 - state machine and loop: `9063acd`
3. Task 3 - fakes and paused-time scheduler tests: `ac3479e`

## Verification

- `cargo test -p filimon plugins::host`: 51 passed, 2 ignored (the wasm tests), 0 failed. This includes the 12 backoff tests, the 16 state-machine tests and 13 `sched_*` tests, all with their exact planned names.
- `cargo test -p filimon plugins::host::wasm_tests -- --ignored`: both plan 03-04 tests pass (`wasm_tracer_stub_polls_and_persists_cursor`, `wasm_restart_resumes_from_persisted_cursor`) against a `stub_source.wasm` built in this worktree.
- `cargo clippy -p filimon --all-targets`: no warning in `backoff.rs` or `scheduler.rs`. Remaining `src/plugins/host/` warnings are pre-existing and out of scope: dead-code on items later plans consume (`for_registry`, `loaded_fetchers`, `HostState::plugin`, loader `Fetcher`/fields, `VecSink::plugin_records`), a redundant closure in `engine.rs`, and `format!` appended to a `String` in `write_registry` in `testing.rs` (03-04 code).

## Deviations from Plan

None - plan executed exactly as written. Every test passed on its first run, so the loop needed no fix from Task 3.

Two interpretation notes (not deviations): the plan's `SinkFailed` text is now the cleaned sink error only (the `item sink failed:` label comes from `describe`), and `send_records` returns the cleaned text instead of the old `sink failed (host side): ...` string.

## Issues Encountered

- The worktree-isolated shell refused compound `git` commands, so commits were issued as separate plain commands. The plan commit ledger file (`gsd-plan-head-before-03-06`) was therefore not written; `plan_head_before` is the known worktree base `0eff1cb`, and `commits: 3` was measured with `git rev-list --count 0eff1cb..HEAD`.
- `Cargo.lock` was not touched (no new dependency).

## Known Stubs

None.

## Threat Flags

None. No new network endpoint, auth path or file access; the threat register mitigations T-3-16..T-3-19 are implemented and covered by `failure_waits_are_never_zero`, `step_never_overflows_for_huge_failure_counts`, `failure_counter_saturates`, `sched_trap_rebuilds_instance_and_reinits_after_backoff`, `sched_init_transient_rebuilds_after_backoff`, `describe_cleans_control_characters`, `fatal_disables_with_cleaned_reason` and `sched_logs_name_the_plugin_on_backoff_trap_and_disable`.

## Flagged Items For Review

- HOST-05 stays flagged: it is covered at the scheduler level with fakes only. Real-wasm trap and rate-limit proofs and the spinning-guest gap (no per-poll deadline; v2 HOST-06) belong to plan 03-10.
- Merge note: `scheduler.rs` still calls `map_items(&spec.name, response.items)` and reads `mapped.records` / `mapped.dropped`; plan 03-08 (running in parallel, `map.rs`) must keep that shape or the merge needs a one-line adjustment. `testing.rs` uses `VecSink::default()` and `VecSink::records()`.

## Self-Check: PASSED

- FOUND: src/plugins/host/backoff.rs, scheduler.rs, testing.rs, mod.rs (`pub mod backoff;`)
- FOUND commits: 4c73b02, 9063acd, ac3479e (ancestors of HEAD)
