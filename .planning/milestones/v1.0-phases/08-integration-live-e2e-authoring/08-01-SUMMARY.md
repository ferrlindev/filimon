---
phase: 08-integration-live-e2e-authoring
plan: 01
subsystem: testing
tags: [wasm-component, replay-fetcher, fluvio, cargo-component, tokio-test, integration-test]

requires:
  - phase: 03-component-host
    provides: Host::load / Host::run, FluvioSink, live_support scratch-topic helpers, fetcher assignment
  - phase: 05-egress-and-auth
    provides: allowlist enforcement before the fetcher, host-side token delivery, write_tokens test helper
  - phase: 06-reddit-and-youtube-plugins
    provides: reddit_source and youtube_source components and their listing / playlist fixtures
provides:
  - replay-fetcher component (fetcher world) serving frozen fixtures, one editable fixtures.rs table
  - ExtSource, write_registry_ext, is_stale, ensure_built (build-on-demand with staleness check) in host test helpers
  - in-crate e2e_replay_ tests: Reddit tracer, both platforms into a scratch Fluvio topic, both platforms into an in-memory sink, staleness rule
affects: [08-02, 08-04, 08-05, plugin-guide]

actuals:
  tokens: 9500
  tasks: 2
  commits: 3

tech-stack:
  added: []
  patterns:
    - "Replay through the real fetcher path: the host checks the allowlist on the original URL, then the assigned replay component answers from compiled-in fixtures"
    - "Runtime skip (printed line, early return) instead of #[ignore]; FILI_TEST_REQUIRE_FLUVIO=1 makes the skip a failure"
    - "One assertion body (assert_replay_items) fed by a Fluvio driver and an in-memory driver; expectations computed from the fixture files at test time"
    - "ensure_built: process-wide mutex held across cargo component build, env_remove of toolchain / target / rustflags variables, staleness ignores generated src/bindings.rs"

key-files:
  created:
    - plugins/replay-fetcher/Cargo.toml
    - plugins/replay-fetcher/Cargo.lock
    - plugins/replay-fetcher/rust-toolchain.toml
    - plugins/replay-fetcher/.cargo/config.toml
    - plugins/replay-fetcher/.gitignore
    - plugins/replay-fetcher/src/lib.rs
    - plugins/replay-fetcher/src/fixtures.rs
    - src/plugins/host/integration_tests.rs
  modified:
    - src/plugins/host/testing.rs
    - src/plugins/host/mod.rs

key-decisions:
  - "Missing cargo-component when a build is needed is a loud panic naming the install command, not a skip (research open question 2; confirmed by the user at Task 3: fail-loudly)"
  - "Reddit replay uses an authorization-code [host.oauth] plus a pre-written token file so the host never calls the real token endpoint"
  - "Order across platforms is not asserted; per-platform order is oldest first, computed by sorting the fixture-derived published value"
  - "ensure_built memoises verified crates in the process-wide set so later tests skip even the staleness walk"

patterns-established:
  - "Replay fetcher split: lib.rs generic glue, fixtures.rs the one file an author edits (the plugin guide's worked example, D-13)"
  - "Scratch topics filimon-test-replay-<nanos>-<pid>-<n>, deleted also when the body panics (catch_unwind)"

requirements-completed: [TEST-02]

duration: 25min
completed: 2026-11-12
status: complete
plan_head_before: a2e32c66493b73235314e2a39580454f917644d1
plan_head_after: c6ccccd27b033bb95cfca51751375c6e7cc24563
commits: 3

