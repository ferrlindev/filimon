---
phase: 04-plugin-cli
plan: 05
subsystem: plugin-cli
tags: [rust, clap, wasmtime, miette, run-once, cursor, tdd, json-lines, redaction]

requires:
  - phase: 04-plugin-cli
    provides: "plan 04-02: record_status / status files; plan 04-04: load_plugin scoped loader, pub(crate) load_source, find-by-name check path"
  - phase: 03-plugin-host
    provides: "InstanceFactory, PluginInstance, WasmtimeFactory, map_items, ItemSink, CursorStore, describe, CycleOutcome"
provides:
  - "fili plugins run-once <name>: one poll of one source printed as a table, or as JSON lines with --json"
  - "--produce: send the batch, then save the cursor (D-07), under the state-directory lock, leaving a status file for list"
  - "once.rs: poll_once, PolledOnce, commit_once, CommitFailure"
  - "scheduler.rs: expose_secrets (the single expose() site), oversize_cursor_outcome (shared cursor-size rule), pub(crate) send_records"
  - "cursor.rs: read_cursor_file, the lock-free cursor parse that CursorStore::read delegates to"
  - "commands/plugins.rs: Produce, execute_run_once, run_once, render_items, find_plugin, async dispatch"
affects: [04-06, plugin-cli]

plan_head_before: fde4953b7c2704676aba5744583f65f4bf16876c
plan_head_after: d164eeb288f3c65d5e2d5a966d1f9c8091639baa

actuals:
  tokens: 19200
  tasks: 3
  commits: 5

tech-stack:
  added: []
  patterns:
    - "one cycle without a loop built on the scheduler's seams; shared rules are shared functions, not copies"
    - "poll is raced against Ctrl-C, the commit (send then cursor write) is never raced"
    - "notes go to stderr in --json mode so stdout is only SourceItem lines"
    - "the starting cursor is read before the component loads, so a corrupt cursor file is reported as itself"

key-files:
  created:
    - src/plugins/host/once.rs
  modified:
    - src/plugins/host/scheduler.rs
    - src/plugins/host/cursor.rs
    - src/plugins/host/mod.rs
    - src/cli.rs
    - src/commands/plugins.rs
    - src/main.rs
    - tests/plugins_cli.rs

key-decisions:
  - "Plan working decisions OQ-2/3/4/5/9 followed as written (200-character clip, disabled source refused, both modes start from the saved cursor, exit 1 on any failure)"
  - "run_once splits the cycle into poll_interruptible (raced against Ctrl-C) and finish_run_once (never raced); execute_run_once is the test seam that chains them and carries a not(test) dead_code allow"
  - "The starting cursor is read right after the scoped config load and before the component loads (lock first for --produce); the plan listed it after the component load, but that order reported a bad component instead of a corrupt cursor"
  - "A cursor write failure after a successful send records a failed status carrying the sent count; a failing sink records SinkFailed; a failed poll cycle records its outcome; a dry run writes no status"
  - "The oversize-cursor rule has one implementation (oversize_cursor_error); the scheduler loop converts it into a guest poll error and run-once uses oversize_cursor_outcome, so both give the same message"

patterns-established:
  - "Binary tests for commands that can write to production never pass the produce option"
  - "Refusal tests use a registry whose component files do not exist, so reaching a component load would change the message"

requirements-completed: [CLI-03]

