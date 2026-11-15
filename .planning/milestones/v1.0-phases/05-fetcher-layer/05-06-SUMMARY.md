---
phase: 05-fetcher-layer
plan: 06
subsystem: plugin-host
tags: [wasmtime, fetcher, allowlist, timeout, run-once, test-fixtures]
status: complete

requires:
  - phase: 05-fetcher-layer
    provides: FetchRoute and FetchPolicy (05-01), the http-fetcher component (05-02), Egress with host-run redirects and one deadline per call (05-03)
provides:
  - Real-wasm proof that the registry's `fetcher` value alone decides who serves a source's request (FETCH-01)
  - Real-wasm proof that the allowlist and the timeout hold for the default fetcher and for a custom fetcher that tries to escape them (FETCH-02)
  - stub-fetcher `x-stub-mode` test behaviours (`spin`, `forward:<url>`) and stub-source `fetch_headers` setting
  - loader::load_route_for_source, the single-source route builder
  - `fili plugins run-once` routes fetch through the assigned fetcher like `fili run`
affects: [05-04, 05-05, 05-07, 05-08]

actuals:
  tokens: 8500
  tasks: 3
  commits: 5
plan_head_before: b2203dedaeb22d8082f29c24bc3ad7a4fa4b94d1
plan_head_after: 6d538894946622d5b4793f282604c049acd3086f

tech-stack:
  added: []
  patterns:
    - "A stub fetcher driven by a request header doubles as the hostile custom fetcher, so registries and components stay unchanged between tests"
    - "Wall-clock assertions time only the poll, not component compilation (a debug build compiles a component in seconds)"
    - "run-once and fili run share one routing rule: the route comes from the registry assignment, a fetcher that cannot load is FetcherUnavailable naming both plugins"

key-files:
  created: []
  modified:
    - src/plugins/host/wasm_tests.rs
    - src/plugins/host/loader.rs
    - src/commands/plugins.rs
    - plugins/stub-fetcher/src/lib.rs
    - plugins/stub-source/src/lib.rs
    - plugins/README.md

key-decisions:
  - "WD-18: the stub fetcher is the custom fetcher, steered by `x-stub-mode` (`spin`, `forward:<url>`); any other request keeps the canned 200"
  - "WD-19: run-once loads the fetcher before the source's own component, so a broken dependency is reported first and by name"
  - "load_route_for_source treats a missing or disabled fetcher entry like a load failure (FetcherUnavailable), matching Host::load"

patterns-established:
  - "poll_timed / poll_logs: poll the real stub source once through a given route, returning logs and the poll's own duration"

requirements-completed: [FETCH-01, FETCH-02]

coverage:
  - id: D1
    description: "Changing only the source's `fetcher =` line changes who serves its requests, with nothing rebuilt"
    requirement: FETCH-01
    verification:
      - kind: integration
        ref: "src/plugins/host/wasm_tests.rs#wasm_changing_the_assigned_fetcher_changes_who_serves_the_request"
        status: pass
    human_judgment: false
  - id: D2
    description: "The default fetcher refuses an off-list host and an off-list redirect, and a slow server fails as timeout in well under the server's delay"
    requirement: FETCH-02
    verification:
      - kind: integration
        ref: "src/plugins/host/wasm_tests.rs#wasm_default_fetcher_is_refused_a_host_outside_the_allowlist"
        status: pass
      - kind: integration
        ref: "src/plugins/host/wasm_tests.rs#wasm_default_fetcher_is_denied_a_redirect_outside_the_allowlist"
        status: pass
      - kind: integration
        ref: "src/plugins/host/wasm_tests.rs#wasm_slow_server_times_out_through_the_default_fetcher"
        status: pass
    human_judgment: false
  - id: D3
    description: "A custom fetcher cannot send outside the source's allowlist or outlive its deadline, and an interrupted call leaves the route usable"
    requirement: FETCH-02
    verification:
      - kind: integration
        ref: "src/plugins/host/wasm_tests.rs#wasm_custom_fetcher_cannot_send_outside_the_allowlist"
        status: pass
      - kind: integration
        ref: "src/plugins/host/wasm_tests.rs#wasm_spinning_custom_fetcher_times_out_and_the_route_survives"
        status: pass
    human_judgment: false
  - id: D4
    description: "The deadline belongs to each call: a slow call times out at its own deadline while a concurrent fast call returns at once"
    requirement: FETCH-02
    verification:
      - kind: integration
        ref: "src/plugins/host/wasm_tests.rs#wasm_a_slow_call_does_not_time_out_a_fast_concurrent_call"
        status: pass
    human_judgment: false
  - id: D5
    description: "`fili plugins run-once` routes a source's fetch through its assigned fetcher; no fetcher key still gets not-configured; an unloadable fetcher is reported naming both plugins before any poll"
    requirement: FETCH-01
    verification:
      - kind: integration
        ref: "src/commands/plugins.rs#wasm_run_once_routes_fetch_through_the_assigned_fetcher"
        status: pass
      - kind: integration
        ref: "src/commands/plugins.rs#wasm_run_once_a_source_without_a_fetcher_still_gets_not_configured"
        status: pass
      - kind: unit
        ref: "src/commands/plugins.rs#run_once_names_both_plugins_when_the_assigned_fetcher_cannot_load"
        status: pass
    human_judgment: false

