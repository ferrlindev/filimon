---
phase: 05-fetcher-layer
plan: 03
subsystem: plugin-host
tags: [reqwest, rustls, egress, redirects, ssrf, tokio, fetcher]
status: complete

requires:
  - phase: 05-fetcher-layer
    provides: FetchRoute and FetchPolicy (05-01), the http-fetcher component (05-02)
provides:
  - Egress: one shared lazy HTTP client (default certificate validation, no automatic redirects) with a host-run redirect loop, an 8 MiB body cap and one deadline per call
  - EgressGrant: the calling consumer's policy, deadline and client, carried in a fetcher's HostState
  - FetchPolicy::check_url, used for the first url and for every redirect target
  - HostEngine::egress and FetchRoute::new taking the shared egress
  - TestHttpServer, Reply and SeenRequest loopback helpers
affects: [05-04, 05-05, 05-06, 05-07, 05-08]

actuals:
  tokens: 22000
  tasks: 2
  commits: 3
plan_head_before: 0086f4a2a20a8c1b187ded4d6079ef01365361cd
plan_head_after: 3a697394b6aea1b0a3bf7937bbd5ba31dcdaa233

tech-stack:
  added: []
  patterns:
    - "Host owns the network: a fetcher's `send` goes through a grant carrying the consumer's own policy and deadline"
    - "Redirects followed by the host, policy checked on every hop, credentials dropped on origin change"
    - "Loopback TestHttpServer on the current-thread runtime so per-thread LogCapture still sees host logs"

key-files:
  created: []
  modified:
    - src/plugins/host/fetch.rs
    - src/plugins/host/engine.rs
    - src/plugins/host/testing.rs
    - src/plugins/host/mod.rs

key-decisions:
  - "WD-09: `send` follows at most five redirects itself, checking the allowlist on every hop; the client's own redirect following is off (costly to reverse)"
  - "WD-10: bodies buffered whole up to 8 MiB; over the cap is `transport` naming 8388608"
  - "WD-11: certificate roots are the client's bundled webpki roots; the OS trust store is not read"
  - "WD-12: no host-side rate limiter; a 429 is an ordinary response"

requirements-completed: [FETCH-02, FETCH-03]

duration: ~60min
completed: 2026-11-11
---

# Phase 5 Plan 03: Egress, Redirects and Deadline Summary

**A fetcher's `outbound-http.send` now reaches the network through one shared certificate-validating client, under the calling source's own allowlist and deadline, with redirects followed and re-checked by the host (five at most), credentials dropped across origins, and bodies capped at 8 MiB.**

## Accomplishments

- Tracer (FETCH-02, FETCH-03): stub-source `fetch_url` routed to the real `http-fetcher` component logs `init fetch status 200`, and the loopback server saw exactly one `GET /hello` (`wasm_tracer_default_fetcher_reaches_a_local_server`).
- `Egress::execute` maps a response to a WIT response with status, every header (repeats kept) and the body; transport errors never carry the url (`without_url` plus the cause chain, cleaned and clipped to 256 characters).
- Send-path policy: a denied fetcher request makes no connection (`egress_denies_a_host_outside_the_allowlist_on_the_send_path`).
- Redirects: allowed hop followed; target outside the allowlist is `Denied` and never contacted; sixth redirect fails as `too many redirects (limit 5)`; 303 (and 301/302 on POST) become GET without body and content headers; 307/308 keep method and body; a redirect without `Location` is returned as is.
- Credentials (`authorization`, `cookie`, `proxy-authorization`) are dropped when scheme, host or port changes and kept on the same origin.
- One deadline wraps the loop and the body read: a slow server and a chain of quick-but-together-slow hops both give `Timeout` in well under 2 s.
- CONNECT and TRACE (any letter case) are `invalid-request` with no connection.
- No `danger_accept` anywhere in `fetch.rs`; `src/crawler.rs`, `Cargo.toml` and `Cargo.lock` untouched.

## Task Commits

