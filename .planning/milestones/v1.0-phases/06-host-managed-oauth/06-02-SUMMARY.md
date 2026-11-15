---
phase: 06-host-managed-oauth
plan: 02
subsystem: auth
tags: [oauth, token-store, file-permissions, advisory-lock, rust]

requires:
  - phase: 06-host-managed-oauth
    provides: "TokenStore (atomic write, lock-free read), AuthError, sched_auth scheduler tests (plan 06-01)"
provides:
  - "Hardened TokenStore::read: symlink, non-regular file, group/other permission bit, 64 KiB size, version and plugin checks with fixed reasons; never repairs, chmods or deletes"
  - "Token directory created 0700 and token/lock files 0600 from the first byte"
  - "AuthLock and TokenStore::lock: per-alias advisory lock on <state-dir>/auth/<alias>.lock, independent of the state directory lock fili run holds"
  - "TokenStore::mark_needs_login and TokenStore::delete, both taking &AuthLock as proof"
affects: [06-03, 06-04, 06-05, 06-06, 06-07, 06-08, 06-09]

actuals:
  tokens: 21000
  tasks: 2
  commits: 2

tech-stack:
  added: []
  patterns:
    - "Fixed-order read-path checks with fixed refusal reasons that never echo file content"
    - "Lock guard as proof parameter (&AuthLock) on read-modify-write methods"
    - "Non-blocking try_lock polled with tokio::time::sleep and tokio::time::Instant"

key-files:
  created: []
  modified:
    - src/plugins/auth/store.rs
    - src/plugins/host/scheduler.rs

key-decisions:
  - "Any group or other permission bit (mask 0o077) is too loose; ownership is not checked (flagged assumption)"
  - "Symlinks are refused as a hardening step beyond D-02 (flagged assumption, RESEARCH A8)"
  - "lock() waits at most LOCK_WAIT (30 s) polling every 25 ms rather than using the blocking lock call"

patterns-established:
  - "Existing corrupt-file scheduler test now sets 0600 on its fixture so it keeps exercising the content refusal, not the permission refusal"

requirements-completed: [AUTH-02]

coverage:
  - id: D1
    description: "Token file created 0600 in a 0700 directory"
    requirement: AUTH-02
    verification:
      - kind: unit
        ref: "plugins::auth::store::tests::write_creates_a_0600_file_in_a_0700_directory"
        status: pass
    human_judgment: false
  - id: D2
    description: "Loose-permission, symlink, directory, oversized, future-version, wrong-plugin and invalid-JSON files are refused with fixed reasons and left alone"
    requirement: AUTH-02
    verification:
      - kind: unit
        ref: "plugins::auth::store::tests (a_file_looser_than_0600_is_refused_naming_plugin_and_path_and_left_alone, a_symlinked_token_file_is_refused, a_directory_in_place_of_the_file_is_refused, an_oversized_token_file_is_refused, a_future_version_is_refused, a_file_for_another_plugin_is_refused, invalid_json_is_refused_without_echoing_content)"
        status: pass
    human_judgment: false
  - id: D3
    description: "A loose-permission token file disables only that plugin"
    requirement: AUTH-02
    verification:
      - kind: integration
        ref: "plugins::host::scheduler::sched_auth::a_loose_permission_token_file_disables_only_that_plugin"
        status: pass
    human_judgment: false
  - id: D4
    description: "Per-alias lock serialises holders, is released on drop, uses its own file and never the state lock"
    requirement: AUTH-02
    verification:
      - kind: unit
        ref: "plugins::auth::store::tests (lock_serialises_two_holders, a_lock_is_released_when_dropped, lock_creates_the_auth_directory_0700, lock_uses_its_own_file_and_never_the_state_lock, two_aliases_do_not_block_each_other)"
        status: pass
    human_judgment: false
  - id: D5
    description: "mark_needs_login keeps all other fields and the mode; delete removes only one alias's file"
    requirement: AUTH-02
    verification:
      - kind: unit
        ref: "plugins::auth::store::tests (mark_needs_login_*, delete_*)"
        status: pass
    human_judgment: false

duration: 25min
completed: 2026-11-11
status: complete
commits: 2
plan_head_before: e7b74c4b6959303bed731a6ffe9910241aa2f0f0
plan_head_after: 51c8f2fbc1edaa780a8886e9c0f5ca69c68ab9da
---

