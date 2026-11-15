---
phase: 05-fetcher-layer
plan: 05
subsystem: plugin-host
tags: [tls, certificate-validation, openssl, reqwest, rustls, egress, fetcher, tests]
status: complete

requires:
  - phase: 05-fetcher-layer
    provides: Egress with default certificate validation and the test-only Egress::with_extra_root (05-03), the http-fetcher component (05-02)
provides:
  - OpensslServer test harness (openssl s_server with a per-test throwaway certificate in a TempDir)
  - tls_ proofs that a self-signed certificate is rejected, accepted only as an explicit root, rejected for another name, and rejected through the real default fetcher component
  - the_fetch_path_has_no_invalid_certificate_switch, an always-on source guard over fetch.rs
  - net_ ignored group against example.com and four badssl.com hosts
affects: [05-06, 05-07, 05-08]

actuals:
  tokens: 4000
  tasks: 2
  commits: 2
plan_head_before: b2203dedaeb22d8082f29c24bc3ad7a4fa4b94d1
plan_head_after: 71e5a0c0b92c1f381b1995ec350baa50ef18ddbc

tech-stack:
  added: []
  patterns:
    - "Offline TLS tests drive the openssl binary as the server: no TLS server crate, no committed key material"
    - "libtest group filter by test function name only: the module is https_tests so neither tls_ nor net_ matches the module path"
    - "Control client with validation off proves a rejection is the certificate and not a broken server"

key-files:
  created:
    - src/plugins/host/https_tests.rs
  modified:
    - src/plugins/host/mod.rs

key-decisions:
  - "WD-16/WD-17/WD-27 followed as planned: openssl s_server as the offline server, a separate ignored net_ group, module named https_tests"
  - "No production change was needed: the client built in 05-03 passed every certificate test as written (hostname verification is on)"

requirements-completed: [FETCH-03]

duration: ~25min (plus a cold 8 min first build of the worktree target directory)
completed: 2026-11-11
---

# Phase 5 Plan 05: HTTPS Certificate Validation Proof Summary

**The host's HTTPS path is shown, not assumed, to validate certificates: a local openssl TLS server with a self-signed certificate is rejected by the production client and by the real default fetcher component, accepted only when its certificate is explicitly trusted, and rejected again for a name the certificate does not cover, with an always-on guard keeping the invalid-certificate switch out of `fetch.rs`.**

## Performance

- **Tasks:** 2 (a tracer and a TDD-flagged auto task)
- **Files modified:** 2 (`https_tests.rs` created, `mod.rs` one declaration)
- **Production code changes:** none

## Accomplishments

- Tracer (FETCH-03): `tls_a_self_signed_certificate_is_rejected` gets `Transport` whose text contains `certificate`, while `tls_the_same_server_answers_a_client_that_skips_validation` shows the same server answers a client with validation off (status 200), so the failure is certificate validation.
- Both directions work: `Egress::with_extra_root(cert)` completes the request with 200 (`tls_a_certificate_trusted_as_a_root_is_accepted`), and with the issuer trusted a request to `localhost` (certificate covers only `IP:127.0.0.1`) is still rejected with a certificate error (`tls_a_certificate_for_another_name_is_rejected`), so hostname verification is on.
- The real `http_fetcher.wasm` component behind a `FetchRoute` rejects the self-signed certificate the same way, and the route counted one fetcher instantiation (`tls_default_fetcher_component_rejects_a_self_signed_certificate`).
- `the_fetch_path_has_no_invalid_certificate_switch` (a normal, non-ignored test) reads `fetch.rs` with `include_str!`, asserts it still shows `redirect::Policy::none()` (so it cannot pass on a wrong file) and contains neither `danger_accept_invalid_certs` nor `danger_accept_invalid_hostnames`. The crawler is neither read nor edited.
- Public-internet group `net_`: `net_a_public_https_site_succeeds_with_validation` and `net_badssl_invalid_certificates_are_rejected` exist and are ignored by default. They were run here with internet access and both passed (example.com returned 200 with `Example Domain`; self-signed, expired, wrong-host and untrusted-root badssl hosts each failed with a certificate error). They are not part of any gating command.
- The harness generates its key and certificate in a per-test `TempDir`, binds `127.0.0.1` only, and `Drop` kills and waits for the child. `git ls-files` lists no `.pem`, `.key`, `.crt` or `.p12`.

