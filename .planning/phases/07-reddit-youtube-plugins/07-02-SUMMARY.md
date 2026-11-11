---
phase: 07-reddit-youtube-plugins
plan: 02
subsystem: plugins-auth
tags: [oauth, client_credentials, toml, config-validation, rust]

requires:
  - phase: 06-host-managed-oauth
    provides: "[host.oauth] table, OAuthConfig, resolve_oauth_secret, PKCE login flow"
provides:
  - "OAuthGrant enum and OAuthConfig.grant ([host.oauth] grant = authorization_code | client_credentials)"
  - "OAuthConfig.authorize_url as Option<Url> (Some exactly for the authorization-code grant)"
  - "OAuthConfig.user_agent (optional, 1-200 printable ASCII) for the host's token requests"
  - "AuthError::GrantHasNoLogin (fili::auth::no_login) and a login that refuses client_credentials before binding a port"
  - "auth/testing.rs client_credentials_config_for helper for plan 07-03"
affects: [07-03, 07-10, 07-11]

actuals:
  tokens: 8500
  tasks: 2
  commits: 4

tech-stack:
  added: []
  patterns:
    - "Grant-aware validation: grant-independent checks first, then grant_problems; every diagnostic is fixed text that never echoes the value"
    - "Narrow-first published TOML shape: a client_credentials table refuses code-flow keys instead of ignoring them"

key-files:
  created: []
  modified:
    - src/plugins/config.rs
    - src/plugins/auth/error.rs
    - src/plugins/auth/login.rs
    - src/plugins/auth/pkce.rs
    - src/plugins/auth/testing.rs

key-decisions:
  - "client_credentials tables are narrow: only token_url, client_id, client_secret_env (required), optional user_agent and settings; authorize_url, redirect_port, authorize_params, revocation_url and scopes are named config errors"
  - "Grant-specific problems are appended after the grant-independent ones, except an unknown grant which is reported first"
  - "The client secret stays in OAuthConfig::client_secret; the existing resolve_oauth_secret path is reused unchanged so it never reaches PluginConfig::secrets"

requirements-completed: [RDDT-01, RDDT-03]

duration: 40min
completed: 2026-11-11
status: complete
plan_head_before: 92801e152e0001a786fe312829139497e0131ab4
plan_head_after: d5579d19d3b9833c0e778cba10f7e8f3791612f4
---

# Phase 7 Plan 02: client_credentials grant and user_agent in [host.oauth] Summary

**`[host.oauth]` now accepts `grant = "client_credentials"` (app-only, no login, host-held secret) and an optional validated `user_agent`, with named never-echoing diagnostics and every existing plugin TOML loading unchanged.**

## Performance

- **Duration:** about 40 min
- **Tasks:** 2 (both TDD, 4 commits)
- **Files modified:** 5 (all under `src/plugins/`)

## Accomplishments

- Typed grant: `OAuthGrant`, `OAuthSection.grant`, `OAuthConfig.grant`; an absent key or `authorization_code` keeps the Phase 6 flow exactly.
- `authorize_url` is optional in the file and `Option<Url>` in `OAuthConfig`; a code-flow table without it is now an `InvalidValue` naming `host.oauth.authorize_url` (previously a serde missing-field error).
- A `client_credentials` table requires `client_secret_env` and rejects `authorize_url`, `redirect_port`, `authorize_params`, `revocation_url` and `scopes`, each named, in that order.
- `login` returns `AuthError::GrantHasNoLogin` as its first statement (proved with a held port: the error is not `RedirectPortInUse`), and a `None` authorize URL maps to the same error instead of panicking.
- `[host.oauth] user_agent`: 1 to 200 printable ASCII, for both grants, loaded into `OAuthConfig::user_agent` (consumer: plan 07-03).
- Pinned that a client_credentials secret never enters `PluginConfig::secrets`, `settings_toml` or Debug output.

## Task Commits

| Task | Phase | Commit | Description |
| ---- | ----- | ------ | ----------- |
| 1 | RED | 095fa20 | failing grant and login tests plus compile scaffold |
| 1 | GREEN | 6b2faf3 | grant-aware validation, build_oauth, login refusal |
| 2 | RED | 0601862 | failing user_agent tests plus compile scaffold, secret isolation test |
| 2 | GREEN | d5579d1 | user_agent validation and loading |

