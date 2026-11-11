---
phase: 06-host-managed-oauth
plan: 09
subsystem: auth
tags: [oauth, wasm, wasmtime, sentinel-tests, cli, readme]

requires:
  - phase: 06-host-managed-oauth
    provides: "plan 06-06 token provider in the scheduler and run-once; plan 06-08 fili auth status and logout"
provides:
  - "SC3 proven twice with sentinels: scripted fake through the real scheduler (rotating refresh) and the real stub-source component through Host::run and poll_once"
  - "stub-source opt-in record_inputs setting (fixture only, no new dependency, WIT untouched)"
  - "tests/auth_cli.rs: ten binary-level tests for fili auth usage, login refusals, status and logout"
  - "plugins/README.md OAuth section documenting the feature as built"
affects: [07-reddit-youtube-plugins, phase-verification]

actuals:
  tokens: 11500
  tasks: 3
  commits: 6
plan_head_before: 018777409d05d7a3c4beec28f730454559b58685
plan_head_after: 66bf3220474c944eae938692351c0da818e4eafc

tech-stack:
  added: []
  patterns:
    - "Recording guest fixture: opt-in setting returns what the guest received in the item raw, read back by the host test"
    - "Non-vacuous absence tests: assert the host really held and used the secret (Basic auth header, rotated file) before asserting the plugin never saw it"

key-files:
  created:
    - tests/auth_cli.rs
  modified:
    - src/plugins/host/scheduler.rs
    - src/plugins/host/wasm_tests.rs
    - plugins/stub-source/src/lib.rs
    - plugins/README.md

key-decisions:
  - "The wasm_auth registry writes a real .env into a temp dir and relies on load_test_registry's .env fallback, so the secret path is the production one"
  - "Binary tests write token files as raw JSON (fields of the store's TokenFile) instead of linking the crate, since fili is a binary-only crate"

patterns-established:
  - "Sentinel strings (AT-/RT-/CS-SENTINEL-*) asserted absent on every stream or recorded input"

requirements-completed: [AUTH-03, AUTH-04]

duration: ~55min
completed: 2026-11-11
status: complete
---

# Phase 6 Plan 09: Prove AUTH-03/AUTH-04 and Document OAuth Summary

**Sentinel recording tests prove a plugin holds only an access token (fake scheduler run through a rotating refresh, and the real stub-source wasm through Host::run and run-once), ten binary tests drive fili auth, and the plugins README documents the whole OAuth feature.**

## Performance

- **Duration:** ~55 min (build time dominated: first cold `cargo test` in the worktree took 3 min)
- **Completed:** 2026-11-11
- **Tasks:** 3
- **Files modified:** 5 (1 created)

## Accomplishments

- SC3 at the fake level: `sched_auth::the_plugin_never_receives_the_refresh_token_or_the_client_secret` runs the real scheduler with a `FakeFactory` over a loaded registry (`[host.secrets]` plus `[host.oauth]` with `client_secret_env`), through a refresh that rotates the refresh token. The recorded inputs plus the spec Debug text contain `AT-NEW` and `API-VALUE-1` and none of `RT-SENTINEL-7f3a`, `RT-ROTATED-0d1e`, `CS-SENTINEL-91bc`. The test also shows the token request carried the client secret as Basic auth and the file on disk holds the rotated refresh token, so the absence is not vacuous.
- SC3 on real wasm: `stub-source` gained an opt-in `record_inputs` setting; three `wasm_auth_*` tests show the guest sees the access token and its own secret and never the refresh token or client secret, through `Host::run` and `poll_once` (the run-once path), and sees `access_token=None` with no login.
- AUTH-04 through the real binary: `tests/auth_cli.rs` (10 tests): `fili auth` alone is exit 2; unknown plugin, plugin without `[host.oauth]`, fetcher refused with exit 1 and no state dir created; `status` table and `--json` array with no banner and no token; works while `<state>/.lock` is held; creates nothing; corrupt (0600) and loose (0644) token files show `unreadable` with no sentinel on either stream; `logout` deletes the file and the second says there is none.
- `plugins/README.md`: new `## OAuth` section (declaring, logging in and out, where tokens live, what a plugin receives, refresh and failures, provider notes, known limits) and the corrected `poll` access-token bullet.

