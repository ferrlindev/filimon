---
phase: 06-host-managed-oauth
plan: 07
subsystem: auth
tags: [oauth, pkce, login, cli, clap, open, rust]

requires:
  - phase: 06-host-managed-oauth
    provides: "CallbackListener, PKCE helpers (plan 06-04); TokenClient::exchange_code, TokenSet (plan 06-05); TokenStore with lock and atomic write (plan 06-02); OAuthConfig with authorize_params and redirect_port (plan 06-03)"
provides:
  - "auth/login.rs: LoginOptions, LoginHooks, LoginSummary and the async login flow"
  - "commands/auth.rs: dispatch (run_auth), login_command, private perform_login seam and CliHooks"
  - "cli.rs: Command::Auth and AuthCommand::Login { plugin, no_browser }"
  - "main.rs: the Command::Auth arm; no banner for auth commands"
  - "open 5.4.5 as a direct dependency (lock gains open, is-docker, is-wsl)"
affects: [06-08, 06-09]

plan_head_before: 1826ed9396a455e35806e3e5d5178a83bb216549
plan_head_after: 7c8fcfa7a7d1e8c477ffa8aa58afef223e91a515

actuals:
  tokens: 13000
  tasks: 2
  commits: 3

tech-stack:
  added: [open 5.4.5]
  patterns:
    - "Login flow behind a two-method hooks trait so tests drive the callback over a real loopback socket with no browser"
    - "Write-last ordering: bind, URL, wait, exchange, then lock and write; every earlier failure leaves the filesystem untouched"
    - "perform_login seam takes an OAuthConfig and an opener closure so the CLI is testable against an http fake token endpoint"

key-files:
  created:
    - src/plugins/auth/login.rs
    - src/commands/auth.rs
  modified:
    - Cargo.toml
    - Cargo.lock
    - src/plugins/auth/mod.rs
    - src/cli.rs
    - src/main.rs
    - src/commands/mod.rs
    - .planning/REQUIREMENTS.md

key-decisions:
  - "login never carries over an older refresh token: it belongs to another grant"
  - "A failed browser launch prints a note on stderr and also the waiting line on stdout, so the user is never left with a silent wait"
  - "Write errors on stdout/stderr inside the hooks are ignored so a closed terminal cannot abort a login; the final success and warning lines do propagate a write failure"

patterns-established:
  - "CLI commands over OAuth plugins refuse unknown, fetcher, disabled and no-[host.oauth] names before opening any socket or creating any directory"

requirements-completed: [AUTH-01]

coverage:
  - id: D1
    description: "fili auth login runs the whole PKCE flow and stores a 0600 login (needs_login false) only after a successful exchange"
    requirement: AUTH-01
    verification:
      - kind: unit
        ref: "plugins::auth::login::tests::login_stores_the_tokens_0600_after_a_matching_callback"
        status: pass
      - kind: unit
        ref: "plugins::auth::login::tests::the_exchange_carries_the_verifier_that_matches_the_authorize_challenge"
        status: pass
    human_judgment: false
  - id: D2
    description: "Every failure (state mismatch, denial, timeout, rejected exchange, busy port) writes nothing and leaves an existing login byte for byte as it was"
    requirement: AUTH-01
    verification:
      - kind: unit
        ref: "plugins::auth::login::tests (a_state_mismatch_writes_nothing_and_leaves_an_existing_login_alone, a_denied_login_writes_nothing, a_timeout_writes_nothing, a_failed_exchange_writes_nothing, a_second_login_on_the_same_port_fails_naming_the_key)"
        status: pass
    human_judgment: false
  - id: D3
    description: "The authorize URL carries authorize_params; the URL is printed and the browser tried unless --no-browser; a failed launch is a note, not a failure"
    requirement: AUTH-01
    verification:
      - kind: unit
        ref: "plugins::auth::login::tests::the_authorize_url_carries_authorize_params"
        status: pass
      - kind: unit
        ref: "commands::auth::tests (login_prints_the_url_and_a_success_note_and_stores_the_file, no_browser_never_calls_the_opener, a_failed_browser_launch_prints_a_note_and_the_login_still_completes)"
        status: pass
    human_judgment: false
  - id: D4
    description: "A login with no refresh token is reported; a missing expires_in is stored as 3600 seconds"
    requirement: AUTH-01
    verification:
      - kind: unit
        ref: "plugins::auth::login::tests (a_login_without_a_refresh_token_is_reported, login_uses_the_default_lifetime_when_expires_in_is_absent)"
        status: pass
      - kind: unit
        ref: "commands::auth::tests::a_login_without_a_refresh_token_warns_on_stderr"
        status: pass
    human_judgment: false
  - id: D5
    description: "No code, verifier, token or client secret reaches a hook, the summary, stdout, stderr or a rendered error"
    requirement: AUTH-01
    verification:
      - kind: unit
        ref: "plugins::auth::login::tests::nothing_secret_reaches_the_hooks_or_the_summary"
        status: pass
      - kind: unit
        ref: "commands::auth::tests::nothing_secret_is_printed"
        status: pass
    human_judgment: false
  - id: D6
    description: "fili auth is wired: clap parsing, plugin-name requirement, refusals naming the plugin, and no banner on stdout"
    requirement: AUTH-01
    verification:
      - kind: unit
        ref: "cli::tests (auth_requires_a_subcommand, auth_login_parses, auth_login_no_browser_flag_parses, auth_login_requires_a_plugin_name); tests::banner_is_skipped_for_auth_commands"
        status: pass
      - kind: unit
        ref: "commands::auth::tests (login_refuses_an_unknown_plugin_listing_the_registered_ones, login_refuses_a_fetcher, login_refuses_a_disabled_source, login_refuses_a_plugin_without_host_oauth)"
        status: pass
    human_judgment: false
  - id: D7
    description: "A real fili auth login against a registered provider app in a real browser"
    requirement: AUTH-01
    verification: []
    human_judgment: true
    rationale: "Browser launch, the provider's consent page and a real redirect cannot be asserted by a unit test; plan 06-09 records this as manual UAT"

