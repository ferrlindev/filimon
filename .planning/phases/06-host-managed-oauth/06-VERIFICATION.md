---
phase: 06-host-managed-oauth
verified: 2026-11-11T00:00:00Z
status: human_needed
score: 4/4 must-haves verified
covered_files:
  - ".planning/phases/06-host-managed-oauth/06-01-PLAN.md"
  - ".planning/phases/06-host-managed-oauth/06-01-SUMMARY.md"
  - ".planning/phases/06-host-managed-oauth/06-02-PLAN.md"
  - ".planning/phases/06-host-managed-oauth/06-02-SUMMARY.md"
  - ".planning/phases/06-host-managed-oauth/06-03-PLAN.md"
  - ".planning/phases/06-host-managed-oauth/06-03-SUMMARY.md"
  - ".planning/phases/06-host-managed-oauth/06-04-PLAN.md"
  - ".planning/phases/06-host-managed-oauth/06-04-SUMMARY.md"
  - ".planning/phases/06-host-managed-oauth/06-05-PLAN.md"
  - ".planning/phases/06-host-managed-oauth/06-05-SUMMARY.md"
  - ".planning/phases/06-host-managed-oauth/06-06-PLAN.md"
  - ".planning/phases/06-host-managed-oauth/06-06-SUMMARY.md"
  - ".planning/phases/06-host-managed-oauth/06-07-PLAN.md"
  - ".planning/phases/06-host-managed-oauth/06-07-SUMMARY.md"
  - ".planning/phases/06-host-managed-oauth/06-08-PLAN.md"
  - ".planning/phases/06-host-managed-oauth/06-08-SUMMARY.md"
  - ".planning/phases/06-host-managed-oauth/06-09-PLAN.md"
  - ".planning/phases/06-host-managed-oauth/06-09-SUMMARY.md"
  - "plugins/README.md"
  - "plugins/stub-source/src/lib.rs"
  - "src/commands/auth.rs"
  - "src/plugins/auth/callback.rs"
  - "src/plugins/auth/client.rs"
  - "src/plugins/auth/login.rs"
  - "src/plugins/auth/pkce.rs"
  - "src/plugins/auth/provider.rs"
  - "src/plugins/auth/store.rs"
  - "src/plugins/config.rs"
  - "src/plugins/host/instance.rs"
  - "src/plugins/host/once.rs"
  - "src/plugins/host/scheduler.rs"
  - "tests/auth_cli.rs"
covered_digest: "v4:sha256:dee2a10efa596fe4b9f4cc4f560fd6c2600ffa5c84dd508a57aefa78a803f43a"
behavior_unverified: 0
overrides_applied: 0
human_verification:
  - test: "Register an OAuth app with a real provider (for example Reddit), put its client id and endpoints in a plugin TOML under [host.oauth], add http://127.0.0.1:17420/callback as the redirect URI, then run `fili auth login <plugin>`, approve in the browser, then `fili auth status`, then `fili run` (or `fili plugins run-once <plugin>`)."
    expected: "The browser opens the provider's authorize page, the callback lands on the local listener and shows the close-this-tab page, the login summary prints, `status` shows the plugin as logged-in with an expiry and a refresh-token flag, and the poll runs with the stored access token. After the access token passes its expiry (or the file's expires_at is edited into the past) the next poll refreshes it without a new login."
    why_human: "ROADMAP SC1 against a live provider needs a registered OAuth app, a real browser and real network. Every part of the flow is proven offline against a local fake provider, but acceptance by a real provider (redirect-URI matching, Basic-auth client credentials, provider-specific authorize params) cannot be automated."
  - test: "Triage the 4 open review warnings in 06-REVIEW.md (WR-01..WR-04). Decide fix now (gap plan or /gsd-code-review 6 --fix) or defer."
    expected: "Each finding gets a disposition. 06-REVIEW-DISPOSITION.md currently records all 8 as `open`."
    why_human: "Advisory findings need a developer decision. None of them falsifies a must-have (see Anti-Patterns)."
---

