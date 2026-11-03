---
phase: 01-plugin-contract-configuration
plan: 02
subsystem: config
tags: [rust, toml, dotenvy, thiserror, miette, secrets, plugins]

# Dependency graph
requires:
  - phase: baseline
    provides: "fluvio-wise baseline commit 12c57b9 (clean Cargo.toml, src/main.rs, src/cli.rs; tracked Cargo.lock)"
provides:
  - "plugins::config::load_registry / load_registry_with_env -> LoadedRegistry (BTreeMap sources/fetchers, registry-relative paths)"
  - "Typed HostConfig (poll interval required; timeout 30s; backoff 60s/3600s/2.0; allowlist default empty) and settings_toml re-serialised [settings]"
  - "plugins::secrets: redacting Secret newtype, read_dotenv (no process-env mutation, line-number-only errors), resolve_secrets keyed by alias"
  - "plugins::error: ConfigError (RegistryUnreadable, RegistryMalformed, PluginFileMissing, PluginFileMalformed, MissingSecret, EmptySecret, DotenvInvalid) and ConfigErrors { #[related] problems } without a diagnostic code"
  - "Root Cargo.toml: toml 1, dotenvy 0.15, thiserror 2; [workspace] exclude = [\"plugins\"]"
affects: [01-03, 01-04, phase-3-host, phase-4-cli]

# Actuals (#2632)
actuals:
  tokens: 11300
  tasks: 2
  commits: 4
plan_head_before: 12c57b99af258d9c4a6523b767e795cdd41f854c
plan_head_after: 08749a8bcb757ce5c38f3a1e3ea3bed69f316736

# Tech tracking
tech-stack:
  added: [toml 1, dotenvy 0.15, thiserror 2]
  patterns:
    - "Collect-all loading: every problem pushed into Vec<ConfigError>, Err(ConfigErrors) when non-empty, never a partial LoadedRegistry"
    - "Single read per file: parse result and NamedSource built from the same in-memory String"
    - "Injectable env lookup (&dyn Fn(&str) -> Option<String>) instead of mutating the process environment"
    - "Redacting Secret newtype: Debug prints Secret(***), no Display/Serialize, value only via expose()"

key-files:
  created:
    - src/plugins/mod.rs
    - src/plugins/config.rs
    - src/plugins/secrets.rs
    - src/plugins/error.rs
  modified:
    - Cargo.toml
    - Cargo.lock
    - src/main.rs

key-decisions:
  - "read_dotenv reads .env once with std::fs::read_to_string and parses it with dotenvy::from_read_iter over that text (instead of from_path_iter), so the line-number lookup uses the same bytes dotenvy parsed; NotFound maps to an empty map"
  - "DotenvInvalid line number = first file line containing the first line of dotenvy's LineParse text, because dotenvy reports only the value part for value errors"
  - ".env is read only when at least one enabled, successfully parsed source has a non-empty [host.secrets]"
  - "Scoped #[allow(clippy::result_large_err, reason = ...)] on read_dotenv and read_plugin: cold one-shot config path, plan-mandated signature, ConfigError carries its NamedSource"
  - "ConfigErrors deliberately has no diagnostic code (matches plan 01-03's rendering contract)"

patterns-established:
  - "Module header divider block + section dividers in every src/plugins file"
  - "TempDir fixtures + injected env closure for all config tests; .env fixtures are never committed"

requirements-completed: [CONF-01, CONF-02]

coverage:
  - id: D1
    description: "filimon.toml + per-plugin TOML load into a typed LoadedRegistry with registry-relative paths, typed [host] defaults, settings_toml, and disabled sources listed but unread"
    requirement: "CONF-01"
    verification:
      - kind: unit
        ref: "src/plugins/config.rs#tests::loads_minimal_registry"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#tests::empty_registry_loads"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#tests::absent_settings_becomes_empty_string"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#tests::disabled_source_is_not_loaded"
        status: pass
    human_judgment: false
  - id: D2
    description: "Per-plugin secret isolation: alias-keyed, process env beats .env, empty/missing/case-mismatched secrets are named errors, values redacted, process env never mutated"
    requirement: "CONF-02"
    verification:
      - kind: unit
        ref: "src/plugins/config.rs#tests::only_aliased_secrets_resolved"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#tests::process_env_overrides_dotenv"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#tests::empty_secret_value_is_error"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#tests::env_lookup_is_case_sensitive"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#tests::malformed_dotenv_reports_line_not_content"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#tests::loading_does_not_mutate_process_env"
        status: pass
      - kind: unit
        ref: "src/plugins/secrets.rs#tests::missing_dotenv_file_is_empty"
        status: pass
      - kind: unit
        ref: "src/plugins/secrets.rs#tests::dotenv_values_parsed_by_dotenvy"
        status: pass
      - kind: unit
        ref: "src/plugins/secrets.rs#tests::secrets_iterate_in_alias_order"
        status: pass
      - kind: unit
        ref: "src/plugins/secrets.rs#tests::secret_debug_is_redacted"
        status: pass
      - kind: other
        ref: "test -d src/plugins && ! grep -rnE 'set_var|remove_var' src/plugins/"
        status: pass
    human_judgment: false
  - id: D3
    description: "Dependencies and workspace exclude added; mod plugins compiled into fili; existing suite and clippy clean for src/plugins/"
    verification:
      - kind: other
        ref: "cargo test --bin fili (20 passed)"
        status: pass
      - kind: other
        ref: "cargo clippy --bin fili --message-format=short | no src/plugins/ lines"
        status: pass
    human_judgment: false