coverage:
  - id: D1
    description: "replay-fetcher component conforms to world fetcher, imports no wasi interface, serves the Reddit and YouTube fixtures and a fixed 404 on a miss"
    requirement: TEST-02
    verification:
      - kind: other
        ref: "(cd plugins/replay-fetcher && cargo component build --release && wasm-tools component targets ../../wit -w fetcher target/wasm32-unknown-unknown/release/replay_fetcher.wasm)"
        status: pass
    human_judgment: false
  - id: D2
    description: "One Reddit poll runs through the real host with its fetch answered by the replay fetcher and exactly the three fixture SourceItems land in a scratch Fluvio topic, oldest first, key reddit:<id>"
    requirement: TEST-02
    verification:
      - kind: integration
        ref: "src/plugins/host/integration_tests.rs#e2e_replay_reddit_poll_lands_exact_items_in_a_scratch_topic"
        status: pass
    human_judgment: false
  - id: D3
    description: "Reddit and YouTube replay in one host run into one scratch topic: six distinct keys, exact per-platform items, API key delivered through the env closure"
    requirement: TEST-02
    verification:
      - kind: integration
        ref: "src/plugins/host/integration_tests.rs#e2e_replay_reddit_and_youtube_land_exact_items_in_a_scratch_topic"
        status: pass
    human_judgment: false
  - id: D4
    description: "The same two-platform host run into an in-memory sink passes the same assertion body with no cluster"
    requirement: TEST-02
    verification:
      - kind: integration
        ref: "src/plugins/host/integration_tests.rs#e2e_replay_host_path_needs_no_cluster"
        status: pass
    human_judgment: false
  - id: D5
    description: "Staleness rule ignores generated src/bindings.rs and sees a newer source, manifest, lock, WIT file, fixture or a missing wasm"
    requirement: TEST-02
    verification:
      - kind: unit
        ref: "src/plugins/host/integration_tests.rs#e2e_replay_staleness_ignores_generated_bindings"
        status: pass
      - kind: unit
        ref: "src/plugins/host/integration_tests.rs#e2e_replay_staleness_sees_a_newer_source_or_fixture"
        status: pass
    human_judgment: false
  - id: D6
    description: "With the cluster stopped, e2e_replay_ passes and prints the skipped: no reachable Fluvio cluster line for the cluster-backed tests"
    requirement: TEST-02
    verification: []
    human_judgment: true
    rationale: "Stopping the user's local Fluvio cluster is not done by an automated command; the plan routes this to a one-time manual check"
  - id: D7
    description: "Missing-toolchain policy of ensure_built (fail loudly with the install command)"
    verification: []
    human_judgment: true
    rationale: "Task 3 decision answered by the user (fail-loudly), which is what is implemented"
---

# Phase 8 Plan 01: Replay Fetcher And E2E Replay Harness Summary

**A fixture-serving `replay-fetcher` component plus an in-crate harness that runs the real Reddit and YouTube components through the host with replayed HTTP and lands exactly the fixture `SourceItem`s in a scratch Fluvio topic (or an in-memory sink), skipping cleanly when no cluster is reachable.**

Status is `complete`: Tasks 1 and 2 are done and verified. Task 3 (`checkpoint:decision`, missing-toolchain policy) was answered by the user with `fail-loudly` on 2026-11-12, which is the implemented behaviour, so no code change was needed.

## Performance

- **Duration:** about 25 min (first run of each component build is a few minutes cold)
- **Completed:** 2026-11-12
- **Tasks:** 3 of 3 (Tasks 1-2 executed; Task 3 decision answered by the user: fail-loudly, no code change)
- **Files modified:** 10 (8 created, 2 modified)

## Accomplishments

- `plugins/replay-fetcher` builds to a conformant `fetcher`-world component (`wasm-tools component targets` exit 0, no `wasi:` import). `lib.rs` is generic glue (substring match over `fixtures::TABLE`, fixed 404 body `{"replay":"no fixture"}`, fixed-text logs only, no outbound-http import); `fixtures.rs` is the one editable table with the Reddit and YouTube entries.
- Tracer: the compiled `reddit_source.wasm` ran inside the host against a real local Fluvio cluster with `fetcher = "replay"`, a stored token and an authorization-code `[host.oauth]`; the three listing items landed in a unique `filimon-test-replay-...` topic, read back oldest first with keys `reddit:<id>`, and the topic was deleted (no scratch topic left behind).
- Both platforms in one host run: six distinct keys, per-platform exact items computed from the fixtures at test time, YouTube API key delivered only through the env closure.
- The same assertion body runs against an in-memory `VecSink` with no cluster (`e2e_replay_host_path_needs_no_cluster`).
- `ensure_built` builds missing or stale components on demand under a process-wide mutex with the inherited toolchain variables removed; `is_stale` ignores the regenerated `src/bindings.rs` (two unit tests).
- Cluster skip: `fluvio_or_skip` prints `skipped: no reachable Fluvio cluster (start one with `fluvio cluster start`)`; `FILI_TEST_REQUIRE_FLUVIO=1` turns it into a failure.

## Task Commits

