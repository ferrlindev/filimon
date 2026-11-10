---
phase: 04-plugin-cli
verified: 2026-11-10T08:00:00Z
status: human_needed
score: 4/4 must-haves verified
covered_files:
  - ".planning/phases/04-plugin-cli/04-01-PLAN.md"
  - ".planning/phases/04-plugin-cli/04-01-SUMMARY.md"
  - ".planning/phases/04-plugin-cli/04-02-PLAN.md"
  - ".planning/phases/04-plugin-cli/04-02-SUMMARY.md"
  - ".planning/phases/04-plugin-cli/04-03-PLAN.md"
  - ".planning/phases/04-plugin-cli/04-03-SUMMARY.md"
  - ".planning/phases/04-plugin-cli/04-04-PLAN.md"
  - ".planning/phases/04-plugin-cli/04-04-SUMMARY.md"
  - ".planning/phases/04-plugin-cli/04-05-PLAN.md"
  - ".planning/phases/04-plugin-cli/04-05-SUMMARY.md"
  - ".planning/phases/04-plugin-cli/04-06-PLAN.md"
  - ".planning/phases/04-plugin-cli/04-06-SUMMARY.md"
  - "src/cli.rs"
  - "src/commands/plugins.rs"
  - "src/commands/run.rs"
  - "src/main.rs"
  - "src/plugins/host/cursor.rs"
  - "src/plugins/host/loader.rs"
  - "src/plugins/host/mod.rs"
  - "src/plugins/host/once.rs"
  - "src/plugins/host/scheduler.rs"
  - "tests/plugins_cli.rs"
covered_digest: "v4:sha256:941a7168e347982dc3e9edaad1ce08331619028a9d330f9c7ca99692e845b78e"
behavior_unverified: 0
overrides_applied: 0
human_verification:
  - test: "Manual UAT of `fili run` against the production topic (plan prohibition T-4-24, intentionally not automated): start `fili run --registry <registry>` against a real cluster with a real source, let it poll at least two cycles, confirm items appear on `filimon-social-items`, press Ctrl+C once, confirm the `[Run] stopping` line, the per-plugin summary and exit 0; restart and confirm the cursor resumes without a gap or repeat; press Ctrl+C twice to confirm the immediate abort path."
    expected: "Items reach the topic; first Ctrl+C stops within the 10 s grace with a summary; second Ctrl+C aborts at once; restart resumes from the saved cursor; `fili plugins list` shows the last poll time and ok."
    why_human: "Needs a live Fluvio cluster and writes to the production topic; automation is prohibited from running `fili run` or `run-once --produce` against any Fluvio topic. Only the in-memory-sink path and the ignored scratch-topic test cover it."
  - test: "Stop `fili run` while stdout is a pipe whose reader exits (for example `fili run 2>&1 | tee run.log`, then Ctrl+C), to observe whether code-review finding CR-01 reproduces."
    expected: "Process stops (summary printed, exit 0). If it instead keeps running and ignores a second Ctrl+C / SIGTERM, CR-01 is confirmed and should be fixed before the command is used under a pipe."
    why_human: "Requires `fili run` against a cluster, which automation may not run. Reasoned from code, not executed."
---

# Phase 4: Plugin CLI Verification Report

**Phase Goal:** Users can inspect, validate, try, and run registered plugins from `fili` without writing code
**Verified:** 2026-11-10
**Status:** human_needed
**Re-verification:** No, initial verification

## Goal Achievement

