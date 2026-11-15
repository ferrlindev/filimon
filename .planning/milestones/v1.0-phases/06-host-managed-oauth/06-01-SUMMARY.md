---
phase: 06-host-managed-oauth
plan: 01
subsystem: auth
tags: [oauth, token-store, wasmtime, scheduler, toml, rust]

requires:
  - phase: 01-plugin-contract
    provides: "WIT `poll` export with `access-token: option<string>` (D-07), fixed to none until this phase"
  - phase: 05-plugin-host
    provides: "PluginInstance trait, scheduler loop, run-once, cursor store atomic-write pattern"
provides:
  - "[host.oauth] TOML table parsed into PluginConfig.oauth (authorize_url, token_url, scopes, client_id) with the https endpoint rule"
  - "plugins::auth module: AccessToken/RefreshToken redacting newtypes, StoredTokens, TokenStore (atomic write, lock-free read), TokenProvider with injectable clock, AuthError"
  - "PluginInstance::poll(cursor, access_token): the stored access token reaches the guest; WIT unchanged"
  - "auth_for_poll decision table shared by the scheduler loop and run-once; CycleOutcome::TokenRefresh and TokenFile"
  - "FakeInput recorder (FakeFactory::inputs, poll_tokens) for later no-leak tests"
affects: [06-02, 06-03, 06-04, 06-05, 06-06, 06-07, 06-08, 06-09]

actuals:
  tokens: 18000
  tasks: 2
  commits: 2

tech-stack:
  added: []
  patterns:
    - "Redacting token newtypes with Serialize/Deserialize (Debug prints ***, no Display)"
    - "Token file re-read at every poll; injectable wall clock instead of tokio paused time"
    - "One auth_for_poll decision table, two callers (loop and run-once)"
    - "Corrupt token file disables only that plugin and is never rewritten"

key-files:
  created:
    - src/plugins/auth/mod.rs
    - src/plugins/auth/error.rs
    - src/plugins/auth/store.rs
    - src/plugins/auth/provider.rs
    - src/plugins/auth/testing.rs
  modified:
    - src/plugins/mod.rs
    - src/plugins/config.rs
    - src/plugins/secrets.rs
    - src/plugins/host/instance.rs
    - src/plugins/host/scheduler.rs
    - src/plugins/host/once.rs
    - src/plugins/host/testing.rs
    - src/plugins/host/mod.rs
    - src/commands/plugins.rs

key-decisions:
  - "Token file read has a 64 KiB size cap (same policy as the status file reader), so a runaway file cannot be slurped into memory"
  - "The login hint names the real alias (`run: fili auth login <alias>`) rather than a literal placeholder"
  - "RefreshDue maps to CycleOutcome::TokenRefresh (backoff, instance kept) until plan 06-06 fills the refresh branch"

patterns-established:
  - "auth/mod.rs carries one module-level dead_code allow with a reason that plan 06-08 removes"
  - "Tests that read logs use the current-thread runtime with LogCapture"

requirements-completed: [AUTH-02, AUTH-03]

coverage:
  - id: D1
    description: "A plugin declaring [host.oauth] with a stored unexpired token is polled with that token, TOML to guest (tracer, loop path)"
    requirement: AUTH-03
    verification:
      - kind: integration
        ref: "plugins::host::scheduler::sched_auth::tracer_a_logged_in_plugin_gets_its_access_token_from_toml_to_poll"
        status: pass
    human_judgment: false
  - id: D2
    description: "fili plugins run-once hands the stored access token to poll and reports a corrupt token file naming plugin and path"
    requirement: AUTH-03
    verification:
      - kind: unit
        ref: "plugins::host::once::tests::once_hands_the_stored_access_token_to_poll"
        status: pass
      - kind: unit
        ref: "plugins::host::once::tests::once_reports_a_token_file_problem_naming_plugin_and_path"
        status: pass
    human_judgment: false
  - id: D3
    description: "[host.oauth] parsing with https-only endpoints naming plugin and field"
    requirement: AUTH-03
    verification:
      - kind: unit
        ref: "plugins::config::tests::oauth_table_loads_into_the_plugin_config"
        status: pass
      - kind: unit
        ref: "plugins::config::tests::oauth_endpoints_must_be_https_naming_plugin_and_field"
        status: pass
    human_judgment: false
  - id: D4
    description: "Atomic token file write/read and the provider decision table with the 60 s margin and injected clock"
    requirement: AUTH-02
    verification:
      - kind: unit
        ref: "plugins::auth::store::tests::write_then_read_round_trips_the_tokens"
        status: pass
      - kind: unit
        ref: "plugins::auth::provider::tests (margin, refresh due, injected clock, needs_login, missing, corrupt)"
        status: pass
    human_judgment: false
  - id: D5
    description: "Scheduler behaviour: file re-read per poll, one hint, needs_login keeps the file, corrupt file disables only that plugin, no-oauth plugin untouched"
    requirement: AUTH-03
    verification:
      - kind: integration
        ref: "plugins::host::scheduler::sched_auth (five tests)"
        status: pass
    human_judgment: false

duration: 40min
completed: 2026-11-11
status: complete
plan_head_before: 1fd00e7672093fb93f6c8b849b43a75ad7f70913
plan_head_after: e9f5a4ba5b2874acddf6edfee52aaf48cdad4b20
---

# Phase 6 Plan 01: Stored login reaches the plugin's `poll` Summary

**A plugin's `[host.oauth]` TOML, a per-plugin atomic token file and a clock-injectable token provider now deliver the stored access token to the guest's `poll` in both the scheduler loop and `fili plugins run-once`, with the WIT contract untouched.**

