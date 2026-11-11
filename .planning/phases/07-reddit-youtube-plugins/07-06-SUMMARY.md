---
phase: 07-reddit-youtube-plugins
plan: 06
subsystem: plugins
tags: [reddit, rate-limit, wasm-component, rust, testing, fixtures]

requires:
  - phase: 07-reddit-youtube-plugins
    provides: "plan 07-04 poll loop, Http seam, replay mock and synthetic listing fixtures"
provides:
  - "classify_response in poll.rs: rate-limit, auth, config and transient classification before parsing"
  - "parse_header_f64 and reset_seconds: float header parsing, round up, clamp, saturate"
  - "synthetic fixtures rate_limited_429, unauthorized_401, malformed"
  - "StrictHttp compliance sweep and fixtures_manifest required-fixture test"
affects: [07-07, 07-11, 07-12]

actuals:
  tokens: 14000
  tasks: 2
  commits: 4
plan_head_before: 042472605e9b78dc7a53753f2ae432d8b3abbed0
plan_head_after: d2b2add989548e88ffa811bf1cd06d976b2f918e

tech-stack:
  added: []
  patterns:
    - "Reactive stateless rate limiting: return rate-limited(reset) and let the host backoff wait"
    - "Strict recording mock that panics on the first non-compliant request"
    - "Fixture manifest test reading replay.rs via include_str! to prove every fixture is exercised"

key-files:
  created:
    - plugins/reddit/tests/fixtures_manifest.rs
    - plugins/reddit/tests/fixtures/rate_limited_429.json
    - plugins/reddit/tests/fixtures/rate_limited_429.meta.json
    - plugins/reddit/tests/fixtures/unauthorized_401.json
    - plugins/reddit/tests/fixtures/unauthorized_401.meta.json
    - plugins/reddit/tests/fixtures/malformed.json
  modified:
    - plugins/reddit/src/poll.rs
    - plugins/reddit/tests/replay.rs
    - plugins/reddit/tests/fixtures/README.md

key-decisions:
  - "Feed key in error texts is the cursor key (r/<name>:<listing>), which names the configured subreddit and listing"
  - "x-ratelimit-remaining below 1.0 is limited on any status; non-finite header values (NaN, inf) parse as absent"
  - "http.rs unchanged: the existing case-insensitive header helper was enough, no header_f64 added there"

patterns-established:
  - "Error texts are fixed phrases plus feed key, status and parsed numbers only"

requirements-completed: [RDDT-03, TEST-01]

coverage:
  - id: D1
    description: "Reddit 429 or x-ratelimit-remaining < 1 returns rate-limited with the float-parsed reset seconds; no retry or sleep"
    requirement: "RDDT-03"
    verification:
      - kind: unit
        ref: "plugins/reddit/tests/replay.rs#http_429_is_rate_limited_with_the_reset_seconds"
        status: pass
      - kind: unit
        ref: "plugins/reddit/tests/replay.rs#remaining_below_one_on_a_200_is_rate_limited_at_the_boundary"
        status: pass
      - kind: unit
        ref: "plugins/reddit/src/poll.rs#rate_limit_headers_parse_as_floats"
        status: pass
      - kind: unit
        ref: "plugins/reddit/tests/replay.rs#no_rate_limit_retry_inside_one_poll"
        status: pass
    human_judgment: false
  - id: D2
    description: "Every non-2xx status and unreadable listing is a typed error, never an empty success; texts never echo bodies"
    requirement: "RDDT-03"
    verification:
      - kind: unit
        ref: "plugins/reddit/tests/replay.rs#every_error_status_maps_to_an_error_never_an_empty_success"
        status: pass
      - kind: unit
        ref: "plugins/reddit/tests/replay.rs#malformed_json_is_transient_without_echoing_the_body"
        status: pass
    human_judgment: false
  - id: D3
    description: "Every request goes to oauth.reddit.com with bearer, accept and the enforced User-Agent"
    requirement: "RDDT-03"
    verification:
      - kind: unit
        ref: "plugins/reddit/tests/replay.rs#every_request_is_official_authenticated_and_carries_the_enforced_user_agent"
        status: pass
    human_judgment: false
  - id: D4
    description: "Every required D-18 Reddit fixture case exists, is labelled synthetic and is exercised"
    requirement: "TEST-01"
    verification:
      - kind: unit
        ref: "plugins/reddit/tests/fixtures_manifest.rs#every_required_fixture_case_exists_is_labelled_and_is_exercised"
        status: pass
    human_judgment: false

duration: 25min
completed: 2026-11-11
status: complete
---

# Phase 7 Plan 06: Reddit Rate Limits and Typed Errors Summary

