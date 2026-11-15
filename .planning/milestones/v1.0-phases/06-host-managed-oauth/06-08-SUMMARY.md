---
phase: 06-host-managed-oauth
plan: 08
subsystem: auth
tags: [oauth, cli, status, logout, revocation, clap, rust]

requires:
  - phase: 06-host-managed-oauth
    provides: "TokenStore (read without lock, lock, delete) from 06-02; TokenClient::revoke and TokenKind from 06-05; TokenProvider decision table from 06-06; AuthCommand::Login, login_command and the CLI test helpers from 06-07"
provides:
  - "config.rs: probe_oauth_sources, OAuthProbeReport, OAuthProbe, OAuthDeclared (lenient read of which sources declare [host.oauth])"
  - "commands/auth.rs: status (collect_status, render_status, render_status_json, describe_expiry) and logout (logout_command, private perform_logout seam), dispatch arms"
  - "cli.rs: AuthCommand::Status { json } and AuthCommand::Logout { plugin }"
  - "auth module with no dead-code allow"
affects: [06-09]

plan_head_before: 130720d2ceed507a3f61da1682c71ff84523af72
plan_head_after: aaf6d381b1b29fcd8bad21ab5e861fbbe86cc3f8

actuals:
  tokens: 16000
  tasks: 2
  commits: 4

tech-stack:
  added: []
  patterns:
    - "Lenient probe: a private view struct with no deny_unknown_fields reads only whether [host.oauth] exists, so status needs no .env and no secret"
    - "Delete-then-revoke ordering under the per-alias lock; the network call is best effort and only ever a warning"
    - "perform_logout seam takes an optional OAuthConfig plus a remembered reason, so every 'not revoked' path is testable without a registry"

key-files:
  created: []
  modified:
    - src/commands/auth.rs
    - src/plugins/config.rs
    - src/cli.rs
    - src/plugins/auth/mod.rs
    - src/plugins/auth/callback.rs
    - src/plugins/auth/client.rs

key-decisions:
  - "status lists only sources that declare [host.oauth] or have a token file; disabled sources are included because they can still hold a login"
  - "A plugin whose TOML cannot be read and that has no token file is a stderr note, not a failure and not a row"
  - "logout of a disabled, no-[host.oauth] or unloadable plugin deletes the file and says the login was not revoked, because the revocation endpoint and client secret come only from a loaded [host.oauth]"
  - "Revocation sends the refresh token when one is stored, else the access token (RFC 7009)"
  - "Two items only tests read (CallbackListener::port, RefreshFailure::text) are gated with cfg(test) instead of re-adding an allow"
  - "find_login_source is now a thin wrapper over a shared find_source(listing, name, verb) so login and logout give the same unknown and fetcher refusals"

patterns-established:
  - "Auth CLI output is built only from plugin names, paths, times and fixed phrases; token values and file content never reach a format string"

requirements-completed: [AUTH-04]

