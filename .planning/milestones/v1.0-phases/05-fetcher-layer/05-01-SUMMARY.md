---
phase: 05-fetcher-layer
plan: 01
subsystem: plugin-host
tags: [wasmtime, component-model, fetcher, allowlist, ssrf, tokio]

requires:
  - phase: 03-host-runtime
    provides: HostEngine, WasmtimeFactory, loader with FetcherPre, stub-source and stub-fetcher fixtures
  - phase: 04-registry
    provides: filimon.toml fetcher assignment per source, allowlist and timeout_secs in [host]
provides:
  - FetchRoute: one fresh fetcher instance per `fetch` call under one deadline
  - FetchPolicy and Allowlist: exact-host allowlist, http/https only, denial before any fetcher exists
  - Host::load builds one route per source that names a fetcher; a source whose fetcher cannot load is not loaded
  - new_guest_store shared by source and fetcher stores
affects: [05-03 fetcher outbound-http send, 05-06 per-call deadline, 05-07 news crawler consumer]

actuals:
  tokens: 8850
  tasks: 3
  commits: 5
plan_head_before: e6cc3f0b33f0ed374e892c502bbd5e65b0ae324f
plan_head_after: 9651498fdde794ca4db47d1a3ebadef6d5366349

tech-stack:
  added: []
  patterns:
    - "Consumer-keyed route: policy and route carry a consumer label, an allowlist, a timeout and a fetcher alias, so sources and the crawler share one enforcement path"
    - "Fresh fetcher instance per call, dropped on timeout; nothing re-used"
    - "Strict loading: a dependency failure is reported against the dependent, never silently degraded"

key-files:
  created:
    - src/plugins/host/fetch.rs
  modified:
    - src/plugins/host/engine.rs
    - src/plugins/host/instance.rs
    - src/plugins/host/loader.rs
    - src/plugins/host/mod.rs
    - src/plugins/host/error.rs
    - src/plugins/config.rs

key-decisions:
  - "WD-06 resolved: awaiting a second store's async export directly inside the `fetch` host import works in wasmtime; no tokio::spawn indirection was needed"
  - "Allowlist entries normalized with url::Host::parse; one trailing dot ignored; an entry that is not a bare host is dropped, kept in rejected() and warned once"
  - "A denial and its warn log carry host and consumer only, never path or query; the url is never echoed in InvalidRequest text"
  - "A source with no fetcher key gets not-configured; no implicit default fetcher"

patterns-established:
  - "HostState::with_route / WasmtimeFactory::with_route: optional route injection, default None keeps existing callers unchanged"
  - "wasm_ prefixed #[ignore] tests for real-fixture routing, plain tests for the pure policy matrix"

requirements-completed: [FETCH-01, FETCH-02]

coverage:
  - id: D1
    description: "A source's fetch import is answered by the fetcher assigned to it, in a fresh instance per call; no route means not-configured"
    requirement: FETCH-01
    verification:
      - kind: integration
        ref: "src/plugins/host/fetch.rs#wasm_tracer_fetch_is_served_by_the_assigned_fetcher"
        status: pass
      - kind: integration
        ref: "src/plugins/host/fetch.rs#wasm_concurrent_fetches_never_share_a_fetcher_instance"
        status: pass
    human_judgment: false
  - id: D2
    description: "Host::load builds a route per source from the registry assignment and skips a source whose fetcher cannot load, naming both"
    requirement: FETCH-01
    verification:
      - kind: integration
        ref: "src/plugins/host/mod.rs#wasm_host_run_routes_a_source_through_the_registry_assigned_fetcher"
        status: pass
      - kind: integration
        ref: "src/plugins/host/mod.rs#wasm_host_load_skips_a_source_whose_fetcher_cannot_load"
        status: pass
    human_judgment: false
  - id: D3
    description: "Ingress allowlist: exact-host matching, empty list denies all, http/https only, denial before any fetcher exists"
    requirement: FETCH-02
    verification:
      - kind: integration
        ref: "src/plugins/host/fetch.rs#wasm_denied_host_never_instantiates_the_fetcher"
        status: pass
      - kind: unit
        ref: "cargo test -p filimon plugins::host::fetch (11 policy tests)"
        status: pass
    human_judgment: false