# Metrics
duration: 13min
completed: 2026-11-03
status: complete
---

# Phase 1 Plan 02: Plugin Configuration Loading Summary

**`filimon.toml` registry, per-plugin TOML and `.env` load into a typed `LoadedRegistry` (toml 1 spans, dotenvy without env mutation, thiserror and miette diagnostics), with alias-keyed per-plugin secrets in a redacting `Secret` newtype.**

## Performance

- **Duration:** 13 min
- **Started:** 2026-11-03T09:58:49Z
- **Completed:** 2026-11-03T10:11:32Z
- **Tasks:** 2 of 2
- **Files modified:** 7 (4 created, 3 modified)

## Accomplishments
- `load_registry` / `load_registry_with_env` resolve every `component`/`config` path against the registry directory. They return `BTreeMap`-ordered sources and fetchers, and collect every problem into one `ConfigErrors` instead of stopping at the first.
- Each enabled source has a typed `HostConfig` with defaults (timeout 30 s, backoff 60 s / 3600 s / 2.0, empty allowlist) and its `[settings]` re-serialised to a TOML string (`""` when absent). Disabled sources are listed with `plugin = None`, and their files are never opened.
- Each plugin receives exactly its `[host.secrets]` aliases, keyed by alias. The process environment beats `.env`. Missing, empty and case-mismatched values become named errors labelled at the env var span. A malformed `.env` line is reported by number only. A tracing line records the env var NAMES each plugin receives.
- The `Secret` Debug output is `Secret(***)`, so derived Debug on `PluginConfig`/`LoadedRegistry` never shows values. `.env` goes into a private map, and the process environment is never modified.

## Task Commits

1. **Task 1: Tracer, registry to plugin TOML to .env to LoadedRegistry**
   - `92607d7` test(01-02): add failing tracer tests for plugin registry loading (RED)
   - `3383e84` feat(01-02): load plugin registry, plugin TOML and .env secrets into LoadedRegistry (GREEN)
2. **Task 2: Secret isolation, precedence, empty values, redaction, no env mutation**
   - `63bfc1e` test(01-02): add secret isolation, precedence and redaction tests (RED)
   - `08749a8` feat(01-02): reject empty secrets and log per-plugin env var names (GREEN)

**Plan metadata:** committed with this SUMMARY (docs(01-02)).

### RED/GREEN record
- **Task 1 RED:** the crate compiled, and all four target tests ran and failed. Each panicked at its `expect("... loads")` because the stubbed `load_registry_with_env` returned `Err(ConfigErrors { problems: [] })`. Result: `0 passed; 4 failed`. Assessment: a valid RED. Every target executed and failed on the planned "registry loads" expectation, with no compile, fixture or unrelated failure. GREEN: `4 passed`.
- **Task 2 RED:** `empty_secret_value_is_error` failed at `expect_err("an empty secret is an error")` because `ALPHA_TOKEN=` still resolved to `""`. Result: `13 passed; 1 failed`. The other nine Task 2 tests passed immediately. They are characterization tests: Task 1's specification already required isolation, precedence, case-sensitive lookup, redaction, line-only `.env` errors and no env mutation. GREEN: `14 passed`.
- `gsd-tools check tdd-red-evidence` was not run. It has no adapter for Rust libtest output, and this is a `type: execute` plan, so the plan-level TDD gate does not apply. tdd.md notes the Rust RED-commit visibility gap.

## Files Created/Modified
- `src/plugins/mod.rs`: module root; `pub mod config/error/secrets`; non-test `dead_code` allow with a reason.
- `src/plugins/config.rs`: private serde file shapes (`deny_unknown_fields`, `toml::Spanned`, `NonZeroU64`), public `LoadedRegistry`/`LoadedSource`/`PluginConfig`/`HostConfig`/`BackoffConfig`/`LoadedFetcher`, the three-pass loader, and 10 tests.
- `src/plugins/secrets.rs`: `Secret`, `ResolvedSecrets`, `read_dotenv`, `resolve_secrets`, and 4 tests.
- `src/plugins/error.rs`: `ConfigError` (7 variants, codes `fili::config::<variant_snake_case>`) and `ConfigErrors`.
- `Cargo.toml`: `toml = "1"`, `dotenvy = "0.15"`, `thiserror = "2"`, `exclude = ["plugins"]` (4 added lines, 0 deleted).
- `Cargo.lock`: resolved the new dependencies. It is tracked, so it was staged.
- `src/main.rs`: `mod plugins;` between `mod output;` and `mod scorer;` (1 added line).