# Phase 6 Plan 02: Hardened token file and per-alias lock Summary

**The token file is now only ever a small 0600 regular file this host wrote (anything else is refused by name and left alone), and login, refresh and logout get a per-alias advisory lock plus lock-guarded `mark_needs_login` and `delete` that never touch the state lock `fili run` holds.**

## Performance

- **Duration:** about 25 min (most of it a cold build in the fresh worktree)
- **Completed:** 2026-11-11
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments

- `TokenStore::read` runs fixed-order checks (`symlink_metadata`, regular file, mode `& 0o077`, size, then JSON, version, plugin match), each a `TokenFile` error naming plugin and path with a fixed reason; invalid JSON never echoes content. No branch writes, chmods or deletes.
- `TokenStore::write` creates the `auth` directory 0700 (`DirBuilder` mode, existing directories untouched) and sets the temp file 0600 before any byte is written.
- `AuthLock` and async `TokenStore::lock`: own file `<alias>.lock` (0600), non-blocking `try_lock` polled every 25 ms for up to 30 s, timed with `tokio::time::Instant`.
- `mark_needs_login` (keeps every field and mode, `Ok(false)` with no file) and `delete` (never reads, works on a file `read` refuses), both taking `&AuthLock`.
- Scheduler: a 0644 token file disables only its own plugin; the file stays 0644 and byte-identical.

## Task Commits

1. **Task 1: Refuse unsafe token files and create files and directory tight** - `79a49d3` (feat)
2. **Task 2: The per-alias lock, mark_needs_login and delete** - `51c8f2f` (feat)

_Note: both tasks are `tdd="true"` inside a `type: execute` plan. Tests and code were written together and committed per task, so there are no separate RED commits; the plan is not `type: tdd`, so no TDD gate compliance section applies._

## Files Created/Modified

- `src/plugins/auth/store.rs` - hardened read/write, `AuthLock`, `lock`, `mark_needs_login`, `delete`, 23 store tests
- `src/plugins/host/scheduler.rs` - `sched_auth::a_loose_permission_token_file_disables_only_that_plugin`; the existing corrupt-file test's fixture is chmod 600

## Decisions Made

- Permission rule is mask `0o077` with no ownership check; symlinks refused. Both were flagged assumptions in the plan; the user has not ruled.
- Non-Unix platforms skip the mode checks and their tests.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Existing corrupt-file scheduler test would have exercised the wrong refusal**
- **Found during:** Task 1
- **Issue:** `a_corrupt_token_file_disables_only_that_plugin` (plan 06-01) wrote its invalid-JSON fixture with the default 0644 mode, which the new permission check now refuses first, so the test would no longer prove the content refusal.
- **Fix:** the fixture is chmod 0600 so the test still pins "not a valid token file".
- **Files modified:** src/plugins/host/scheduler.rs
- **Commit:** 79a49d3

The `token_types_never_print_their_value` test (from 06-01) was tightened to also assert the refresh token sentinel and `***`, and `an_alias_that_fails_the_name_rule_never_builds_a_path` was renamed to the plan's `an_invalid_alias_is_refused_before_any_path_is_built`. Otherwise the plan ran as written.

## Issues Encountered

None. The worktree needed a full cold compile (about 6 min) on first test run.

## Known Stubs

None.

## Threat Flags

None. T-06-06, T-06-07, T-06-08 are mitigated as planned; no new endpoint or auth path.

## Next Phase Readiness

- `AuthLock`, `mark_needs_login` and `delete` are ready for login (06-03/04), refresh (06-06) and logout plans. Items unused outside tests remain covered by the module-level dead-code allow in `auth/mod.rs`.

## Verification

- `cargo test -p filimon`: 475 + 12 passed, 0 failed.
- `cargo clippy -p filimon --all-targets`: no warning in `src/plugins/auth/` or `scheduler.rs`.
- `git diff -- Cargo.toml Cargo.lock wit`: empty. No `CursorStore` reference under `src/plugins/auth`.

## Self-Check: PASSED

- FOUND: src/plugins/auth/store.rs, src/plugins/host/scheduler.rs
- FOUND commits: 79a49d3, 51c8f2f (ancestors of HEAD)
- All 22 tests named in the plan report `ok`

---
*Phase: 06-host-managed-oauth*
*Completed: 2026-11-11*
