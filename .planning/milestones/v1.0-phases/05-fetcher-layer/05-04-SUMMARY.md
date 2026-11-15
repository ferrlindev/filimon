---
phase: 05-fetcher-layer
plan: 04
subsystem: plugin-host
tags: [reqwest, egress, request-validation, ssrf, log-hygiene, fetcher]
status: complete

requires:
  - phase: 05-fetcher-layer
    provides: FetchPolicy url rules (05-01), Egress and TestHttpServer (05-03)
provides:
  - FetchPolicy::check validates the whole request, not only the url: url length, body size, header count and size, header syntax, forbidden headers
  - MAX_REQUEST_BODY_BYTES (pub, 1 MiB) and the private MAX_REQUEST_HEADERS, MAX_REQUEST_HEADER_BYTES, MAX_URL_BYTES and FORBIDDEN_REQUEST_HEADERS
  - Tests proving header order, binary bodies, 429 pass-through and that the host's logs carry no secret
affects: [05-05, 05-06, 05-07, 05-08]

actuals:
  tokens: 9500
  tasks: 2
  commits: 3
plan_head_before: b2203dedaeb22d8082f29c24bc3ad7a4fa4b94d1
plan_head_after: d76bc1714bd1a2e344c1e6d5c36b1a1941f7671f

tech-stack:
  added: []
  patterns:
    - "One FetchPolicy::check serves the source's fetch and the fetcher's send, so a request rule holds at both"
    - "Fixed error texts that never echo guest data; a forbidden header is reported by its fixed list entry"

key-files:
  created: []
  modified:
    - src/plugins/host/fetch.rs

key-decisions:
  - "WD-13: forbidden request headers are host, content-length, transfer-encoding, connection, proxy-connection, upgrade, te, trailer"
  - "WD-14: limits are 100 headers, 64 KiB of header text, 1 MiB body, 8 KiB url (fixed constants)"
  - "WD-15: only the order of repeated header names is guaranteed; cross-name order is unspecified"

requirements-completed: [FETCH-02]

duration: 15min
completed: 2026-11-11
---

# Phase 5 Plan 04: Request Validation and Log Hygiene Summary

**`FetchPolicy::check` now refuses a `host` header, framing and hop-by-hop headers, malformed headers and oversized urls, bodies and header sets before any connection, at both the source's `fetch` and the fetcher's `send`, and tests show headers, binary bodies and a 429 pass through faithfully while the host's logs hold no header, body, query or path.**

## Performance

- **Duration:** 15 min
- **Started:** 2026-11-11T02:36:54Z
- **Completed:** 2026-11-11T02:52:15Z
- **Tasks:** 2
- **Files modified:** 1 (`src/plugins/host/fetch.rs`)

## Accomplishments

- Request rules (T-5-14): url over 8 KiB is `url is too long` (checked before parsing, so never `Denied`); body over 1 MiB is `request body exceeds 1048576 bytes`; more than 100 headers or 64 KiB of header text is `headers are too large`; a bad name or a value with a line break is `header is not valid`; a forbidden header is ``header `<fixed name>` is not allowed``. A request exactly at each limit passes.
- The header syntax check reuses `to_header_map`, so there is one source of truth; the duplicate early check in `Egress::run` was removed.
- Fidelity (FETCH-02): same-name response headers come back in server order, same-name request headers reach the server in guest order, all 256 byte values round-trip through `send` unchanged, a 429 with `retry-after` is an ordinary response.
- Log hygiene (T-5-15): under `LogCapture`, a POST with a secret in the bearer header, body and query string and a secret in the response body leaves none of them, nor the url path, in any `fili::plugins` line; the test also requires the `outbound request` line naming `127.0.0.1`, so it cannot pass vacuously.
- `Cargo.toml`, `Cargo.lock`, `testing.rs` and `src/crawler.rs` are untouched.

## Task Commits

1. **Task 1: refuse hostile requests before any connection** - RED `951f573` (test), GREEN `4f68904` (feat)
2. **Task 2: fidelity, 429 pass-through, log hygiene** - `d76bc17` (test)

**Plan metadata:** the docs commit that adds this file.