# Phase 6: Host-Managed OAuth Verification Report

**Phase Goal:** Users can log in to a plugin's platform once through OAuth 2.0 PKCE. The host keeps tokens fresh and passes plugins only current access tokens
**Verified:** 2026-11-11
**Status:** human_needed
**Re-verification:** No, initial verification

## Goal Achievement

### Observable Truths (ROADMAP success criteria)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | `fili auth login <plugin>` sends the user to the authorize URL (endpoints and scopes from the plugin TOML), receives the code on a local callback, completes PKCE, stores tokens locally | VERIFIED offline; live-provider run is a human item | `src/plugins/auth/login.rs::login` (pkce verifier/S256 challenge, loopback `callback.rs` listener on 127.0.0.1:redirect_port, code exchange via `client.rs`, 0600 atomic write via `store.rs`). Tests pass: `pkce::tests::rfc7636_appendix_b_vector`, `login::tests::login_stores_the_tokens_0600_after_a_matching_c...`, `the_exchange_carries_the_verifier_that_matches_the_authorize_challenge`, `the_authorize_url_carries_authorize_params`, state-mismatch/denied/timeout write nothing. CLI wired: `cli.rs` `Command::Auth`/`AuthCommand`, `main.rs:134`, `commands/auth.rs`. A real provider was never contacted (human item 1). |
| 2 | The host refreshes an access token automatically before it expires | VERIFIED | `provider.rs::access_token`/`renew`: lazy refresh when under `REFRESH_MARGIN_SECS = 60`, under in-process gate plus per-alias file lock, re-read after lock, atomic persist before hand-out, rotated refresh token kept/replaced, default 3600 s. Called immediately before every poll at `scheduler.rs:527` and in `once.rs:75`. Tests pass: `a_token_with_60_seconds_left_is_not_refreshed`, `a_token_with_59_seconds_left_is_refreshed_and_persisted`, `two_concurrent_calls_make_one_refresh_request`, `sched_auth::a_due_token_is_refreshed_before_poll_and_the_new_token_reaches_poll`. |
| 3 | A logged-in plugin gets a current access token via the WIT token import and never sees the refresh token; a recording test component can confirm | VERIFIED | Type-level: `TokenOutcome::Token` carries only `AccessToken`; `instance.rs:73,146` pass only `access_token: Option<&str>` to `call_poll`; client secret kept out of `PluginConfig.secrets`. Real wasm: I ran `cargo test -p filimon -- --ignored wasm_auth` (3 passed): `wasm_auth_the_guest_sees_the_access_token_and_never_the_refresh_token_or_the_client_secret`, `wasm_auth_run_once_path_hands_the_guest_the_token_too`, `wasm_auth_a_plugin_without_a_login_gets_no_token`; they assert the recorded inputs contain the access token and the plugin's own secret and pass `assert_no_host_credentials`. `git diff 05e3f05^ HEAD -- wit` is empty (WIT contract unchanged; `poll(cursor, access-token)` already existed from Phase 1). |
| 4 | `fili auth status` lists stored logins and expiry state per plugin; `fili auth logout <plugin>` deletes tokens and later polls run unauthenticated | VERIFIED | `commands/auth.rs` (status/logout), `tests/auth_cli.rs` drives the real `fili` binary: `status_prints_each_state_without_token_values_and_no_banner`, `status_json_stdout_parses_as_an_array`, `status_works_while_the_state_directory_lock_is_held`, `status_does_not_create_the_state_dir`, `logout_deletes_the_token_file_and_a_second_logout_says_there_is_none`, `status_never_prints_a_token_even_for_a_corrupt_file`. After logout: `later_polls_run_without_a_token_after_logout`, `a_refresh_never_resurrects_a_logged_out_plugin`. |