## TDD Gate Compliance

RED then GREEN commits exist for both tasks (`test(07-02)` precedes `feat(07-02)`).
RED evidence (semantic assessment): every target test executed and failed on the planned behavior, not on a build error. The RED commits carry a small compile scaffold (the new type surface set to defaults) so the tests build; without it a missing symbol would be a build failure, not RED. Task 1 RED: the six config tests failed because `grant` was an unknown field or `authorize_url` a missing field (`PluginFileMalformed` instead of the planned result), and the login test failed with `RedirectPortInUse` where `GrantHasNoLogin` was expected. Task 2 RED: the three `user_agent` tests failed on `unknown field user_agent`. The `gsd_run check tdd-red-evidence` classifier was not run (the cargo human-readable output is not one of its supported formats); the semantic inspection was done by hand.
`client_credentials_secret_is_a_client_secret_not_a_plugin_secret` passed at RED by design: it pins isolation behavior that Task 1 already provides (the plan names it a Task 2 test), so it is a regression pin, not a driver of new behavior.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Existing config tests used the non-Option `authorize_url`**
- **Found during:** Task 1 scaffold
- **Issue:** `oauth_table_loads_into_the_plugin_config` and `oauth_endpoints_must_be_https_naming_plugin_and_field` called `.as_str()` / `.scheme()` on `authorize_url`, which is now `Option<Url>`.
- **Fix:** read through `.as_ref().map(...)`; assertions unchanged in meaning.
- **Files modified:** src/plugins/config.rs
- **Commit:** 095fa20

**2. [Rule 3 - Blocking] `#[allow(dead_code)]` on `OAuthConfig::user_agent`**
- **Found during:** Task 2 clippy
- **Issue:** the field is loaded and validated here but only read by plan 07-03, so clippy warned `field is never read`.
- **Fix:** `#[allow(dead_code, reason = ...)]` naming plan 07-03, the repo's existing pattern for a staged consumer. Remove it when 07-03 lands.
- **Files modified:** src/plugins/config.rs
- **Commit:** d5579d1

**3. [Style] rustfmt of the new config.rs tests**
- The Task 1 tests were formatted in the Task 2 GREEN commit; pre-existing unformatted hunks in the repo were left as they were.

**Total deviations:** 3 (2 blocking fixes, 1 style). **Impact:** none on behavior or the published TOML shape.

## Issues Encountered

- `cargo clippy` reports three pre-existing `Duration` readability warnings in `src/plugins/config.rs` tests (around lines 1489-1492, not touched by this plan), so the plan's "no diagnostic for config.rs" check is not literally zero at HEAD. Out of scope; no diagnostic comes from lines this plan changed.
- `src/commands/auth.rs` needed no change: its `OAuthConfig` literal uses struct-update from `oauth_config_for`, which now carries the new fields.

## Known Stubs

None.

## Threat Flags

None beyond the plan's register. T-07-06 (secret isolation), T-07-07 (header injection via user_agent), T-07-08 (https endpoints) and T-07-09 (grant confusion) are mitigated by the tests named in the plan. T-07-SC: `git diff 75b078e -- wit Cargo.toml Cargo.lock src/commands/auth.rs` is empty.

## Verification

- `cargo test -p filimon` : 643 + 10 + 12 passed, 0 failed.
- Every test named in both tasks reports `ok` (the plan's `<verify>` loops, run for both tasks).
- All grep acceptance criteria pass.

## Next Phase Readiness

Plan 07-03 can read `OAuthConfig::grant`, `user_agent` and `client_secret`, and use `client_credentials_config_for`. Plan 07-10 owns the CLI refusal and `app-only` status row; plan 07-11 writes sample TOMLs against this shape.

## Self-Check: PASSED

- src/plugins/config.rs, error.rs, login.rs, pkce.rs, testing.rs exist and contain the named symbols.
- Commits 095fa20, 6b2faf3, 0601862, d5579d1 are ancestors of HEAD.
