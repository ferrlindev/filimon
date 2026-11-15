---
phase: 05-fetcher-layer
plan: 08
subsystem: fetcher-layer
tags: [documentation, readme, wit, fetcher, crawler, verification-sweep]

requires:
  - phase: 05-fetcher-layer
    provides: routing, enforcement, default fetcher, TLS validation and the [crawler] table (plans 05-01 to 05-07)
provides:
  - "plugins/README.md: `## Fetcher layer` (assigning a fetcher, what the host enforces, the crawler), true Known limits, five test groups"
  - "README.md: `### Crawling through a fetcher component`"
  - "wit/host.wit and wit/types.wit doc comments that state the host's real fetch and send behaviour (comments only)"
  - "examples/plugins/filimon.toml: commented `[crawler]` example"
  - "Full-phase verification sweep recorded with exact pass counts"
affects: [phase-verification, verify-work, oauth-phase, headless-browser-fetcher]

actuals:
  tokens: 4500
  tasks: 2
  commits: 2
plan_head_before: 6414774c7e4cc7de2bae499cb305099f20364823
plan_head_after: 1f846b32dcb8bb165b76da34c1a546abd2da204c

tech-stack:
  added: []
  patterns:
    - "Documentation follows the code: every limit in the README was read from fetch.rs / config.rs / produce.rs, not copied from the plans"
    - "WIT edits are comments only, so `filimon:plugin@0.1.0` and every built plugin stay valid"

key-files:
  created: []
  modified:
    - plugins/README.md
    - README.md
    - wit/host.wit
    - wit/types.wit
    - examples/plugins/filimon.toml

key-decisions:
  - "The README states the trust-store choice (bundled roots, no operating-system store, no TLS-intercepting proxy CA, no setting to add a root) as current behaviour, closing the FETCH-03 unclassified edge as a flagged assumption"
  - "The README says plainly that the built-in crawler client keeps its existing behaviour, which does not validate certificates (src/crawler.rs builds it with danger_accept_invalid_certs(true)); only the fetcher path validates"
  - "The `Interim imports` subsection is renamed `Imports`: nothing about the fetch import is interim any more"

patterns-established:
  - "A README limit table row is a constant or rule in fetch.rs; a rule not in the code is not in the README"

requirements-completed: [FETCH-01, FETCH-02, FETCH-03, FETCH-04]

coverage:
  - id: D1
    description: "plugins/README.md describes assigning a fetcher, the enforced rules, the [crawler] table and the five test groups, and no longer calls the fetch import interim or the allowlist and timeout unenforced"
    requirement: "FETCH-01"
    verification:
      - kind: other
        ref: "plan 05-08 Task 1 <automated> heading, test-command and stale-text greps (all pass)"
        status: pass
    human_judgment: true
    rationale: "Whether the prose is accurate and clear to an operator is a reading judgment; the greps prove presence and absence of the required text, and each constant was compared with fetch.rs by hand"
  - id: D2
    description: "Root README documents the [crawler] table for fili produce, the messages printed in each case, and that only the fetcher path validates certificates"
    requirement: "FETCH-04"
    verification:
      - kind: other
        ref: "grep -F '[crawler]' README.md && grep -F 'Crawling through a fetcher component' README.md"
        status: pass
    human_judgment: true
    rationale: "Prose accuracy against select_page_fetcher's three messages was checked by reading the code, not by a test"
  - id: D3
    description: "WIT contract unchanged apart from doc comments; wasm-tools parses the package; sample registry loads with the commented [crawler] example"
    requirement: "FETCH-02"
    verification:
      - kind: other
        ref: "wasm-tools component wit wit (package filimon:plugin@0.1.0 present) and the awk comment-only diff check"
        status: pass
      - kind: integration
        ref: "cargo run -p filimon --bin fili -- --registry examples/plugins/filimon.toml plugins check http"
        status: pass
      - kind: unit
        ref: "cargo test -p filimon plugins::tests (49 passed)"
        status: pass
    human_judgment: false
  - id: D4
    description: "Full-phase sweep: unit, real-wasm, TLS, sample registry and clippy groups pass together"
    requirement: "FETCH-03"
    verification:
      - kind: unit
        ref: "cargo test -p filimon (433 passed, 62 ignored)"
        status: pass
      - kind: integration
        ref: "cargo test -p filimon -- --ignored wasm_ (41 passed); -- --ignored tls_ (5 passed)"
        status: pass
    human_judgment: false

duration: 25min
completed: 2026-11-11
status: complete
---

# Phase 5 Plan 08: Document the Fetcher Layer Summary

**READMEs, WIT comments and the sample registry now state exactly what the fetcher layer enforces and what it does not, and the whole phase's unit, real-wasm and TLS groups pass together (433 / 41 / 5).**

## Performance

