---
phase: 04-plugin-cli
plan: 04
subsystem: plugin-cli
tags: [rust, clap, wasmtime, miette, check-command, config-scoping, redaction, binary-tests]

requires:
  - phase: 04-plugin-cli
    provides: "plan 04-03: PluginsCommand::List, list(registry, state_dir_flag, json, out, err), dispatch, tests/plugins_cli.rs helpers"
  - phase: 03-plugin-host
    provides: "load_source / load_fetcher preflight and linker, HostEngine, HostError, config loader"
provides:
  - "fili plugins check <name>: validates one plugin's component against its WIT world and its TOML and secrets, without polling"
  - "check_component in loader.rs: the single shared world check (no second checker); PluginKind::world is pub, load_source is pub(crate)"
  - "load_plugin_with_env / load_plugin: plugin-scoped config load that never reads or reports another plugin's problems"
  - "ConfigError::plugin() accessor"
  - "run_check / render_check / check and CheckReport, ComponentCheck, ConfigCheck in commands/plugins.rs"
  - "WASI_IMPORT_COMPONENT shared in plugins::host::testing"
affects: [04-05, 04-06, plugin-cli]

plan_head_before: c35d32e76e61d6d63bc36035572eca9806afd35e
plan_head_after: 6e5ad753480e72082b47bd0129a668a480241a20

actuals:
  tokens: 15500
  tasks: 2
  commits: 2

tech-stack:
  added: []
  patterns:
    - "check reports each part (component, config) independently so a failing component never hides the config result"
    - "diagnostics go to stderr through miette::Report, the summary to stdout, failure is an Err so main exits 1"
    - "one private loader body (load_registry_inner) with an optional `only` scope, so the unscoped path is byte-for-byte the old behaviour"

key-files:
  created: []
  modified:
    - src/plugins/error.rs
    - src/plugins/config.rs
    - src/plugins/host/loader.rs
    - src/plugins/host/testing.rs
    - src/cli.rs
    - src/commands/plugins.rs
    - tests/plugins_cli.rs

key-decisions:
  - "OQ-3/OQ-5/OQ-9/OQ-11 working decisions followed: disabled source validates component only and prints config: skipped; check never runs guest init; exit 1 on any failed part; the resolved-secrets info log stays"
  - "The scoped loader still runs for a disabled source so a registry-level problem that names it (duplicate name) is reported; its own TOML is never read"
  - "A name defined under both [sources] and [fetchers] resolves to the source first; the duplicate-name problem is reported through the scoped config load"
  - "CheckReport carries no enabled flag or component path (they were unused); the component path appears in the component diagnostic"

patterns-established:
  - "Binary tests squash whitespace and the miette gutter before searching stderr, because miette wraps long paths and sentences"
  - "Redaction tests use one distinctive value and assert absence of the whole value and of two fragments across summary, Debug and rendered diagnostics"

requirements-completed: [CLI-02]

coverage:
  - id: D1
    description: "fili plugins check stub against the real prebuilt stub source exits 0 with check passed, never runs guest init and does not create the state directory"
    requirement: CLI-02
    verification:
      - kind: integration
        ref: "tests/plugins_cli.rs#wasm_check_passes_the_real_stub_source (--ignored)"
        status: pass
      - kind: integration
        ref: "tests/plugins_cli.rs#wasm_check_passes_the_real_stub_fetcher (--ignored)"
        status: pass
    human_judgment: false
  - id: D2
    description: "A broken or wrong component is a non-zero exit naming plugin, path and reason (ComponentLoad, ForeignImports, WrongWorld, zero-byte file)"
    requirement: CLI-02
    verification:
      - kind: integration
        ref: "tests/plugins_cli.rs#check_garbage_component_exits_nonzero_naming_plugin_and_path"
        status: pass
      - kind: unit
        ref: "src/commands/plugins.rs#check_garbage_component_names_plugin_and_path, check_zero_byte_component_is_a_component_load_failure, check_wasi_import_component_names_the_import, wasm_check_a_fetcher_component_registered_as_a_source_is_a_wrong_world (--ignored)"
        status: pass
      - kind: unit
        ref: "src/plugins/host/loader.rs#check_component_reports_garbage_as_a_component_load_failure, check_component_names_foreign_imports, wasm_check_component_matches_the_declared_world (--ignored)"
        status: pass
    human_judgment: false
  - id: D3
    description: "Config problems (bad TOML, bad [host] value, missing secret, unknown fetcher, .env) name the plugin and field or key and are scoped to the checked plugin only"
    requirement: CLI-02
    verification:
      - kind: unit
        ref: "src/plugins/config.rs#plugin_only_load_ignores_other_plugins_problems, plugin_only_load_reports_that_plugins_problems, plugin_only_load_reads_dotenv_only_when_the_plugin_declares_secrets, plugin_only_load_of_a_disabled_source_skips_its_config, plugin_only_load_keeps_a_duplicate_name_problem_for_that_name, config_error_plugin_names_every_per_plugin_variant"
        status: pass
      - kind: unit
        ref: "src/commands/plugins.rs#check_bad_toml_and_bad_host_value_name_plugin_and_field, check_missing_secret_names_plugin_alias_and_env_var, check_reports_an_unknown_fetcher_reference, check_ignores_other_plugins_broken_config, check_reports_a_dotenv_problem_only_when_the_plugin_needs_it"
        status: pass
      - kind: integration
        ref: "tests/plugins_cli.rs#check_a_broken_config_exits_nonzero_naming_plugin_and_field"
        status: pass
    human_judgment: false
  - id: D4
    description: "Honest reporting: a disabled source prints config: skipped, a fetcher prints config: none, and check passed never hides a failed part"
    requirement: CLI-02
    verification:
      - kind: unit
        ref: "src/commands/plugins.rs#check_disabled_source_validates_component_only_and_says_config_skipped"
        status: pass
    human_judgment: false
  - id: D5
    description: "Secret values never appear in check output or diagnostics; alias and env var names do"
    requirement: CLI-02
    verification:
      - kind: unit
        ref: "src/commands/plugins.rs#check_does_not_print_secret_values"
        status: pass
      - kind: integration
        ref: "tests/plugins_cli.rs#check_never_prints_a_secret_value"
        status: pass
    human_judgment: false
  - id: D6
    description: "Edge cases: empty, unknown and hostile names exit 1 listing the registered plugins, never a panic or a path lookup; the echoed name is sanitized and clipped to 64 characters; check creates no state directory"
    requirement: CLI-02
    verification:
      - kind: unit
        ref: "src/commands/plugins.rs#check_unknown_name_lists_registered_plugins, check_empty_name_is_unknown_and_never_a_path, check_name_is_compared_exactly_and_echoed_sanitized"
        status: pass
      - kind: integration
        ref: "tests/plugins_cli.rs#check_unknown_plugin_exits_nonzero_and_names_the_registered_ones, check_does_not_create_the_state_dir"
        status: pass
    human_judgment: false

