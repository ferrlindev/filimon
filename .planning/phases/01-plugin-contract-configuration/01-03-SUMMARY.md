---
phase: 01-plugin-contract-configuration
plan: 03
subsystem: config
tags: [rust, miette, toml, url, diagnostics, plugins, validation]

# Dependency graph
requires:
  - phase: 01-plugin-contract-configuration
    provides: "01-02 loader: load_registry_with_env, ConfigError/ConfigErrors, toml::Spanned file shapes, resolve_secrets"
provides:
  - "ConfigError variants UnknownFetcher, DisabledFetcher, InvalidName, DuplicateName, InvalidValue (codes fili::config::unknown_fetcher / disabled_fetcher / invalid_name / duplicate_name / invalid_value)"
  - "Registry-level validation: plugin-name charset [a-z0-9_-]+, name uniqueness across [sources]/[fetchers], fetcher reference defined and enabled (enabled sources only)"
  - "Plugin-file validation: host.allowlist[i] via url::Host::parse, backoff multiplier finite and >= 1.0, max_secs >= initial_secs, each labelled at the TOML value"
  - "Deterministic collect-all order: InvalidName (sources, fetchers), DuplicateName, DotenvInvalid, then per enabled source in name order (fetcher ref, plugin file, host values, secrets)"
  - "Test helper render(&ConfigErrors) (GraphicalReportHandler, unicode_nocolor, width 400) plus 19 diagnostic tests"
affects: [01-04, phase-4-cli, phase-5-allowlist]

# Actuals (#2632)
actuals:
  tokens: 9150
  tasks: 3
  commits: 6
plan_head_before: a4cb7be11d7daeea01bf4106c46945e595dcfdc7
plan_head_after: a67a9ef

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Problem buckets: registry-level Vec first, then BTreeMap<source name, Vec<ConfigError>> flattened in name order, so report order never depends on pass order"
    - "Optional spanned values: Option<toml::Spanned<T>> with accessor defaults, so a range diagnostic labels a value only when the operator wrote it"
    - "Render-based diagnostic tests: assert on the full miette graphical report, not just Display"

key-files:
  created: []
  modified:
    - src/plugins/error.rs
    - src/plugins/config.rs

key-decisions:
  - "Invalid-name sources are reported once (InvalidName) and skipped by every later pass via loadable(), so a bad name never produces follow-on file or secret errors"
  - "DuplicateName is checked over all names, valid or not; duplicated entries still load, so their own problems are reported too"
  - "InvalidName carries no source span: the toml crate's BTreeMap keys are plain Strings and the message already names `<table>.<name>`"
  - "Backoff fields became Option<toml::Spanned<..>> (defaults applied by accessors); when max_secs is absent and an explicit initial_secs exceeds the default max, the max_secs diagnostic labels initial_secs"
  - "Host range problems do not stop secret resolution, so one run reports both a bad allowlist entry and a missing secret for the same plugin"
  - "Problem ordering was implemented in Task 2 (needed for InvalidName-first); Task 3's collects_all_problems therefore passed at its RED commit as a characterization test"

patterns-established:
  - "Every fili::config:: code maps to exactly one problem; ConfigErrors stays code-free"
  - "RegistrySource { name, text } copies into each diagnostic needing a registry snippet"

requirements-completed: [CONF-03]

coverage:
  - id: D1
    description: "Unknown or disabled fetcher reference from an enabled source is a diagnostic naming sources.<name>.fetcher with a registry span; disabled sources are exempt"
    requirement: "CONF-03"
    verification:
      - kind: unit
        ref: "src/plugins/config.rs#tests::unknown_fetcher"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#tests::disabled_fetcher"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#tests::unknown_fetcher_on_disabled_source_ignored"
        status: pass
    human_judgment: false
  - id: D2
    description: "Registry-level diagnostics: unreadable/malformed registry (syntax, unknown key, duplicate table), missing plugin file citing sources.<name>.config, invalid plugin names, names in both tables"
    requirement: "CONF-03"
    verification:
      - kind: unit
        ref: "src/plugins/config.rs#tests::registry_missing"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#tests::registry_malformed"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#tests::registry_unknown_key"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#tests::duplicate_table_is_malformed"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#tests::missing_plugin_file"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#tests::invalid_plugin_name"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#tests::duplicate_plugin_name"
        status: pass
    human_judgment: false
  - id: D3
    description: "Plugin-file diagnostics naming plugin and field: malformed TOML, unknown [host] key, zero poll interval, missing [host], non-host allowlist entry, bad backoff multiplier / max_secs, missing secret labelled 'declared here'"
    requirement: "CONF-03"
    verification:
      - kind: unit
        ref: "src/plugins/config.rs#tests::malformed_plugin_toml"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#tests::unknown_host_key"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#tests::zero_poll_interval"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#tests::missing_host_table"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#tests::invalid_allowlist_entry"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#tests::invalid_backoff"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#tests::missing_secret"
        status: pass
    human_judgment: false
  - id: D4
    description: "Collect-all, fail-closed report: 5 problems across 4 plugins in a stable order, one fili::config:: diagnostic each; the rendered report never contains a secret value from .env or the process env"
    requirement: "CONF-03"
    verification:
      - kind: unit
        ref: "src/plugins/config.rs#tests::collects_all_problems"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#tests::rendered_report_omits_secret_values"
        status: pass
      - kind: other
        ref: "cargo test --bin fili (39 passed)"
        status: pass
      - kind: other
        ref: "cargo clippy --bin fili --message-format=short | no src/plugins/ lines"
        status: pass
    human_judgment: false

