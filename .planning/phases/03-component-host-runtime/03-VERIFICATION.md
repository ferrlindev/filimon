---
phase: 03-component-host-runtime
verified: 2026-11-09T21:00:00Z
status: human_needed
score: 4/4 must-haves verified
covered_files:
  - ".planning/phases/03-component-host-runtime/03-01-PLAN.md"
  - ".planning/phases/03-component-host-runtime/03-01-SUMMARY.md"
  - ".planning/phases/03-component-host-runtime/03-02-PLAN.md"
  - ".planning/phases/03-component-host-runtime/03-02-SUMMARY.md"
  - ".planning/phases/03-component-host-runtime/03-03-PLAN.md"
  - ".planning/phases/03-component-host-runtime/03-03-SUMMARY.md"
  - ".planning/phases/03-component-host-runtime/03-04-PLAN.md"
  - ".planning/phases/03-component-host-runtime/03-04-SUMMARY.md"
  - ".planning/phases/03-component-host-runtime/03-05-PLAN.md"
  - ".planning/phases/03-component-host-runtime/03-05-SUMMARY.md"
  - ".planning/phases/03-component-host-runtime/03-06-PLAN.md"
  - ".planning/phases/03-component-host-runtime/03-06-SUMMARY.md"
  - ".planning/phases/03-component-host-runtime/03-07-PLAN.md"
  - ".planning/phases/03-component-host-runtime/03-07-SUMMARY.md"
  - ".planning/phases/03-component-host-runtime/03-08-PLAN.md"
  - ".planning/phases/03-component-host-runtime/03-08-SUMMARY.md"
  - ".planning/phases/03-component-host-runtime/03-09-PLAN.md"
  - ".planning/phases/03-component-host-runtime/03-09-SUMMARY.md"
  - ".planning/phases/03-component-host-runtime/03-10-PLAN.md"
  - ".planning/phases/03-component-host-runtime/03-10-SUMMARY.md"
  - "Cargo.toml"
  - "plugins/fault-source/src/lib.rs"
  - "rust-toolchain.toml"
  - "src/cli.rs"
  - "src/main.rs"
  - "src/plugins/host/backoff.rs"
  - "src/plugins/host/cursor.rs"
  - "src/plugins/host/engine.rs"
  - "src/plugins/host/error.rs"
  - "src/plugins/host/instance.rs"
  - "src/plugins/host/loader.rs"
  - "src/plugins/host/map.rs"
  - "src/plugins/host/mod.rs"
  - "src/plugins/host/scheduler.rs"
  - "src/plugins/host/sink.rs"
  - "src/plugins/host/testing.rs"
  - "src/plugins/host/wasm_tests.rs"
  - "src/streaming.rs"
covered_digest: "v4:sha256:318b532e400160b260c1ac5a45960ec75642bac698eec9616caf2bd6cb0557a9"
behavior_unverified: 0
overrides_applied: 0
human_verification:
  - test: "Confirm the HOST-01 edge: 'nothing unlisted is loaded' is satisfied by registry-only loading (no directory scan; a sibling .wasm not in filimon.toml and a listed-but-disabled component are never opened)"
    expected: "User agrees this wording and its test proof (loader records every opened path; sibling and disabled files absent from it; no read_dir/glob anywhere under src/plugins/host) is the intended meaning, including that a listed component whose plugin TOML failed to load is also not opened"
    why_human: "Planning-time edge probe flagged for the user and not yet confirmed. The proof is by instrumentation (`LoadReport.opened`) plus absence of any scanning code, not by an OS-level 'file was never read' check."
  - test: "Confirm the HOST-05 isolation edge, including the known spinning-guest gap"
    expected: "User accepts that isolation means: a trap, guest error, host-side sink error, or task panic in one plugin never stops the process or other plugins; a spinning guest is NOT terminated while the host runs (it yields every epoch tick, never blocks siblings or shutdown, but stays stuck in its poll and is never polled again); no CPU/memory limits (HOST-06) and no cross-poll dedupe (HOST-07)"
    why_human: "Planning-time edge probe flagged for the user and not yet confirmed. The spinning-guest and memory behaviours are user-accepted gaps (RESEARCH Open Question 3), but the acceptance of the edge itself has not been recorded."
  - test: "Triage the 7 open review warnings (03-REVIEW-DISPOSITION.md, all 'open') and decide which must be fixed before Phase 4 builds on the host, in particular WR-03 (oversized record key wedge), WR-02 (silent epoch-ticker failure), WR-07 (empty --state-dir) and the non-deferred half of WR-01 (item/cursor/log caps)"
    expected: "Each finding is marked fixed, deferred (with a target phase) or declined in 03-REVIEW-DISPOSITION.md"
    why_human: "Advisory review findings; no must-have fails on the normal path, but several are hostile/buggy-plugin hardening gaps that bear on HOST-05 and HOST-04. A fix-or-defer decision belongs to the user."
