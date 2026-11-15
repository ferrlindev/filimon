---
phase: 06-host-managed-oauth
plan: 06
subsystem: auth
tags: [oauth, refresh, token-provider, locking, scheduler, rust]

requires:
  - phase: 06-host-managed-oauth
    provides: "TokenProvider, TokenOutcome, with_clock, auth_for_poll and CycleOutcome::TokenRefresh (06-01); TokenStore lock, mark_needs_login, read, write (06-02); TokenClient::refresh, RefreshFailure, DEFAULT_EXPIRES_IN_SECS (06-05)"
provides:
  - "TokenProvider::access_token refreshes a due token lazily before a poll: in-process gate, per-alias lock, re-read, refresh, merge, atomic write, needs_login marking"
  - "TokenOutcome::RefreshFailed(String) replaces the interim refresh-due variant"
  - "auth_for_poll maps RefreshFailed to CycleOutcome::TokenRefresh, so the Phase 3 backoff applies in the loop and in run-once"
affects: [06-07, 06-08, 06-09]

status: complete
plan_head_before: 1826ed9396a455e35806e3e5d5178a83bb216549
plan_head_after: 97e9c44d591e035da5abddd823772dc2071d2b09

actuals:
  tokens: 10800
  tasks: 2
  commits: 3

tech-stack:
  added: []
  patterns:
    - "Gate (tokio Mutex) plus file lock around read, refresh and write, with a re-read after taking the lock"
    - "Persist the refreshed login before its access token is returned; a failed save returns no token"
    - "Failure classes decide the outcome: Permanent marks needs_login, Transient and Rejected never do"

key-files:
  created: []
  modified:
    - src/plugins/auth/provider.rs
    - src/plugins/host/scheduler.rs
    - src/plugins/host/once.rs
    - src/commands/plugins.rs

key-decisions:
  - "The lock and the gate are held across the token request, so a login started during a refresh waits for it (up to the lock's 30 s wait) and is never overwritten by it"
  - "A transient or rejected failure with a still-valid access token hands over the current token and logs one warning; with an expired one the poll is skipped as a transient cycle"
  - "An expired login with no refresh token is marked needs_login under the lock after a re-read; a still-valid one is used as it is"

patterns-established:
  - "Lazy refresh before poll, no background timer (D-07)"

requirements-completed: [AUTH-02, AUTH-03]

duration: 60min
completed: 2026-11-11
---

# Phase 6 Plan 06: Refresh Path Summary

**Lazy OAuth refresh before every poll: an in-process gate plus the per-alias lock, a re-read after locking, the new login saved atomically before its token is used, and D-09 failure classes wired into the scheduler and `fili plugins run-once`.**

## What was built

- `provider.rs`: `access_token` re-reads the file each call and returns a token with at least 60 seconds left unchanged. Under that it calls `renew`, which takes the gate and the per-alias lock, re-reads, and only refreshes if the file is still due. `persist` merges the answer (kept refresh token when omitted, rotated one when sent, 3600 s default lifetime, saturating arithmetic) and writes before returning the token. A save failure returns `RefreshFailed("the refreshed login could not be saved: ...")` and no token. `invalid_grant` marks `needs_login` (file kept) and logs one warning naming `fili auth login <plugin>`. Transient and rejected failures never mark anything.
- `scheduler.rs`: `auth_for_poll` maps `RefreshFailed(text)` to `CycleOutcome::TokenRefresh(text)`; no other scheduler code changed. `poll_once` needed no change.
- The interim refresh-due variant and the `unused_async` allow from plan 06-01 are gone.

## TDD record

- RED (`92a84c9`): `RefreshFailed` replaced the interim variant with a placeholder body returning a fixed `RefreshFailed`. 17 new refresh tests compiled and 15 tests failed on their assertions (a token was expected, a file was expected to change, and so on). Semantic assessment: failures were on the planned behavior, not build, import or fixture errors. Four new tests passed under the placeholder (`a_token_with_60_seconds_left_is_not_refreshed`, `a_login_flagged_needs_login_is_not_refreshed`, `an_omitted_refresh_token_keeps_the_stored_one`, `refresh_failure_text_never_contains_the_tokens`) assert behavior the placeholder happens to satisfy; they guard against regression rather than driving the code. The `gsd_run check tdd-red-evidence` classifier does not parse `cargo test` output, so no machine record was produced.
- GREEN (`29fdfca`): the real refresh path; all 24 provider tests pass.
- Task 2 (`97e9c44`): the seven wiring tests passed on first run, since the one-line mapping had already landed in the RED commit to keep the crate compiling. They are verification tests, not drivers.

## Verification

- `cargo test -p filimon plugins::auth::provider`: 24 passed, all 17 behavior tests `ok`.
- `cargo test -p filimon`: 575 + 12 passed, 0 failed.
- The seven wiring tests report `ok`.
- `cargo clippy -p filimon --all-targets`: no warning for `auth/`, `scheduler.rs`, `once.rs`, `instance.rs` or `commands/plugins.rs`.
- Acceptance greps: `RefreshFailed`, `tokio::sync::Mutex`, `OnceLock`, `mark_needs_login`, `DEFAULT_EXPIRES_IN_SECS` present; `RefreshDue`, `unused_async`, `CursorStore` (under `auth/`) absent. `Cargo.toml`, `Cargo.lock` and `wit` unchanged.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Existing test `the_injected_clock_decides_expiry` asserted the removed variant**
- **Found during:** Task 1
- **Issue:** It expected the interim refresh-due outcome for a token with 10 seconds left and no refresh token; under the new rules that token is still used.
- **Fix:** The late clock is now 10_010 (expired, no refresh token) and the expectation is `NeedsLogin`. The clock-decides-expiry point is kept. The plan's own test `a_token_one_second_under_the_margin_is_refresh_due` was deleted as instructed.
- **Files modified:** `src/plugins/auth/provider.rs`
- **Commit:** 92a84c9

**2. [Scheduler mapping moved earlier] `RefreshFailed` mapping landed in the RED commit**
- **Why:** replacing the variant breaks the crate's build, so the one-line scheduler mapping had to move with it. No behavior beyond the plan.

None otherwise: plan executed as written.

## Known Stubs

None.

## Threat Flags

None. The only new surface is the provider calling the token endpoint it already could reach through `TokenClient`.

## Notes for later plans

- `cargo fmt --check` reports pre-existing drift in `scheduler.rs`, `commands/plugins.rs` and elsewhere; I did not reformat to keep the diff to this plan's changes.
- The flagged assumptions from the plan stand: a refresh is not raced against shutdown, and a crash between the provider's answer and the file write can lose a rotated refresh token (for plan 06-09's README).

## Self-Check: PASSED

- FOUND: src/plugins/auth/provider.rs, src/plugins/host/scheduler.rs, src/plugins/host/once.rs, src/commands/plugins.rs
- FOUND commits: 92a84c9, 29fdfca, 97e9c44