# Metrics
duration: 12min
completed: 2026-11-03
status: complete
---

# Phase 1 Plan 03: Plugin Configuration Diagnostics Summary

**Every plugin configuration problem is now a miette diagnostic that names the plugin and the `sources.<name>.*` / `host.*` field, with a TOML span where one exists. Problems are collected in a fixed registry-then-per-source order into one report, and loading fails closed. New checks cover fetcher references, name rules, `url::Host::parse` allowlist validation and backoff bounds.**

## Performance

- **Duration:** about 12 min
- **Started:** 2026-11-03T10:14Z (approximate; base commit time)
- **Completed:** 2026-11-03T10:26Z
- **Tasks:** 3 of 3
- **Files modified:** 2

## Accomplishments
- An enabled source whose `fetcher` is undefined or disabled produces `UnknownFetcher` or `DisabledFetcher`. The diagnostic is labelled at the registry value, and its help lists the defined fetchers. Disabled sources are not checked.
- Plugin names must match `[a-z0-9_-]+`, giving `InvalidName` for sources then fetchers. A name used under both tables gives `DuplicateName`. An invalid-name source is reported once and never loaded further.
- Each `[host]` allowlist entry must pass `url::Host::parse`. The backoff multiplier must be finite and >= 1.0, and `max_secs` must be >= `initial_secs`. Each failure is an `InvalidValue` naming `host.allowlist[i]` or `host.backoff.*`, labelled at the value.
- Problem order is deterministic and documented: InvalidName, DuplicateName, DotenvInvalid, then per enabled source in name order (fetcher ref, plugin file, host values, secrets). This replaces 01-02's pass-dependent order, as the 01-02 executor flagged.
- The render tests prove one `fili::config::` diagnostic per problem and the `N problem(s)` header. Secret values from `.env` and from the process env never appear in the rendered report or the Debug output.

## Task Commits

1. **Task 1 (tracer): unknown fetcher, then a rendered diagnostic**
   - `7213ee3` test(01-03): add failing unknown_fetcher diagnostic test (RED)
   - `a82efaa` feat(01-03): report unknown fetcher references with a registry span (GREEN)
2. **Task 2: registry-level diagnostics**
   - `26ab7a0` test(01-03): add registry-level diagnostic tests (RED)
   - `df9cb35` feat(01-03): validate plugin names, cross-table duplicates and disabled fetchers (GREEN)
3. **Task 3: plugin-file diagnostics, collect-all, no secret values**
   - `68fed3c` test(01-03): add plugin-file, collect-all and secret-redaction diagnostic tests (RED)
   - `a67a9ef` feat(01-03): range-check [host] allowlist and backoff values (GREEN)

**Plan metadata:** committed with this SUMMARY (docs(01-03)).