- **Duration:** about 25 min
- **Started:** 2026-11-11T03:05:00Z (approx.)
- **Completed:** 2026-11-11T03:30:25Z
- **Tasks:** 2
- **Files modified:** 5 (documentation, WIT comments and one example file; no `src`, `Cargo.toml` or `Cargo.lock` change)

## Accomplishments

- `plugins/README.md` gained `## Fetcher layer` with `### Assigning a fetcher`, `### What the host enforces` (a rule table and an error-by-error account of `invalid-request`, `denied`, `timeout`, `transport` and `not-configured`) and `### The crawler`. The stale interim sentence and the "timeout and allowlist are not yet enforced" limit are gone. Known limits now say: DNS rebinding or a private address behind an allowlisted name is not blocked; the allowlist is a list of names, not an address filter; a fetcher sees every header it serves so only trusted fetchers should be registered; there is no per-source rate limiter and a 429 is an ordinary response; and the bundled-roots trust store means a TLS-intercepting proxy CA is not trusted.
- Test instructions list five groups with exact filters and needs (`plugins::host`, `wasm_`, `live_`, `tls_`, `net_`), keep "a group that cannot run is not a pass", and extend the single-substring rule to the new filters.
- Root `README.md` has `### Crawling through a fetcher component`: the `[crawler]` TOML, the three one-line messages `fili produce` prints, the error when the table is unusable, the allowed-hosts rule and the certificate statement.
- `wit/host.wit` and `wit/types.wit`: comment-only edits (the awk check shows every changed line is a doc comment or blank); the "Phase 3 interim" wording is gone. `examples/plugins/filimon.toml` shows a commented `[crawler]` example and loads as before.

## Task Commits

1. **Task 1: READMEs describe the fetcher layer as built (tracer)** - `f7e0bf7` (docs)
2. **Task 2: WIT doc comments, sample `[crawler]` example, verification sweep** - `1f846b3` (docs)

**Plan metadata:** the SUMMARY commit that follows this file.

The Task 1 tracer `<verify>` was re-run end to end after the commit (headings, test commands, `five redirects`, `8 MiB`, `not-configured`, `[crawler]`, stale-text negative greps, `x-stub-mode`) and passed before Task 2 began.

## Files Created/Modified

- `plugins/README.md` - Fetcher layer section, Imports, Known limits, five test groups, `run-once` paragraph, stub-source bullet
- `README.md` - Crawling through a fetcher component
- `wit/host.wit` - fetch and send doc comments
- `wit/types.wit` - `not-configured`, `denied`, `timeout`, `transport`, `invalid-request` doc comments
- `examples/plugins/filimon.toml` - commented `[crawler]` example

## Constants compared with `src/plugins/host/fetch.rs` (acceptance check, one by one)

| Constant | Code | README (enforcement table) |
|----------|------|----------------------------|
| `MAX_REDIRECTS` | 5 | "At most five redirects (`MAX_REDIRECTS`)" |
| `MAX_RESPONSE_BYTES` | 8 * 1024 * 1024 | "At most 8 MiB (`MAX_RESPONSE_BYTES`)" |
| `MAX_REQUEST_BODY_BYTES` | 1024 * 1024 | "a body of at most 1 MiB" |
| `MAX_REQUEST_HEADERS` | 100 | "At most 100 headers" |
| `MAX_REQUEST_HEADER_BYTES` | 64 * 1024 | "64 KiB of header text (names plus values)" |
| `MAX_URL_BYTES` | 8 * 1024 | "a url of at most 8 KiB" |
| `FORBIDDEN_REQUEST_HEADERS` | host, content-length, transfer-encoding, connection, proxy-connection, upgrade, te, trailer | the same eight, in the same order |

Other values checked against code: `[host] timeout_secs` default 30 (`DEFAULT_TIMEOUT_SECS`), `[crawler] timeout_secs` default 15 (`DEFAULT_CRAWLER_TIMEOUT_SECS`), `CONNECT`/`TRACE` refused, credential headers removed on an origin change (`authorization`, `cookie`, `proxy-authorization`).

## Verification sweep results

| Group | Command | Result |
|-------|---------|--------|
| Unit | `cargo test -p filimon` | 433 passed, 0 failed, 62 ignored (plus a second binary: 12 passed, 3 ignored) |
| Real wasm | `cargo test -p filimon -- --ignored wasm_` | 41 passed, 0 failed (plus a second binary: 3 passed) |
| TLS | `cargo test -p filimon -- --ignored tls_` | 5 passed, 0 failed (`openssl` at `/usr/local/bin/openssl`) |
| Sample registry | `cargo test -p filimon plugins::tests` | 49 passed, 0 failed, 4 ignored |
| Default fetcher | `fili --registry examples/plugins/filimon.toml plugins check http` | `component: ok`, `check passed` |
| WIT | `wasm-tools component wit wit` | parses; `package filimon:plugin@0.1.0` present in all three files |
| Clippy | `cargo clippy -p filimon --all-targets` | Finished; no warning in the clean files; config.rs 3, crawler.rs 5, main.rs 3, testing.rs 3 (equal to baseline), produce.rs 0 (baseline 4) |
| `net_` | `cargo test -p filimon -- --ignored net_` | **NOT RUN** (needs internet access to public sites and badssl.com; not attempted from this isolated executor) |
| Live cluster | `fili produce <url>` with a `[crawler]` registry against a local Fluvio cluster, then `fili consume` | **NOT RUN** (no cluster in this worktree environment) |