duration: ~75min
completed: 2026-11-11
---

# Phase 5 Plan 06: Routing and Enforcement Proof, run-once Routing Summary

**On real wasm components the registry's `fetcher` line alone decides who serves a source's request, the allowlist and the per-call timeout hold for the default fetcher and for a hostile custom one (steered by an `x-stub-mode` header on the stub fetcher), and `fili plugins run-once` now builds the same fetch route as `fili run`.**

## Performance

- **Duration:** about 75 min (start time was not recorded at launch; estimated, including a long first build of the worktree)
- **Completed:** 2026-11-11
- **Tasks:** 3
- **Files modified:** 6 (none created)

## Accomplishments

- Tracer (FETCH-01, roadmap criterion 1): two registries that differ only in the source's `fetcher =` value, same three components. With `stub-fetcher` the log has `stub fetch:` and `init fetch status 200` and the loopback server saw nothing; with `http` the log has `init fetch status 200`, no `stub fetch:` line, and the server saw exactly one `GET /swap`.
- FETCH-02, default fetcher: an off-list host is `Denied` before a fetcher starts (`route.calls() == 0`, server saw nothing); a redirect from an allowed host to `localhost` is `Denied` and the second host was never contacted (server saw only `/r`); a server that waits 5 s fails as `Timeout` with the poll itself well under 3 s.
- FETCH-02, custom fetcher: the stub fetcher is told to `forward:http://evil.invalid/steal`, its `send` is `Denied` naming `evil.invalid` after its own `stub fetch:` line; a spinning fetcher is cut off as `Timeout` under 3 s and the same route then serves a normal call (`calls() == 2`).
- Concurrency edge: a 3 s call and an immediate call through one route with a 1 s deadline give `Timeout` for the slow one and 200 for the fast one, which finished first and under 800 ms.
- `fili plugins run-once` loads the assigned fetcher first (WD-19) via `load_route_for_source` and gives the factory the route: the fetcher's own log line and `init fetch status 200` appear; no `fetcher` key still gets `FetchError::NotConfigured` and the item is printed; an unloadable fetcher fails with `HostError::FetcherUnavailable` naming `stub` and `f`, before any poll and with no state directory created.
- Both fixtures stay backward compatible: all pre-existing wasm tests still pass with the rebuilt components.

## Task Commits

1. **Task 1: tracer, the registry's fetcher value decides who serves** - `4a48062` (feat; tests only, no production code)
2. **Task 2: enforcement on real wasm** - RED `4020e03` (test), GREEN `4ac4718` (feat: fixtures plus README)
3. **Task 3: run-once routing** - RED `b1e0cfa` (test), GREEN `6d53889` (feat)

**Plan metadata:** committed with this SUMMARY (docs).

## TDD Gate Compliance

Task 2 and Task 3 each have a `test(05-06)` commit followed by a `feat(05-06)` commit.