### RED/GREEN record
- **Task 1 RED:** `unknown_fetcher` compiled, ran and panicked at `expect_err("an unknown fetcher is an error")`, because loading returned `Ok(LoadedRegistry)`. Result: `0 passed; 1 failed`. Assessment: a valid RED. The target failed on the planned behavior, with no compile, fixture or unrelated failure. GREEN: `1 passed`, with all 15 plugin tests green.
- **Tracer gate:** interactive run, `human_verify_mode = end-of-phase`, `<verify>` automated-only. I re-ran the verify command and got `1 passed`, then expanded.
- **Task 2 RED:** `disabled_fetcher` and `duplicate_plugin_name` panicked at `expect_err` (the load returned Ok). `invalid_plugin_name` failed `left: 1, right: 2`: the bad-name source produced only a `PluginFileMissing`, and no `InvalidName`s. Result: `17 passed; 3 failed`. The other six Task 2 tests are characterization tests (`registry_missing`, `registry_malformed`, `registry_unknown_key`, `duplicate_table_is_malformed`, `missing_plugin_file`, `unknown_fetcher_on_disabled_source_ignored`). They passed at RED because 01-02 already produced those diagnostics with the required field paths. GREEN: `24 passed`.
- **Task 3 RED:** `invalid_allowlist_entry` and `invalid_backoff` panicked at `expect_err` (the load returned Ok). Result: `31 passed; 2 failed`. `malformed_plugin_toml`, `unknown_host_key`, `zero_poll_interval`, `missing_host_table` and `missing_secret` pin 01-02 behavior. `collects_all_problems` passed because Task 2 GREEN already introduced the ordering. `rendered_report_omits_secret_values` pins redaction. GREEN: `33 passed` (plugins), `39 passed` (whole binary).
- `gsd-tools check tdd-red-evidence` was not run. It has no adapter for Rust libtest output, and this is a `type: execute` plan, so the plan-level TDD gate does not apply. tdd.md documents the Rust RED-commit visibility gap.

## Files Created/Modified
- `src/plugins/error.rs`: five new `ConfigError` variants. `ConfigErrors` is unchanged and still has no code.
- `src/plugins/config.rs`:
  - `RegistrySource`, `is_valid_name`, `loadable` and `registry_level_problems`.
  - `fetcher_problem`, now handling Unknown and Disabled.
  - `host_problems` and `finish_plugin`.
  - Backoff fields are now `Option<toml::Spanned<..>>`.
  - The loader collects into a registry-level Vec plus per-source buckets.
  - The `render` test helper and 19 new tests.

## Decisions Made
See `key-decisions` in the frontmatter. In short:
- Invalid-name sources are skipped after reporting.
- Duplicates still load.
- InvalidName has no span.
- Backoff spans are labelled only when the operator wrote the value.
- Host range problems do not stop secret resolution.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Clippy `too_many_lines` on `load_registry_with_env` after the bucket restructure**
- **Found during:** Task 2 (clippy check before the GREEN commit)
- **Issue:** The function reached 110 lines, over pedantic's 100. That added a new `src/plugins/` clippy line, which the Task 3 gate forbids.
- **Fix:** Moved the per-source secret resolution and the `PluginConfig` assembly into the private helper `finish_plugin`. Behavior is unchanged, and the tracing line still records env var names only.
- **Files modified:** src/plugins/config.rs
- **Verification:** The clippy short output has no `src/plugins/` lines, and all tests pass.
- **Committed in:** df9cb35

**2. [Sequencing] Deterministic problem order landed in Task 2 rather than Task 3**
- **Found during:** Task 2
- **Issue:** InvalidName must come before every per-source problem, so the registry-level/per-source bucket split was needed to make `invalid_plugin_name` pass.
- **Fix:** I implemented the full ordering in Task 2 GREEN. Task 3 added only `InvalidValue` and the host checks into the existing bucket. As a result, `collects_all_problems` was a characterization test at Task 3 RED.
- **Files modified:** src/plugins/config.rs
- **Committed in:** df9cb35

---

**Total deviations:** 2 (1 Rule 3 lint blocker, 1 task-sequencing note)
**Impact on plan:** None on behavior, interfaces or test names. The signatures of `load_registry`, `load_registry_with_env` and `resolve_secrets` are unchanged.

## Issues Encountered
- The worktree sandbox rejects compound shell commands that combine `git` with variables or loops. I ran the commit guards and per-test checks as plain separate commands instead. The HEAD branch and base assertions passed at spawn: branch `worktree-agent-ac5a37cae4ad7e307`, base `a4cb7be`.
- I ran `rustfmt --edition 2024` on the two touched files. `rustfmt --check` on `src/plugins/mod.rs` is clean.

## Shared files not touched
- In worktree mode, `.planning/STATE.md`, `.planning/ROADMAP.md` and `.planning/REQUIREMENTS.md` were left for the orchestrator's post-wave writes. `requirements-completed` lists CONF-03 for it to mark.
- No 01-04 files were touched (`examples/plugins/*`, `src/plugins/mod.rs`, `src/cli.rs`).

## User Setup Required
None. No external service configuration is required.

## Next Phase Readiness
- Plan 01-04's sample files must satisfy the new rules: names in `[a-z0-9_-]`, unique across tables, every fetcher reference defined and enabled, bare-host allowlist entries, and backoff max >= initial with multiplier >= 1.0. The coupling note in the plan frontmatter already states this.
- Phase 4's `fili plugins check` can render `ConfigErrors` directly. Every problem already carries its code, field path and span.

---
*Phase: 01-plugin-contract-configuration*
*Completed: 2026-11-03*