### Observable Truths (ROADMAP success criteria)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | `fili plugins list` shows every registered source and fetcher with kind, enabled state, last poll status (time and ok/error) | VERIFIED | Real binary against `examples/plugins/filimon.toml` printed `NAME KIND ENABLED FETCHER LAST POLL` with 3 sources and 2 fetchers (`-` for never polled, `n/a` for fetchers), exit 0, and created no state directory. `--json` emits one array with `state` per plugin. Status write/read round trip is covered by `tracer_polled_status_appears_in_the_list` and `sched_status_*` unit tests (all pass). Scheduler calls `record_status` after every cycle (scheduler.rs:490). |
| 2 | `fili plugins check <name>` validates the component against its WIT world and TOML/secrets without polling; a broken component or config exits non-zero naming the problem | VERIFIED | Real binary: `check stub` exits 0 with `component: ok (... fits the source world)` and `config: ok`, and the guest's init greeting never appears. `check reddit` (missing component and secrets) exits 1 naming plugin, path, field and env var, never a value. `check nope` exits 1 listing registered plugins. Ignored wasm tests `wasm_check_passes_the_real_stub_source/_fetcher`, `wasm_check_a_fetcher_component_registered_as_a_source_is_a_wrong_world`, `wasm_check_component_matches_the_declared_world` pass. |
| 3 | `fili plugins run-once <name>` does a single poll and prints items; nothing is produced unless the produce option is given | VERIFIED | Real binary: `run-once stub` printed `[1/1] stub:stub-0 ... polled 1 item (0 dropped)` and `dry run: nothing was produced and the cursor was not saved`; state dir was not created. `run-once http` (a fetcher) is refused, exit 1. `commit_once` (once.rs:117-121) calls `send_records` and only then `cursors.write` (D-07); tests `commit_sends_then_writes_the_cursor_in_order` and the oversize/failed-sink cases pass. `--produce` was NOT executed (prohibited). |
| 4 | The run command polls every enabled source plugin on its interval and produces items to Fluvio until stopped | VERIFIED for the nominal path (see warning CR-01) | `Command::Run` dispatches to `run_plugin_host` (main.rs:130) -> `run()` -> `FluvioSink::connect_social` -> `run_with_sink` -> `Host::run_with_abort`. `wasm_run_with_sink_polls_every_enabled_source_and_skips_disabled` passes with the real stub component and an in-memory sink. Supervisor tests (grace, second-signal abort, healthy shutdown, panic vs abort, same-interval independence, ordering) pass. `forward_signals_first_cancels_shutdown_and_second_aborts` passes. The production Fluvio sink and a real signal stop were not exercised (human item 1). |

**Score:** 4/4 truths verified (0 present, behavior-unverified; none were left VERIFIED on presence alone, each cites a passing test or a real-binary run).

### Plan-level must-haves spot-checked

| Claim | Status | Evidence |
|-------|--------|----------|
| No banner for `plugins`/`run` | VERIFIED | `banner_is_skipped_for_plugin_commands`, `banner_is_skipped_for_run` pass; real-binary list output started with the table header. |
| Blank `--state-dir` rejected (WR-07), `plugins` without subcommand is a usage error | VERIFIED | `list_with_a_blank_state_dir_flag_is_a_usage_error`, `plugins_without_a_subcommand_is_a_usage_error` pass. |
| `list`/`check`/dry-run create nothing and take no lock | VERIFIED | Real binary left no state dir; `check_does_not_create_the_state_dir`, `list_works_while_the_state_dir_lock_is_held` pass. |
| Supervisor 10 s grace, abort reports `aborted at shutdown` (WR-10) | VERIFIED | `plugins::host::tests` all pass (374 passed, 0 failed in `cargo test -p filimon`). |
| Unused-code allows removed | VERIFIED | grep finds no `allow(dead_code/unused)` in `src/plugins`, `src/cli.rs`, `src/commands/plugins.rs`, `src/commands/run.rs`. |
| No test runs `fili run`/produce through the binary | VERIFIED (by design) | `tests/plugins_cli.rs` contains only list/check/dry-run binary tests. |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Full suite | `cargo test -p filimon` | lib 374 passed / 0 failed / 34 ignored; `tests/plugins_cli.rs` 12 passed / 3 ignored | PASS |
| Real-wasm tests | `cargo test -p filimon -- --ignored wasm_` (fixtures present) | 20 + 3 passed, 0 failed | PASS |
| list / list --json | real binary, sample registry | table and JSON array as expected, exit 0 | PASS |
| check stub / reddit / nope | real binary | 0 / 1 / 1 with named problems | PASS |
| run-once stub dry run, run-once on a fetcher | real binary | item printed, nothing saved; fetcher refused | PASS |
| `live_` tests (scratch Fluvio topic) | not run | no cluster use permitted | SKIP |

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| CLI-01 | 04-01, 04-03 | `plugins list` | SATISFIED | Truth 1 |
| CLI-02 | 04-04 | `plugins check <name>` | SATISFIED | Truth 2 |
| CLI-03 | 04-05 | `plugins run-once <name>` | SATISFIED | Truth 3 (dry run executed; produce proven with in-memory sinks only) |
| CLI-04 | 04-02, 04-06 | run command polling all enabled sources, producing to Fluvio | SATISFIED in code and in-memory tests; production topic path pending human UAT | Truth 4 |