coverage:
  - id: D1
    description: "fili plugins run-once <name> performs one poll and prints the items; without --produce nothing is sent, no lock is taken and the state directory is not created"
    requirement: CLI-03
    verification:
      - kind: integration
        ref: "tests/plugins_cli.rs#wasm_run_once_stub_dry_run_prints_stub_0_and_creates_no_state (--ignored)"
        status: pass
      - kind: unit
        ref: "src/commands/plugins.rs#run_once_dry_run_prints_items_and_leaves_the_state_dir_absent, run_once_dry_run_reads_the_saved_cursor_without_the_lock"
        status: pass
      - kind: unit
        ref: "src/plugins/host/once.rs#once_polls_one_cycle_and_maps_items, once_passes_the_saved_cursor_and_returns_the_next_one"
        status: pass
    human_judgment: false
  - id: D2
    description: "--produce sends then saves the cursor (D-07), a failing sink leaves the cursor byte-identical, zero items skip the sink and still save the cursor, a cursor write failure reports the sent count, a second producer is refused by the state-dir lock"
    requirement: CLI-03
    verification:
      - kind: unit
        ref: "src/plugins/host/once.rs#commit_sends_then_writes_the_cursor_in_that_order, commit_sink_failure_leaves_the_cursor_file_byte_identical, commit_with_zero_items_skips_the_sink_and_still_writes_the_cursor, commit_cursor_write_failure_reports_how_many_records_were_sent"
        status: pass
      - kind: unit
        ref: "src/commands/plugins.rs#run_once_produce_sends_then_saves_the_cursor_and_records_status, run_once_produce_with_a_failing_sink_exits_nonzero_and_keeps_the_cursor, run_once_produce_reports_a_cursor_that_could_not_be_saved, run_once_produce_is_refused_while_another_process_holds_the_state_dir"
        status: pass
    human_judgment: false
  - id: D3
    description: "The scheduler's own cursor-size rule and single secret-exposure site are shared; an oversize next cursor is a fatal poll error and nothing is sent or saved"
    requirement: CLI-03
    verification:
      - kind: unit
        ref: "src/plugins/host/once.rs#once_rejects_an_oversize_cursor_like_the_scheduler"
        status: pass
      - kind: unit
        ref: "src/plugins/host/scheduler.rs#sched_oversize_next_cursor_is_a_plugin_fault_and_never_persisted (unchanged, still passes)"
        status: pass
    human_judgment: false
  - id: D4
    description: "Output rules: --json is one SourceItem per line in guest order with notes on stderr; human output is sanitized and clipped by characters; duplicate keys are all kept and the dropped count is shown; zero items is a clean success"
    requirement: CLI-03
    verification:
      - kind: unit
        ref: "src/commands/plugins.rs#run_once_json_prints_one_source_item_per_line_in_guest_order, run_once_zero_items_json_writes_nothing_to_stdout, run_once_json_with_produce_keeps_stdout_machine_readable, run_once_human_output_is_sanitized_and_clipped_by_characters, run_once_keeps_duplicate_keys_in_guest_order_and_reports_dropped, run_once_zero_items_is_a_clean_success"
        status: pass
      - kind: integration
        ref: "src/commands/plugins.rs#wasm_run_once_stub_json_prints_a_social_source_item (--ignored)"
        status: pass
    human_judgment: false
  - id: D5
    description: "Refusals and failure reporting: unknown name, fetcher and disabled source are refused before any component load or state-dir access; a failed cycle prints the cleaned describe text and fails; a corrupt cursor names the file and is left untouched; secret values never appear in output, status or errors"
    requirement: CLI-03
    verification:
      - kind: unit
        ref: "src/commands/plugins.rs#run_once_refuses_a_disabled_source, run_once_refuses_a_fetcher, run_once_unknown_name_lists_registered_plugins, run_once_failed_cycle_prints_the_describe_text_and_fails, run_once_dry_run_with_a_corrupt_cursor_fails_naming_the_file_and_leaves_it, run_once_never_prints_secret_values"
        status: pass
    human_judgment: false
  - id: D6
    description: "Ctrl-C interruption of a spinning guest during the poll"
    requirement: CLI-03
    verification: []
    human_judgment: true
    rationale: "tokio::select! against ctrl_c is wired and mirrors the scheduler's cancellation, but no automated test sends a signal to the binary; the interrupt path was not exercised"

duration: ~35min
completed: 2026-11-10
status: complete
---

# Phase 4 Plan 05: `fili plugins run-once` Summary

**`fili plugins run-once <name>` polls one source once and prints its items (table, or compact `SourceItem` JSON lines with `--json`); without `--produce` it never sends, locks or writes anything, and with `--produce` it sends the batch and only then saves the cursor under the state-directory lock, reusing the scheduler's own secret-exposure site, cursor-size rule and `send_records`.**

## Performance

- **Duration:** about 35 min (the start time was not captured at spawn; this is an estimate, a cold dependency build of the worktree ran in the background)
- **Completed:** 2026-11-10T07:04Z
- **Tasks:** 3 (1 tracer, 2 TDD)
- **Files modified:** 8 (1 created)

## Accomplishments

