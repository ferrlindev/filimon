---
phase: 06-host-managed-oauth
plan: 04
subsystem: auth
tags: [oauth, pkce, rfc7636, loopback, tokio, sha2, getrandom, rust]

requires:
  - phase: 06-host-managed-oauth
    provides: "plugins::auth module, AuthError, OAuthConfig (plan 06-01)"
provides:
  - "pkce.rs: Verifier (redacting), generate_verifier, generate_state, s256_challenge, build_authorize_url"
  - "callback.rs: CallbackListener (bind, port, redirect_uri, wait_for_code), AuthCode (redacting), CALLBACK_PATH, LOGIN_TIMEOUT"
  - "AuthError::Random, RedirectPortInUse, CallbackDenied, StateMismatch, CallbackTimeout, CallbackInvalid"
  - "sha2 0.10 and getrandom 0.3 as direct dependencies (no new package in Cargo.lock)"
  - "OAuthConfig.authorize_params (Vec<(String, String)>, empty until plan 06-03 loads it from TOML)"
affects: [06-05, 06-07, 06-08]

actuals:
  tokens: 9500
  tasks: 2
  commits: 2

tech-stack:
  added: [sha2 0.10, getrandom 0.3]
  patterns:
    - "Hand-written one-shot loopback HTTP listener on tokio TcpListener, one task per connection, mpsc decision channel"
    - "Redacting newtypes for verifier and code (Debug prints ***, no Display)"
    - "Real-socket, real-time tests (no paused time) for the listener"

key-files:
  created:
    - src/plugins/auth/pkce.rs
    - src/plugins/auth/callback.rs
  modified:
    - Cargo.toml
    - Cargo.lock
    - src/plugins/auth/mod.rs
    - src/plugins/auth/error.rs
    - src/plugins/auth/testing.rs
    - src/plugins/config.rs

key-decisions:
  - "Request line is validated as soon as it is complete, so garbage and non-GET requests are answered 400 at once; a valid-looking request is read to the blank line (8 KiB cap, 2 s timeout) so the browser's reply is not cut by an early close"
  - "A silent client that times out is dropped with no reply; only requests that actually arrived get a 400"
  - "An empty `error=` value is not treated as a denial (falls through to the code check)"
  - "A target starting with // is rejected as malformed, so it can never be resolved against another authority"

patterns-established:
  - "Decision order for /callback: state, then error, then code; every /callback request gets the same fixed 200 page"

requirements-completed: [AUTH-01]

coverage:
  - id: D1
    description: "S256 PKCE challenge matches the RFC 7636 Appendix B vector; verifier and state are 43 URL-safe CSPRNG characters"
    requirement: AUTH-01
    verification:
      - kind: unit
        ref: "plugins::auth::pkce::tests::rfc7636_appendix_b_vector"
        status: pass
      - kind: unit
        ref: "plugins::auth::pkce::tests (verifier_is_43_url_safe_characters, state_is_43_url_safe_characters, two_verifiers_differ, verifier_never_prints_its_value)"
        status: pass
    human_judgment: false
  - id: D2
    description: "Authorize URL carries the standard parameters, keeps an endpoint query, omits empty scope, appends authorize_params after the standard ones (D-14)"
    requirement: AUTH-01
    verification:
      - kind: unit
        ref: "plugins::auth::pkce::tests (four authorize_url_* tests)"
        status: pass
    human_judgment: false
  - id: D3
    description: "Loopback-only one-shot listener: binds 127.0.0.1, fails fast on a busy port naming host.oauth.redirect_port, times out, and releases the port"
    requirement: AUTH-01
    verification:
      - kind: unit
        ref: "plugins::auth::callback::tests (the_listener_binds_loopback_only, redirect_uri_uses_the_literal_loopback_address, bind_fails_when_the_port_is_in_use_naming_the_config_key, the_wait_times_out_with_a_named_error, the_listener_is_closed_after_the_wait_ends)"
        status: pass
    human_judgment: false
  - id: D4
    description: "Callback judging: state mismatch, denied, invalid, and the matching code, with named errors that carry no code or state"
    requirement: AUTH-01
    verification:
      - kind: unit
        ref: "plugins::auth::callback::tests (callback_returns_the_code_for_a_matching_state, a_state_mismatch_is_a_named_error, a_missing_state_is_a_state_mismatch, a_denied_callback_is_a_named_error, a_matching_state_without_a_code_is_an_invalid_callback, codes_never_appear_in_debug_or_error_text)"
        status: pass
    human_judgment: false
  - id: D5
    description: "Stray and hostile traffic cannot end or stall the login; the reply page is static and reflects nothing"
    requirement: AUTH-01
    verification:
      - kind: unit
        ref: "plugins::auth::callback::tests (callback_page_is_static_and_reflects_nothing, a_wrong_path_gets_404_and_the_wait_continues, garbage_and_oversize_requests_get_400_and_the_wait_continues, a_non_get_method_gets_400_and_the_wait_continues, a_silent_client_cannot_stall_the_wait)"
        status: pass
    human_judgment: false

duration: 35min
completed: 2026-11-11
status: complete
commits: 2
plan_head_before: e7b74c4b6959303bed731a6ffe9910241aa2f0f0
plan_head_after: 02913e5664553f162afab1d2b1656b07844c1c5c
---

# Phase 6 Plan 04: PKCE and the loopback callback listener Summary

