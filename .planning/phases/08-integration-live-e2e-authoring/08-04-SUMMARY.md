---
phase: 08-integration-live-e2e-authoring
plan: 04
subsystem: testing
tags: [rust, wasm-components, live-e2e, credentials, dotenv, fluvio]
requires:
  - phase: 08-01
    provides: Backend enum, replay harness, drive_fluvio, fluvio_or_skip
provides:
  - Credential-gated live backend on the shared host test body (Backend::Live)
  - Gating helpers (missing_key, skip_line, lookup_env, load_live_env) with the documented skip line
  - Repository-root .env.example with the four live keys
  - plugins/README.md documentation of test groups 6 and 7 and the replay-fetcher and template plugins
affects: [08-06 human live run, docs/plugin-guide.md]
tech-stack:
  added: []
  patterns: [runtime skip instead of ignore, secrets only through the loader env closure, fixed-text failure messages]
key-files:
  created: [.env.example]
  modified: [src/plugins/host/integration_tests.rs, plugins/README.md]
key-decisions:
  - "Live keys go in a new repository-root .env.example, not examples/plugins/.env.example, because sample_env_example_lists_every_declared_secret pins the sample file"
  - "Live failure text is fixed (platform name only); the drive error is mapped to 'the live <platform> poll failed' so no request, response, header or item can leak"
patterns-established:
  - "Credentialed tests return before building anything or touching Fluvio when a key is missing"
requirements-completed: [TEST-03]
duration: 40min
completed: 2026-11-12
status: complete
actuals:
  tokens: 28000
  tasks: 3
  commits: 3
plan_head_before: b4da86a1162ba61461ea50dfdf855fad7aba02a5
plan_head_after: cff00601da8cef4113964085646119c01a886f98
commits: 3
---

# Phase 8 Plan 04: Live credentialed mode Summary

The 08-01 host test body now has a live backend: with Reddit and YouTube keys in the environment or the repository `.env`, the same run goes through the `http` fetcher to the real APIs and asserts structure only; without them each test prints `skipped: missing <KEY>` and passes.

## What was built

- **Task 1** (b0dd0bf): `missing_key`, `skip_line`, `lookup_env`, `load_live_env_from`, `load_live_env` (honours `FILI_TEST_ENV_FILE`, default `<repo>/.env`, read by the literal `read_dotenv` parser; a rejected file is treated as absent with a fixed note); six gate tests; root `.env.example`.
- **Task 2** (eae7c71): `Backend::Live(HashMap)` with `build_live_registry` (http fetcher, app-only `reddit` via `client_credentials`, `youtube` via API key; secrets only in the env closure map, the public client id is the only credential-derived file content); `live_structure_problem`; `e2e_credentialed_reddit_live_poll_...` and `..._youtube_live_poll_...`; the no-secret-on-disk test; the structure-check test. One poll per test (interval 3600 s, host cancelled after the first delivered batch).
- **Task 3** (cff0060): README entries for `replay-fetcher/` and `template/`, build and conformance lines, test groups 6 and 7, `FILI_TEST_REQUIRE_FLUVIO`, `FILI_TEST_ENV_FILE`, skip lines, first-run cost, and that a skip is not a pass.

## Verification

- `FILI_TEST_ENV_FILE=/dev/null cargo test -p filimon e2e_credentialed_ -- --nocapture`: 10 passed; printed `skipped: missing REDDIT_CLIENT_ID` and `skipped: missing YOUTUBE_API_KEY`. (Shell had no credential variables set; `env -u ...` form was blocked by the sandbox wrapper, so the equivalent plain invocation was used.)
- `sample_env_example_lists_every_declared_secret` still passes; `examples/plugins/.env.example` untouched.
- README verify greps pass; README diff is 55 added, 5 removed lines.
- The registry-files test built the real http-fetcher, reddit and youtube components.

## Not proven here

The positive live path (real keys against the real APIs) and the cluster-backed live read-back were not run: no credentials and no Fluvio cluster in this environment. They belong to the human run in plan 08-06. The live tests skip, so a green run here is not a pass of TEST-03's live behaviour.

## Deviations from Plan

None in behaviour. Notes:
- `cargo fmt -p filimon` reformatted unrelated files; those changes were reverted with per-file `git checkout --` before committing, so only plan files are in the commits.
- `load_registry` gained a `backend` parameter so a live registry load failure uses a fixed message (Rule 2, credential safety).

## Known Stubs

None.

## Threat Flags

None.

## Self-Check: PASSED

- .env.example, src/plugins/host/integration_tests.rs, plugins/README.md present; commits b0dd0bf, eae7c71, cff0060 reachable from HEAD.
