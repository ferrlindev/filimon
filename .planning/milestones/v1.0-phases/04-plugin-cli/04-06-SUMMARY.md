---
phase: 04-plugin-cli
plan: 06
subsystem: plugin-cli
tags: [rust, clap, tokio, signals, graceful-shutdown, fluvio, miette, wasmtime-host]

requires:
  - phase: 04-plugin-cli
    provides: "plan 04-02: Host::load accessors, Host::run_with_abort, bounded shutdown; plan 04-05: shared sink/cursor seams and the state_dir_for pattern"
  - phase: 03-component-host-runtime
    provides: "Host, FluvioSink::connect_social, ItemSink, VecSink, CursorStore, live_support scratch-topic helpers"
provides:
  - "fili run: polls every enabled source on its interval and produces to Fluvio until stopped"
  - "run_with_sink<S: ItemSink>: generic, signal-free core (strict startup report, load failures, summary)"
  - "ensure_enabled_sources, run_outcome (OQ-9 exit rule), render_run_summary"
  - "SignalSource trait, forward_signals (first request -> shutdown, second -> abort), OsSignals (SIGINT + SIGTERM)"
  - "No blanket unused-code allow in plugins/mod.rs; remaining unused items narrowed to item-level allows with reasons"
  - "plugins/README.md Plugin CLI section and corrected shutdown/status text; README.md command list"
affects: [05-fetcher-layer, 08-integration-live-e2e-authoring]

plan_head_before: 1e1235ec5bc9d2351c496e1af37bdbb7ac5df8e3
plan_head_after: 3bae594e2daded3192c1098966a4d75af8cde6a5

actuals:
  tokens: 13000
  tasks: 3
  commits: 4

tech-stack:
  added: []
  patterns:
    - "generic run core over ItemSink + a SignalSource trait so the only network and OS touch points are in one thin wrapper"
    - "held signal streams (not per-call ctrl_c) so a request between two next() calls is not lost"
    - "item-level cfg_attr(not(test), allow(dead_code, reason)) instead of a module-wide allow"

key-files:
  created:
    - src/commands/run.rs
  modified:
    - src/commands/mod.rs
    - src/commands/plugins.rs
    - src/cli.rs
    - src/main.rs
    - src/plugins/mod.rs
    - src/plugins/config.rs
    - src/plugins/host/mod.rs
    - src/plugins/host/engine.rs
    - README.md
    - plugins/README.md

key-decisions:
  - "Plan working decisions OQ-1/8/9/10/11 followed as written (top-level `fili run`, 10 s grace with second-signal abort, exit 1 only when every plugin was fault-disabled or nothing could start, load failures printed and the rest run, secrets log untouched)"
  - "OsSignals holds SIGINT and SIGTERM streams on Unix (plan said ctrl_c() plus a SIGTERM stream); ctrl_c() is used only off Unix. Both are held so neither request is lost between next() calls"
  - "The `Plugins` subcommand doc comment no longer says 'run' (run is now its own top-level command)"
  - "Unused items left after removing the blanket allow: LoadedSource.config_path/fetcher, HostReport's three lists, Host::run and with_shutdown_grace (all cfg(test)-only users), HostState::plugin (Phase 5 allowlist); each carries an item-level allow with a reason"

patterns-established:
  - "Commands that touch the production topic keep the connect in one function (run) and are tested through a generic core with VecSink or a scratch topic"
  - "Stop handling is a pure token-forwarder task that the caller aborts when the host ends on its own"

requirements-completed: [CLI-04]