---

# Phase 3: Component Host Runtime Verification Report

**Phase Goal:** `fili` hosts WASM components. It loads only what the registry lists, polls each source on its schedule, persists cursors across restarts, isolates failing plugins, and produces returned items to Fluvio as `SourceItem` records
**Verified:** 2026-11-09
**Status:** human_needed
**Re-verification:** No - initial verification

All four roadmap success criteria are backed by code and by tests I re-ran on the integrated tree. Status is `human_needed` rather than `passed` only because two planning-time edge probes are still unconfirmed by the user (Human Verification items 1 and 2), and because the advisory review ledger is untriaged (item 3). No must-have is FAILED.

## Goal Achievement

### Observable Truths (ROADMAP success criteria are the contract)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | Source and fetcher components listed and enabled in `filimon.toml` are loaded; a `.wasm` sitting beside them but not in the registry is never instantiated | VERIFIED | `load_components` (src/plugins/host/loader.rs:74-118) iterates only `registry.sources`/`registry.fetchers`, skips `!enabled` (and sources with no loaded plugin config), and records every opened path in `LoadReport.opened`. `grep read_dir/glob/WalkDir src/plugins` finds only a test helper in cursor.rs. Test `unlisted_and_disabled_components_are_never_opened` writes a, b(disabled), c(unlisted) and asserts `opened == [a]`. Fetchers: `wasm_stub_fetcher_loads_as_fetcher` passes (real wasm); wrong-world and foreign (WASI) imports are rejected with named errors (`wasm_stub_source_rejected_as_fetcher`, `wasm_stub_fetcher_rejected_as_source`, `preflight_imports`). |
| 2 | A test source is polled at its TOML interval; its items appear in a Fluvio topic as `SourceItem` records; after stop and restart polling resumes from the persisted cursor | VERIFIED | Interval: `run_plugin` waits `cfg.poll_interval` after `PollOk` (scheduler.rs:147-152); `sched_polls_are_fixed_delay_and_never_overlap` passes (fixed-delay, not fixed-rate). Fluvio: `FluvioSink::send_batch` sends each record with key `{platform}:{id}` then flushes (sink.rs:76-93); production sink is only built for `TOPIC_SOCIAL_ITEMS`. Cursor: written atomically (temp + fsync + rename + dir fsync) only after `send_batch` returned (scheduler.rs:372-391; cursor.rs `write`). Verified live by me: `live_host_end_to_end_stub_to_fluvio_with_restart` (real stub wasm to a scratch topic, run, stop, restart on same state dir, read back: ids `stub-0..stub-N-1`, no gap, no repeat, key == `stub:{id}`, `source_type == Social`) passes. Also `wasm_restart_resumes_from_persisted_cursor` and `wasm_tracer_stub_polls_and_persists_cursor` pass. |
| 3 | On an error or rate limit (429) the host backs off using the plugin TOML backoff settings and polls again after the window | VERIFIED | `next_action` (scheduler.rs:144-176) + `backoff::step`/`rate_limited_wait` (backoff.rs): `initial * multiplier^n` capped at `max`, `rate-limited(Some(n))` waits `max(n, step)` capped, success resets; `fatal`/`config` disable, `auth-required` backs off like transient. `cfg.backoff` is the plugin's own `HostConfig` (PluginSpec.host, loaded from per-plugin TOML in Phase 1). Pure-function tests plus paused-time scheduler tests (`sched_*`) pass; real wasm: `wasm_rate_limited_is_backed_off_with_retry_after_not_disabled`, `wasm_transient_and_auth_required_are_backed_off`, `wasm_fatal_disables_only_that_plugin`, `wasm_bad_settings_disable_with_a_config_error`, `wasm_init_fatal_disables_plugin_before_any_poll` all pass. Note: 429 is surfaced by the guest as `PollError::RateLimited`; host-side HTTP 429 detection arrives with the Phase 5 fetch layer (out of this phase's scope). |
| 4 | A component that traps or errors is logged with its plugin name and backed off, while the other plugins keep polling and the `fili` process stays up | VERIFIED (user confirmation of the edge pending, see Human Verification 2) | Traps/host faults classify to `InstanceFatal`, discard the instance, count as transient (`classify`, `next_action`); each plugin is its own `JoinSet` task and a task panic is caught and reported for that plugin only (mod.rs:153-173). Logs carry `plugin = %name` and pass through `clean_guest_text`. Real wasm: `wasm_trap_is_isolated_logged_and_instance_rebuilt` (trapper rebuilt after backoff; neighbour keeps its exact schedule; logs contain `plugin=trapper` and the trap; no ANSI), `wasm_host_task_panic_is_isolated_and_reported`, `wasm_spinning_guest_is_cancelled_on_shutdown_and_does_not_block_other_plugins` all pass. |