- Tracer (CLI-03, SC3): `fili --registry examples/plugins/filimon.toml --state-dir /tmp/fili-ro-demo plugins run-once stub` against the real stub component prints `[1/1] stub:stub-0`, its url, title, author, published and text, `polled 1 item (0 dropped)` and the dry-run notice, exits 0, prints no banner, makes no Fluvio connection and leaves `/tmp/fili-ro-demo` absent. Tracer gate: the `<verify>` was automated-only and was run end to end after task 1 (all three commands passed), so expansion continued.
- Shared seams instead of copies (finding F5): `expose_secrets` is the only `expose()` outside tests (`scheduler.rs` count 1, `once.rs` and `commands/plugins.rs` count 0); `oversize_cursor_outcome` and `send_records` serve both the loop and run-once; `read_cursor_file` is the single cursor parse. Every pre-existing `sched_*` and cursor test still passes unchanged.
- `--produce`: lock first (a held lock fails fast with `StateDirLocked` before any poll or connection), cursor read, sink connect, poll raced against Ctrl-C, then print, send, save the cursor; the commit is not raced. Status is written only in produce mode: `ok` with the item count, `SinkFailed`, the poll failure, or a failed status carrying the sent count when the cursor cannot be saved.
- Output: items in guest order; human fields pass `clean_guest_text(.., 200)` (character-based); `--json` is serde-escaped with every note on stderr; the dropped count is always shown; zero items is a clean success.
- Tests: unit-test binary 330 -> 363 passed (32 ignored); `tests/plugins_cli.rs` 12 passed (3 ignored). The two ignored real-wasm run-once tests pass with `--ignored wasm_run_once`. No test passes `--produce` through the binary and none connects to Fluvio.

## Task Commits

1. **Task 1: end-to-end dry run (tracer)** - `e6af539` (feat)
2. **Task 2: `--produce` path** - `4d57912` (test, RED) then `9a78646` (feat, GREEN)
3. **Task 3: output rules, refusals, guarantees** - `aeb0bb9` (test, RED) then `d164eeb` (feat, GREEN)

## TDD Gate Compliance

Task 2: RED `4d57912` added the tests plus a no-op `commit_once` stub so they compile; 4 `commit_*` tests and 4 command-level produce tests failed on their planned assertions (sent count 0 instead of 2, cursor file absent, `expect_err` on an `Ok`), not on compile or load errors. GREEN `9a78646` makes them pass. No REFACTOR commit.

Task 3: RED `aeb0bb9` added the tests; 4 failed on their targets (the three `--json` tests, because `--json` returned a "not available yet" error, and the corrupt-cursor test, because the component error was reported instead of the cursor error). GREEN `d164eeb` implements JSON lines and reorders the cursor read. The other tests of this task (sanitizing, duplicates, lock-free read, refusals, secret redaction) already passed against the task 1 and 2 code on their first run, so they have no RED; they are regression tests for behaviour built earlier.

`gsd_run check tdd-red-evidence` was not applied: it has adapters for TAP, JUnit, swift-testing, `unittest` and `go test`, not for the libtest text report `cargo test` produces. The RED evidence is the real `cargo test` output (named tests FAILED on assertion lines) inspected by hand, and the failing counts above.

## Files Created/Modified

- `src/plugins/host/once.rs` - new: `poll_once`, `PolledOnce`, `commit_once`, `CommitFailure`, 14 tests
- `src/plugins/host/scheduler.rs` - `expose_secrets`, `oversize_cursor_error` / `oversize_cursor_outcome`, `send_records` pub(crate); the loop uses the helpers
- `src/plugins/host/cursor.rs` - `read_cursor_file`; `CursorStore::read` delegates; 2 tests
- `src/plugins/host/mod.rs` - `pub mod once;`
- `src/cli.rs` - `PluginsCommand::RunOnce { name, produce, json }` and 2 clap tests
- `src/commands/plugins.rs` - `Produce`, `render_items`, `render_items_json`, `finish_run_once`, `commit_and_record`, `find_plugin` (factored out of `run_check`), `find_source`, `open_state_store`, `poll_interruptible`, `execute_run_once`, `run_once`, async `dispatch`, 24 run-once tests; rustfmt applied (the file was rustfmt-clean at base)
- `src/main.rs` - the dispatch call is awaited
- `tests/plugins_cli.rs` - ignored `wasm_run_once_stub_dry_run_prints_stub_0_and_creates_no_state`