duration: 55min
completed: 2026-11-11
status: complete
---

# Phase 6 Plan 07: fili auth login Summary

**`fili auth login <plugin> [--no-browser]` runs the whole PKCE flow (loopback listener, authorize URL with authorize_params, code exchange with the matching verifier) and writes the 0600 token file only after the exchange succeeded, printing no secret.**

## Performance

- **Duration:** about 55 min, mostly cold `cargo` builds in the new worktree
- **Completed:** 2026-11-11
- **Tasks:** 2 (both `tdd="true"`)
- **Files modified:** 9 (2 created)

## Accomplishments

- `login()` composes bind, authorize URL, hooks, wait, exchange, then `store.lock()` and `store.write()` in that order. Nothing is locked, written or created before the exchange succeeds, so a mismatch, denial, timeout or rejected exchange leaves an existing login byte for byte unchanged (and creates no `auth` directory when there was none).
- The `LoginHooks` trait (two infallible methods) is the only seam; hooks receive only the authorize URL (public parameters).
- `commands/auth.rs` checks the plugin first (unknown name lists the registered sources; fetcher, disabled source and no-`[host.oauth]` each name the plugin) and only then loads the scoped config, resolves the state directory and starts the login. It never touches `CursorStore` or the state-directory lock.
- Success prints the RFC 3339 expiry and the "picks up on next poll after any backoff" note; a missing refresh token adds a stderr warning with the end time and the `authorize_params` hint.
- `fili auth` is wired in clap and `main.rs`, with the banner suppressed so stdout holds only the command's output.
- `open` 5.4.5 added for `open::that_detached`; the lock gained exactly `open`, `is-docker` and `is-wsl` (526 to 529 packages).

## Task Commits

1. **Task 1: login flow behind hooks** - `6272163` (test, RED), `3e63659` (feat, GREEN)
2. **Task 2: `fili auth login` wired into the CLI** - `7c8fcfa` (feat)

**Plan metadata:** committed with this summary (docs: complete plan)

## TDD Gate Compliance

- **Task 1 RED:** 12 tests written against a compiling stub of `login` that returned `CallbackInvalid`. All 12 failed (0 passed) on their planned assertions: each reached the `login(...)` result or a stored-file/URL assertion and saw `Err(CallbackInvalid { reason: "not implemented" })` instead of the planned outcome; none failed on a compile or fixture error. Semantic assessment: valid RED. `gsd_run check tdd-red-evidence` was not run (plan is `type: execute`, `workflow.tdd_mode` not enabled; the Rust output is not a supported report format).
- **Task 1 GREEN:** all 12 pass.
- **Task 2:** a compile-level RED does not exist for new CLI variants and a new module (tests cannot compile without the code), so tests and implementation share one commit, as in plans 06-04. No `type: tdd` gate applies.