The two not-run items remain operator checks from the plan's `<verification>`; nothing here claims them.

## Decisions Made

- Trust store is documented as current behaviour with an explicit TLS-intercepting-proxy consequence, because the user has not ruled on FETCH-03's edge (flagged assumption in the plan).
- The built-in client's lack of certificate validation is stated openly in both READMEs rather than only "keeps its existing behaviour", because the code is `danger_accept_invalid_certs(true)` and T-5-26 is about not hiding limits.
- `### Interim imports` became `### Imports` (nothing is interim any more).

## Deviations from Plan

### Where the code differs from the plan (WD-25: the code wins)

- **Baseline numbers in the plan are stale.** The plan expected a baseline of 374 passed and 20 `wasm_` tests; the built phase has 433 passed and 41 `wasm_` tests (the earlier waves added them). The plan's thresholds (>420, >30, >=5) all hold.
- **Allowlist entry handling is two-tier.** A non-bare-host entry in a source's `[host] allowlist` is a registry error at load time (`host.allowlist[i]`), but a non-bare-host entry in `[crawler] allowlist` has no load-time check and only logs a warning and never matches. The README states the source rule and the "never matches" rule; the crawler-table difference is recorded here, not in the README.
- **No default-fetcher fallback.** The plan says "the default fetcher built from `plugins/http-fetcher`". The code has no implicit default (`src/plugins/host/mod.rs`: "there is no implicit default fetcher"), so the README says the default is the sample registry's `[fetchers.http]` entry and that a source with no `fetcher` key gets `not-configured`.

### Auto-fixed Issues

**1. [Rule 3 - Blocking] A zsh word-splitting slip created stray empty directories while copying fixtures**
- **Found during:** Task 2 (copying the gitignored `.wasm` fixtures from the main checkout)
- **Issue:** `set -- $p` does not split in zsh, so empty directories named like `plugins/stub-source stub_source/target/...` were created in the worktree.
- **Fix:** removed only those empty directories (`find ... -type d -empty -delete`, no `git clean`), then copied the four fixtures with explicit paths.
- **Files modified:** none tracked (ignored `target/` paths only)
- **Verification:** `ls plugins/` shows only the four real crate directories; `git status` clean of them.
- **Committed in:** not applicable

---

**Total deviations:** 1 auto-fixed (1 blocking, untracked scratch only)
**Impact on plan:** none. All three code-versus-plan differences are documentation of what the code does, per WD-25.

## Issues Encountered

None beyond the fixture-copy slip above. The fixtures used are the freshly built `.wasm` files from the main checkout; this plan changes no plugin source.

## Known Stubs

None.

## Threat Flags

None. The plan adds no code, endpoint or file access. T-5-26 is mitigated: every limit is compared with the constants above, the accepted risks (DNS rebinding, a fetcher seeing request headers, no rate limiter, the trust-store choice) are listed under Known limits, and the stale interim sentences are gone (negative greps pass). T-5-SC: `git diff --quiet -- Cargo.toml Cargo.lock src` exits 0.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- Phase 5 is documented and its automated groups pass together. Remaining before the phase can be called fully verified: the operator checks `net_` and the live-cluster `fili produce` then `fili consume` run, both not run here.
- Known accepted risks for later phases: DNS rebinding behind an allowlisted name; a fetcher sees request headers; no per-source rate limiter; the built-in crawler client does not validate certificates.

## Self-Check: PASSED

- `plugins/README.md`, `README.md`, `wit/host.wit`, `wit/types.wit`, `examples/plugins/filimon.toml` exist and are modified in commits `f7e0bf7` and `1f846b3`, both ancestors of HEAD.
- All task `<acceptance_criteria>` re-run and pass: headings, stale-text negative greps, `TLS-intercepting`, `Plugin secrets` and `.env format` sections untouched (diff hunks all fall outside them), `git diff --quiet -- Cargo.toml Cargo.lock src`, `redirect` in host.wit, no `Phase 3 interim`, `^# \[crawler\]`, commented-only toml diff, comment-only WIT diff, one `package filimon:plugin@0.1.0;` line per file.

---
*Phase: 05-fetcher-layer*
*Completed: 2026-11-11*