coverage:
  - id: D1
    description: "fili run polls every enabled source through the real host into a sink and skips disabled sources (tracer)"
    requirement: CLI-04
    verification:
      - kind: integration
        ref: "src/commands/run.rs#wasm_run_with_sink_polls_every_enabled_source_and_skips_disabled (--ignored)"
        status: pass
    human_judgment: false
  - id: D2
    description: "Honest, fail-fast startup: no enabled source, a held state directory and zero loadable sources are errors before polling; load failures are printed naming plugin and path"
    requirement: CLI-04
    verification:
      - kind: unit
        ref: "src/commands/run.rs#run_refuses_a_registry_with_no_enabled_source, run_with_sink_refuses_a_locked_state_dir, run_with_sink_prints_load_failures_and_errors_when_nothing_loaded"
        status: pass
    human_judgment: false
  - id: D3
    description: "Summary sorted by plugin name with counts and sanitized, clipped reasons; exit rule fails only when every plugin was disabled by a fault"
    requirement: CLI-04
    verification:
      - kind: unit
        ref: "src/commands/run.rs#run_summary_lists_plugins_sorted_with_counts_and_reasons, run_outcome_is_an_error_only_when_every_plugin_was_disabled_by_a_fault"
        status: pass
    human_judgment: false
  - id: D4
    description: "First stop request cancels only the shutdown token, the second cancels the abort token; a dropped forwarder cancels nothing"
    requirement: CLI-04
    verification:
      - kind: unit
        ref: "src/commands/run.rs#forward_signals_first_cancels_shutdown_and_second_aborts, forward_signals_ignores_a_cancelled_shutdown_when_the_host_ended_on_its_own"
        status: pass
    human_judgment: false
  - id: D5
    description: "Command-level run produces to a real scratch topic and a restart resumes from the saved cursor with no gap or repeat"
    requirement: CLI-04
    verification:
      - kind: integration
        ref: "src/commands/run.rs#live_run_with_sink_produces_to_a_scratch_topic_and_restart_resumes (--ignored, local cluster)"
        status: pass
    human_judgment: false
  - id: D6
    description: "clap surface: `fili run` parses, takes --registry and --state-dir before or after, rejects an empty state dir; no banner for run"
    requirement: CLI-04
    verification:
      - kind: unit
        ref: "src/cli.rs#run_parses, run_takes_the_global_flags, run_rejects_an_empty_state_dir; src/main.rs#banner_is_skipped_for_run"
        status: pass
    human_judgment: false
  - id: D7
    description: "Carry-overs closed: blanket unused-code allow removed, documentation describes the real shutdown, status file and commands"
    requirement: CLI-04
    verification:
      - kind: other
        ref: "grep gates on src/plugins/mod.rs, plugins/README.md, README.md; cargo clippy --all-targets clean for the plan's gate files"
        status: pass
    human_judgment: false
  - id: D8
    description: "Real `fili run` binary against the production topic filimon-social-items: Ctrl-C stops within about 10 s, summary printed, exit 0, restart resumes from the cursor, SIGTERM and second-signal abort"
    verification: []
    human_judgment: true
    rationale: "The plan forbids any automated test from connecting the production sink or running `fili run` through the binary; the real-signal behaviour (SIGINT, SIGTERM, second-signal abort, exit code) is the manual human-check in plan task 3"

duration: ~75min
completed: 2026-11-10
status: complete
---

# Phase 4 Plan 06: `fili run` Summary

**`fili run` polls every enabled source plugin on its interval into Fluvio until stopped: strict load, honest startup report, SIGINT/SIGTERM graceful stop with a second-signal abort, a per-plugin summary and a defined exit rule, over a generic sink so everything but the one production connect is tested without a cluster.**

## Performance

- **Duration:** about 75 min (most of it cargo compile time and the tests)
- **Completed:** 2026-11-10
- **Tasks:** 3 (1 tracer, 1 TDD, 1 auto)
- **Files modified:** 11 (1 created)

## Accomplishments

