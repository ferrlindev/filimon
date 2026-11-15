---
phase: 07-reddit-youtube-plugins
plan: 08
subsystem: plugins/youtube
tags: [youtube, quota, error-classification, fixtures, tdd]
requires: [07-05]
provides:
  - classify_youtube (status plus safe Google reason to typed PollError)
  - exact D-15 quota transient text
  - YouTube fixture manifest (D-18)
affects: [07-09, 07-12]
tech-stack:
  added: []
  patterns: [typed error classification before parsing, strict recording mock]
key-files:
  created:
    - plugins/youtube/tests/fixtures_manifest.rs
    - plugins/youtube/tests/fixtures/quota_exceeded_403.json
    - plugins/youtube/tests/fixtures/quota_exceeded_403.meta.json
    - plugins/youtube/tests/fixtures/unauthorized_401.json
    - plugins/youtube/tests/fixtures/unauthorized_401.meta.json
    - plugins/youtube/tests/fixtures/malformed.json
  modified:
    - plugins/youtube/src/api.rs
    - plugins/youtube/src/poll.rs
    - plugins/youtube/tests/replay.rs
    - plugins/youtube/tests/fixtures/README.md
key-decisions:
  - "A Google error reason is echoed only when it matches [A-Za-z0-9_]{1,40}; otherwise it is dropped from the error text."
  - "404 is a Config error naming settings.channels and the channel id (the UC to UU swap is undocumented, so a missing playlist must be named)."
  - "classify_youtube runs before page parsing; the 200-body unreadable-page error from 07-05 is kept."
requirements-completed: [YTUB-03, TEST-01]
status: complete
commits: 3
plan_head_before: 042472605e9b78dc7a53753f2ae432d8b3abbed0
plan_head_after: 07ee0fd6aeeb9d26b17d2dcdc22bf6807479d442
actuals:
  tokens: 14000
  tasks: 2
  commits: 3
duration: ~20 min
completed: 2026-11-11
---

# Phase 7 Plan 08: YouTube quota and error classification Summary

YouTube polls now classify every non-2xx response into a named typed error, with the quota-exceeded case returning the exact D-15 transient text, plus credential-mode tests, a low-cost request sweep and a fixture-manifest test.

## What was built

- `api.rs`: `QUOTA_EXCEEDED_TEXT`, `error_reason` (safe alphabet, 1 to 40 characters) and `classify_youtube`. `quotaExceeded` and `dailyLimitExceeded` give the D-15 `Transient`; 429 and `rateLimitExceeded`/`userRateLimitExceeded` give `RateLimited(None)`; 401 is `AuthRequired`; `keyInvalid`, `accessNotConfigured`, `insufficientPermissions`, `forbidden` and 404 are `Config` errors naming the problem; everything else is `Transient("YouTube HTTP <status>")`.
- `poll.rs`: replaces the stopgap with `classify_youtube`; the poll aborts on the first error with no items and no cursor, no retry or sleep.
- Synthetic fixtures `quota_exceeded_403`, `unauthorized_401` (with meta files) and `malformed`; README labels every fixture `synthetic`.
- Tests: 12 classification tests, 3 credential and sweep tests in `tests/replay.rs`, the alphabet and 2xx unit tests in `api.rs`, and `tests/fixtures_manifest.rs`.

## TDD record

- RED (`15bb3a4`): 6 of the 12 new classification tests failed on their planned assertions (for example `left: Transient("YouTube HTTP 403")` against the D-15 text); the others passed against the stopgap, which already returned an error for every non-200 status. Semantic assessment: the failures are the planned behavior assertions, not compile, fixture or load faults.
- GREEN (`6fd0314`): classification implemented; 36 integration and 13 unit tests pass.
- Task 2 (`07ee0fd`): tests only, no production change, as planned; they pass against the existing request builder.
- No REFACTOR commit.

## Verification

- `cargo test --target x86_64-apple-darwin` in `plugins/youtube`: 13 unit + 39 replay + 1 manifest tests pass.
- `cargo component build --release` and `wasm-tools component targets ../../wit -w source` exit 0; no `wasi:` import in the component WIT.
- Acceptance greps pass (quotaExceeded and the reset text in `api.rs`, fixture status, no `thread::sleep`, README labels, no change to `wit`, `Cargo.toml`, `Cargo.lock` against 75b078e).

## Deviations from Plan

None - plan executed exactly as written. Notes: the plan's `<read_first>` named `plugins/reddit/tests/fixtures_manifest.rs`, which does not exist in this tree, so the manifest test was written from the plan's behavior block. The existing crate is not rustfmt-clean (pre-existing, out of scope); only new code was kept tidy.

## Known Stubs

None.

## Threat Flags

None. No new endpoints, auth paths or trust boundaries beyond the plan's threat model (T-07-33 to T-07-36 mitigated by the tests named in the plan).

## Self-Check: PASSED

- Files created exist: `fixtures_manifest.rs`, the five new fixture files.
- Commits `15bb3a4`, `6fd0314`, `07ee0fd` are ancestors of HEAD.