## Files Created/Modified

- `src/plugins/auth/login.rs` - the flow, hooks trait, options, summary and 12 tests
- `src/commands/auth.rs` - dispatch, `login_command`, `perform_login`, `CliHooks` and 9 tests
- `src/cli.rs` - `Command::Auth`, `AuthCommand::Login` and 4 parse tests
- `src/main.rs` - the `Auth` arm, banner exclusion and its test
- `src/commands/mod.rs` - `pub mod auth;` and `run_auth`
- `src/plugins/auth/mod.rs` - `pub mod login;` and the header line
- `Cargo.toml`, `Cargo.lock` - `open = "5"` (plus `is-docker`, `is-wsl` in the lock)
- `.planning/REQUIREMENTS.md` - AUTH-01 marked complete (this is the last plan of the phase declaring it; every sibling already has a SUMMARY)

## Decisions Made

See `key-decisions`. All four flagged assumptions in the plan were followed as written: an existing login is simply replaced after a successful exchange, a login with no refresh token is stored and reported, the success note mentions the poll-loop lag, and Ctrl-C leaves nothing written.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] `rustfmt` run on one file reformatted the whole crate**
- **Found during:** Task 1 GREEN
- **Issue:** `cargo fmt -- <file>` formats every file in the workspace; the baseline is not rustfmt-clean, so 24 unrelated files changed.
- **Fix:** reverted the 24 unrelated files path by path with `git checkout -- <file>` before any commit; only `login.rs` kept its formatting. Later formatting used `rustfmt --edition 2024 src/commands/auth.rs` only.
- **Verification:** `git status --short` showed only the intended files before each commit.

**2. [Rule 1 - Test name] Plan's `no_browser_never_calls_the_opener` is used in two modules**
- Both `plugins::auth::login::tests::no_browser_never_calls_the_opener` and `commands::auth::tests::no_browser_never_calls_the_opener` exist, as the plan lists; no change needed, noted so the verify loops (which grep module-qualified names) are read correctly.

---

**Total deviations:** 1 auto-fixed (1 blocking, process only). **Impact:** none on the shipped code.

## Issues Encountered

- Cold build of the worktree's `target/` took several minutes (run once; later builds incremental).
- `redirect_port` busy error: the `host.oauth.redirect_port` key lives in the miette `help` text of `RedirectPortInUse` (plan 06-04 note), so the test asserts over Display plus help. `fili auth login` renders the diagnostic through miette's `Report`, so the user sees the help.

## Verification Results

- `cargo test -p filimon plugins::auth::login`: 12 passed.
- `cargo test -p filimon`: 577 passed (plus 12 in the second binary), 0 failed, 62 ignored (pre-existing).
- The plan's task 2 automated name loop: no "missing" lines; all 14 named tests report `ok`.
- `cargo clippy -p filimon --all-targets`: `Finished`; no warning under `src/commands/auth.rs` or `src/plugins/auth/`; `cli.rs` and `main.rs` show 3 each (baseline).
- Acceptance greps all pass (write-after-exchange `awk` order check, no `tracing::` in `login.rs`, no `CursorStore` in `auth.rs`, `open` in `Cargo.toml` and `Cargo.lock`, 529 packages at most 532); `git diff --quiet -- wit` passes.

## Known Stubs

None.

## Threat Flags

None. The new surface (browser launch, loopback listener use, token endpoint) is covered by T-06-26 to T-06-30 and T-06-SC: the opener receives only the host-built authorize URL through `open::that_detached`; nothing from the callback or network reaches it. The package install succeeded normally, so no package-verification checkpoint was needed.

## User Setup Required

None - no external service configuration required. A real provider app registration is part of manual UAT in plan 06-09.

## Next Phase Readiness

- Plan 06-08 adds `status` and `logout` to `commands/auth.rs` (same `dispatch` match, same `find`-style lookups) and removes the `#![allow(dead_code)]` in `auth/mod.rs`.
- Plan 06-09 records the manual UAT of `fili auth login` against a real provider.

---
*Phase: 06-host-managed-oauth*
*Completed: 2026-11-11*

## Self-Check: PASSED

- FOUND: src/plugins/auth/login.rs, src/commands/auth.rs
- FOUND commits: 6272163, 3e63659, 7c8fcfa (ancestors of HEAD)
- All 12 flow tests and 14 CLI/command tests report `ok`