- Tracer (CLI-04, SC4): `run_with_sink` loads with `Host::load`, prints the state directory and loaded sources and fetchers, prints one stderr line per load failure naming kind, plugin, component path and the `HostError` text, errors with `no source plugin could be loaded` when nothing loaded, then runs `Host::run_with_abort` and prints the summary. The ignored real-wasm test polls `stub` and `stub2` (stub component, paused time), skips the disabled `off` source, writes `stub.json` and `stub2.json` and no `off.json`. The tracer `<verify>` was automated-only and was run end to end after task 1 (all three commands passed), so expansion continued.
- Exit rule (OQ-9): an operator stop exits 0 even when tasks were aborted (named in a warning); exit 1 only when every plugin ended disabled by a fault, when no source is enabled, when none loaded, or on a startup error. `fili --registry <empty> run` exits 1 with a message naming the registry and `enabled = true`, before any network use.
- Stopping: `forward_signals` turns the first request into the shutdown token (prints the 10 s grace line) and the second into the abort token; `OsSignals` holds SIGINT and (Unix) SIGTERM streams. `run` spawns the forwarder before the host loads, and aborts it once the host returns.
- Cluster proof: `live_run_with_sink_produces_to_a_scratch_topic_and_restart_resumes` ran against the local cluster (passed twice); the topic ids are `stub-0 .. stub-(n-1)` across two runs on one state directory, keys `stub:<id>`, and the `filimon-test-run-*` scratch topic is deleted (none left on the cluster).
- Carry-overs: the module-level `dead_code` allow in `src/plugins/mod.rs` is gone; the four still-unused items carry item-level allows with reasons; `Cli.state_dir` and `HostOptions::for_registry` now have callers; the README claims about shutdown (returns within seconds) and poll status (not persisted) are replaced with the real behaviour; the new `## Plugin CLI` section documents list, check, run-once, run, the status file and exit codes.
- Tests: unit-test binary 363 -> 374 passed (34 ignored, was 32); `tests/plugins_cli.rs` 12 passed (3 ignored); `--ignored wasm_` 20 + 3 passed; clippy `Finished` with no line for the plan's gate files.

## Task Commits

1. **Task 1: end-to-end `fili run` core (tracer)** - `f757099` (feat)
2. **Task 2: signals** - `f656efd` (test, RED) then `ddeaaa7` (feat, GREEN)
3. **Task 3: carry-overs, clap tests, docs** - `3bae594` (chore)

## TDD Gate Compliance

Task 2: RED `f656efd` added the tests plus a `forward_signals` stub that waits forever (a stub that returned at once failed on a closed channel, which is not the planned assertion, so it was changed first). `forward_signals_first_cancels_shutdown_and_second_aborts` failed on `the first request starts a graceful stop`, the planned assertion. `forward_signals_ignores_a_cancelled_shutdown_when_the_host_ended_on_its_own` passed at RED (it is a regression guard on the abort path). GREEN `ddeaaa7` implements the forwarder and the real source. `gsd_run check tdd-red-evidence` was not applied: it has no adapter for libtest text, so the RED evidence is the real `cargo test` output inspected by hand. No REFACTOR commit.

## Files Created/Modified

- `src/commands/run.rs` - new: `ensure_enabled_sources`, `run_outcome`, `render_run_summary`, `run_with_sink`, `run`, `SignalSource`, `forward_signals`, `OsSignals`, 9 tests
- `src/commands/mod.rs`, `src/commands/plugins.rs` - `pub mod run`, `run_plugin_host` re-export, `state_dir_for` is `pub(crate)`
- `src/cli.rs` - `Command::Run`, three clap tests; `Plugins` doc no longer says "run"
- `src/main.rs` - dispatch arm, banner skip for `Run`, `banner_is_skipped_for_run`, header comment
- `src/plugins/mod.rs` - blanket allow removed
- `src/plugins/config.rs`, `src/plugins/host/engine.rs`, `src/plugins/host/mod.rs` - item-level allows with reasons; host header comment updated
- `README.md`, `plugins/README.md` - documentation

## Decisions Made