duration: 14min
completed: 2026-11-10
status: complete
---

# Phase 4 Plan 04: `fili plugins check` Summary

**`fili plugins check <name>` validates one plugin without polling it: the component goes through the host's own preflight (`check_component`, no second checker) and its TOML and `.env` secrets through a plugin-scoped config loader, with exit 1 and a diagnostic naming plugin, path or field on any failure and no secret value ever printed.**

## Performance

- **Duration:** about 14 min of work (a cold build of this worktree took about 3 minutes)
- **Started:** 2026-11-10T06:32:46Z
- **Completed:** 2026-11-10T06:46Z
- **Tasks:** 2 (1 tracer, 1 TDD)
- **Files modified:** 7 (none created)

## Accomplishments

- Tracer (CLI-02, SC2): `fili --registry examples/plugins/filimon.toml plugins check stub` against the real prebuilt stub component exits 0, prints `plugin `stub` (source)`, a `component: ok` line naming the `source` world, `config: ok (poll interval 60s, secrets: none)` and `check passed`. The stub's init-time greeting never reaches stderr and no state directory is created. Tracer gate: the `<verify>` was automated-only and was run end to end after task 1 (all three commands passed), so expansion continued.
- `check_component(kind, name, path, engine)` in `loader.rs` reuses `load_source` / `load_fetcher` and discards the linked result; nothing is instantiated. `load_source` is now `pub(crate)` and `PluginKind::world` is `pub`.
- `ConfigError::plugin()` and `load_plugin_with_env` / `load_plugin`: the body of `load_registry_with_env` became a private `load_registry_inner(path, env, only)`; the unscoped path is unchanged (every pre-existing config test still passes). With a scope, no other plugin's TOML is read, `.env` is read only when the checked plugin declares secrets, and registry-level problems are kept only when they concern the checked name.
- Failure modes: garbage and zero-byte components (`ComponentLoad`), WASI imports (`ForeignImports`), a fetcher registered as a source (`WrongWorld`), bad TOML, `poll_interval_secs = 0`, bad allowlist entry, missing secret (alias and env var name), unknown fetcher reference, malformed `.env` (path and line, never content), unknown / empty / hostile names.
- Tests added: task 1 has 1 clap test, 3 config tests and 2 binary tests (one ignored real-wasm); task 2 adds 3 config tests, 3 loader tests (one ignored), 14 command tests (one ignored) and 5 binary tests (one ignored). Unit-test binary: 308 -> 330 passed, 31 ignored; the four ignored real-wasm tests plus the two ignored binary ones all pass with `--ignored wasm_check`.
- The optional spot check `plugins check reddit` against the sample registry exits 1, naming the missing `components/reddit.wasm` and the two unset secrets.

## Task Commits

1. **Task 1: check one real plugin end to end (tracer)** - `701b1cf` (feat)
2. **Task 2: error cases, isolation, disabled and fetcher handling, redaction, exit codes** - `6e5ad75` (test)

## TDD Gate Compliance

