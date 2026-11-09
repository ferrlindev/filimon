---
phase: 03-component-host-runtime
plan: 04
subsystem: plugin-host
tags: [wasmtime, component-model, tokio, epoch-interruption, cursor-store, tracer]
requires:
  - phase: 03-component-host-runtime
    provides: "03-03: wasmtime 48 / base64 / tokio-util dependencies and the Rust 1.96.0 root pin"
  - phase: 01-plugin-contract
    provides: "wit/ source world, LoadedRegistry, stub-source fixture"
provides:
  - "src/plugins/host/ module tree: Host::load / Host::run, HostEngine, loader, instance traits, scheduler, CursorStore, ItemSink, map_items"
  - "State file format <state_dir>/<alias>.json {version:1, plugin, cursor: base64|null} (D-05)"
  - "Tracer proof: registry-listed stub source -> SourceItem -> sink -> atomic cursor, and restart resume"
affects: [03-05, 03-06, 03-07, 03-08, 03-09, 03-10]
tech-stack:
  added: []
  patterns:
    - "async wasmtime 48 host: bindgen! imports async|trappable, exports async; epoch yield on a std::thread ticker"
    - "InstanceFactory / PluginInstance / ItemSink traits return impl Future + Send (no async-trait)"
    - "one tokio task per plugin, fixed-delay wait, cursor written only after send_batch (D-07)"
key-files:
  created:
    - src/plugins/host/mod.rs
    - src/plugins/host/error.rs
    - src/plugins/host/engine.rs
    - src/plugins/host/loader.rs
    - src/plugins/host/instance.rs
    - src/plugins/host/scheduler.rs
    - src/plugins/host/cursor.rs
    - src/plugins/host/sink.rs
    - src/plugins/host/map.rs
    - src/plugins/host/testing.rs
    - src/plugins/host/wasm_tests.rs
  modified:
    - src/plugins/mod.rs
key-decisions:
  - "WASI is not linked at all; the Linker holds only filimon:plugin/* (named rejection text is plan 03-07)"
  - "Interim imports: fetch -> not-configured, access token none, log -> tracing with a plugin field (cleaned, 4096-char clip)"
  - "Cursor persist failure keeps the in-memory cursor and keeps running (RESEARCH A6)"
  - "Fluvio key is {platform}:{id}, assuming a single-partition topic (RESEARCH A4, unconfirmed with the user)"
requirements-completed: []
requirements-advanced: [HOST-01, HOST-02, HOST-04]
status: complete
commits: 2
plan_head_before: 32bb5134d3915494a420375fc4ed784f43080790
plan_head_after: 69d55275564fcea1a37cf15b281def9170a89dfb
actuals:
  tokens: 14000
  tasks: 2
  commits: 2
duration: 40 min
completed: 2026-11-09
---

# Phase 3 Plan 04: Tracer slice - poll, map, sink, persist cursor, resume Summary

A registry-listed `stub` source is loaded, polled under paused time, mapped to a social `SourceItem` (key `stub:stub-0`), handed to a sink and checkpointed to `<state_dir>/stub.json` (cursor `AQ==`) only after the sink returned; a fresh host on the same state dir resumes at `stub-3`.

## Accomplishments

- `src/plugins/host/` created with one file per layer, matching the `<interfaces>` contract (names and signatures as specified).
- Engine: `epoch_interruption(true)`, 10 ms ticker on a dedicated `std::thread` stopped and joined on drop, no WASI linked, `async_support` never called.
- Loader: iterates only `enabled && plugin.is_some()` registry sources, records every opened path in `LoadReport.opened`, one bad component never stops the others; no `read_dir`.
- Scheduler: per-plugin task (read cursor, instantiate, `init` with secrets exposed only there, poll loop raced against shutdown, send bounded by `SINK_TIMEOUT` and not raced against shutdown, then cursor write, then fixed-delay wait). Every non-success outcome ends that plugin with a logged `disabled` reason; a task panic is reported for that plugin only.
- Cursor store: atomic temp file + `sync_all` + rename, version 1, tolerant reads; missing file is a first poll, unreadable/corrupt file is `CursorUnreadable` / `CursorCorrupt` naming plugin and path (D-08).
- Tests: 10 non-ignored unit tests (all named tests from the plan) and 2 ignored wasm tests, all passing. `cargo test -p filimon`: 144 passed, 0 failed, 10 ignored. `cargo clippy -p filimon` prints no line from `src/plugins/host/`.

## Task Commits

1. Task 1 (tracer): host module tree and tracer test - `c6a30bd`
2. Task 2: restart resumes from persisted cursor test - `69d5527`

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Built the stub-source fixture inside the worktree**
- **Found during:** Task 1 verification
- **Issue:** `fixture_wasm` resolves against `CARGO_MANIFEST_DIR`, so the prebuilt `.wasm` in the main checkout is invisible from the worktree.
- **Fix:** ran `cargo component build --release` in `plugins/stub-source` (gitignored `target/`; nothing committed, no machine-local rustup setting touched).

**2. [Rule 1 - Lint] Clippy pedantic fixes in new files**
- Split `send_records` out of `run_plugin` (too_many_lines), `Duration::from_mins(1)` for `SINK_TIMEOUT`, renamed private `Host` field to `runtime` (struct_field_names), and an `#[allow(clippy::needless_pass_by_value, reason = ...)]` on `Host::load` to keep the fixed `HostOptions`-by-value signature.

### Notes

- Task 2 is marked `tdd="true"` but its test passed on first run with no production change, because the Task 1 slice already reads the persisted cursor before `init` and writes it only after the sink call. No RED commit exists for it; the test is a regression guard, not a driver of new code.
- The file-creation heredoc/append for the second test was replaced by an Edit because the sandbox refused compound shell commands; no content change.

## Known Stubs

None. Interim imports (`fetch` -> `not-configured`, access token none) are deliberate per the plan (Phase 5/6 replace them).

## Threat Flags

None. T-3-07 (no WASI linked), T-3-08 (no directory scan), T-3-09 (secrets exposed only when building the `init` list; nothing sensitive logged), T-3-10 (`clean_guest_text` on every guest string, unit-tested), T-3-11 (cursor path built only from registry aliases), T-3-12 (epoch yield on every Store), T-3-SC (no new package) addressed as specified. T-3-13 remains an accepted risk (no CPU/memory limit; HOST-06 deferred).

## Open items for later plans

- Test-build dead-code warning: `LoadFailure.{kind,name,error}` are never read yet (plan 03-07 consumes them). Non-test build is covered by the module-level `allow(dead_code)`.
- HOST-01 unlisted/disabled-files-never-opened proof is plan 03-07; the Fluvio key assumption (single partition) is still unconfirmed with the user.

## Worktree notes

STATE.md and ROADMAP.md were not touched (orchestrator owns them). Git-dir sentinel/ledger files were not used; `commits:` measured from the dispatch base `32bb513`: `git rev-list --count 32bb513..HEAD` = 2 before this SUMMARY commit.

## Self-Check: PASSED

- FOUND: all eleven files under `src/plugins/host/` and `pub mod host;` in `src/plugins/mod.rs`
- FOUND commits: `c6a30bd`, `69d5527`