## Task Commits

1. **Task 1: SC3 fake level** - `0368e49` (test). Characterization test of already-built behavior; see TDD Gate Compliance.
2. **Task 2: SC3 real component**
   - RED `9b8e9b9` (test): three `wasm_auth_*` tests
   - GREEN `bba2394` (feat): `record_inputs` in `stub-source`
3. **Task 3: fili auth binary + docs**
   - `49d8f77` (test): `tests/auth_cli.rs`
   - `c7ec5fe` (refactor): `write!` instead of `push_str(&format!(..))` to clear a clippy warning in the new file
   - `66bf322` (docs): README OAuth section and corrected bullet

**Plan metadata:** committed after this file (docs: complete plan).

## TDD Gate Compliance

- **Task 2 (real RED then GREEN):** RED run against the unmodified fixture (`stub_source.wasm` from the Phase 3 build). All three `wasm_auth` tests failed because `init` rejected the new `record_inputs` key: `PollError::Config("... unknown field `record_inputs`, expected one of `greeting`, `fetch_url`, `fetch_headers`")`; `poll_once` panicked on that error and the two `Host::run` tests saw zero polls (`left: 0, right: 1`). Semantic assessment: the target tests executed and failed for the intended reason (the planned fixture feature is absent). The registry, `.env` secret resolution, fixture load and token file all worked, so no setup or import fault. The classifier `gsd_run check tdd-red-evidence` was not run: `cargo test` console output is not one of its supported report formats and no reporter rerun was attempted. GREEN after `cargo component build --release`: 3 passed; the whole `--ignored wasm_` group: 44 + 3 passed, 0 failed.
- **Tasks 1 and 3 (no RED commit):** both prove behavior built in plans 06-06 to 06-08 (scheduler token path, `fili auth` commands, covered by unit tests there). Their new tests passed on first run, which is the expected and intended outcome for sentinel/integration proof of existing behavior, not an "unexpected green" of a missing feature. Their non-vacuity comes from in-test assertions (Basic auth header, rotated file, every status state, deleted file), not from a failing commit. Flagged here so the TDD review does not read it as a missing RED for these two tasks.

## Verification Results