1. **Task 1: End-to-end replay of one Reddit poll into a scratch Fluvio topic** (tracer) - `71beb5f` (feat)
2. **Task 2: YouTube replay, both platforms, shared assertion (TDD)**
   - RED - `21c3fee` (test): `e2e_replay_host_path_needs_no_cluster` expected 6 records and got 3 (no YouTube fixture entry yet); it ran, the Reddit half produced its 3 records, so the failure was on the planned assertion, not a load or syntax fault
   - GREEN - `c6ccccd` (feat): YouTube fixture entry added, all five `e2e_replay_` tests pass
3. **Task 3: Confirm the missing-toolchain policy** - decision answered by the user: `fail-loudly` (no code change)

**Plan metadata:** this SUMMARY (docs commit).

## Files Created/Modified

- `plugins/replay-fetcher/Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `.cargo/config.toml`, `.gitignore` - crate scaffold copied from stub-fetcher (own workspace, toolchain pin 1.96.0, no new dependency)
- `plugins/replay-fetcher/src/lib.rs` - generic fetcher glue
- `plugins/replay-fetcher/src/fixtures.rs` - the fixture table (Reddit `oauth.reddit.com/r/rust/new`, YouTube `playlistId=UU_x5XG1OV2P6uZZ5FSM9Ttw`)
- `src/plugins/host/testing.rs` - `ExtSource`, `write_registry_ext`, `is_stale`, `ensure_built`; existing helpers untouched
- `src/plugins/host/integration_tests.rs` - harness, drivers, expected-item computation, five tests
- `src/plugins/host/mod.rs` - `#[cfg(test)] mod integration_tests;`

## Decisions Made

- Fail loudly (panic naming the install command) when `cargo-component` is missing and a build is needed; confirmed by the user at Task 3.
- `ensure_built` memoises verified crates for the life of the process, so the staleness walk runs once per component per test binary run.
- The Reddit ordering check is derived (sort the fixture posts by `created_utc`) rather than a literal id list, keeping every expected value computed from the fixtures.

## Deviations from Plan

None - plan executed exactly as written. Two small additions inside the plan's scope: the Task 1 host-run helper was written generic over the sink up front (so Task 2 only adds the in-memory driver), and `rustfmt` / clippy cleanups (single-pattern `match` to `if let`, doc backticks, a scoped cast allow) were applied to the new test module only. The repository's existing files are not fmt-clean and were left alone.

**Total deviations:** 0 auto-fixed.

## Issues Encountered

- Cold builds: the first `cargo test` compiles the whole crate plus three components (several minutes); later runs take seconds. The README test section for this cost is owned by a later plan.
- The shell guard in this worktree refused a few compound commands; they were split. No effect on the result.

## Authentication Gates

None.

## Known Stubs

None.

## Threat Flags

None. `ensure_built` spawning `cargo component build` is the plan's T-08-04 surface and is mitigated as specified (single mutex, toolchain variables removed). T-08-01 gates hold: no `connect_social`, no `#[ignore`, no `start_paused` in the module; T-08-02: no log call carries request data; T-08-03: the sample registry does not mention the replay fetcher.

## Verification Evidence

- `FILI_TEST_REQUIRE_FLUVIO=1 cargo test -p filimon e2e_replay_` against the user's local cluster: 5 passed, 0 failed (reddit tracer, both platforms, host path no cluster, two staleness tests).
- `cargo test -p filimon plugins::host -- --skip e2e_`: 222 passed, 0 failed, 47 ignored (existing host tests unaffected).
- Task 1 and Task 2 acceptance greps all exit 0; `git ls-files` lists no `.wasm`; `wit/` unchanged.
- Not run: the manual skip-path check (stop the cluster, run `cargo test -p filimon e2e_replay_ -- --nocapture`, expect the skip line for the cluster-backed tests). Routed to the verifier / user as a human check; the skip branch is implemented and reviewed but was not exercised.

## Next Phase Readiness

- Plan 08-04 can add the live backend to the same `Backend` / `assert_replay_items` body.
- Plan 08-05 can use `plugins/replay-fetcher` (`lib.rs` glue + `fixtures.rs`) as the guide's worked fetcher example.
- Resolved: the user confirmed `fail-loudly` for the Task 3 toolchain policy. The `skip-with-message` alternative was not chosen. The skip-on-no-cluster path remains unexercised and is a human check (coverage D6).

---
*Phase: 08-integration-live-e2e-authoring*
*Completed: 2026-11-12*

## Self-Check: PASSED

All created files exist on disk; commits `71beb5f`, `21c3fee`, `c6ccccd` are ancestors of HEAD.