**Score:** 4/4 truths verified (0 present-but-behavior-unverified; every behaviour-dependent truth has a passing named test, including the real-wasm and live-cluster ones).

### Deferred / Accepted Gaps (not actionable gaps)

| # | Item | Addressed In | Evidence |
|---|------|--------------|----------|
| 1 | Per-plugin CPU and memory limits; a spinning guest is never terminated while the host runs | v2 HOST-06 (REQUIREMENTS.md line 100); user-accepted at RESEARCH Open Question 3 | plugins/README.md:263-267 states the limit as implemented; `wasm_spinning_guest...` proves the host stays responsive and shuts down |
| 2 | Cross-poll item dedupe | v2 HOST-07 (REQUIREMENTS.md line 101) | plugins/README.md:268-269 |
| 3 | Host-enforced fetch allowlist/timeout; real fetch routing (`fetch` returns `not-configured`) | Phase 5 | ROADMAP Phase 5 SC1-SC2 |
| 4 | A user-facing `fili` command that runs the host (`Host::load`/`Host::run` have no caller in `main.rs`; `--state-dir` is parsed with `allow(dead_code)`) | Phase 4 SC4 ("The run command polls every enabled source plugin ...") | 03-CONTEXT.md lines 11 and 36 deliberately scoped Phase 3 to a library API plus test harness. The Phase 3 success criteria are all stated at host/test-source level and are met. |
| 5 | Access token is always `none` | Phase 6 | instance.rs `call_poll(.., None)` |

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/plugins/host/{mod,engine,instance,loader,scheduler,backoff,cursor,map,sink,error}.rs` | Host runtime | VERIFIED | All exist, substantive (160-1400 lines), wired: `Host::load` -> `load_components` -> `WasmtimeFactory` -> `run_plugin` -> `map_items` -> `ItemSink` -> `CursorStore`. No TBD/FIXME/XXX/todo!/unimplemented! in any phase file. |
| `plugins/fault-source` + built `fault_source.wasm` | Fault-injection fixture | VERIFIED | Source exists; built wasm present under `plugins/fault-source/target/wasm32-unknown-unknown/release/`; drives 15 passing real-wasm tests |
| `Cargo.toml`, `rust-toolchain.toml` | wasmtime 48, toolchain 1.96.0, rust-version 1.95, base64, tokio-util | VERIFIED | wasmtime 48.0.8 in Cargo.lock, no `wasmtime-wasi` in lock; `rustc --version` from the repo root reports 1.96.0 (override gone); SmartModule crates keep nested 1.85.0 pins |
| `src/cli.rs` `--state-dir`/`FILI_STATE_DIR`; `src/main.rs` stderr subscriber with `fili::plugins=info` default | State dir flag, logging | VERIFIED (wired for logging; flag unconsumed until Phase 4) | `init_logging()` is called in `main` before dispatch; default filter test passes |
| `src/streaming.rs` `TypedProducer::clear_errors` | Sticky-error recovery | VERIFIED | Present (streaming.rs:252), called by `FluvioSink` on every failure path |
| `plugins/README.md` host section | Guarantees and limits as implemented | VERIFIED | Limits section at lines ~263-273 matches the code |
| `.gitignore` `.fili-state/` | State dir not committed | VERIFIED | Line 28 |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `Host::load` | `load_components` | registry-only iteration | WIRED | mod.rs:104 |
| loaded `SourcePre` | `run_plugin` task per plugin | `WasmtimeFactory` + `JoinSet` | WIRED | mod.rs:107-114, 141-151 |
| `run_plugin` poll result | `map_items` -> `ItemSink::send_batch` | `send_records` under `SINK_TIMEOUT` | WIRED | scheduler.rs:367-372 |
| `send_batch` success | `CursorStore::write` | commit-after-flush | WIRED | scheduler.rs:372-391 (write only inside the `Ok(count)` arm) |
| persisted cursor | next `instance.poll` | `cursors.read` at task start, in-memory update after write | WIRED | scheduler.rs:305, 361, 390 |
| plugin TOML backoff/interval | `next_action` | `PluginSpec.host` from `LoadedSource.plugin` | WIRED | scheduler.rs:44-58, 144 |
| `FluvioSink` | `filimon-social-items` only | `connect_social` / `TOPIC_SOCIAL_ITEMS`; no plugin or registry value selects a topic | WIRED | sink.rs:65-68 |
| `Host` | CLI (`fili` command) | - | NOT WIRED (by design) | No command calls `Host`; deferred to Phase 4 (see Deferred item 4) |

### Data-Flow Trace (Level 4)

| Artifact | Data | Source | Produces Real Data | Status |
|----------|------|--------|--------------------|--------|
| `FluvioSink` records | `SourceItem` | real stub-source wasm poll -> `map_items` | Yes, read back from a real Fluvio topic in the live test | FLOWING |
| Cursor file | `next_cursor` bytes | guest `poll` response, written after sink flush | Yes, restart test resumes `stub-N` | FLOWING |

### Behavioral Spot-Checks (re-run by me on the integrated tree)

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Default suite | `cargo test --offline -p filimon` | 238 passed, 0 failed, 26 ignored | PASS |
| Real-wasm isolation, backoff, restart, loader | `cargo test --offline -p filimon -- --ignored wasm_` | 15 passed, 0 failed | PASS |
| Live Fluvio: sink order/keys, restart no gap/no repeat, missing-topic message | `cargo test --offline -p filimon -- --ignored live_` (scratch topics only; I confirmed in code that live tests create `filimon-test-*` topics and never touch the real topics) | 3 passed, 0 failed | PASS |

### Probe Execution

No `probe-*.sh` scripts are declared by or present for this phase. Step 7c not applicable.

### Requirements Coverage

Every ID appears in PLAN frontmatter (03-01: HOST-01..05; 03-02: HOST-03, HOST-05; 03-03: HOST-01..05; 03-04: HOST-01, HOST-02, HOST-04; 03-05: HOST-02, HOST-05; 03-06: HOST-02, HOST-03, HOST-05; 03-07: HOST-01; 03-08: HOST-04; 03-09: HOST-02; 03-10: HOST-02..05) and in REQUIREMENTS.md (lines 18-22, traceability rows 132-136, all still "Pending"). No orphaned requirements: REQUIREMENTS.md maps exactly HOST-01..05 to Phase 3, and all are claimed.

| Requirement | Source Plans | Description | Status | Evidence |
|-------------|--------------|-------------|--------|----------|
| HOST-01 | 03-01, 03-03, 03-04, 03-07 | Load only registry-listed source and fetcher components; nothing unlisted is loaded | SATISFIED (edge pending user confirmation) | Truth 1. Open: Human Verification 1 |
| HOST-02 | 03-01, 03-03, 03-04, 03-05, 03-06, 03-09, 03-10 | Poll each enabled source on its interval, persist returned cursor, resume after restart | SATISFIED | Truth 2: fixed-delay scheduler, atomic commit-after-flush cursor, live restart test with no gap/no repeat |
| HOST-03 | 03-01, 03-02, 03-03, 03-06, 03-10 | Per-plugin TOML poll interval and backoff on errors / 429 | SATISFIED | Truth 3. 429 as guest `rate-limited`; host-side HTTP 429 detection belongs to Phase 5 |
| HOST-04 | 03-01, 03-03, 03-04, 03-08, 03-10 | Returned items produced to Fluvio as `SourceItem` records | SATISFIED | Truth 2 live test (key `{platform}:{id}`, `SourceType::Social`, deserialised back as `SourceItem`). Caveat: WR-03/WR-04/WR-05 are hardening gaps on hostile items |
| HOST-05 | 03-01, 03-02, 03-03, 03-05, 03-06, 03-10 | A trapping/erroring plugin is logged and backed off without crashing `fili` or stopping others | SATISFIED (edge pending user confirmation) | Truth 4. Open: Human Verification 2; WR-01/WR-02 are hardening caveats |

Plan 03-01 correctly declares `requirements-completed: []` (it is a consent gate); completion above is my per-requirement judgment from evidence. The REQUIREMENTS.md checkboxes and traceability table are still unticked and should be updated by the orchestrator once the human items are settled.

### Code Review Findings That Bear on the Must-Haves (03-REVIEW.md, 0 critical, 7 warnings, 4 info; all `open`)

I read the cited code to confirm each is real; none changes a success-criterion verdict on the normal path, but they are not nothing.

| ID | Confirmed in code? | Bears on | Goal still achieved? |
|----|--------------------|----------|----------------------|
| WR-03 record key unbounded and not in `MAX_RECORD_BYTES` (map.rs:70-89; cap checks only the serialized value, key `{platform}:{id}` is guest-controlled) plus every sink error is treated as transient (scheduler.rs `SinkFailed` keeps instance and cursor) | Yes | HOST-04, HOST-05 | Yes for SC2/SC4. A hostile or buggy plugin emitting ~1 MB id/platform can make one poison record fail `send_batch` permanently; the plugin then retries forever (bounded by its backoff `max`), re-sending the valid records before it each time (duplicate growth) and never advancing its cursor. It is isolated (other plugins unaffected, process up), so SC4 holds, but "produced as SourceItem records" degrades for that plugin. Recommend fixing before real plugins (Phase 7). |
| WR-01 no `ResourceLimiter`; unbounded items per poll, cursor size, log volume (instance.rs:95-105, engine.rs) | Yes | HOST-05 | Memory limiting is explicitly deferred to v2 HOST-06 (user-accepted). The items/cursor/log caps are not named in HOST-06's text and are untracked; a guest can OOM the process via `memory.grow`, which contradicts "does not take down the host" for the hostile-plugin case. Accepted gap, but the non-memory caps need a decision. |
| WR-02 `EpochTicker` spawn failure swallowed with `.ok()` (engine.rs:155-164) | Yes | HOST-05 | Yes under normal conditions. If the thread cannot spawn, `HostEngine::new` still returns `Ok` and the anti-wedge guarantee silently vanishes. Low probability, cheap fix. |
| WR-04 URL scheme unrestricted (`javascript:`, `file:` accepted) (map.rs:58-63) | Yes | HOST-04 | Yes. Hardening gap against a hostile plugin, not a criterion failure. |
| WR-05 guest-chosen `platform` can impersonate another plugin; key encoding ambiguous (map.rs:65-70) | Yes | HOST-04 | Yes. Policy decision needed; downstream consumers key on `{platform}:{id}`. |
| WR-06 `send_batch` not cancellation-safe under the sink timeout (sink.rs:76-93, scheduler.rs:222) | Yes | HOST-04, HOST-05 | Yes. Only reachable on a 60 s sink stall; extra duplicates and one extra backoff cycle. |
| WR-07 empty `--state-dir` resolves to CWD (mod.rs:52-57) | Yes | HOST-02 | Yes today, because no command consumes the flag yet. Must be fixed or validated when Phase 4 wires it. |
| IN-01..IN-03 | Not individually re-verified | HOST-05 / HOST-02 | Cosmetic or edge-hardening. IN-03 (missing `cursor` key reads as first poll) is a small contradiction of the "never reset automatically" policy for hand-edited files. |
| IN-04 core isolation tests are all `#[ignore]` and need manual fixture builds; no CI wiring | Yes (26 ignored tests; there is no CI config in repo) | HOST-05 | Real-wasm isolation proof only exists when someone runs `-- --ignored wasm_`. I ran it and it passes; ongoing regression protection is process-dependent. |

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| src/plugins/host/* , src/cli.rs, src/main.rs, src/streaming.rs, plugins/fault-source/src/lib.rs | - | TBD/FIXME/XXX/TODO/HACK/todo!/unimplemented! scan | none found | No debt-marker blocker |
| src/cli.rs | 44-50 | `#[allow(dead_code)]` on `state_dir` outside tests | Info | Intentional, documented as consumed by Phase 4 run command |
| src/plugins/host/engine.rs | 150-160 | `fetch`/`outbound-http` return `NotConfigured` | Info | Documented interim stub for Phase 5; not a Phase 3 truth |

No stubs affect a roadmap truth: the sink, cursor store, scheduler and loader are real implementations exercised against real wasm and a real cluster.

### Human Verification Required

#### 1. HOST-01 edge: "nothing unlisted is loaded"

**Test:** Review the planning-time edge probe and the proof described in Truth 1 (instrumented `opened` list, no scanning code, disabled and unlisted garbage files never opened, a sibling-file test).
**Expected:** You confirm this is the intended meaning, including that a source enabled in the registry whose plugin TOML failed to load is also not opened.
**Why human:** Flagged for the user at planning time and not yet confirmed. The proof is by instrumentation and code absence, not OS-level file-access tracing.

#### 2. HOST-05 edge: isolation including the spinning-guest gap

**Test:** Review the isolation guarantees listed in Truth 4 against the known gaps (spinning guest never terminated while the host runs; no CPU/memory limits, HOST-06; no dedupe, HOST-07; WR-01 memory-bomb can OOM the process).
**Expected:** You accept this as the meaning of "without crashing `fili` or stopping other plugins" for v1.
**Why human:** Flagged at planning time and not yet confirmed; it is a scope/risk-acceptance call, not something code inspection can settle.

#### 3. Review disposition

**Test:** Triage the 11 findings in `03-REVIEW-DISPOSITION.md` (all `open`).
**Expected:** Each marked fixed, deferred-to-phase, or declined. My recommendation: fix WR-03, WR-02 and WR-07 and decide the WR-01 non-memory caps before Phase 4 or real plugins build on this host.
**Why human:** Advisory findings; the fix-versus-defer call is the user's.

### Gaps Summary

No gaps: no truth failed, no artifact is missing or a stub, and no key link inside the phase's scope is unwired. The one unwired link (`Host` to a CLI command) is intentional and is owned by Phase 4 SC4. The phase goal is achieved at the host-library level, proven by 238 default tests, 15 real-wasm tests and 3 live-cluster tests that I re-ran myself. The status is `human_needed` because the two edge probes are unconfirmed and the review ledger is untriaged.

Not independently re-run by me: the `smdk build` proof that the three SmartModules still build on their nested 1.85.0 pins (plan 03-03 build prerequisite, not a roadmap truth). I only confirmed the nested `rust-toolchain.toml` files are intact.

---

_Verified: 2026-11-09_
_Verifier: Claude (gsd-verifier)_