coverage:
  - id: D1
    description: "fili auth status lists each registered source that declares [host.oauth] or has a login with state, relative and RFC 3339 expiry and refresh flag, never a token; --json is a fixed-key array"
    requirement: AUTH-04
    verification:
      - kind: unit
        ref: "commands::auth::tests::status_lists_each_state_without_token_values"
        status: pass
      - kind: unit
        ref: "commands::auth::tests::status_json_is_an_array_with_no_token_fields"
        status: pass
      - kind: unit
        ref: "commands::auth::tests::describe_expiry_formats_each_range"
        status: pass
    human_judgment: false
  - id: D2
    description: "status is lock-free, creates nothing and needs no .env or secret; unreadable token files and unreadable plugin configs are rows or notes, never failures"
    requirement: AUTH-04
    verification:
      - kind: unit
        ref: "commands::auth::tests (status_works_while_the_state_lock_is_held, status_creates_nothing, status_reports_an_unreadable_token_file_naming_plugin_and_path_without_content, status_notes_a_plugin_whose_config_cannot_be_read)"
        status: pass
      - kind: unit
        ref: "plugins::config::tests (probe_oauth_sources_reports_declared_undeclared_and_unreadable, probe_does_not_read_dotenv_or_resolve_secrets)"
        status: pass
    human_judgment: false
  - id: D3
    description: "needs-login rows and expired rows without a refresh token say to run fili auth login again (D-09)"
    requirement: AUTH-04
    verification:
      - kind: unit
        ref: "commands::auth::tests (status_says_to_log_in_again_for_a_needs_login_row, status_shows_an_expired_row_with_its_refresh_flag)"
        status: pass
    human_judgment: false
  - id: D4
    description: "fili auth logout deletes the local file first under the per-alias lock, then best-effort revokes; a failed revocation warns and exits 0; a refresh waiting on the lock never resurrects the login"
    requirement: AUTH-04
    verification:
      - kind: unit
        ref: "commands::auth::tests (logout_posts_the_refresh_token_to_the_revocation_url_and_deletes_first, a_failed_revocation_warns_and_still_succeeds_with_the_file_gone, a_refresh_never_resurrects_a_logged_out_plugin, later_polls_run_without_a_token_after_logout)"
        status: pass
    human_judgment: false
  - id: D5
    description: "logout edge cases: no stored login, loose-permission file, no revocation_url, fetcher and unknown names, other plugins' logins untouched, no token ever printed"
    requirement: AUTH-04
    verification:
      - kind: unit
        ref: "commands::auth::tests (logout_without_a_stored_login_succeeds_with_a_note_and_creates_nothing, logout_with_a_loose_permission_file_deletes_it_without_revoking, logout_without_a_revocation_url_makes_no_network_call, logout_refuses_a_fetcher_and_an_unknown_name, logout_leaves_other_plugins_logins_alone, logout_never_prints_the_tokens)"
        status: pass
    human_judgment: false
  - id: D6
    description: "The auth module and OAuthConfig carry no dead-code allow: every item has a real reader, test-only readers are cfg(test)"
    requirement: AUTH-04
    verification:
      - kind: other
        ref: "! grep -rn 'allow(dead_code' src/plugins/auth; cargo clippy shows no warning in commands/auth.rs or plugins/auth/"
        status: pass
    human_judgment: false

duration: 50min
completed: 2026-11-11
status: complete
---

# Phase 6 Plan 08: fili auth status and logout Summary

**`fili auth status` (lock-free, creation-free, fixed-key `--json`, never a token) and `fili auth logout <plugin>` (local delete under the per-alias lock first, then best-effort RFC 7009 revocation), with the auth module's dead-code allows removed.**

## Performance

- **Duration:** about 50 min (the start time was not captured; estimated from the work done)
- **Completed:** 2026-11-11T08:23:04Z
- **Tasks:** 2 (both TDD, each a RED commit then a GREEN commit)
- **Files modified:** 6

## Accomplishments

- `fili auth status`: one row per source that declares `[host.oauth]` or has a token file, with state (`logged-in`, `expired`, `needs-login`, `not-logged-in`, `unreadable`), relative plus RFC 3339 expiry, and a refresh flag. It reads with `TokenStore::read` (no lock, nothing created) and learns which plugins declare OAuth from a lenient TOML probe that never touches `.env` or a secret.
- Hints after the table: `fili auth login <plugin>` for `needs-login` rows and for expired rows with no refresh token (D-09); the cleaned detail (plugin, path, fixed reason) for `unreadable` rows.
- `fili auth logout <plugin>`: local file deleted under the lock before any network call, then a best-effort revoke (refresh token when stored, else access token). Failures warn on stderr and exit 0. A loose-permission or corrupt file is still deleted, without revoking.
- Removed the module-level `dead_code` allow in `plugins/auth/mod.rs` and the one on `OAuthConfig`; the only two unread items left were test-only and are now `#[cfg(test)]`.

## Task Commits

1. **Task 1: `fili auth status` with a lenient config probe**
   - `4827319` test (RED): stubs plus 14 tests; 12 failed on the stub, the 2 cli parse tests pass because the variant is declarative
   - `cddd3fc` feat (GREEN): probe, collect, render, JSON, status
2. **Task 2: `fili auth logout` and the dead-code allow removal**
   - `192cb13` test (RED): stubs plus 15 tests; 13 failed on the stub
   - `aaf6d38` feat (GREEN): `logout_command`, `perform_logout`, allow removals

**Plan metadata:** committed with this SUMMARY (docs: complete plan).

## Files Created/Modified

