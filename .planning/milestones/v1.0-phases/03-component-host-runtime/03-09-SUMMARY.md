---
phase: 03-component-host-runtime
plan: 09
subsystem: plugin-host
tags: [rust, cursor, persistence, scheduler, tokio, file-lock]
requires:
  - phase: 03-component-host-runtime
    provides: "CursorStore core and run_plugin loop (03-04, 03-06), fakes in testing.rs"
provides:
  - "Hardened CursorStore: plugin and version checks, single-instance directory lock, directory fsync"
  - "HostError::StateDirLocked"
  - "Scheduler cursor-policy tests (D-07, D-08, A6, empty/null/equal-cursor edges)"
affects: [03-10, phase-04]
tech-stack:
  added: []
  patterns:
    - "std File::try_lock on <state_dir>/.lock held for the store's lifetime"
    - "read path never writes, renames or deletes (bad files left for the operator)"
key-files:
  created: []
  modified:
    - src/plugins/host/cursor.rs
    - src/plugins/host/error.rs
    - src/plugins/host/scheduler.rs
key-decisions:
  - "Version check runs before the plugin-equality check so a newer file is reported as an unsupported version"
  - "Lock file is opened with create+write and truncate(false); lock released by dropping the store"
patterns-established:
  - "InspectingSink in scheduler tests reads the cursor file from inside send_batch to prove commit ordering"
requirements-completed: [HOST-02]
status: complete
commits: 3
plan_head_before: d16e438f13fda8d3e7e9d56341652805c0682901
plan_head_after: 1c90ccb2803496a45a3a552dacee8f9d4c662617
actuals:
  tokens: 9000
  tasks: 2
  commits: 3
duration: 25min
completed: 2026-11-09
---

# Phase 3 Plan 09: Cursor persistence hardening Summary

**Versioned, plugin-checked, atomically written cursor state files with a per-directory single-instance lock, plus nine scheduler tests that pin commit-after-flush and never-reset-a-bad-file policies, all without wasm or a cluster.**

## Accomplishments

- `CursorStore::read` returns `CursorCorrupt` for a `plugin` mismatch or a `version` greater than 1 (reason clipped via `clean_guest_text`), tolerates unknown keys, never writes on a read path.
- `CursorStore::open` takes `File::try_lock` on `<dir>/.lock`; a held lock maps to the new `HostError::StateDirLocked` (code `fili::host::state_dir_locked`); the lock is released on drop.
- `write` additionally fsyncs the directory after the rename (best effort).
- Twelve new cursor tests plus the three from 03-04; nine new `sched_*` tests (commit-after-flush, persistent sink failure, persist failure keeps in-memory cursor, corrupt and unreadable file isolation with unchanged bytes and no instantiation, empty items, null next cursor, equal cursors in order, restart resume).

## Task Commits

1. Task 1 RED: `3a65f2e` test - cursor hardening tests and `StateDirLocked` variant (4 tests failing)
2. Task 1 GREEN: `fb211fb` feat - plugin/version checks, directory lock, directory fsync
3. Task 2: `1c90ccb` test - scheduler cursor-policy tests

## Deviations from Plan

None - plan executed as written. The scheduler written in 03-04/03-06 already satisfied every policy the nine new tests assert, so `scheduler.rs` production code is unchanged (the plan allowed fixing it only if a test failed).

## Verification

- `cargo test -p filimon plugins::host`: 98 passed, 0 failed (three consecutive runs, no flakiness).
- `cargo clippy -p filimon --message-format=short`: no line starting with `src/plugins/host/`. With `--all-targets`, remaining host warnings are pre-existing and outside this plan's files (mod.rs, engine.rs, sink.rs, testing.rs).

## Known Stubs

None.

## Threat Flags

None. T-3-28 to T-3-31 are mitigated as planned (corrupt-file handling, directory lock, atomic write plus dir fsync, fixed three-key file shape).

## Self-Check: PASSED

- src/plugins/host/cursor.rs, error.rs, scheduler.rs modified and committed
- Commits 3a65f2e, fb211fb, 1c90ccb present on the branch
