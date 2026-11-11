---
phase: 06-host-managed-oauth
plan: 05
subsystem: auth
tags: [oauth, token-endpoint, refresh, revocation, reqwest, pkce, rust]

requires:
  - phase: 06-host-managed-oauth
    provides: "OAuthConfig with token_url, client_id, client_secret, revocation_url (plans 06-01, 06-03); Verifier and AuthCode (plan 06-04); RefreshToken (plan 06-02)"
provides:
  - "client.rs: TokenClient (new, exchange_code, refresh, revoke), TokenSet, TokenKind, RefreshFailure (Permanent, Transient, Rejected) with text(), DEFAULT_EXPIRES_IN_SECS"
  - "AuthError::TokenRequest, TokenRejected, TokenResponse, RevokeFailed"
  - "Reply::json(status, body) in host/testing.rs; oauth_config_for, token_body, form_pairs in auth/testing.rs"
affects: [06-06, 06-07, 06-08]

plan_head_before: 818fc48ff51976020246ac47cee848162040e63d
plan_head_after: 9c1eadcb156652ee71aa222e8332d819a2c32a54

actuals:
  tokens: 70000
  tasks: 2
  commits: 4

tech-stack:
  added: []
  patterns:
    - "Hand-written token client: three form POSTs on the async reqwest::Client, one .form call per request"
    - "Redirect-proof, size-capped (64 KiB, enforced while reading) and time-capped (30 s / 10 s) host HTTP client"
    - "Failure classes as an enum so the caller decides needs_login, backoff or surfacing (D-09)"

key-files:
  created:
    - src/plugins/auth/client.rs
  modified:
    - src/plugins/auth/mod.rs
    - src/plugins/auth/error.rs
    - src/plugins/auth/testing.rs
    - src/plugins/auth/callback.rs
    - src/plugins/host/testing.rs

key-decisions:
  - "A client without a secret sends client_id in the form body; a client with a secret uses HTTP Basic and keeps the secret out of the body (RESEARCH Open Question 2, D-12)"
  - "A 2xx refresh answer with no usable access token is Transient, not Permanent: a broken answer is no reason to discard the login (D-09)"
  - "Revocation failures carry only the HTTP status or the cleaned transport text, never the provider's description, since that text could echo the token"
  - "The 64 KiB cap also rejects early on an oversized Content-Length before reading the body"

requirements-completed: [AUTH-01, AUTH-02, AUTH-04]

duration: 40min
completed: 2026-11-11
status: complete
---

# Phase 6 Plan 05: Token Endpoint Client Summary

**Hardened async token client: PKCE code exchange, refresh with permanent/transient/rejected classification, and revocation, with no credential or provider body ever reaching an error text**

## Performance

- **Duration:** about 40 min (dominated by cold cargo builds in the new worktree)
- **Completed:** 2026-11-11T07:45:00Z
- **Tasks:** 2 (both TDD)
- **Files modified:** 6 (one new)

## Accomplishments

- `TokenClient::exchange_code` posts the PKCE form; a public client sends `client_id` in the body, a client with a secret uses Basic auth and never puts the secret in the body.
- Hardening (D-13): `redirect(Policy::none())`, 30 s request and 10 s connect timeouts, 64 KiB cap enforced while reading, `Accept: application/json`, `fili/<version>` User-Agent, default certificate validation with no override, async client only.
- Token answers parse `expires_in` as a number or numeric string; a missing one stays `None`; empty `refresh_token` is treated as absent.
- `TokenClient::refresh` maps every outcome to `RefreshFailure`: `invalid_grant` on 400/401 is `Permanent`; transport errors, 5xx, 408, 429, `temporarily_unavailable`, `server_error`, unparseable 5xx bodies, a 200 with no access token and oversize answers are `Transient`; other 4xx and any redirect are `Rejected`.
- `TokenClient::revoke` posts `token` and `token_type_hint` with the same client auth, treats any 2xx as success, makes no request and returns `Ok(false)` without a `revocation_url`.
- Provider text is cleaned and clipped to 160 characters; bodies and request URLs are never echoed.

## Task Commits

1. **Task 1: client, code exchange, revocation** - `a27723a` (test, RED), `bfca850` (feat, GREEN)
2. **Task 2: refresh grant and classification** - `8d59f8a` (test, RED), `9c1eadc` (feat, GREEN)

## TDD Gate Compliance