- Task 2 RED: `wasm_custom_fetcher_cannot_send_outside_the_allowlist` and `wasm_spinning_custom_fetcher_times_out_and_the_route_survives` failed because the stub source rejected the new `fetch_headers` setting (`unknown field`), i.e. the fixture lacked the feature under test. The other four passed already: the default-fetcher and concurrency behaviour was built in plans 05-01 and 05-03, so they are an expected unexpected-green, not wrong tests. One test (the 400 ms timeout) failed first on a wrong measurement (it timed component compilation) and was fixed in the test before the RED commit.
- Task 3 RED: `run_once_names_both_plugins_when_the_assigned_fetcher_cannot_load` failed with `ComponentLoad` for the source (not `FetcherUnavailable`) and `wasm_run_once_routes_fetch_through_the_assigned_fetcher` failed with `init fetch failed: FetchError::NotConfigured`; `wasm_run_once_a_source_without_a_fetcher_still_gets_not_configured` passed already, as it describes today's behaviour.
- Task 1 is a tracer that adds no production code; it passed on first run, proving the existing routing.

The repository has no `tdd-red-evidence` adapter for libtest output, so no classifier verdict was produced; the assessment above is console evidence and the executor's own.

## Files Created/Modified

- `src/plugins/host/wasm_tests.rs` - seven real-wasm tests plus helpers (`registry_assigning`, `run_until_one_record`, `route_over`, `poll_timed`, `poll_logs`, `start_with_port`)
- `plugins/stub-fetcher/src/lib.rs` - `x-stub-mode` header: `spin`, `forward:<url>`; now uses `outbound-http`
- `plugins/stub-source/src/lib.rs` - optional `fetch_headers` table on the init fetch
- `plugins/README.md` - documents the two fixture extensions as host-test only
- `src/plugins/host/loader.rs` - `load_route_for_source`
- `src/commands/plugins.rs` - `run_once` builds the route before loading the source; three tests

## Decisions Made

- WD-18 and WD-19 as stated in the plan.
- `load_route_for_source` reports a fetcher entry that is missing or disabled as `FetcherUnavailable` (reason "not registered" / "disabled in filimon.toml"), matching what `Host::load` does for a fetcher that is not loaded.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Plan inconsistency] Guest log text quoted without the `FetchError::` prefix**
- **Found during:** Task 2 (same finding as plan 05-01)
- **Issue:** The plan quotes `init fetch failed: Denied(` and `init fetch failed: Timeout`; the fixture prints the wit-bindgen Debug form `FetchError::Denied(...)` and `FetchError::Timeout`.
- **Fix:** Assertions match the real text. Behaviour under test is unchanged.
- **Files modified:** src/plugins/host/wasm_tests.rs
- **Committed in:** 4020e03

**2. [Rule 1 - Bug in test] Wall-clock bound measured component compilation**
- **Found during:** Task 2 RED run
- **Issue:** `poll_logs` as specified loads (compiles) the stub source inside the timed region; in a debug build that took about 7 s and failed the 3 s bound for a call that correctly timed out at 400 ms.
- **Fix:** Added `poll_timed`, which times only `poll_once`; `poll_logs` wraps it. `poll_logs`/`poll_timed` also take the `HostEngine`, because the route's compiled component and the source must share one wasmtime engine (the plan's two-argument signature cannot do that).
- **Files modified:** src/plugins/host/wasm_tests.rs
- **Committed in:** 4020e03

**3. [Rule 3 - Blocking] The wasm fixtures were not built in the worktree**
- **Found during:** Task 1 precondition check
- **Issue:** The `.wasm` files are gitignored and absent from a fresh worktree.
- **Fix:** Copied the three prebuilt fixtures from the main checkout (read-only) into this worktree's ignored `plugins/*/target` paths and built `plugins/http-fetcher` here. Nothing tracked changed. Then rebuilt `stub-fetcher` and `stub-source` per Task 2.
- **Committed in:** not applicable (ignored files)

