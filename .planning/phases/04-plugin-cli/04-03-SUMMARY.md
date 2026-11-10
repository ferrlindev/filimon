---
phase: 04-plugin-cli
plan: 03
subsystem: plugin-cli
tags: [rust, clap, serde_json, list-command, sanitization, binary-tests]

requires:
  - phase: 04-plugin-cli
    provides: "plan 04-01: status sidecar, read_status, list_registry, collect_list, render_list, list, dispatch, tests/plugins_cli.rs helpers"
provides:
  - "fili plugins list --json: one compact JSON array on stdout with name, kind, enabled, fetcher, last_poll {state, at, items, error}"
  - "stderr notes for registry names skipped by the name rule (table and sanitized name)"
  - "full CLI-01 test coverage: every status shape, ordering, adjacency, empty registry, broken plugin TOML, held lock, creates-nothing, sanitizing, clipping"
  - "binary tests for the sample registry and exit codes 1 and 2"
affects: [04-04, 04-05, 04-06, plugin-cli]

plan_head_before: 6cac67e31bf5e2a05a378b1ce47fb5eb8e0bb47a
plan_head_after: e877421549ce159b650ba39e332834d9d71bb385

actuals:
  tokens: 7500
  tasks: 2
  commits: 3

tech-stack:
  added: []
  patterns:
    - "list takes (registry, state_dir_flag, json, out, err) so every output path is testable with in-memory writers"
    - "file-derived strings are cleaned with clean_guest_text and bounded per output form (table 80 for error, JSON 512)"

key-files:
  created: []
  modified:
    - src/cli.rs
    - src/commands/plugins.rs
    - tests/plugins_cli.rs

key-decisions:
  - "JSON mode never prints the empty-registry sentence: an empty registry prints [] (OQ-2)"
  - "Skipped-name notes are best effort (write errors ignored) so an unwritable stderr cannot change list's exit status"
  - "JSON clip lengths: at 64 characters, error and unreadable reason 512; table clip for error stays 80"

patterns-established:
  - "Binary tests that compare miette output remove whitespace and the gutter character first, because miette wraps long paths"

requirements-completed: [CLI-01]

coverage:
  - id: D1
    description: "fili plugins list --json prints exactly one JSON array through the real binary, no banner, creates nothing"
    requirement: CLI-01
    verification:
      - kind: integration
        ref: "tests/plugins_cli.rs#list_json_stdout_parses_as_a_json_array"
        status: pass
      - kind: unit
        ref: "src/commands/plugins.rs#list_json_prints_one_array_with_a_row_per_plugin"
        status: pass
    human_judgment: false
  - id: D2
    description: "list shows every plugin in every status shape, in source-then-fetcher name order, with same-named source and fetcher as two rows"
    requirement: CLI-01
    verification:
      - kind: unit
        ref: "src/commands/plugins.rs#list_rows_cover_every_status_shape, list_orders_sources_then_fetchers_by_name, list_keeps_same_named_source_and_fetcher_as_two_rows, list_never_reads_a_status_file_for_a_fetcher, list_empty_registry_prints_a_no_plugins_line_and_json_empty_array"
        status: pass
    human_judgment: false
  - id: D3
    description: "list tolerates broken plugin TOML and missing components, works while the state-dir lock is held, creates nothing"
    requirement: CLI-01
    verification:
      - kind: unit
        ref: "src/commands/plugins.rs#list_survives_a_broken_plugin_toml_and_a_missing_component, list_works_while_the_state_dir_lock_is_held, list_creates_nothing"
        status: pass
      - kind: integration
        ref: "tests/plugins_cli.rs#list_survives_a_broken_plugin_toml"
        status: pass
    human_judgment: false
  - id: D4
    description: "file-derived text is sanitized and clipped; invalid registry names are reported on stderr and never used as paths"
    requirement: CLI-01
    verification:
      - kind: unit
        ref: "src/commands/plugins.rs#list_sanitizes_status_text_from_files, list_clips_the_error_in_the_table_but_not_in_json, list_reports_skipped_invalid_names_and_never_uses_them_as_paths"
        status: pass
    human_judgment: false
  - id: D5
    description: "exit codes: 1 for a missing registry (names the path), 2 for a blank --state-dir and for plugins without a subcommand"
    requirement: CLI-01
    verification:
      - kind: integration
        ref: "tests/plugins_cli.rs#list_a_missing_registry_exits_one_and_names_the_path, list_with_a_blank_state_dir_flag_is_a_usage_error, plugins_without_a_subcommand_is_a_usage_error"
        status: pass
    human_judgment: false

duration: 40min
completed: 2026-11-10
status: complete
---

# Phase 4 Plan 03: `fili plugins list` JSON output and full coverage Summary

**`fili plugins list --json` prints one parseable JSON array through the real binary, and `list` is now covered for every status shape, broken plugin, held lock, hostile status text and exit code, with skipped registry names reported on stderr.**

## Performance

- **Duration:** about 40 min (the first build in this worktree took about 3 minutes)
- **Completed:** 2026-11-10
- **Tasks:** 2 (1 tracer, 1 TDD)
- **Files modified:** 3 (none created)

## Accomplishments