## Performance

- **Duration:** about 40 min
- **Completed:** 2026-11-11
- **Tasks:** 2
- **Files modified:** 14 (5 created, 9 modified)

## Accomplishments

- Tracer path proven end to end: registry TOML -> `PluginSpec::from_loaded` -> `TokenProvider` -> scheduler loop -> `PluginInstance::poll` (test runs under paused tokio time while expiry uses the wall clock).
- `PluginInstance::poll` gained `access_token: Option<&str>`; `WasmtimeInstance` passes it straight into `call_poll`. `git diff` on `wit/`, `Cargo.toml` and `Cargo.lock` is empty.
- `plugins::auth` module: redacting `AccessToken`/`RefreshToken`, `StoredTokens`, `TokenStore` (same-directory temp file, fsync, rename; lock-free read that never creates anything; alias checked with `is_valid_name` before any path is built), `TokenProvider` (re-read each poll; 60 s margin), `AuthError` with `fili::auth::*` codes.
- Scheduler: `CycleOutcome::TokenRefresh` (transient, backoff, instance kept) and `TokenFile` (disables only that plugin; file left byte for byte), one login hint per transition, `auth_for_poll` shared by loop and `run-once`.
- `FakeFactory` now records every `init` and `poll` input (`FakeInput`), ready for plan 06-09's no-leak tests.

## Task Commits

1. **Task 1: A stored login's access token reaches the plugin's `poll`** - `05e3f05` (feat)
2. **Task 2: Pin the decision table and the scheduler behaviour around it** - `e9f5a4b` (test)

**Plan metadata:** committed with this SUMMARY (docs: complete plan)

_Note: Task 2 is `tdd="true"` inside a `type: execute` plan, and its own action says "if everything already holds, this task only adds tests". Its tests passed against Task 1's code on the first run, so there is no RED commit and no GREEN fix commit. The plan is not `type: tdd`, so no TDD gate compliance section applies._

## Files Created/Modified

- `src/plugins/auth/store.rs` - token newtypes, `StoredTokens`, `TokenStore` (atomic write, lock-free read)
- `src/plugins/auth/provider.rs` - `TokenProvider`, `TokenOutcome`, `Clock`, `REFRESH_MARGIN_SECS`
- `src/plugins/auth/error.rs` - `AuthError` (`token_file`, `token_write`, `invalid_name`)
- `src/plugins/auth/testing.rs` - `oauth_config`, `unix_now`, `write_tokens`, `provider_for` (cfg(test))
- `src/plugins/auth/mod.rs` - module docs and the removable dead-code allow
- `src/plugins/config.rs` - `OAuthSection`, `OAuthConfig`, `PluginConfig.oauth`, https endpoint rule, `is_valid_name` made `pub(crate)`
- `src/plugins/host/instance.rs` - `poll` takes the access token
- `src/plugins/host/scheduler.rs` - `PluginSpec.auth`, `from_loaded(source, state_dir)`, new outcomes, `auth_for_poll`, `warn_login_hint`, `sched_auth` tests
- `src/plugins/host/once.rs` - consults `auth_for_poll` before poll
- `src/plugins/host/testing.rs` - `FakeInput`, `inputs()`, `poll_tokens()`
- `src/plugins/host/mod.rs`, `src/commands/plugins.rs`, `src/plugins/secrets.rs`, `src/plugins/mod.rs` - call-site and literal updates

## Decisions Made

- Token files are read with a 64 KiB cap (matching the status-file reader) and an oversize file is a `TokenFile` problem, not a silent truncation.
- The hint text substitutes the real alias so the user can paste the command; the alias already obeys the `[a-z0-9_-]` rule.
- `RefreshDue` is mapped to a transient `TokenRefresh` outcome for now; the real refresh lands in plan 06-06.

## Deviations from Plan

None - plan executed exactly as written. (Task 2's tests needed no production fixes.)

## Issues Encountered

- The repository is not `cargo fmt` clean at baseline (for example `src/cli.rs`), so formatting was applied only to the new `src/plugins/auth/` files and to my own hunks elsewhere.

## Known Stubs

None. `OAuthConfig` fields and the `auth/` module items that nothing reads yet carry documented dead-code allows (module-level in `auth/mod.rs`, which plan 06-08 removes; `cfg_attr(not(test), ...)` on `OAuthConfig`). `RefreshDue` is an intentional seam, filled by plan 06-06.

## Threat Flags

None. All new surface (token file reads, provider) is covered by the plan's threat model (T-06-01 to T-06-05); no new network endpoint or auth path was added.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Seams ready for later plans: `TokenStore`, `TokenOutcome::RefreshDue`, `CycleOutcome::TokenRefresh`/`TokenFile`, `PluginSpec.auth`, `FakeInput` recorder.
- Plan 06-02 adds permission hardening and the per-alias lock on top of `TokenStore`; the read path currently does not check file mode.

## Verification

- `cargo test -p filimon`: 455 + 12 passed, 0 failed (62 + 3 ignored, pre-existing).
- `cargo clippy -p filimon --all-targets`: no warning in any touched file that was clean at baseline; `config.rs`, `secrets.rs`, `host/testing.rs` remain at their 3 baseline warnings each.
- `git diff 1fd00e7..HEAD -- wit Cargo.toml Cargo.lock`: empty.

## Self-Check: PASSED

- FOUND: src/plugins/auth/{mod,error,store,provider,testing}.rs
- FOUND commits: 05e3f05 (feat), e9f5a4b (test), both ancestors of HEAD
- All 18 tests named in the plan report `ok`

---
*Phase: 06-host-managed-oauth*
*Completed: 2026-11-11*