**Score:** 4/4 truths verified by code and tests (SC1's live-provider leg is routed to human verification, as instructed).

PLAN-level must-haves (D-01..D-15 truths across plans 01-09) were spot-checked against code and test names, not re-derived one by one: 0600 file in 0700 dir with loose-permission refusal (store.rs), `[host.oauth]` https-only and secret-resolution rules (config.rs), loopback-only fixed-port callback with 404/400 stray-traffic handling (callback.rs), host-owned redirect-free 64 KiB-capped HTTP client with Permanent/Transient/Rejected refresh classification (client.rs), needs_login handling (provider.rs). Each named test exists in the tree and the suite is green.

### Required Artifacts

| Artifact | Status | Details |
|----------|--------|---------|
| `src/plugins/auth/{store,provider,client,pkce,callback,login,error,testing,mod}.rs` | VERIFIED | Substantive (5,000+ lines with tests), wired from scheduler, once and commands/auth. No dead-code allows remain in `src/plugins/auth` or `commands/auth.rs`. |
| `src/commands/auth.rs` | VERIFIED | Login, status, logout implemented; wired in `main.rs`/`cli.rs`. |
| `src/plugins/config.rs` (`OAuthConfig`, `probe_oauth_sources`) | VERIFIED | `[host.oauth]` parsed beside `[host]`. |
| `src/plugins/host/scheduler.rs`, `once.rs`, `instance.rs` | VERIFIED | `auth_for_poll` is the single decision table used by both callers. |
| `tests/auth_cli.rs` | VERIFIED | Real-binary tests pass. |
| `plugins/stub-source` `record_inputs`; `plugins/README.md` | VERIFIED | Only guest fixture and docs changed under `plugins/`; `examples/` and `wit/` untouched. |

### Key Link Verification

| From | To | Via | Status |
|------|----|-----|--------|
| `scheduler.rs` | `provider.rs` | `auth_for_poll` calls `access_token()` before every `instance.poll` (scheduler.rs:527-553) | WIRED |
| `once.rs` | `scheduler.rs` | `auth_for_poll(spec)` at once.rs:75, `.poll(..., token.expose)` at :81 | WIRED |
| `instance.rs` | `wit/worlds.wit` | `call_poll(&mut store, cursor, access_token)` | WIRED |
| `main.rs`/`cli.rs` | `commands/auth.rs` | `Command::Auth` dispatch | WIRED |

### Data-Flow Trace (Level 4)

Token flows from the on-disk file (`TokenStore::read`, re-read each poll) through `TokenProvider` to the guest's `poll` argument; the real-component test proves the value arrives and the refresh token and client secret do not. Status rows are computed from the real store, not static data. FLOWING.

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Full suite | `cargo test -p filimon` | 632 + 10 + 12 passed, 0 failed | PASS |
| Real-wasm token isolation | `cargo test -p filimon -- --ignored wasm_auth` | 3 passed | PASS |
| WIT unchanged by phase | `git diff 05e3f05^ HEAD -- wit` | empty | PASS |
| Clippy on phase files | `cargo clippy -p filimon --all-targets` | No warning lines in `src/plugins/auth/*`, `commands/auth.rs`, `scheduler.rs`, `once.rs`, `instance.rs`. Warnings in `config.rs`, `secrets.rs`, `mod.rs`, `host/testing.rs` are on lines last touched by pre-phase commits (92607d7, c6a30bd). | PASS |

### Probe Execution

No probe scripts declared or found. SKIPPED.

### Requirements Coverage

| Requirement | Source Plans | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| AUTH-01 | 06-03, 06-04, 06-05, 06-07 | `fili auth login` PKCE via local callback using TOML endpoints/scopes | SATISFIED offline (live check pending) | login.rs, callback.rs, pkce.rs, client.rs, config.rs; tests above |
| AUTH-02 | 06-01, 06-02, 06-05, 06-06 | Tokens stored locally, refreshed automatically before expiry | SATISFIED | store.rs (0600), provider.rs lazy refresh, scheduler/once wiring |
| AUTH-03 | 06-01, 06-03, 06-06, 06-09 | Plugins get only a current access token, never refresh tokens | SATISFIED | type-level isolation plus real-wasm recording test |
| AUTH-04 | 06-05, 06-08, 06-09 | `fili auth status` / `logout` | SATISFIED | commands/auth.rs, tests/auth_cli.rs |

All four IDs in the phase's PLAN frontmatter (AUTH-01..04) are accounted for. No orphaned requirements: REQUIREMENTS.md maps exactly AUTH-01..04 to Phase 6.

**Tracking drift (not a code gap):** `.planning/REQUIREMENTS.md` still shows AUTH-02, AUTH-03 and AUTH-04 as unchecked/`Pending` (only AUTH-01 is `[x]`/Complete), and the ROADMAP Phase 6 line is `[ ]`. The orchestrator should update these when it closes the phase.

### Prohibitions

Plan 06-01 (and later plans) declare prohibitions with `status: unresolved`: no token/secret/code/verifier/state in any log, diagnostic or output (D-06); a corrupt or loose-permission token file must not be treated as logged out or auto-repaired (D-02). LLM-judge verdict (NON-AUTHORITATIVE, human review recommended): both hold. Evidence: redacting `AccessToken`/`RefreshToken`/`AuthCode`/`Verifier` types (`token_types_never_print_their_value`, `codes_never_appear_in_debug_or_error_text`), `nothing_secret_reaches_the_hooks_or_the_summary`, `commands::auth::tests::nothing_secret_is_printed`, `status_never_prints_a_token_even_for_a_corrupt_file`, `a_corrupt_token_file_disables_only_that_plugin`, `a_file_looser_than_0600_is_refused_naming_plugin_and_path_and_left_alone`. 06-SECURITY.md records threats_open 0. Flag: unverified-prohibition, human review recommended (judgment tier, not independently re-audited line by line).

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| (phase files) | - | TBD/FIXME/XXX/todo!/unimplemented! | none found | Debt-marker gate clear |
| `src/plugins/auth/callback.rs` | 319-321 | WR-01: any `/callback` request with a wrong state aborts the login | Warning (advisory) | Local nuisance abort of a login attempt; matches the plan 04 truth as written, and the next login retry works. No goal truth fails. |
| `src/commands/auth.rs` | 663-683 | WR-02: logout reads tokens before taking the lock, may revoke a stale token | Warning (advisory) | Best-effort revoke edge case; local deletion still correct. |
| `src/plugins/auth/client.rs` | 244-246 | WR-03: Basic credentials not form-urlencoded per RFC 6749 2.3.1 | Warning (advisory) | Only affects client ids/secrets with reserved characters; may matter for a live provider (see human item 1). |
| `scheduler.rs` / `provider.rs` | 523-536 | WR-04: refresh not interruptible by shutdown; can wait on the lock | Warning (advisory) | Delayed shutdown, bounded by the 30 s HTTP timeout. |
| callback.rs, config.rs, provider.rs, stub-source | - | IN-01..IN-04 | Info | Cosmetic or documentation items. |

None is a blocker. All 8 are `open` in 06-REVIEW-DISPOSITION.md (human item 2).

### Human Verification Required

1. **Live provider login.** Register an OAuth app (for example Reddit), configure `[host.oauth]` in a plugin TOML, register `http://127.0.0.1:17420/callback`, run `fili auth login <plugin>`, approve in the browser, then `fili auth status` and a poll. Expected: login completes and stores a 0600 token file, status shows logged-in with expiry, polling uses the token, and an expired token refreshes without another login. Why human: needs a registered app, a browser and a real network; ROADMAP SC1 end to end.
2. **Review-warning triage.** Decide fix or defer for WR-01..WR-04 (WR-03 is the one most likely to matter against a live provider).

### Gaps Summary

No gaps. All four roadmap success criteria and all four requirement IDs are backed by implemented, wired code and passing tests (independently re-run: full suite green, real-wasm token-isolation group green, WIT diff empty, no debt markers, no new clippy warnings in phase files). The status is `human_needed` solely because the live-provider login cannot be automated, plus the open advisory review findings awaiting disposition.

---

_Verified: 2026-11-11_
_Verifier: Claude (gsd-verifier)_