1. **Task 1: tracer, source fetch reaches the network through the default fetcher** - `098b1ea` (feat)
2. **Task 2: redirects, credentials, deadline, methods** - RED `fc7c884` (test), GREEN `3a69739` (feat)

## TDD Gate Compliance

Task 2 has a `test(05-03)` commit followed by a `feat(05-03)` commit.

- RED: nine of the eleven new tests failed on their assertions (redirects not followed, no limit, credentials not handled, CONNECT/TRACE not refused, one-deadline case returned a response instead of timing out). Two passed already (`egress_returns_a_redirect_without_location_as_is`, `egress_deadline_expires_on_a_slow_server`): the task 1 single-send path satisfies them by construction, an expected unexpected-green rather than a wrong test.
- The repository has no `tdd-red-evidence` adapter for libtest output; the assessment above is console evidence and the executor's own.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] The default fetcher fixture was not built in the worktree**
- **Found during:** Task 1 precondition check
- **Issue:** `http_fetcher.wasm` and the three older fixtures are gitignored and absent from the worktree.
- **Fix:** Copied the three prebuilt fixtures from the main checkout into this worktree's ignored `plugins/*/target` paths (read-only on main) and built `plugins/http-fetcher` here with `cargo component build --release`. Nothing tracked changed.
- **Committed in:** not applicable (ignored files)

**2. [Rule 3 - Blocking] Lint gate: clippy `too_many_lines` and `format!` appended to a String**
- **Found during:** Task 1 and Task 2 clippy runs
- **Fix:** `TestHttpServer` builds its response head from a collected `Vec<String>`; `Egress::run` was split with `read_capped` and `drop_headers` helpers. Host files are back at the baseline (three pre-existing `testing.rs` warnings).
- **Committed in:** `098b1ea`, `3a69739`

**3. [Rule 2 - Test support] `Reply::with_header` and `Reply::status` added**
- **Issue:** The plan lists `ok`, `redirect`, `delayed` and `without_length` only, but the first test needs an `x-test` response header and the no-`Location` test needs a bodyless 302.
- **Fix:** Added both builders to `testing.rs`; `SeenRequest::header` added as an accessor.

---

**Total deviations:** 3 auto-fixed (all Rule 2/3). **Impact:** none on behaviour or scope.

## Verification Results

- `cargo test -p filimon plugins::host::fetch::tests::egress`: 15 passed.
- `cargo test -p filimon`: 400 passed, 0 failed (plan gate: more than 395).
- `cargo test -p filimon -- --ignored wasm_`: 26 passed in the unit binary, 3 in the integration binary (includes the tracer and every earlier fixture test).
- `cargo clippy -p filimon --all-targets`: no warning in `fetch.rs`, `engine.rs` or `mod.rs`; `testing.rs` has its 3 baseline warnings.
- Grep gates: `Policy::none` present, no `Policy::limited`, no `danger_accept`, `proxy-authorization` present; `git diff` clean for `Cargo.toml`, `Cargo.lock`, `src/crawler.rs`.

## Known Stubs

None.

## Threat Flags

None beyond the plan's register. T-5-08 through T-5-12 are mitigated and covered by the named tests; T-5-10's behavioural proof (self-signed rejected, trusted root accepted) is plan 05-05, and `Egress::with_extra_root` is in place for it (marked `allow(dead_code)` until then). T-5-13 is accepted as planned. The environment's proxy variables keep their usual effect (WD-11 assumption); none were set in this run.

## Issues Encountered

- Flagged assumptions from the plan remain open for the user: the trust-store choice (WD-11) and proxy environment variables being honoured.
- Plan start time was not captured, so the duration is an estimate.

## Self-Check: PASSED

- `src/plugins/host/fetch.rs`, `engine.rs`, `testing.rs`, `mod.rs`: FOUND (modified).
- Commits `098b1ea`, `fc7c884`, `3a69739`: in HEAD history; 3 commits counted from the recorded base `0086f4a`.

---
*Phase: 05-fetcher-layer*
*Completed: 2026-11-11*
