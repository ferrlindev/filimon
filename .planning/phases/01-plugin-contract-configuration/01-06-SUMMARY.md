---
phase: 01-plugin-contract-configuration
plan: 06
subsystem: testing
tags: [rust, clap, cli, registry, test-hardening]
gap_closure: true
review_findings_addressed: [IN-03]
hardened_items:
  - "01-VERIFICATION.md coincidental_reliance_items[0] (truth 39: `fili --registry` default is `filimon.toml`, D-13)"

# Dependency graph
requires:
  - phase: 01-plugin-contract-configuration (plan 01-04)
    provides: "global `--registry` flag with env FILI_REGISTRY and default DEFAULT_REGISTRY = filimon.toml"
provides:
  - "Environment-independent assertion of the `--registry` default via Cli::command() argument metadata (get_default_values)"
affects: [phase-01-verification, cli]

# Actuals (#2632)
actuals:
  tokens: 1337    # chars/4 over src/cli.rs (5346 chars), the only file changed
  tasks: 1
  commits: 1      # measured: git rev-list --count 4821ae5..HEAD before the SUMMARY commit
plan_head_before: 4821ae52ebeb472325f278d383bc1520b0a76929
plan_head_after: 57ba4c1ec574f580e08989600abbf1704f91f848

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Assert clap defaults from Cli::command() metadata (Arg::get_default_values), never by parsing, so process env cannot change what the test observes"

key-files:
  created: []
  modified:
    - src/cli.rs

key-decisions:
  - "Asserted the default on clap argument metadata rather than mutating or clearing FILI_REGISTRY in the test, so the test neither reads nor writes process env"

patterns-established:
  - "Clap default tests: find the arg by id on Cli::command(), assert get_default_values() length and value"

requirements-completed: [CONF-01]

coverage:
  - id: D1
    description: "cli::tests::registry_flag_defaults_to_filimon_toml asserts the `--registry` default (filimon.toml, D-13) unconditionally, even with FILI_REGISTRY exported"
    requirement: "CONF-01"
    verification:
      - kind: unit
        ref: "FILI_REGISTRY=/nonexistent/elsewhere.toml cargo test --bin fili cli::tests:: (test result: ok. 4 passed; registry_flag_defaults_to_filimon_toml ... ok)"
        status: pass
      - kind: other
        ref: "! grep -qF 'var_os(\"FILI_REGISTRY\")' src/cli.rs && grep -q 'get_default_values' src/cli.rs"
        status: pass
    human_judgment: false
  - id: D2
    description: "Production CLI behavior unchanged: `fili --help` still shows --registry, FILI_REGISTRY and the filimon.toml default"
    requirement: "CONF-01"
    verification:
      - kind: other
        ref: "cargo run --quiet --bin fili -- --help | grep --registry / FILI_REGISTRY / filimon.toml"
        status: pass
      - kind: unit
        ref: "cargo test --bin fili (test result: ok. 45 passed)"
        status: pass
    human_judgment: false

# Metrics
duration: 5min
completed: 2026-11-03
status: complete
---

# Phase 1 Plan 06: Environment-independent `--registry` default test Summary

**`registry_flag_defaults_to_filimon_toml` now asserts `filimon.toml` through `Cli::command()` argument metadata (`get_default_values()`), so an exported `FILI_REGISTRY` can no longer turn it into a silent pass.**

## Performance

- **Duration:** 5 min
- **Started:** 2026-11-03T11:55:27Z
- **Completed:** 2026-11-03T12:00:00Z
- **Tasks:** 1
- **Files modified:** 1

## Accomplishments
- Removed the early return that skipped the assertion when `FILI_REGISTRY` was set (review IN-03).
- The test finds the `registry` argument on `Cli::command()` and asserts its single default equals both `DEFAULT_REGISTRY` and the literal `filimon.toml` (D-13).
- The test reads and writes no environment variable. It passes with `FILI_REGISTRY=/nonexistent/elsewhere.toml` exported.
- Hardens 01-VERIFICATION.md `coincidental_reliance_items[0]` (truth 39). It is now an unconditional check.

## Task Commits

1. **Task 1: Assert the `--registry` default from Cli::command() metadata, independent of FILI_REGISTRY** - `57ba4c1` (test)

**Plan metadata:** recorded in the docs(01-06) commit that adds this SUMMARY

## Files Created/Modified
- `src/cli.rs` - Rewrote the body of `cli::tests::registry_flag_defaults_to_filimon_toml` only (inside `mod tests`). No changes to the `Cli` struct, `DEFAULT_REGISTRY`, or the other three registry tests.

## Verification Evidence

| Check | Command | Result |
|-------|---------|--------|
| Task verify 1 | `FILI_REGISTRY=/nonexistent/elsewhere.toml cargo test --bin fili cli::tests::` | PASS: `test result: ok. 4 passed`; `cli::tests::registry_flag_defaults_to_filimon_toml ... ok` |
| Task verify 2 | `! grep -qF 'var_os("FILI_REGISTRY")' src/cli.rs && grep -q 'get_default_values' src/cli.rs` | PASS |
| Task verify 3 | `cargo run --quiet --bin fili -- --help` then grep `--registry`, `FILI_REGISTRY`, `filimon.toml` | PASS: `--registry <REGISTRY> ... [env: FILI_REGISTRY=] [default: filimon.toml]` |
| AC: diff scope | `git diff HEAD -- src/cli.rs` (before commit) | PASS: single hunk at lines 100-115, inside `mod tests` |
| Plan verification 3 | `cargo test --bin fili` | PASS: `test result: ok. 45 passed` (this worktree only; plan 01-05 runs in parallel, so the orchestrator should re-run after both merge) |
| Unset env | `cargo test --bin fili cli::tests::` | PASS: 4 passed |

The tracer feedback gate re-ran the automated verify (interactive mode, `human_verify_mode: end-of-phase`, verify has only `<automated>` checks). It passed, so no checkpoint was raised.

## Decisions Made
- Compared clap's `OsStr` default directly against `&str` (`DEFAULT_REGISTRY` and `"filimon.toml"`), as the plan specified. No new imports were needed. `CommandFactory` and `OsStr` were already imported in the test module.

## Deviations from Plan

None - plan executed exactly as written.

## Issues Encountered
- The build prints 7 `dead_code` warnings from `src/main.rs`, `src/models.rs`, `src/output.rs` and `src/streaming.rs`, plus `load_registry`. They are older than this plan and come from files it does not touch. They were left alone.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness
- Truth 39 (D-13 default) is now proven on any machine and in CI.
- After plan 01-05 merges in this wave, re-run `cargo test --bin fili` on the combined tree (plan verification step 3).

---
*Phase: 01-plugin-contract-configuration*
*Completed: 2026-11-03*

## Self-Check: PASSED

- FOUND: .planning/phases/01-plugin-contract-configuration/01-06-SUMMARY.md
- FOUND: 57ba4c1 (task 1 commit, ancestor of HEAD)
- FOUND: 7927848 (SUMMARY commit, ancestor of HEAD)
- All 3 task verify commands, all 4 acceptance criteria, and the full `cargo test --bin fili` suite (45 passed) re-ran green