duration: 25min
completed: 2026-11-11
status: complete
---

# Phase 5 Plan 01: Fetcher Routing and Ingress Policy Summary

**A source's `fetch` import now runs the registry-assigned fetcher component in a fresh wasmtime instance per call, behind an exact-host allowlist that refuses before any fetcher starts.**

## Performance

- **Duration:** about 25 min (start time was not recorded at launch; estimated from the first build)
- **Completed:** 2026-11-11T02:14Z
- **Tasks:** 3
- **Files modified:** 7 (one created)

## Accomplishments

- Tracer (FETCH-01): stub-source `fetch_url` routed to stub-fetcher logs `init fetch status 200`, the fetcher's own `stub fetch:` line appears under `plugin=stub-fetcher`, and `FetchRoute::calls()` is 1. With no route the source logs `init fetch failed: FetchError::NotConfigured` and no fetcher runs.
- Eight concurrent `fetch` calls through one route get eight instances and each its own answer (`x-stub-url` echo).
- FETCH-02 ingress: a host outside the allowlist is `Denied` before any fetcher exists (`calls() == 0`, no fetcher log line). Matrix covers look-alike suffixes and prefixes, userinfo tricks, case, one trailing dot, any port, IP literals, wildcard entries (never match), empty list (deny all), non-http schemes, empty or unparsable urls, and denial text that omits path and query.
- `Host::load` builds one `FetchRoute` per source that names a fetcher (one `Arc<FetcherPre>` shared by every source naming it). A source whose fetcher failed to load is not loaded and not polled; the load failure is a `HostError::FetcherUnavailable` naming both. A source with no `fetcher` key is built without a route.
- Both dead-code allows that waited for this phase are gone (`LoadedSource.fetcher`, `HostState::plugin`, the latter removed with its accessor since nothing read it).

## Task Commits

1. **Task 1: tracer, source fetch to fresh fetcher instance** - `c155d7a` (feat)
2. **Task 2: Host::load routes and strict fetcher loading** - RED `9e0e14c` (test), GREEN `02b596e` (feat)
3. **Task 3: allowlist and URL validation matrix** - RED `ed4e224` (test), GREEN `9651498` (feat)

**Plan metadata:** committed with this SUMMARY (docs).

## TDD Gate Compliance

Task 2 and Task 3 each have a `test(05-01)` commit followed by a `feat(05-01)` commit.

- Task 2 RED: both `wasm_host_*` tests compiled and failed on their target assertions (sources loaded with no route and logged `FetchError::NotConfigured`; the skip test saw `["stub", "stub2"]` still loaded). The `HostError::FetcherUnavailable` variant was committed with the RED tests so they compile.
- Task 3 RED: three of eleven policy tests failed on their assertions (`allowlist_ignores_case_and_one_trailing_dot`, `allowlist_entries_that_are_not_bare_hosts_never_match`, `only_http_and_https_urls_pass_the_policy`). The other eight passed already, because the tracer's minimal policy (parse, host, exact lowercase match) covered them; that is an expected unexpected-green for edges the tracer satisfied by construction, not a wrong test. An empty `rejected()` accessor was added in the RED commit so the tests compile.

The RED evidence is console `cargo test` output; the repository has no `gsd_run check tdd-red-evidence` adapter for Rust's libtest format, so no classifier verdict was produced and the semantic assessment above is the executor's own.

## Decisions Made

- **WD-06:** calling the fetcher's async export from inside the source's `fetch` host import worked directly. No spawn, no error about fibers or nested stores. The route's future is awaited inline, so a timeout drop cancels the whole call.
- Test assertions use the guest's actual Debug text. The prebuilt fixtures log `FetchError::Denied(...)` and `FetchError::NotConfigured`, not the bare `Denied(` and `NotConfigured` the plan quoted.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Plan quoted guest log text that the fixture does not produce**
- **Found during:** Task 1
- **Issue:** The plan's assertions looked for `init fetch failed: NotConfigured` and `init fetch failed: Denied(`. The stub-source fixture prints the wit-bindgen Debug form `FetchError::NotConfigured` / `FetchError::Denied(...)`.
- **Fix:** Assertions now match `init fetch failed: FetchError::NotConfigured` and `init fetch failed: FetchError::Denied(`. Behavior under test is unchanged.
- **Files modified:** src/plugins/host/fetch.rs
- **Committed in:** c155d7a

