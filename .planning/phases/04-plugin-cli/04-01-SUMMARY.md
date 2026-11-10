---
phase: 04-plugin-cli
plan: 01
subsystem: plugin-cli
tags: [rust, clap, tokio, wasmtime-host, cursor-state, status-sidecar, miette]

requires:
  - phase: 03-component-host-runtime
    provides: scheduler (run_plugin, settle, describe), CursorStore, HostOptions::resolve_state_dir, test fakes
provides:
  - "<alias>.status.json sidecar written by the scheduler after every cycle outcome (PollStatus, write_status)"
  - "lock-free, read-only read_status / StatusRead"
  - "list_registry: registry-only listing (RegistryListing, ListedPlugin, SkippedName)"
  - "fili plugins list (collect_list, render_list, list, dispatch) with no banner"
  - "require_state_dir and a non_blank_path clap parser (WR-07)"
  - "first integration test file tests/plugins_cli.rs driving the real binary"
affects: [04-02, 04-03, 04-04, 04-05, 04-06, plugin-cli]

plan_head_before: e4c493836756b868ccc610eea77e39d832fd487d
plan_head_after: 31ab5122365d609515031eb2f2fed01b9279d23f

actuals:
  tokens: 14000
  tasks: 2
  commits: 3

tech-stack:
  added: []
  patterns:
    - "status sidecar file separate from the cursor file so a status write can never alter a cursor"
    - "lock-free read-only state reads for CLI commands (never create the directory, never take the lock)"
    - "binary-level tests through CARGO_BIN_EXE_fili"

key-files:
  created:
    - src/commands/plugins.rs
    - tests/plugins_cli.rs
  modified:
    - src/plugins/host/cursor.rs
    - src/plugins/host/error.rs
    - src/plugins/host/scheduler.rs
    - src/plugins/config.rs
    - src/cli.rs
    - src/main.rs
    - src/commands/mod.rs

key-decisions:
  - "Status is its own file <alias>.status.json with {version,plugin,at,ok,error,items}; the D-05 cursor format is untouched (OQ-6)"
  - "A disabling fault records an ok:false status carrying the cleaned disable reason; no separate disabled field (OQ-7)"
  - "A status write failure is one tracing::warn naming the plugin: never a plugin failure, never a disable, never a cursor change"
  - "Padded table columns: the header is not byte-for-byte single-spaced; tests compare it by whitespace-split tokens"
  - "SkippedName.table is &'static str (sources or fetchers)"

patterns-established:
  - "record_status(cursors, plugin, outcome, items) plus a text-level sibling for paths with no CycleOutcome"
  - "state_dir_for in commands/plugins.rs: resolve then require_state_dir, reused by later commands"

requirements-completed: [CLI-01]

duration: 52min
completed: 2026-11-10
status: complete
---

# Phase 4 Plan 01: Status sidecar and `fili plugins list` tracer Summary

**`fili plugins list` reads a lock-free `<alias>.status.json` sidecar that the scheduler writes after every cycle outcome, runs as the real binary with no banner, and the empty `--state-dir` carry-over (WR-07) is refused by clap and by the store.**

## Performance

- **Duration:** about 52 min (the first full `cargo test` build dominated)
- **Started:** 2026-11-10T05:12:00Z (approx.)
- **Completed:** 2026-11-10T06:04:20Z
- **Tasks:** 2 (1 tracer, 1 TDD)
- **Files modified:** 9 (2 created)

## Accomplishments

- Tracer (CLI-01, SC1): a plugin polled by the scheduler shows `<RFC 3339 UTC seconds> ok (1 item)` in LAST POLL through `collect_list` + `render_list`, and the compiled `fili --registry ... --state-dir ... plugins list` prints the header and one row per plugin, no `╔` banner, and does not create the state directory.
- Scheduler records `ok`, `error` (cleaned `describe` text) and `items` for a good poll, guest error, trap, sink failure, oversize cursor, build failure, init failure, disable and corrupt-cursor disable. A status write never changes a cursor file (byte-identical checks) and a failing write is only a warning (`failures` 0, `disabled` None).
- `read_status` works while another `CursorStore` holds the lock, creates nothing, and turns missing / zero-byte / garbage / over-64-KiB / foreign-plugin / future-version files into `Missing` or `Unreadable(reason)` with fixed-text reasons.
- WR-07: `--state-dir ""` and `"   "` are clap usage errors; `CursorStore::open` and `list` reject an empty or blank directory as `HostError::StateDir` ("state directory must not be empty"). The unused-code allow above `Cli.state_dir` is gone.
- Registry read-and-parse step of `load_registry_with_env` moved into a private shared helper with no behaviour change; the `registry_missing`, `registry_malformed` and `registry_unknown_key` tests still pass.

## Task Commits

1. **Task 1: tracer, a polled plugin's last poll shows up in `fili plugins list`** - `829cffa` (feat)
2. **Task 2 RED: failing tests for status records and the WR-07 guard** - `ff648a6` (test)
3. **Task 2 GREEN: record every cycle outcome, refuse an empty state dir** - `31ab512` (feat)