## Decisions Made
- `.env` is read once (`read_to_string`) and parsed with `dotenvy::from_read_iter` over that text, so the line-number lookup and the parse use identical bytes. The plan named `from_path_iter`; behavior is the same, with one fewer file read.
- The `DotenvInvalid` line number is the first file line that contains the first line of dotenvy's `LineParse` text. dotenvy reports only the value part for value-level errors, for example `s3cr3t value`, so a line-start match would miss them.
- `.env` is opened only when some enabled, parsed source has a non-empty `[host.secrets]`.
- The `[host]` allowlist spans are dropped in `HostConfig` (`Vec<String>`). Plan 01-03's host-value validation can read them from the private `HostSection` before conversion.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Clippy `result_large_err` on `read_dotenv` and `read_plugin`**
- **Found during:** Task 1 (clippy gate)
- **Issue:** `ConfigError` exceeds clippy's 128-byte Err threshold because variants carry a `NamedSource<String>`. That produced two new `src/plugins/` warnings.
- **Fix:** Scoped `#[allow(clippy::result_large_err, reason = "cold one-shot config load; ConfigError carries its diagnostic source")]` on the two functions. `read_dotenv`'s signature is fixed by the plan, and plan 01-03 extends `ConfigError`, so boxing it now would churn both plans.
- **Files modified:** src/plugins/secrets.rs, src/plugins/config.rs
- **Verification:** clippy short output has no `src/plugins/` line.
- **Committed in:** 3383e84

**2. [Rule 3 - Blocking] Clippy `too_many_lines` on `load_registry_with_env` after rustfmt**
- **Found during:** Task 2 (clippy gate)
- **Issue:** rustfmt expanded the function to 108 lines, over pedantic's 100.
- **Fix:** Extracted the private `load_fetchers` helper. No behavior change.
- **Files modified:** src/plugins/config.rs
- **Verification:** clippy gate exit 0, and all 20 tests pass.
- **Committed in:** 08749a8

**3. [Verification adjustment] Task 1 phase-hunk acceptance checks evaluated over the task's commit range**
- **Found during:** Task 1 acceptance gate
- **Issue:** The `tdd="true"` RED/GREEN cycle split Task 1 into two commits. Cargo.toml, Cargo.lock and src/main.rs are needed to compile the RED tests, so they live in the RED commit `92607d7`. The literal `git show --numstat HEAD -- ...` checks assumed a single commit.
- **Fix:** Ran the same checks over the task range `12c57b9..3383e84` (identical to `12c57b9..HEAD` for these paths). Results: Cargo.toml `4 0`, src/main.rs `1 0`. The name list is exactly Cargo.lock, Cargo.toml, src/main.rs and src/plugins/{config,error,mod,secrets}.rs, and Cargo.lock is tracked. This proves no unrelated working-tree change was folded in.
- **Files modified:** none

---

**Total deviations:** 3 (2 Rule 3 lint blockers, 1 verification-scope adjustment)
**Impact on plan:** None on behavior or interfaces. All public items, test names and TOML keys match the plan's artifact list.

## Issues Encountered
- `cargo fmt -- --check <files>` checks the whole workspace and reports pre-existing formatting drift in `smartmodules/*` (baseline code, out of scope, not touched). I used `rustfmt --check --edition 2024 src/plugins/mod.rs` for the new module instead, and it is clean.
- The pre-existing dead-code warnings in `src/main.rs`, `src/models.rs`, `src/output.rs` and `src/streaming.rs` are baseline and out of scope. In test builds `load_registry` (the process-env wrapper) shows as unused, because the tests use `load_registry_with_env`. The non-test build allows it via the module's `dead_code` allow.

## Shared files not touched
- In worktree mode, `.planning/STATE.md`, `.planning/ROADMAP.md` and `.planning/REQUIREMENTS.md` were left for the orchestrator's post-wave writes. `requirements-completed` above lists CONF-01 and CONF-02 for it to mark. The committed-sample half of SC3 is proven in plan 01-04.

## User Setup Required
None. No external service configuration is required.

## Next Phase Readiness
- Plan 01-03 can add `UnknownFetcher`, `DisabledFetcher`, `InvalidName`, `DuplicateName` and `InvalidValue` to `ConfigError`, plus validation in `load_registry_with_env`. The current problem order is per-source plugin-file problems, then `DotenvInvalid`, then secrets. 01-03's deterministic-ordering truth requires `DotenvInvalid` before the per-source problems, so 01-03 must reorder.
- Plan 01-04 can point the `--registry` flag and the sample `examples/plugins/` at `load_registry`.

---
*Phase: 01-plugin-contract-configuration*
*Completed: 2026-11-03*
