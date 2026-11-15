---
phase: 06-host-managed-oauth
plan: 03
subsystem: auth
tags: [oauth, config, secrets, toml, validation, rust]

requires:
  - phase: 06-host-managed-oauth
    provides: "plan 06-01: OAuthSection/OAuthConfig with the four core keys, oauth_problems, PluginConfig.oauth"
  - phase: 01-plugin-contract
    provides: "Phase 1 secret rules (process env wins even when empty, then .env) and the Secret redacting type"
provides:
  - "Full [host.oauth] key set: client_secret_env, revocation_url, redirect_port (default DEFAULT_REDIRECT_PORT = 17420), authorize_params"
  - "Complete collect-all validation: https + host + no user-info on all three endpoints, port 1..=65535, scope text, standard-parameter collisions, with plugin and field named"
  - "resolve_oauth_secret and ConfigError::MissingOAuthSecret / EmptyOAuthSecret; the client secret lives only in OAuthConfig.client_secret"
  - "Secret::for_test (cfg(test)) and the full-struct auth/testing.rs oauth_config()"
affects: [06-04, 06-05, 06-06, 06-07, 06-08, 06-09]

actuals:
  tokens: 24000
  tasks: 2
  commits: 2

tech-stack:
  added: []
  patterns:
    - "oauth_problems collects every problem of the table in a fixed field order and runs from read_plugin, so the OAuthConfig is built only when the table has none"
    - "Client secret is a separate field resolved in pass 3; ResolvedSecrets (what init receives) is never touched"
    - "Pass 2 reads .env when a plugin has [host.secrets] aliases OR a client_secret_env"

key-files:
  created: []
  modified:
    - src/plugins/config.rs
    - src/plugins/secrets.rs
    - src/plugins/error.rs
    - src/plugins/auth/testing.rs

key-decisions:
  - "oauth_problems moved out of host_problems into read_plugin so the same call yields both the diagnostics and the 'table is valid' signal; problem order is unchanged because it was already last"
  - "Field order of the collected problems is: authorize_url, token_url, revocation_url, client_id, redirect_port, scopes by index, client_secret_env, authorize_params by key"
  - "One shared endpoint message for scheme, host and user-info violations ('must be an https:// URL with a host and no user-info'); it never echoes the URL, so credentials cannot leak"

patterns-established:
  - "New ConfigError variants carry env var names and paths only and are added to ConfigError::plugin()"
  - "Large-Err clippy lint avoided by mapping to Vec<ConfigError> at the call site instead of an Err-returning closure"

requirements-completed: [AUTH-01, AUTH-03]

duration: 75min
completed: 2026-11-11
status: complete
plan_head_before: e7b74c4b6959303bed731a6ffe9910241aa2f0f0
plan_head_after: 1e381c5edb27b089f9b67af7fc9f53d311b6e4db
---

# Phase 6 Plan 03: The rest of `[host.oauth]` and the client secret Summary

**`[host.oauth]` now takes `client_secret_env`, `revocation_url`, `redirect_port` (default 17420) and `authorize_params`, rejects every bad endpoint, port, scope and reserved parameter with plugin and field named in one report, and resolves the client secret through the Phase 1 `.env` rules into a field the guest's `init` can never receive.**

## Performance

- **Duration:** about 75 min (most of it a cold `cargo` build in a fresh worktree)
- **Completed:** 2026-11-11
- **Tasks:** 2
- **Files modified:** 4

## Accomplishments

- The whole D-11/D-14 key set parses under the existing `deny_unknown_fields` rule; an inline `client_secret` is a `PluginFileMalformed` error naming it (D-12).
- `https_endpoint` now also refuses user-info, applied to `authorize_url`, `token_url` and `revocation_url`; `redirect_port = 0`, scopes that are empty or contain whitespace (`host.oauth.scopes[<i>]`), an empty `client_secret_env`, and `authorize_params` keys equal (ASCII case-insensitive) to `state`, `code_challenge`, `code_challenge_method`, `redirect_uri`, `client_id`, `response_type` or `scope` are each an `InvalidValue`; all problems of one plugin come back together.
- `resolve_oauth_secret` mirrors `resolve_secrets` exactly (process value wins even when empty, then `.env`, unusable `.env` adds no false missing-secret problem). `MissingOAuthSecret` and `EmptyOAuthSecret` (codes `fili::config::missing_oauth_secret` / `empty_oauth_secret`) carry env var names and paths only and are in `ConfigError::plugin()`, so a scoped `load_plugin` keeps them for the named plugin.
- Pass 2 now reads `.env` for a plugin whose only secret is `client_secret_env`.
- The client secret goes only into `OAuthConfig.client_secret`; `PluginConfig.secrets`, `settings_toml`, Debug output and every diagnostic stay free of it (sentinel test over success output, `Debug`, `Display` and rendered miette reports).