No REFACTOR commit: nothing worth changing after GREEN.

**Plan metadata:** committed with this SUMMARY (docs: complete plan).

## TDD Gate Compliance

- **RED:** `ff648a6`. 8 tests failed on their planned assertions: `state_dir_empty_value_is_rejected` and `state_dir_blank_value_is_rejected` (clap accepted the value, `expect_err` panicked), `list_rejects_an_empty_state_dir` and `open_rejects_*` (no error for an empty dir), and `sched_status_records_a_build_failure`, `..._an_init_failure`, `..._a_corrupt_cursor_disable_and_keeps_the_bad_file` (no status file written at those sites). `require_state_dir` was a no-op stub in this commit so the tests compile. The 20+ other new tests in the same commit exercise Task 1 code and passed already; they are regression coverage, not RED evidence. Semantic assessment: each failure is the assertion for the intended missing behaviour, not a compile, fixture or setup fault.
- **RED evidence classifier:** `gsd_run check tdd-red-evidence` does not support libtest console output (supported formats are TAP, JUnit, swift-testing, unittest, go test), so it was not run. The assessment above is the manual inspection of the real `cargo test` output.
- **GREEN:** `31ab512`, all 285 unit tests and the integration test pass.
- Task 1 was `type="tracer"` (not tdd), its tests ship in the same commit as the code.

## Files Created/Modified

- `src/plugins/host/cursor.rs` - `PollStatus`, `StatusRead`, `status_path`, `CursorStore::write_status`, lock-free `read_status`, `require_state_dir`, `open` guard, new tests
- `src/plugins/host/error.rs` - `HostError::StatusWrite` (`fili::host::status_write`)
- `src/plugins/host/scheduler.rs` - `record_status`, `record_status_text`, calls at the poll loop, build, init and corrupt-cursor sites, `sched_status_*` tests
- `src/plugins/config.rs` - private `read_registry`, `RegistryListing`, `ListedPlugin`, `SkippedName`, `list_registry`
- `src/cli.rs` - `Command::Plugins`, `PluginsCommand::List`, `non_blank_path`, new CLI tests
- `src/main.rs` - `shows_banner`, `Command::Plugins` dispatch arm, banner unit test
- `src/commands/mod.rs`, `src/commands/plugins.rs` - the `list` command (`collect_list`, `render_list`, `list`, `dispatch`, `state_dir_for`)
- `tests/plugins_cli.rs` - binary-level test `list_prints_the_registered_plugins_and_no_banner`

## Decisions Made

- The OQ-2, OQ-6, OQ-7, OQ-9 and OQ-11 working decisions from the plan were followed as written.
- `RegistryListing.path` is read by `list` for the empty-registry message, so the field is not dead in the test build; `skipped` is consumed by plan 04-03 (it is in the dead-code-allowed `plugins` module for the normal build).

## Deviations from Plan

None - plan executed exactly as written. Two small interpretations, no behavioural deviation:

- The plan said the header is `NAME  KIND  ENABLED  FETCHER  LAST POLL` and also that columns are padded to the widest cell; with both, the header is padded too. Tests compare the header's whitespace-separated tokens.
- `require_state_dir` appears in Task 1's artifact list but Task 2's actions; it was added in Task 2 (as a stub in RED, real in GREEN), so Task 1's `list` first used plain `resolve_state_dir`, then switched to `state_dir_for`.

**Total deviations:** 0 auto-fixed.

## Issues Encountered

- The RED run of `open_rejects_an_empty_state_dir` / `open_rejects_a_blank_state_dir` ran against the unguarded `open`, which created a stray `.lock` in the package root and a directory named three spaces with a `.lock` inside. The root `.lock` was removed. The directory `   /` (three spaces) could not be removed because the safety check blocks `rmdir` on a path that is only whitespace; it is untracked, not committed, and the tests no longer create it. It should be deleted when this worktree is discarded.
- Baseline unit-test count was 255 passed; it is now 285 (plus 1 integration test).

## Known Stubs

None.

## Threat Flags

None. The new surface (a status file read by `list`, `--state-dir` handling) is exactly what the plan's threat model T-4-01 through T-4-05 already covers and mitigates; no new endpoints or trust boundaries.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Plans 04-02 to 04-06 can reuse `state_dir_for`, `collect_list`/`render_list`, `read_status` and the integration-test helpers in `tests/plugins_cli.rs`.
- Plan 04-03 consumes `RegistryListing.skipped` (stderr lines) and asserts the adjacency and ordering edges.

---
*Phase: 04-plugin-cli*
*Completed: 2026-11-10*

## Self-Check: PASSED

- Files exist: `src/commands/plugins.rs`, `tests/plugins_cli.rs`, and the seven modified files (checked below).
- Commits exist: `829cffa`, `ff648a6`, `31ab512` (ancestors of HEAD).
- `cargo test -p filimon`: 285 passed, 0 failed (baseline 255); `cargo test -p filimon --test plugins_cli`: 1 passed.
- `cargo clippy -p filimon --all-targets`: no warning line starts with a new or touched file in the plan's list; `Cargo.toml` and `Cargo.lock` unchanged.