**Reddit responses are classified before parsing: 429 or `x-ratelimit-remaining` below 1 returns `rate-limited` with the float-parsed reset seconds, every other failure is a named typed error, and a strict-mock sweep plus a fixture manifest test prove compliance and fixture completeness.**

## Performance

- **Duration:** about 25 min
- **Tasks:** 2 (both TDD)
- **Files:** 3 modified, 6 created

## Accomplishments

- `poll.rs` gained `classify_response`, `parse_header_f64` and `reset_seconds`, replacing the tracer's "anything but 200 is transient" stopgap. Order: rate limit, 2xx continue, 401 auth-required, 404 config, 403 named transient, everything else named transient.
- Rate limiting is reactive and stateless: no sleep, no retry, no counters. A limit aborts the whole poll with no items and no cursor, so the host backoff (Phase 3) waits and the next poll starts clean.
- Three synthetic fixtures with meta files (429 with the three rate-limit headers, 401, truncated JSON with a sentinel word).
- `StrictHttp` mock asserts host, `authorization: bearer`, `accept` and the exact `filimon:<app_id>:<version> (by /u/<contact>)` User-Agent on every request across success, rate-limit, 401, 503 and custom-app-id scenarios.
- `tests/fixtures_manifest.rs` proves each D-18 Reddit case exists, parses, is labelled `synthetic` in the README and is referenced by `replay.rs`; a second test requires every file in the directory to be named and labelled.

## Task Commits

1. Task 1 RED: `0800fba` test(07-06): failing classification tests, fixtures, stubs
2. Task 1 GREEN: `b34b96d` feat(07-06): classify Reddit responses from status and rate-limit headers
3. Task 2 RED: `22b97ec` test(07-06): compliance sweep and manifest tests
4. Task 2 GREEN: `d2b2add` feat(07-06): label every Reddit fixture in the README

## TDD Gate Compliance

- Task 1 RED: eight integration tests failed on their planned assertions (for example `http_429_is_rate_limited_with_the_reset_seconds`, `http_401_is_auth_required`, `remaining_below_one_on_a_200_is_rate_limited_at_the_boundary`); the unit test failed on its stubbed helpers. `http_5xx_is_transient` and `malformed_json_is_transient_without_echoing_the_body` passed already because the tracer mapped any non-200 to a transient; they remain as regression guards. `gsd_run check tdd-red-evidence` was not run (cargo test output is not a supported report format); semantic assessment: the named targets executed and failed on the planned assertions, not on compile or fixture faults.
- Task 2 RED: both manifest tests failed because README rows lacked a provenance label. The sweep test was green immediately: the plan states no production change, so it proves existing behaviour.
- GREEN commits follow each RED. No REFACTOR commit.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Updated an earlier test's expected text**
- **Found during:** Task 1 RED
- **Issue:** `a_failed_feed_fails_the_whole_poll_with_no_items_and_no_cursor` asserted the exact tracer text `Reddit HTTP 500`; the plan's required text is `Reddit HTTP <status> for <feed key>`.
- **Fix:** expectation changed to `Reddit HTTP 500 for r/programming:new`; it still asserts the body and token are not echoed.
- **Files modified:** plugins/reddit/tests/replay.rs
- **Commit:** 0800fba

**2. [Rule 3 - Blocking] Added a second fixture test and a seventh manifest case**
- The manifest also lists `listing_hot` and a second test checks every file in the directory is README-labelled, so existing fixtures (listing_newer, listing_marked_post_gone) are covered too. This required relabelling the older README rows as `synthetic`.

**Total deviations:** 2 (1 Rule 1, 1 Rule 3 scope-adjacent). **Impact:** none on behaviour; README rows for plan 07-04 fixtures were relabelled.

## Verification

- `cargo test --offline --target x86_64-apple-darwin --manifest-path plugins/reddit/Cargo.toml`: 26 unit, 44 replay and 2 manifest tests pass; every test named in both tasks reports `ok`.
- Task acceptance greps pass; `git diff --quiet 75b078e -- wit Cargo.toml Cargo.lock` exits 0; no `thread::sleep` in `plugins/reddit/src`.
- `cargo component build --release` succeeds (artifact under `target/wasm32-wasip1/release`). The `wasm-tools component targets` check was not run: the worktree command guard refused the invocation, so the WIT conformance check of the built component is left for the orchestrator or the next wave.
- Clippy warnings seen (collapsible `if` in `poll.rs` item loop, `get().is_none()` in an older test) are pre-existing and out of scope.

## Known Stubs

None.

## Threat Flags

None. No new network surface; error texts and header parsing follow the plan's threat register (T-07-25 to T-07-28).

## Self-Check: PASSED

- Created files exist: fixtures_manifest.rs, rate_limited_429 (json and meta), unauthorized_401 (json and meta), malformed.json.
- Commits 0800fba, b34b96d, 22b97ec, d2b2add are ancestors of HEAD.