## Task Commits

1. **Task 1: The remaining `[host.oauth]` keys and the complete validation** - `9479c69` (feat)
2. **Task 2: Resolve `client_secret_env` into a field the guest never sees** - `1e381c5` (feat)

**Plan metadata:** committed with this SUMMARY (docs: complete plan)

## TDD notes

Both tasks are `tdd="true"` inside a `type: execute` plan (no TDD gate section applies). Tests were written first for each task and run before any implementation: both runs failed to compile (task 1: `DEFAULT_REDIRECT_PORT` and the new `OAuthConfig` fields missing; task 2: `ConfigError::MissingOAuthSecret`, `EmptyOAuthSecret` and `Secret::for_test` missing), which is the RED evidence. Because a test that does not compile cannot be committed without breaking every later commit, test and implementation are one commit per task rather than a separate `test(...)` commit.

## Files Created/Modified

- `src/plugins/config.rs` - `DEFAULT_REDIRECT_PORT`, `STANDARD_AUTHORIZE_PARAMS`, full `OAuthSection`/`OAuthConfig`, extended `oauth_problems`, `build_oauth`, pass-2 trigger, secret resolution in `finish_plugin`, 18 new tests
- `src/plugins/secrets.rs` - `resolve_oauth_secret`, `#[cfg(test)] Secret::for_test` and its test
- `src/plugins/error.rs` - `MissingOAuthSecret`, `EmptyOAuthSecret`, both in `plugin()`
- `src/plugins/auth/testing.rs` - `oauth_config()` returns the full struct

## Decisions Made

See `key-decisions` above. `Cargo.toml`, `Cargo.lock`, `wit/` and `examples/` are untouched.

## Deviations from Plan

None - plan executed exactly as written.

Notes that are not deviations:
- The plan's task 1 verify lists `oauth_table_loads_into_the_plugin_config` and `oauth_endpoints_must_be_https_naming_plugin_and_field` from plan 06-01; both still report `ok`.
- The userinfo test first asserted the message does not contain `user`, which the rule text "no user-info" itself contains; the assertion was narrowed to the credential forms (`pw`, `user@`, `user:`) before the first commit (test bug, no production change).
- The existing `config_error_plugin_names_every_per_plugin_variant` test would have crossed clippy's 100-line limit with two more variants, so the two new variants have their own test (`config_error_plugin_names_the_oauth_secret_variants`).

## Issues Encountered

- The repository is not `cargo fmt` clean at baseline; `rustfmt` was applied to the touched files and the five unrelated pre-existing hunks it reformatted in `config.rs` and `secrets.rs` were put back, so the diff holds only this plan's changes.

## Known Stubs

None. `OAuthConfig`'s new fields are read by later plans of this phase; they sit under 06-01's existing `cfg_attr(not(test), allow(dead_code))`.

## Threat Flags

None. Every new surface (endpoint URLs, ports, parameters, the client secret) is covered by T-06-09 to T-06-12 in the plan's threat model.

## User Setup Required

None.

## Next Phase Readiness

- Plans 06-04 to 06-09 can read `OAuthConfig.redirect_port`, `revocation_url`, `authorize_params` and `client_secret` directly; `Secret::for_test` is available for their tests.
- The default port 17420 is documented in the `DEFAULT_REDIRECT_PORT` doc comment; plan 06-09's README section must repeat that a provider app registers `http://127.0.0.1:17420/callback`.

## Verification

- `cargo test -p filimon`: 474 + 12 passed, 0 failed (62 + 3 ignored, pre-existing).
- All 19 tests named in the plan's verify commands (11 in task 1 incl. plan 06-01's two, 8 in task 2) report `ok` in `plugins::config::tests`.
- `cargo clippy -p filimon --all-targets`: `config.rs` and `secrets.rs` at their 3 baseline warnings each, none in `error.rs` or `auth/`.
- `git status`: no change to `Cargo.toml`, `Cargo.lock`, `wit/`, `examples/`.
- `awk` count of `OAuthSecret` in `ConfigError::plugin()` is 2.

## Self-Check: PASSED

- FOUND: src/plugins/config.rs, src/plugins/secrets.rs, src/plugins/error.rs, src/plugins/auth/testing.rs
- FOUND commits: 9479c69, 1e381c5 (ancestors of HEAD)

---
*Phase: 06-host-managed-oauth*
*Completed: 2026-11-11*