## Task Commits

1. **Task 1: self-signed HTTPS server rejected by the production client and default fetcher (tracer)** - `b13578a` (test)
2. **Task 2: always-on invalid-certificate guard and the public-internet proofs** - `71e5a0c` (test)

**Plan metadata:** committed with this SUMMARY (docs).

## Files Created/Modified

- `src/plugins/host/https_tests.rs` - OpensslServer harness, five `tls_` tests, the source guard, two `net_` tests
- `src/plugins/host/mod.rs` - `#[cfg(test)] mod https_tests;`

## Decisions Made

None beyond the plan's working decisions (WD-16, WD-17, WD-27). One small judgement: `net_badssl_invalid_certificates_are_rejected` prints each host's error text to stderr (visible with `--nocapture`) instead of asserting the url is absent from it, because rustls names the host in its name-mismatch error text and the plan does not ask for that assertion.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] The default fetcher fixture was not built in the worktree**
- **Found during:** Task 1 precondition check
- **Issue:** `plugins/http-fetcher/target/.../http_fetcher.wasm` is gitignored and absent in a fresh worktree (and absent from the main checkout too).
- **Fix:** Built it in this worktree with `cargo component build --release` in `plugins/http-fetcher`. Nothing tracked changed.
- **Committed in:** not applicable (ignored file)

---

**Total deviations:** 1 auto-fixed (Rule 3, environment only). **Impact:** none on scope or behaviour.

## TDD Gate Compliance

Task 2 is flagged `tdd="true"` but only adds tests over behaviour that already exists (the client was built validating in 05-03), so there is no GREEN production commit. `the_fetch_path_has_no_invalid_certificate_switch` passes at once by construction: it guards a property that already holds. No RED commit was possible without first breaking `fetch.rs`, which the plan forbids. The tracer tests are likewise verification of existing behaviour; their discriminating power comes from the control test (the same server answers a client that skips validation) and the two-direction trusted-root test.

## Verification Results

- `cargo test -p filimon -- --ignored tls_`: 5 passed (all five named tests report `ok`); the other binaries run zero tests.
- `cargo test -p filimon plugins::host`: 187 passed, 0 failed, 33 ignored (before task 2); `cargo test -p filimon plugins::host::https_tests`: guard `ok`, seven ignored.
- `cargo test -p filimon -- --list --ignored`: both `net_` tests are listed.
- `cargo test -p filimon -- --ignored net_`: 2 passed (internet available during this run).
- `cargo clippy -p filimon --all-targets`: finished; no warning in `https_tests.rs` or `mod.rs`.
- Acceptance greps: `mod https_tests;` present; `s_server` and `Drop for OpensslServer` present; `TempDir` present and no key material tracked; `danger_accept` absent from `fetch.rs`; `git diff` clean for `Cargo.toml`, `Cargo.lock`, `src/crawler.rs` and `src/plugins/host/fetch.rs`; the guard is `#[test]` with no `#[ignore`.

## Known Stubs

None.

## Threat Flags

None beyond the plan's register. T-5-27 is covered by the four behavioural tests plus the guard; T-5-16 by the `TempDir` and `Drop` design; T-5-17 accepted as planned (the `net_` tests are ignored, one GET per host); T-5-SC: no dependency added.

## Issues Encountered

- First compile in the worktree took about 8 minutes (cold target directory); not a defect.
- Flagged assumptions from the plan remain open for the user: roots are the client's bundled roots (WD-11) and a TLS-intercepting proxy CA is not trusted.
- The `allow(dead_code)` on `Egress::with_extra_root` in `fetch.rs` is now stale (the method is used by these tests) but harmless; it was left untouched because the plan forbids editing `fetch.rs`.

## Next Phase Readiness

FETCH-03 is proven: the default fetcher validates certificates. Later plans in this phase can rely on `OpensslServer`-style loopback TLS only inside this module (it is private); nothing is exported.

## Self-Check: PASSED

- `src/plugins/host/https_tests.rs`: FOUND (created); `src/plugins/host/mod.rs`: FOUND (modified).
- Commits `b13578a` and `71e5a0c`: in HEAD history; 2 commits counted from the recorded base `b2203de`.

---
*Phase: 05-fetcher-layer*
*Completed: 2026-11-11*
