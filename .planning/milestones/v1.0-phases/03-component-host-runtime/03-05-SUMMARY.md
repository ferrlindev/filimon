---
phase: 03-component-host-runtime
plan: 05
subsystem: infra
tags: [clap, tracing, tracing-subscriber, cli, logging]

requires:
  - phase: 03-component-host-runtime
    provides: "03-03 wasmtime/toolchain deps; tracing-subscriber env-filter already a direct dependency"
provides:
  - "Global --state-dir / FILI_STATE_DIR argument (Cli.state_dir: Option<PathBuf>, no default)"
  - "stderr tracing subscriber installed at the top of main with fili::plugins=info default filter"
affects: [03-04 HostOptions::resolve_state_dir, 03-06, 03-10, phase-04 run command]

actuals:
  tokens: 2400
  tasks: 2
  commits: 2

tech-stack:
  added: []
  patterns:
    - "Log directive derived from env!(CARGO_CRATE_NAME) so it cannot drift from the crate name"
    - "Argument-metadata tests (get_env/is_global_set/get_default_values) so operator env cannot skip checks"

key-files:
  created: []
  modified: [src/cli.rs, src/main.rs]

key-decisions:
  - "Default log filter is plugin-host events only (fili::plugins=info); existing commands' output unchanged unless RUST_LOG is set (RESEARCH Open Question 2)"
  - "Corrects RESEARCH A3: directive is fili::plugins=info, not filimon::plugins=info (binary crate is named fili)"
  - "state_dir has no default: None means resolve .fili-state/ next to the registry file (D-06)"

patterns-established:
  - "init_logging uses try_init and ignores Err so a second init never panics; logs go to stderr only"

requirements-completed: []

duration: 25min
completed: 2026-11-09
status: complete
plan_head_before: 32bb5134d3915494a420375fc4ed784f43080790
plan_head_after: 120174f
commits: 2
---

# Phase 3 Plan 05: State-dir flag and tracing subscriber Summary

**Global `--state-dir`/`FILI_STATE_DIR` flag (no default) plus a stderr `tracing_subscriber` with a `fili::plugins=info` default so host warn/error lines become visible.**

## Accomplishments
- `Cli.state_dir: Option<PathBuf>` beside `--registry`, global, env `FILI_STATE_DIR`, no default; dead-code allow gated on `not(test)` with a Phase 4 reason.
- `default_log_directive`, `log_filter`, `init_logging` in `src/main.rs`; `init_logging()` runs right after `Cli::parse()` and before the banner.
- `RUST_LOG` overrides the default; blank or invalid values fall back to it.
- 6 new tests (3 in cli, 3 in main), all pass; whole suite: 140 passed, 0 failed. `fili --help` prints help and lists `--state-dir [env: FILI_STATE_DIR=]`.

## Task Commits
1. Task 1: global --state-dir flag - `7b8f266`
2. Task 2: stderr tracing subscriber - `120174f`

## Deviations from Plan
None - plan executed as written. The `cargo clippy` run shows no new warning on the added code (remaining src/main.rs and src/cli.rs warnings are pre-existing lines).

## Notes for review
- HOST-02 and HOST-05 are only partly advanced here (cursor directory flag; log visibility). Neither is marked complete: HOST-05's "logged and backed off" is judged in plans 03-06 and 03-10, and HOST-02 needs the scheduler/state plans.
- RESEARCH Open Question 2 ("plugin events only by default") is resolved here but was not confirmed with the user.

## Known Stubs
None. `state_dir` is intentionally unread until the Phase 4 run command (documented via the dead_code reason).

## Threat Flags
None.

## Self-Check: PASSED
- src/cli.rs and src/main.rs modified and committed (7b8f266, 120174f both ancestors of HEAD).