- Tracer (CLI-01): `PluginsCommand::List { json }`, `render_list_json` and the `last_poll` object (`never`, `not-applicable`, `ok`, `error`, `unreadable`, with `at`, `items`, `error`). The real binary prints exactly one array whose first byte is `[`, no banner, and the state directory is not created.
- `list` now writes one stderr note per registry name that breaks the name rule (table plus cleaned name); such names never become paths and never reach stdout.
- 12 new unit tests and 5 new binary tests (the plan named 13 unit and 6 binary; the remaining names were already in place from 04-01). All planned names exist and pass.
- Tracer gate: the tracer `<verify>` was automated-only, re-run after task 1 and passed; expansion continued ("Tracer verified end-to-end").

## Task Commits

1. **Task 1: `list --json` end to end (tracer)** - `a296a8f` (feat)
2. **Task 2 RED: failing skipped-name test plus the list coverage set** - `fb75241` (test)
3. **Task 2 GREEN: skipped-name notes on stderr** - `e877421` (feat)

No REFACTOR commit: the clippy and rustfmt cleanup of the new tests went into the GREEN commit.

## TDD Gate Compliance

- **RED:** `fb75241`. One test failed, on its planned assertion: `list_reports_skipped_invalid_names_and_never_uses_them_as_paths` panicked at `assert!(err.contains("sources"))` with `err == ""` (list wrote no note). The other 15 new tests in the same commit passed already, because `list_registry`, `collect_list`, sanitization and the no-lock read were built in plan 04-01; they are regression and edge coverage, not RED evidence. Semantic assessment: the failure is the assertion for the missing behaviour, not a compile, fixture or setup fault.
- **RED evidence classifier:** `gsd_run check tdd-red-evidence` does not support libtest console output, so it was not run; the assessment above is a manual inspection of the real `cargo test` output.
- **GREEN:** `e877421`, 308 unit tests and 7 binary tests pass.
- A first RED run also showed `list_a_missing_registry_exits_one_and_names_the_path` failing; that was a defect in the test (miette wraps the long temp path across lines, so a plain `contains` missed it), fixed before the RED commit, not product behaviour.

## Files Created/Modified

- `src/cli.rs` - `PluginsCommand::List { json }` with `--json`; clap test `plugins_list_json_flag_parses`
- `src/commands/plugins.rs` - `render_list_json`, `last_poll_json`, JSON clip constants, `list(registry, state_dir_flag, json, out, err)`, skipped-name notes, `dispatch` passing locked stdout and stderr, the coverage tests
- `tests/plugins_cli.rs` - binary tests for `--json`, the sample registry, broken plugin TOML, exit codes 1 and 2

## Decisions Made

- The OQ-2, OQ-6 and OQ-9 working decisions and the three flagged edge assumptions (adjacency, empty, ordering) were followed as written.
- Skipped-name notes ignore stderr write errors so a closed stderr cannot change `list`'s exit status.
- rustfmt was run on `src/commands/plugins.rs` and `tests/plugins_cli.rs` only, which also reflowed a few lines of plan 04-01's test code in the first file. `src/cli.rs` and other pre-existing files were not reformatted.

## Deviations from Plan

None - plan executed exactly as written. Two small interpretations:

- The plan lists `list_rejects_an_empty_state_dir` among the new tests; it already existed from 04-01 and was only updated for the new `list` signature.
- Plan 04-01's `plugins_list_parses` was updated for the `List { json: false }` pattern, as the plan allowed.

**Total deviations:** 0 auto-fixed.

## Issues Encountered

- Pre-existing clippy warnings in `src/cli.rs` (doc backticks at lines 42 and 46, a `format!` at line 80) and a pre-existing `cargo fmt --check` diff in `src/cli.rs` and `src/commands/consume.rs` are untouched and out of scope; no clippy line starts with `src/commands/plugins.rs` or `tests/plugins_cli.rs`.
- The `awk` acceptance check passes: production code above `#[cfg(test)]` contains no `create_dir`, `CursorStore::open` or `try_lock` (count 0). No binary test invokes `run` or `--produce`. `Cargo.toml` and `Cargo.lock` are unchanged.
- Unit-test count: 285 at plan start (04-01 summary) to 308 now, above the plan's 255 threshold.

## Known Stubs

None.

## Threat Flags

None. The new surface (JSON on stdout, a stderr note) is covered by T-4-09, T-4-10 and T-4-11, each of which has a mitigating test.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- `list` is complete for CLI-01. Plans 04-04 to 04-06 can reuse `list`'s `(out, err)` writer pattern and the `tests/plugins_cli.rs` helpers.
- The optional manual spot check against `examples/plugins/filimon.toml` is covered by the binary test `list_shows_the_sample_registry_plugins`.

---
*Phase: 04-plugin-cli*
*Completed: 2026-11-10*

## Self-Check: PASSED

- Files exist: `src/cli.rs`, `src/commands/plugins.rs`, `tests/plugins_cli.rs`.
- Commits exist and are ancestors of HEAD: `a296a8f`, `fb75241`, `e877421`.
- `cargo test -p filimon`: 308 passed, 0 failed; `--test plugins_cli`: 7 passed; clippy clean for the touched plan files.