All four IDs in the PLAN frontmatter match REQUIREMENTS.md, and REQUIREMENTS.md maps no other ID to Phase 4: no orphans. Housekeeping: REQUIREMENTS.md still shows CLI-01..04 as unchecked and `Pending`, and ROADMAP.md lists Phase 4 as `[ ]`; the orchestrator should update these on completion.

### Anti-Patterns Found

No `TBD`/`FIXME`/`XXX` in the phase-modified source files. No stubs or hollow data flows found: list/check/run-once all call real loader, store and host code.

### Judgment on code review findings (all open)

| Finding | Defeats a success criterion? | Severity here |
|---------|------------------------------|---------------|
| CR-01 `forward_signals` `println!` before `shutdown.cancel()` (run.rs:295-310) | Not as written. Confirmed in code: the print precedes both cancels, and `println!` panics on EPIPE, which is the failure the review describes. It bites only when stdout is a pipe whose reader dies with Ctrl+C (e.g. `\| tee`). With a tty, file or journald stdout, SC4's stop path works and is test-proven. I did not reproduce it because that needs `fili run` against a cluster. In the affected setup the process cannot be stopped cooperatively, so the "until it is stopped" half of SC4 degrades to SIGKILL. | WARNING. Two-line fix (cancel first, `writeln!(..).ok()`), recommended before real use. If the caller rules a piped `fili run` in scope, treat it as a gap and move to `gaps_found`. |
| WR-01 aborted/panicked plugin prints zero counters in summary | No. SC4 does not require summary counters; the "aborted at shutdown" state is correct. Misleading output. | WARNING |
| WR-02 scheduler writes status `ok:true` when the sink succeeded but the cursor write failed (scheduler.rs:458-482); `run-once --produce` records `ok:false` for the same case | Marginal. SC1 asks for ok/error last-poll status; in this rare disk-failure case `list` shows ok while a replay is pending. Confirmed in code. | WARNING |
| WR-03 guest error text could echo init secrets into status file/list/run-once | No SC covers it; it conflicts with the phase's "secrets never printed" intent for guest-controlled text only. | WARNING |
| IN-01..IN-04 stale comments, README startup order, unbounded cursor-file read, unsanitized check diagnostics | No. | INFO |

### Human Verification Required

1. **Production UAT of `fili run`** (intentionally not automated, T-4-24). See frontmatter for the exact steps and expected result.
2. **CR-01 reproduction under a pipe.** See frontmatter.

### Gaps Summary

No success criterion is failed. All four are backed by passing tests (full suite plus the real-wasm ignored group) and real-binary runs of `list`, `check` and a dry-run `run-once` against the sample registry. The only unverified part is the live `fili run` path against Fluvio, which was prohibited from automation, so status is `human_needed`. The open Critical review finding CR-01 is a real stop-path defect in a piped-stdout configuration, rated a warning because it does not break the criterion as written. Recommend fixing CR-01 (and WR-02) in a small gap plan before the manual UAT.

---

_Verified: 2026-11-10_
_Verifier: Claude (gsd-verifier)_