- **RED (task 1):** 16 tests written against compiling stubs (`exchange_code` returned `TokenRequest`, `revoke` returned `Ok(false)`). 12 failed on their planned assertions (wrong error variant, missing form fields, `Ok(false)` instead of `Ok(true)`); 4 passed vacuously because the stub's behavior coincided (hung endpoint, unreachable endpoint, revoke without URL, no credential in errors). Semantic assessment: every failing test reached its assertion; none failed on a compile or fixture error.
- **RED (task 2):** 15 tests against a `Transient` stub of `refresh`. 10 failed on the planned assertions; 5 coincided with the stub (5xx, 408/429, connection error, 200 without token, text-leak check).
- **GREEN:** both implementations pass all 31 client tests.
- `gsd_run check tdd-red-evidence` was not run: this plan is `type: execute` with `tdd="true"` tasks and `workflow.tdd_mode` is not enabled; RED evidence was captured from the test output above.

## Files Created/Modified

- `src/plugins/auth/client.rs` - the token client, parsers, failure classification and 31 tests
- `src/plugins/auth/error.rs` - four new `AuthError` variants (`fili::auth::token_request`, `token_rejected`, `token_response`, `revoke_failed`)
- `src/plugins/auth/mod.rs` - `pub mod client;`
- `src/plugins/auth/testing.rs` - `oauth_config_for`, `token_body`, `form_pairs`
- `src/plugins/auth/callback.rs` - `AuthCode::for_test` (cfg(test))
- `src/plugins/host/testing.rs` - `Reply::json`

## Decisions Made

See `key-decisions` above. All flagged assumptions from the plan were followed as written (public client sends `client_id` in the body, Phase 7 owns Reddit-style empty-password Basic, missing `expires_in` left to the caller, other 4xx never mark needs_login, User-Agent `fili/<version>`).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] AuthCode has no constructor reachable from client tests**
- **Found during:** Task 1 (RED)
- **Issue:** `AuthCode`'s field is private to `callback.rs`, so `client.rs` tests could not build one, and the plan's `files_modified` omits `callback.rs`.
- **Fix:** added `#[cfg(test)] pub(crate) fn AuthCode::for_test(&str)`; no production surface change.
- **Files modified:** `src/plugins/auth/callback.rs`
- **Committed in:** `a27723a`

**2. [Rule 1 - Bug in plan] Test name `408_and_429_are_transient` is not a valid Rust identifier**
- **Found during:** Task 2 (RED)
- **Issue:** an identifier cannot start with a digit, so the test cannot carry the name the plan's verify loop greps for.
- **Fix:** named it `status_408_and_429_are_transient`. The plan's automated loop will therefore print `missing 408_and_429_are_transient` for that one name; the test exists and passes (`plugins::auth::client::tests::status_408_and_429_are_transient ... ok`).
- **Files modified:** `src/plugins/auth/client.rs`
- **Committed in:** `8d59f8a`

**3. [Rule 3 - Blocking] `Reply` has no body-plus-status constructor**
- **Found during:** Task 2 (RED)
- **Issue:** non-JSON 4xx/5xx bodies needed a status; only `Reply::json` (planned) and `Reply::ok` exist.
- **Fix:** non-JSON bodies are sent through `Reply::json(status, text)` (the content-type header is irrelevant to the classifier). No extra helper added.
- **Committed in:** `8d59f8a`

---

**Total deviations:** 3 auto-fixed (1 bug in the plan, 2 blocking). **Impact:** none on behavior; one verify-loop name needs a manual rename in the plan's grep.

## Issues Encountered

- Cold build of the worktree's `target/` took several minutes; run once in the background while the code was written.
- `cargo clippy` reports 3 warnings for `host/testing.rs` (the baseline count) and none for `src/plugins/auth/`.

## Verification Results

- `cargo test -p filimon plugins::auth::client`: 31 passed.
- `cargo test -p filimon`: 551 passed, 0 failed (62 ignored, pre-existing).
- `cargo clippy -p filimon --all-targets`: no warning under `src/plugins/auth/`; 3 under `host/testing.rs`.
- Acceptance greps (`Policy::none`, `MAX_RESPONSE_BYTES`, `basic_auth`, no `reqwest::blocking`, no `danger_accept`, `fn json`) pass; `git diff --quiet -- Cargo.toml Cargo.lock wit` passes.

## Known Stubs

None.

## Threat Flags

None. The new network surface (host to the provider's token and revocation endpoints) is the one in the plan's threat model (T-06-18 to T-06-21), each mitigated and tested.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Plan 06-06 can call `TokenClient::refresh` and map `RefreshFailure` to `needs_login`, backoff or surfacing.
- Plans 06-07 and 06-08 can call `exchange_code` and `revoke`; callers default a `None` `expires_in` to `DEFAULT_EXPIRES_IN_SECS` and keep the stored refresh token when the answer carries none.
- The `#![allow(dead_code)]` in `auth/mod.rs` still stands until plan 06-08.

---
*Phase: 06-host-managed-oauth*
*Completed: 2026-11-11*

## Self-Check: PASSED

- `src/plugins/auth/client.rs`: FOUND
- Commits `a27723a`, `bfca850`, `8d59f8a`, `9c1eadc`: FOUND on HEAD