## Decisions Made

See `key-decisions` above. The user-visible ones are the plan's working decisions (OQ-2/3/4/5/9), all followed. The one change to the plan's described order is that the cursor is read before the component loads.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Ordering] Read the starting cursor before loading the component**
- **Found during:** Task 3 (the corrupt-cursor test)
- **Issue:** task 1's action described loading the component, then reading the cursor. With that order a corrupt cursor file is hidden behind any component problem, and the plan's own truth ("a corrupt file fails with the existing error naming plugin and file") cannot be shown without a real wasm component.
- **Fix:** `run_once` now reads the cursor (lock-free for a dry run; lock then read for `--produce`) right after the scoped config load and before the component loads. Refusals still come first and touch nothing.
- **Files modified:** `src/commands/plugins.rs`
- **Verification:** `run_once_dry_run_with_a_corrupt_cursor_fails_naming_the_file_and_leaves_it` (RED then GREEN)
- **Committed in:** `d164eeb`

**2. [Rule 3 - Blocking] Environment: gitignored wasm fixtures copied into the worktree**
- **Issue:** the real-wasm verify needs the prebuilt stub components, which are gitignored build output and absent from a fresh worktree.
- **Fix:** copied `stub_source.wasm` and `stub_fetcher.wasm` from the main checkout's `plugins/*/target/wasm32-unknown-unknown/release/` into the same ignored paths (no rebuild, nothing tracked or committed).

**3. [Plan note] Extra tests**
- `once_build_failure_is_a_build_failed_outcome` was split out of `once_failures_are_cycle_outcomes_with_describe_text` (the plan folded a failing factory into that test), plus `run_once_produce_reports_a_cursor_that_could_not_be_saved`, `run_once_zero_items_json_writes_nothing_to_stdout`, `run_once_json_with_produce_keeps_stdout_machine_readable` and `run_once_notice_and_counts_use_the_documented_wording`. All plan-named tests exist with the planned names.

**Total deviations:** 1 auto-fixed ordering change, 1 environment step, 1 test split. **Impact:** none on scope; the ordering change makes the corrupt-cursor behaviour testable without wasm.

## Issues Encountered

- `execute_run_once` is the test seam but `run_once` calls the two halves itself (to race only the poll against Ctrl-C), so it is unused outside tests; it carries `#[cfg_attr(not(test), allow(dead_code, reason = ..))]` like other code in this repository.
- Pre-existing and untouched: `rustfmt --check` reports 11 diffs in `scheduler.rs` (same 11 at the base commit), and `cli.rs` / `testing.rs` / `config.rs` have old diffs. Clippy shows no line for `src/commands/plugins.rs`, `tests/plugins_cli.rs`, `once.rs`, `cursor.rs` or `scheduler.rs`.

## Known Stubs

None.

## Threat Flags

None. The new surface (a name argument, a component load, a cursor read, an optional production send) is covered by T-4-17 to T-4-23, each with a mitigating test; `Cargo.toml` and `Cargo.lock` are unchanged. Not exercised automatically: the Ctrl-C interrupt of a spinning guest (T-4-22) and a real Fluvio send (by design, T-4-23).

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- `run-once` is ready as the tool for checking real plugins (`fili plugins run-once reddit`); the host-library `poll_once` / `commit_once` are available for plan 04-06.
- `dispatch` is now async and `main` awaits it; no other caller exists.

---
*Phase: 04-plugin-cli*
*Completed: 2026-11-10*

## Self-Check: PASSED

- Files exist: `src/plugins/host/once.rs`, `src/plugins/host/scheduler.rs`, `src/plugins/host/cursor.rs`, `src/plugins/host/mod.rs`, `src/cli.rs`, `src/commands/plugins.rs`, `src/main.rs`, `tests/plugins_cli.rs`.
- Commits exist and are ancestors of HEAD: `e6af539`, `4d57912`, `9a78646`, `aeb0bb9`, `d164eeb`.
- `cargo test -p filimon`: 363 passed, 0 failed (32 ignored); `--test plugins_cli`: 12 passed (3 ignored); `cargo test -p filimon -- --ignored wasm_run_once`: 2 passed; clippy: `Finished`, no line for the plan's gate files; `git diff -- Cargo.toml Cargo.lock` empty; no `"--produce"` in `tests/plugins_cli.rs`.