- `cargo test -p filimon plugins::host::scheduler`: SC3 test `ok`
- `cargo test -p filimon -- --ignored wasm_auth`: 3 passed. `-- --ignored wasm_`: 44 + 3 passed, 0 failed
- `cargo test -p filimon --test auth_cli`: 10 passed
- `cargo test -p filimon` (plan's exact check, no `FAILED`/`N failed`/`^error`): 632 + 10 + 12 passed, 65 + 3 ignored (live Fluvio, wasm fixtures), 0 failed
- `cargo clippy -p filimon --all-targets`: plan's exact criterion passes; no warning in `auth/`, `scheduler.rs`, `once.rs`, `instance.rs`, `commands/auth.rs`, `commands/plugins.rs`, `tests/auth_cli.rs`, `wasm_tests.rs`; `config.rs`, `secrets.rs`, `host/testing.rs` still at their 3 baseline warnings
- `git diff` against the plan base for `examples`, `wit`, `Cargo.toml`, `Cargo.lock`, `plugins/stub-source/Cargo.toml`, `plugins/stub-source/Cargo.lock`: empty
- README acceptance greps: `^## OAuth`, `17420`, `authorize_params`, `needs_login`, `fili auth status` all match; `until the OAuth phase` has 0 matches

## Files Created/Modified

- `src/plugins/host/scheduler.rs` - `sched_auth` SC3 test and `secret_registry` helper
- `src/plugins/host/wasm_tests.rs` - `auth_registry`, `recorded_text`, `assert_no_host_credentials` and three `wasm_auth_*` tests
- `plugins/stub-source/src/lib.rs` - opt-in `record_inputs` setting, `thread_local` for what `init` received
- `tests/auth_cli.rs` - ten binary tests, helpers for registries and prepared token files
- `plugins/README.md` - OAuth section, corrected bullet

## Decisions Made

- The `wasm_auth` tests put `STUB_API_KEY` and `STUB_CLIENT_SECRET` in a `.env` in the temp registry directory and rely on `load_test_registry` (which passes `|_| None` as env, so the `.env` fallback is the only source). That exercises the production secret path rather than a closure.
- `tests/auth_cli.rs` builds token files as raw JSON rather than through `TokenStore`, because `fili` is a binary-only crate with no library target for an integration test to link; this also checks the on-disk shape the README documents.
- README states that an invalid `[host.oauth]` stops `fili run` (like any invalid plugin TOML) and that `enabled = false` skips it, matching the strict loader rather than the plan's looser phrasing.

## Deviations from Plan

None - plan executed exactly as written, apart from small implementation notes below that did not change any criterion.

- **Implementation note:** the plan describes `load_test_registry` as reading the `.env` beside the registry; it does so via the loader's fallback with an env closure that returns nothing, which is what the tests rely on.
- **Implementation note (Rule 1, own new file):** the first `tests/auth_cli.rs` used `push_str(&format!(..))`, which clippy flagged; fixed in `c7ec5fe`, and a missing `PermissionsExt` import fixed before the first commit of that file.

**Total deviations:** 0 auto-fixed beyond the above notes.

## Issues Encountered

- The worktree had no `plugins/*/target` fixtures (gitignored, untracked). Copied the four prebuilt `.wasm` files from the main checkout's `target` dirs (read-only source) to get the old-fixture RED, then rebuilt `stub-source` in the worktree with `cargo component build --release` (12 s) for GREEN. Nothing under `plugins/*/target` is tracked.
- `cargo fmt --check` reports pre-existing differences in `scheduler.rs` (not from this plan); formatting was not changed to keep the diff to the plan's scope.

## Human Check Still Open

The plan's manual UAT cannot be automated (needs a registered provider app and a browser). It remains for the verifier or user:

1. Register an OAuth app (for example a Google Desktop client or a Reddit installed app) with redirect URI `http://127.0.0.1:17420/callback`.
2. Add a source whose plugin TOML declares `[host.oauth]` for that provider (with `client_secret_env` and a `.env` entry if needed; `authorize_params = { duration = "permanent" }` for Reddit).
3. `fili auth login <plugin>`: URL printed, browser opens, terminal reports login and expiry.
4. `fili auth status`: `logged-in`, refresh token shown as `yes`, no token text; `ls -l <state-dir>/auth/<plugin>.json` is `-rw-------`.
5. `fili auth login <plugin> --no-browser`: prints the URL and waits.
6. `fili auth logout <plugin>`: file gone, `status` shows `not-logged-in`, and (with a revocation endpoint) the grant disappears at the provider.

## Known Stubs

None. No hardcoded empty values, placeholders or unwired data in the files changed.

## Threat Flags

None. No new network endpoint, auth path or trust-boundary file access beyond the plan's threat model (T-06-35, T-06-36 and T-06-SC are mitigated: sentinel tests, README placeholders only with `examples/` untouched, no dependency change).

## Requirements

`requirements-completed: [AUTH-03, AUTH-04]` per the plan. `.planning/REQUIREMENTS.md` was not touched in this worktree: AUTH-03 and AUTH-04 are declared by other plans of this phase too, so the shared-ID gate and the central write belong to the orchestrator after merge.

## Next Phase Readiness

- Phase 6 plans are complete after this plan; the phase is ready for verification, with the manual provider check above as the one open human item.
- Phase 7 (Reddit, YouTube) can write `[host.oauth]` against the documented keys; the Reddit app-only versus PKCE choice remains a Phase 7 research item, noted in the README.

## Self-Check: PASSED

- Created/modified files exist: `tests/auth_cli.rs`, `plugins/README.md`, `plugins/stub-source/src/lib.rs`, `src/plugins/host/scheduler.rs`, `src/plugins/host/wasm_tests.rs`
- Commits are ancestors of HEAD: `0368e49`, `9b8e9b9`, `bba2394`, `49d8f77`, `c7ec5fe`, `66bf322`

---
*Phase: 06-host-managed-oauth*
*Completed: 2026-11-11*