## TDD Gate Compliance

Task 1 has a `test(05-04)` commit (`951f573`) followed by a `feat(05-04)` commit (`4f68904`).

- RED: five of the seven new tests failed on their assertions (`a_host_header_is_refused_before_any_connection`: the request reached the server and returned 200; `framing_and_hop_by_hop_headers_are_refused`, `too_many_or_too_large_headers_are_refused`, `an_oversized_url_is_refused`: `check` returned `Ok`; `an_oversized_request_body_is_refused`: 200 response). The constants were added in the RED commit so the tests compile and fail on behaviour rather than on a missing symbol. Two tests passed already (`a_malformed_header_is_refused_without_echoing_it`, because `to_header_map` already refused them with a fixed text, and `a_request_at_the_limits_is_accepted`, because nothing was limited yet): expected unexpected-green, guarding behaviour GREEN must keep.
- The repository has no `tdd-red-evidence` adapter for libtest output; the RED assessment is console evidence and the executor's own.
- Task 2: all five tests passed on first run. The plan anticipated this ("if everything already holds, this task only adds the tests"), so it is a single `test` commit with no GREEN; the existing `Egress::run` already iterates every header value, appends request headers, moves bytes without decoding, treats 429 like any status and logs only consumer, method, host, status and body length.

## Deviations from Plan

### Auto-fixed Issues

**1. [Plan drift] `Reply::with_header` already existed**
- **Found during:** Task 2 read_first
- **Issue:** Plan 05-03 added `Reply::with_header` (see its summary), so the plan's `testing.rs` change was already done. `SeenRequest.headers` already keeps received order with duplicates.
- **Fix:** No change to `testing.rs`; the existing builder is used.
- **Files modified:** none

**2. [Plan drift] Task 2 implementation step was empty**
- **Found during:** Task 2
- **Issue:** The five behaviors already held, so no fix to `Egress::execute` was needed.
- **Fix:** Tests only.

---

**Total deviations:** 2 (both plan drift against earlier plans, no code impact). **Impact:** none on scope or behaviour.

## Verification Results

- `cargo test -p filimon plugins::host::fetch`: 38 passed, 0 failed (all twelve named tests report `ok`).
- `cargo test -p filimon`: 412 passed, 0 failed in the unit binary (gate: more than 405); 12 passed in the integration binary.
- `cargo clippy -p filimon --all-targets`: no warning in `fetch.rs`; `testing.rs` has its 3 baseline warnings.
- Acceptance greps: the four limit constants, `FORBIDDEN_REQUEST_HEADERS` with all eight names, `fn with_header`, `outbound request` present; no `danger_accept`; no single-line log call names a `url`, `headers` or `body` field; `git diff` clean for `Cargo.toml` and `Cargo.lock`.
- The ignored `wasm_` fixture tests were not re-run: this plan changes no wasm path, and `check` only got stricter for headers, bodies and urls that the stub and default fetchers' tests do not exercise.

## Known Stubs

None.

## Threat Flags

None beyond the plan's register. T-5-14 and T-5-15 are mitigated and covered by the named tests; T-5-SC holds (no package added).

## Issues Encountered

- A fresh worktree has an empty `target/`, so the first `cargo test` compiled the whole dependency tree (about 6 minutes).
- The plan's prohibition (no headers, bodies, url path or query in logs or error text) is recorded `unresolved` in the plan; it is honoured and proven by `egress_never_logs_headers_bodies_or_query_strings` and the fixed error texts, but its status field is the planner's to flip.
- Flagged assumption still open: FETCH-02 ordering is resolved as the same-name guarantee (WD-15); the user has not ruled.

## Next Phase Readiness

Plan 05-05 can build on a complete request policy: any request a fetcher's `send` or a source's `fetch` carries is now fully judged before any connection.

## Self-Check: PASSED

- `src/plugins/host/fetch.rs`: FOUND (modified).
- Commits `951f573`, `4f68904`, `d76bc17`: in HEAD history; 3 commits counted from the recorded base `b2203de`.

---
*Phase: 05-fetcher-layer*
*Completed: 2026-11-11*