**4. [Rule 2 - Missing documentation] `plugins/README.md` contradicted the fixture**
- **Found during:** Task 2
- **Issue:** The README said the stub fetcher "never calls `outbound-http`", and the threat register (T-5-18) relies on the README stating that the fixture modes are for host tests only. The file is not in the plan's `files_modified`.
- **Fix:** Updated the two README entries.
- **Files modified:** plugins/README.md
- **Committed in:** 4ac4718

---

**Total deviations:** 4 auto-fixed (2 Rule 1, 1 Rule 2, 1 Rule 3). **Impact:** none on scope or behaviour.

## Issues Encountered

- **Orchestrator action:** the rebuilt `stub_fetcher.wasm` and `stub_source.wasm` are gitignored, so they exist only in this worktree. After merging, the main checkout's copies are stale: rebuild both with `(cd plugins/stub-fetcher && cargo component build --release)` and the same for `stub-source`, or the new `wasm_` tests (and `wasm_run_once_*`) fail there on `unknown field fetch_headers`.
- Plan start time was not captured, so the duration is an estimate.
- `cargo fmt --check` is not clean in many pre-existing files; nothing was reformatted.

## Verification Results

- `cargo test -p filimon -- --ignored wasm_changing_the_assigned_fetcher`: 1 passed.
- The six Task 2 enforcement tests: 6 passed. `wasm_` ignored suite: 35 passed in the unit binary (plan gate: more than 30; baseline 26 plus 9 from this plan), 3 passed in `plugins_cli`.
- `cargo test -p filimon commands::plugins`: 47 passed, 4 ignored. `cargo test -p filimon --test plugins_cli`: 12 passed, 3 ignored. `cargo test -p filimon`: 401 passed, 0 failed.
- `cargo test -p filimon -- --ignored wasm_run_once`: the two new tests and the existing `wasm_run_once_stub_json_prints_a_social_source_item` pass; the CLI binary test `wasm_run_once_stub_dry_run_prints_stub_0_and_creates_no_state` passes.
- Fixture conformance: `wasm-tools component targets` accepts `stub_fetcher.wasm` against world `fetcher` and `stub_source.wasm` against world `source`.
- `cargo clippy -p filimon --all-targets`: finished with no warning in `loader.rs`, `wasm_tests.rs` or `commands/plugins.rs`.
- Acceptance greps: `x-stub-mode`, `fetch_headers`, `outbound_http` present in the fixtures and `x-stub-mode` absent from `examples/`; fixture and root `Cargo.toml`/`Cargo.lock` unchanged; `load_route_for_source` appears at line 816 of `commands/plugins.rs`, before `load_source(` at line 817.

## Known Stubs

None.

## Threat Flags

None beyond the plan's register. T-5-19 (run-once routing parity) is mitigated and covered by `run_once_names_both_plugins_when_the_assigned_fetcher_cannot_load` and `wasm_run_once_routes_fetch_through_the_assigned_fetcher`; T-5-20's cap (a fetcher cannot ship a header to an unlisted host) is proven by `wasm_custom_fetcher_cannot_send_outside_the_allowlist`; T-5-18 and T-5-SC hold (modes absent from `examples/`, no manifest or lockfile changed).

## User Setup Required

None.

## Next Phase Readiness

- Roadmap success criteria 1 and 2 are proven on real components; 05-04 (ordering and request validation), 05-05 (TLS) and 05-07/05-08 can build on `route_over`, `poll_timed` and `load_route_for_source`.
- FETCH-01 and FETCH-02 are not marked complete in REQUIREMENTS.md here: both IDs are shared with sibling plans, so the orchestrator owns that write.

## Self-Check: PASSED

- `src/plugins/host/wasm_tests.rs`, `src/plugins/host/loader.rs`, `src/commands/plugins.rs`, `plugins/stub-fetcher/src/lib.rs`, `plugins/stub-source/src/lib.rs`, `plugins/README.md`: FOUND (modified).
- Commits `4a48062`, `4020e03`, `4ac4718`, `b1e0cfa`, `6d53889`: in HEAD history; 5 commits counted from the recorded base `b2203de`.

---
*Phase: 05-fetcher-layer*
*Completed: 2026-11-11*