**2. [Rule 1 - Plan inconsistency] Skip test expects two source failures, not one**
- **Found during:** Task 2
- **Issue:** The plan's skip test says "one Source failure named stub", but its registry has two sources (`stub`, `stub2`) naming the same broken fetcher, so both are correctly reported.
- **Fix:** The test asserts one Fetcher failure plus two Source failures, checks `stub`'s error is `FetcherUnavailable` and names both `stub` and `stub-fetcher`.
- **Files modified:** src/plugins/host/mod.rs
- **Committed in:** 9e0e14c

**3. [Rule 3 - Blocking] Clippy gate had three warnings in touched host files**
- **Found during:** Task 3 verification
- **Issue:** The plan's clippy criterion needs zero warnings in the six host files. My test code used `Duration::from_secs(60)` (twice), and `engine.rs:309` (a pre-existing test line from commit c6a30bd) used a redundant closure, which the installed clippy flags.
- **Fix:** `Duration::from_mins(1)` in fetch.rs; `any(char::is_control)` in engine.rs.
- **Files modified:** src/plugins/host/fetch.rs, src/plugins/host/engine.rs
- **Committed in:** 9651498

---

**Total deviations:** 3 auto-fixed (2 Rule 1, 1 Rule 3)
**Impact on plan:** None on scope or behavior; test text and one lint nit.

## Issues Encountered

- The worktree did not contain the built wasm fixtures (they are gitignored and live only in the main checkout). I copied the three prebuilt `.wasm` files into the worktree's own ignored `plugins/*/target/...` paths so the `wasm_` tests could run. They are untracked and ignored; nothing was written to the main checkout.
- `cargo fmt --check` reports diffs in many pre-existing files, so the repo is not rustfmt-clean; I did not reformat anything.
- Plan start time was not captured, so the duration is an estimate.

## Verification Results

- `cargo test -p filimon -- --ignored wasm_tracer_fetch wasm_concurrent_fetches wasm_denied_host`: 3 passed.
- `cargo test -p filimon -- --ignored wasm_`: 25 passed (baseline 20, plus five from this plan), 3 more in the integration binary.
- `cargo test -p filimon`: 385 passed, 0 failed (baseline 374, plus 11 policy tests).
- `cargo clippy -p filimon --all-targets`: no warning in the six host files; `config.rs` has its 3 baseline warnings.
- `git diff --quiet -- Cargo.toml Cargo.lock`: clean, no dependency added.
- All grep acceptance criteria for tasks 1, 2 and 3 pass.

## Known Stubs

None. The fetcher's own `outbound-http.send` still answers `NotConfigured`, which is the plan's stated interim state for plan 05-03, not a stub introduced here.

## Threat Flags

None. The new surface (the `fetch` import and the fetcher instance) is the one the plan's threat register covers: T-5-01 through T-5-06 are mitigated and tested; T-5-02 (end-to-end timeout) is proven in plan 05-06.

## User Setup Required

None.

## Next Phase Readiness

- 05-03 can give a fetcher's `outbound-http.send` real network access; `fetcher_route` and `poll_stub_source` test helpers are in `fetch.rs` for reuse, and `testing.rs` is untouched for 05-03 to own.
- 05-04, 05-06 and 05-07 build on `FetchRoute`/`FetchPolicy`. `FetchPolicy::new` already takes a consumer label, so the crawler can reuse it.
- FETCH-01 and FETCH-02 are not marked complete in REQUIREMENTS.md here: both IDs are shared with sibling plans (05-03, 05-06, 05-08) that have no SUMMARY yet, so the orchestrator owns that write.

## Self-Check: PASSED

Verified: `src/plugins/host/fetch.rs` exists; commits `c155d7a`, `9e0e14c`, `02b596e`, `ed4e224`, `9651498` are ancestors of HEAD; 5 commits counted from the recorded base `e6cc3f0`.

---
*Phase: 05-fetcher-layer*
*Completed: 2026-11-11*