See `key-decisions` above. The only departure from the plan text is `OsSignals` using held SIGINT and SIGTERM streams on Unix instead of `ctrl_c()`, so a second Ctrl-C arriving between two `next()` calls cannot be lost.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Stub test sources lacked the greeting setting**
- **Found during:** Task 1 (the real-wasm test)
- **Issue:** the stub component refuses `init` without a `greeting` setting, so both plugins were disabled and polled 0 times.
- **Fix:** the test helper writes `greeting = "hello from run"` for every test source.
- **Files modified:** `src/commands/run.rs`
- **Committed in:** `f757099`

**2. [Rule 3 - Blocking] Environment: gitignored wasm fixtures copied into the worktree**
- **Issue:** the real-wasm and live tests need the prebuilt `stub_source.wasm`, `stub_fetcher.wasm` and `fault_source.wasm`, which are gitignored build output absent from a fresh worktree.
- **Fix:** copied the three files from the main checkout's ignored `plugins/*/target/wasm32-unknown-unknown/release/` paths into the same ignored paths (no rebuild, nothing tracked or committed).

**3. [Rule 1 - Lint] clippy and rustfmt on the new file**
- `ScriptedSignals::next` in the tests became `async fn` (clippy `manual_async_fn`); `rustfmt` applied to `src/commands/run.rs` only; `HostState::plugin`'s allow is unconditional because no test reads it. Committed in `3bae594`.

**Total deviations:** 1 bug in a test helper, 1 environment step, 1 lint cleanup. **Impact:** none on scope.

## Issues Encountered

- A `fili run` through the real binary was not performed (by design, T-4-24). The only binary check made was `fili --registry <empty file> run`, which exits 1 before any network use.
- Pre-existing and untouched: clippy and rustfmt differences in other files (`cli.rs` doc-comment backticks, `main.rs` format strings, `config.rs` durations), the `streaming.rs`/`models.rs` dead-code warnings.

## Known Stubs

None.

## Threat Flags

None. The new surface (the `run` command, OS signal handlers) is covered by T-4-24 to T-4-29. `FluvioSink::connect_social` is called only from `run` (and run-once produce); no test in `run.rs` calls it or `run(`; the cluster test names only a scratch topic. `Cargo.toml`, `Cargo.lock` and `fluvio/` are unchanged. Not exercised automatically: real SIGINT/SIGTERM delivery to the binary and the production-topic run (manual human-check below).

## User Setup Required

None for automation. Manual UAT left to the user (plan task 3 human-check): with a scratch registry listing only the stub, run `cargo run -q -- --registry <scratch>/filimon.toml run`, confirm `fluvio consume filimon-social-items -B -d` shows `stub:stub-0`, `stub-1`, that `plugins list` shows `ok (1 item)` while `run` is up, that Ctrl-C ends it within about 10 seconds with a summary and exit 0 (also `kill -TERM`, and a second Ctrl-C during the grace), and that a second `run` resumes at `stub-N`.

## Next Phase Readiness

- Phase 4 gates pass except the manual real-binary check. Phase 5 consumes `LoadedSource.fetcher` and `HostState::plugin` (both carry allows naming it).
- Not run here: nothing from the plan's verification list; the live group ran against the local cluster and used scratch topics only.

---
*Phase: 04-plugin-cli*
*Completed: 2026-11-10*

## Self-Check: PASSED

- Files exist: `src/commands/run.rs`, `src/commands/mod.rs`, `src/cli.rs`, `src/main.rs`, `src/plugins/mod.rs`, `plugins/README.md`, `README.md`.
- Commits exist and are ancestors of HEAD: `f757099`, `f656efd`, `ddeaaa7`, `3bae594`.
- `cargo test -p filimon`: 374 passed, 0 failed (34 ignored); `--test plugins_cli`: 12 passed; `-- --ignored wasm_`: 20 + 3 passed; `-- --ignored live_run`: 1 passed (scratch topic only); clippy: `Finished`, no line for the plan's gate files; `Cargo.toml`, `Cargo.lock`, `fluvio/` unchanged against the plan base; no `connect_social` or `run(` in the test module of `run.rs`.