- `src/commands/auth.rs` - status (`StatusState`, `StatusRow`, `collect_status`, `describe_expiry`, `render_status`, `render_status_json`, `status`), logout (`logout_command`, `perform_logout`, `revoke_failure_text`), shared `find_source`, dispatch arms, and the tests
- `src/plugins/config.rs` - `probe_oauth_sources`, `OAuthProbeReport`, `OAuthProbe`, `OAuthDeclared`, the lenient `OAuthView`; `OAuthConfig` dead-code allow removed
- `src/cli.rs` - `AuthCommand::Status { json }` and `AuthCommand::Logout { plugin }` with four parse tests
- `src/plugins/auth/mod.rs` - module-level dead-code allow removed
- `src/plugins/auth/callback.rs`, `src/plugins/auth/client.rs` - `CallbackListener::port` and `RefreshFailure::text` gated with `#[cfg(test)]` (test-only readers)

## Decisions Made

See `key-decisions` above. In short: sources with no OAuth and no login stay out of the table; unreadable plugin configs are notes; logout can only revoke when `[host.oauth]` loads, otherwise it deletes and says so.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Two test-only readers gated with `cfg(test)` in files outside `files_modified`**
- **Found during:** Task 2 (removing the module-level allow)
- **Issue:** With the allow gone, rustc warned that `CallbackListener::port` and `RefreshFailure::text` are never used outside tests.
- **Fix:** Added `#[cfg(test)]` to both, exactly as the plan instructs for items only tests use (step 3). No allow added.
- **Files modified:** `src/plugins/auth/callback.rs`, `src/plugins/auth/client.rs`
- **Verification:** `cargo build` clean for `plugins/auth`; `cargo test` 631 + 12 pass; clippy shows no warning in `plugins/auth/`
- **Committed in:** `aaf6d38`

**2. [Rule 1 - Bug] Clippy warnings introduced by the new code**
- **Found during:** Task 2 clippy gate
- **Issue:** a missing-backticks doc warning on `revocation_url` in `cli.rs` (the file's baseline is exactly 3 warnings) and two `format!` appended to `String` in a test
- **Fix:** backticked the identifier; collected the test output in a `Vec<String>`
- **Files modified:** `src/cli.rs`, `src/commands/auth.rs`
- **Committed in:** `aaf6d38`

---

**Total deviations:** 2 auto-fixed (1 blocking, 1 bug)
**Impact on plan:** Both were needed to meet the plan's own clippy and no-allow criteria. No scope creep.

## TDD Gate Compliance

Both tasks have a `test(06-08)` commit before their `feat(06-08)` commit. RED was made real by stubbing the new functions with `todo!()` so the test binary compiles and the tests fail on the stub ("not yet implemented"), not on a build error. Two caveats on the RED evidence:

- The cli parse tests (`auth_status_parses`, `auth_status_json_flag_parses`, `auth_logout_parses`, `auth_logout_requires_a_plugin_name`) pass at RED because the clap variants are declarative and had to exist for the test binary to compile.
- `a_refresh_never_resurrects_a_logged_out_plugin` passes at RED. It exercises the store lock and `TokenProvider` re-read that plan 06-05 and 06-06 already built, not new logout code; it is kept as the regression guard the plan requires (T-06-33).

## Issues Encountered

- `rustfmt` run on `src/cli.rs` and `src/plugins/config.rs` reformatted lines unrelated to this plan; those hunks were reverted so the diff holds only this plan's changes. `src/commands/auth.rs` was formatted whole.
- AUTH-04 is also declared by plan 06-09, which has no SUMMARY yet, so `requirements mark-complete` was intentionally not applied (shared-ID gate); REQUIREMENTS.md is unchanged.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Plan 06-09 can document and end-to-end check `fili auth status` and `fili auth logout`; both commands and their output shapes are final.
- Known limits for 06-09: revocation needs the plugin enabled with a loadable `[host.oauth]` (and its client secret set); the logout reads the stored tokens before taking the lock, so a refresh that rotated the refresh token in that instant would revoke the older one.

## Self-Check: PASSED

- Files: `src/commands/auth.rs`, `src/plugins/config.rs`, `src/cli.rs`, `src/plugins/auth/mod.rs`, `src/plugins/auth/callback.rs`, `src/plugins/auth/client.rs` all present and modified.
- Commits `4827319`, `cddd3fc`, `192cb13`, `aaf6d38` are ancestors of HEAD.
- Plan verify: all 14 + 14 named tests report `ok`; `cargo test -p filimon` 631 + 12 passed, 0 failed; clippy reports no warning for `src/commands/auth.rs` or `src/plugins/auth/` and 3 each for `config.rs` and `cli.rs` (baseline); every acceptance grep passes; `Cargo.toml`, `Cargo.lock` and `wit` unchanged.

---
*Phase: 06-host-managed-oauth*
*Completed: 2026-11-11*
