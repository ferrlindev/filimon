---
phase: 07-reddit-youtube-plugins
plan: 03
subsystem: plugins/auth
tags: [oauth, client_credentials, reddit, token-provider, user-agent]
requires:
  - phase: 07-02
    provides: "[host.oauth] client_credentials grant and user_agent config, client_credentials_config_for test helper"
provides:
  - "TokenClient::client_credentials (Basic auth, grant_type-only form, answer classification)"
  - "Configurable User-Agent for host token and revocation requests"
  - "In-memory single-flight app token cache in TokenProvider"
affects: [07-10, 07-11]
tech-stack:
  added: []
  patterns:
    - "tokio mutex held across the request as the single-flight guard"
    - "app grant branches inside TokenProvider::access_token, so scheduler, run-once and WIT are untouched"
key-files:
  created: []
  modified:
    - src/plugins/auth/client.rs
    - src/plugins/auth/provider.rs
key-decisions:
  - "App token cached in memory only; no token file or auth directory for the client_credentials grant (Open Question 4)"
  - "A 2xx answer carrying an OAuth error body is Rejected, not Transient"
  - "Permanent refresh failures cannot happen for an app grant but are mapped like Transient/Rejected so the provider never returns NeedsLogin"
requirements-completed: [RDDT-01, RDDT-03]
status: complete
duration: 40min
completed: 2026-11-11
commits: 4
plan_head_before: 964becf69497d827360fbe1065467f229750f784
plan_head_after: 76556bee944334288ba030a265fd3ad5ed851806
actuals:
  tokens: 21000
  tasks: 2
  commits: 4
---

# Phase 7 Plan 03: client_credentials app token Summary

**Host-side Reddit app token: Basic-auth `client_credentials` request, in-memory single-flight cache with a 60 s renewal margin, and an operator-set User-Agent for token and revocation calls.**

## What was built

- `TokenClient::client_credentials` posts exactly `grant_type=client_credentials` through the existing `post_form` (HTTP Basic, `Accept: application/json`, no secret in the body). Transport and oversize failures are `Transient`; a 2xx without a usable token is `Rejected` when the body carries an OAuth `error` string, else `Transient`; non-success statuses reuse `classify_refresh_error` unchanged (401 is `Rejected("HTTP 401")`, 5xx/408/429 are `Transient`).
- `TokenClient::build` takes `oauth.user_agent` when set, else `fili/<version>`. Because the same client sends revocations, the User-Agent applies to both.
- `TokenProvider` gained `app_token: tokio::sync::Mutex<Option<AppToken>>` and a branch at the top of `access_token` for `OAuthGrant::ClientCredentials`. A cached token with at least 60 s left is reused; otherwise one request is made under the lock. A missing `expires_in` means 3600 s. A failed refetch keeps a still-valid cached token (one `tracing::warn!`), else returns `RefreshFailed("the app token request failed: <cleaned text>")`. The grant never returns `NotLoggedIn`, `NeedsLogin` or `FileProblem`, and never touches the token store.
- Module header documents the second decision table.

## TDD gate compliance

- Task 1 RED `6f2ff81` (stub method returning Transient, 8 of 9 new client tests fail on their planned assertions; the "never carry the secret" test passes trivially against the stub), GREEN `10c21c4`.
- Task 2 RED `0a44812` (no stub needed: tests compile against existing APIs; 10 new tests fail on their assertions, e.g. NotLoggedIn instead of a token), GREEN `76556be`.
- The RED evidence was inspected by hand from `cargo test` output (the repo's Rust test report is not in a format the `tdd-red-evidence` classifier supports).
- No REFACTOR commit.

## Verification

- `cargo test -p filimon plugins::auth`: 136 passed. `cargo test -p filimon` (all targets): 662 + 10 + 12 passed, 0 failed (includes `plugins::host`; scheduler untouched).
- All test names listed in both tasks report `ok`, as do the pre-existing `the_request_carries_accept_json_and_a_fili_user_agent`, `exchange_with_a_secret_uses_basic_auth_and_no_body_secret`, `two_concurrent_calls_make_one_refresh_request`, `a_token_with_59_seconds_left_is_refreshed_and_persisted`.
- `cargo clippy -p filimon --all-targets` shows no diagnostic under `src/plugins/auth/` after Task 2.
- `git diff 75b078e -- wit Cargo.toml Cargo.lock src/plugins/host/scheduler.rs` is empty.

## Deviations from Plan

**1. [Test adjustment] Failed-refetch test clock 1040 became 1050**
- **Found during:** Task 2 tests
- **Issue:** with `expires_in` 100 (expires 1100), clock 1040 leaves exactly 60 s, which is the margin: the cache is reused with no request, so the failure path would not run.
- **Fix:** the test uses clock 1050 (50 s left, inside the margin) and asserts a request was made and the cached token is still returned; clock 1101 still gives `RefreshFailed`.
- **Files modified:** src/plugins/auth/provider.rs
- **Commit:** 0a44812

**2. [Transient state] Task 1 clippy criterion**
- After Task 1 alone, clippy warned `client_credentials` is never used (dead code, `src/plugins/auth/client.rs`). It disappears once Task 2 calls it; the final clippy run is clean for `src/plugins/auth/`.

**3. [Process] `cargo fmt` side effect**
- A whole-crate `cargo fmt` reformatted many unrelated files because the repo is not rustfmt-clean. They were reverted file by file with `git checkout -- <file>`; only the two plan files are modified. The two files are not rustfmt-clean against the installed rustfmt either (a few pre-existing lines); formatting was left as the repo has it.

## Known Stubs

None.

## Threat Flags

None. The only new outbound request is the token POST already covered by T-07-10 to T-07-14; mitigations are tested (secret and body sentinels absent from errors and Debug, redirect not followed, one request under a lock, no `auth` directory, stale login file ignored).

## Self-Check: PASSED

- FOUND: src/plugins/auth/client.rs, src/plugins/auth/provider.rs
- FOUND commits: 6f2ff81, 10c21c4, 0a44812, 76556be (ancestors of HEAD)