Task 2 is `tdd="true"` but there is no RED/GREEN pair, and this is a gate violation to flag: the tracer in task 1 (a `type="tracer"` task, production code plus tests in one commit) already built `run_check`, `render_check`, `check`, the scoped loader and `check_component`, so every test written in task 2 passed on its first run. There was no failing target test to commit as RED and no production change to commit as GREEN, so the task is one `test(04-04)` commit, and `gsd_run check tdd-red-evidence` was not applicable.

Mitigation: the tests were shown to be able to fail. With the scoping predicate in `load_registry_inner` forced to match every plugin, 6 of the new tests fail (`check_ignores_other_plugins_broken_config`, `check_reports_a_dotenv_problem_only_when_the_plugin_needs_it`, and four `plugin_only_load_*`); the predicate was then restored and the whole suite re-run green. The mutation was never committed.

## Files Created/Modified

- `src/plugins/error.rs` - `ConfigError::plugin()`
- `src/plugins/config.rs` - `load_registry_inner`, `load_plugin_with_env`, `load_plugin`, six scoped-loading tests
- `src/plugins/host/loader.rs` - `check_component`, `pub(crate) load_source`, `pub PluginKind::world`, three tests
- `src/plugins/host/testing.rs` - `pub(crate) const WASI_IMPORT_COMPONENT` moved from the loader tests
- `src/cli.rs` - `PluginsCommand::Check { name }` and the `plugins_check_parses` test
- `src/commands/plugins.rs` - `CheckReport`, `ComponentCheck`, `ConfigCheck`, `run_check`, `render_check`, `write_diagnostics`, `check`, dispatch wiring, 14 tests
- `tests/plugins_cli.rs` - helpers (`stderr`, `squash`, `write_check_registry`) and 7 binary tests

## Decisions Made

- The OQ-3, OQ-5, OQ-9 and OQ-11 working decisions and the two flagged edge assumptions (empty/unknown name and zero-byte component, exact-byte name with sanitized echo) were followed as written.
- A disabled source still goes through the scoped loader so that a registry-level problem naming it (a duplicate source/fetcher name) is reported; only its own TOML is skipped.
- `CheckReport` omits an `enabled` flag and a component path: nothing read them (dead-code warning) and the path is already in the component diagnostic.

## Deviations from Plan

None - plan executed exactly as written, except for the TDD gate note above (the plan did not call for a split RED/GREEN, but task 2 is marked `tdd="true"`), and one environment step:

- The plan's real-wasm verify needs prebuilt fixtures, which are gitignored build output and so absent from a fresh worktree. The two stub `.wasm` files were copied from the main checkout's `plugins/*/target/wasm32-unknown-unknown/release/` into the same ignored paths in this worktree (no rebuild). They are not tracked and not committed.

**Total deviations:** 0 auto-fixed.

## Issues Encountered

- `src/plugins/config.rs:220: function load_plugin is never used` is a new dead-code warning (the plan requires `load_plugin` as public API for the process-environment mirror, and `check` uses `load_plugin_with_env` so tests can inject env). The same file already warns for the equally unused `load_registry`. The plan's clippy gate lists `config.rs` as untouched and no listed file produced a new line.
- Pre-existing, out of scope and untouched: clippy lines in `src/cli.rs` (42, 46, 80), three `testing.rs` lines (now at 71, 80, 84 after the constant moved) and three `Duration` lines in the config tests; `rustfmt --check` diffs in `config.rs` (three, old code), `testing.rs` (a use line) and `cli.rs`. New code in the touched files is rustfmt-clean.
- The final stderr line of a failed check is `Error: × plugin `<name>` failed its check`, which is main's rendering of the returned error; the diagnostics above it carry the detail.

## Known Stubs

None.

## Threat Flags

None. The new surface (a name argument, component compile, config and `.env` read, terminal output) is covered by T-4-12 to T-4-16, each with a mitigating test; no Cargo change (`git diff -- Cargo.toml Cargo.lock` is empty).

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- `check_component` and `pub(crate) load_source` are ready for plan 04-05 as the plan states; `load_plugin_with_env` can scope a single-plugin load for `run-once`.
- `check` takes no cursor store, builds no instance and never creates the state directory, so the lock and state-dir handling of later plans is untouched.

---
*Phase: 04-plugin-cli*
*Completed: 2026-11-10*

## Self-Check: PASSED

- Files exist: `src/plugins/error.rs`, `src/plugins/config.rs`, `src/plugins/host/loader.rs`, `src/plugins/host/testing.rs`, `src/cli.rs`, `src/commands/plugins.rs`, `tests/plugins_cli.rs`.
- Commits exist and are ancestors of HEAD: `701b1cf`, `6e5ad75`.
- `cargo test -p filimon`: 330 passed, 0 failed (31 ignored); `--test plugins_cli`: 12 passed (2 ignored); `cargo test -p filimon -- --ignored wasm_check`: 4 passed; clippy shows no new line for the plan's gate files; `Cargo.toml` and `Cargo.lock` unchanged.