**RFC 7636 S256 PKCE (CSPRNG verifier and state, authorize URL with the plugin's extra parameters) and a hand-written one-shot `127.0.0.1` callback listener that verifies `state`, rejects hostile traffic, serves a fixed page and never prints a code, verifier or state.**

## Performance

- **Duration:** about 35 min (most of it cold `cargo` builds)
- **Completed:** 2026-11-11
- **Tasks:** 2
- **Files:** 8 (2 created, 6 modified)

## Accomplishments

- `pkce.rs`: `s256_challenge` passes the RFC 7636 Appendix B vector; `generate_verifier` and `generate_state` are 32 `getrandom::fill` bytes as 43 `URL_SAFE_NO_PAD` characters; `build_authorize_url` uses `query_pairs_mut` (no manual escaping), keeps an existing endpoint query and appends `authorize_params` after `code_challenge_method`.
- `callback.rs`: `bind` takes `("127.0.0.1", port)` only; `wait_for_code` consumes the listener (port free on return), accepts in a loop with one task per connection, and judges `/callback` as state, then `error`, then `code`. Wrong path gets 404; garbage, non-GET, a `//host` target or an over-8-KiB head gets 400; a silent client is dropped after 2 s and never delays the wait.
- Seventeen listener tests run over real loopback sockets and real time (RESEARCH Pitfall 6): the plan listed sixteen, and I added `an_error_callback_with_a_wrong_state_cannot_print_provider_text` to pin the flagged assumption that `error=` is judged after the state check.
- Six new `AuthError` variants, none of which can carry a code, a state or a verifier.

## Task Commits

1. **Task 1: Dependencies, PKCE verifier, challenge, state and authorize URL** - `b168bb3` (feat)
2. **Task 2: One-shot loopback callback listener** - `02913e5` (feat)

_Note: both tasks are `tdd="true"` in a `type: execute` plan, with tests inline in the same Rust file as the code (the Rust RED-commit gap in `tdd.md`). Tests and implementation were written together and each task is one commit; there is no separate RED commit. No `type: tdd` gate applies._

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] `OAuthConfig.authorize_params` did not exist in this worktree**
- **Found during:** Task 1
- **Issue:** `build_authorize_url` reads `oauth.authorize_params` and the tests set it, but that field is added by plan 06-03, which runs in the same wave (wave 2, both depend only on 06-01) and is not in this worktree's base.
- **Fix:** Added only `pub authorize_params: Vec<(String, String)>` (same name and type as 06-03's spec) at the end of `OAuthConfig`, `authorize_params: Vec::new()` in `build_oauth`, and `authorize_params: vec![]` in `auth::testing::oauth_config()`. I did not add `redirect_port` or the other 06-03 fields: `CallbackListener::bind(port)` takes the port as an argument, so it needs none of them.
- **Files modified:** `src/plugins/config.rs`, `src/plugins/auth/testing.rs`
- **Commit:** `b168bb3`
- **Merge note for the orchestrator:** plan 06-03 edits the same three spots (the `OAuthConfig` struct, the `build_oauth` literal, `oauth_config()`). Expect a textual conflict; resolve by taking 06-03's side, which carries the same `authorize_params` field plus its other fields.

**2. [Rule 1 - Style] Two clippy pedantic warnings in `callback.rs`**
- **Found during:** Task 2 verification
- **Issue:** `Duration::from_secs(300)` and an unnecessary lazy closure on the timeout result.
- **Fix:** `Duration::from_mins(5)` (rust-version is 1.95) and `unwrap_or(Err(..))`.
- **Commit:** `02913e5`

**Total deviations:** 2 auto-fixed (1 blocking, 1 style). **Impact:** none on behaviour; the first needs a mechanical conflict resolution at wave merge.

## Issues Encountered

- `RedirectPortInUse`'s Display text is the plan's wording and carries the port; the `host.oauth.redirect_port` key is in its miette `help`, as the plan's error spec says. The test asserts the port in the message and the key in the help text, not both in `Display`.
- The error-handling plan truth "naming `host.oauth.redirect_port`" is therefore met through the diagnostic help; plan 06-07 should render the diagnostic (not only `to_string()`) when it surfaces this error.

## Known Stubs

None. `callback.rs` and `pkce.rs` are not yet called by the CLI; the module-level dead-code allow in `auth/mod.rs` (removed by plan 06-08) covers them. `OAuthConfig.authorize_params` is always empty until plan 06-03 loads it from TOML.

## Threat Flags

None. The only new surface is the loopback listener, which is the plan's modelled T-06-13 to T-06-17.

## User Setup Required

None.

## Next Phase Readiness

- Plan 06-05 (token client) and 06-07 (`fili auth login`) can compose `CallbackListener::bind(oauth.redirect_port)`, `generate_verifier`, `s256_challenge`, `generate_state`, `build_authorize_url(oauth, &redirect_uri, &state, &challenge)` and `wait_for_code`.
- Plan 06-07 must also resolve the `OAuthConfig` merge with 06-03 (see deviation 1).

## Verification

- `cargo test -p filimon plugins::auth::pkce`: 9 passed.
- `cargo test -p filimon plugins::auth::callback`: 17 passed.
- `cargo test -p filimon`: 481 + 12 passed, 0 failed.
- `cargo clippy -p filimon --all-targets`: no warning under `src/plugins/auth/`.
- `grep -c '^name = ' Cargo.lock` is 526 (no new package; the lock gained only `getrandom 0.3.3` and `sha2` on the root package). `git diff -- wit` is empty.
- `rustfmt --check` clean on the new and edited auth files.

## Self-Check: PASSED

- FOUND: src/plugins/auth/pkce.rs, src/plugins/auth/callback.rs
- FOUND commits: b168bb3, 02913e5 (both ancestors of HEAD)
- All 9 PKCE and 16 plan-named callback tests report `ok`

---
*Phase: 06-host-managed-oauth*
*Completed: 2026-11-11*
