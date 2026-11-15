---
phase: 03-component-host-runtime
verified: 2026-11-10T00:00:00Z
status: passed
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
  - "plugins/README.md"
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
covered_digest: "v4:sha256:f0ed78de26e83e811d575b3a96e75acd73d802da4e42db1ded860c0aec013c16"
behavior_unverified: 0
overrides_applied: 0
re_verification:
  previous_status: human_needed
  previous_score: 4/4
  gaps_closed: []
  gaps_remaining: []
  regressions: []
human_verification:
  - test: "Decide on the findings raised AFTER the UAT session (03-REVIEW.md WR-08, WR-09, WR-10, IN-05, IN-06) and record a disposition for each in 03-REVIEW-DISPOSITION.md. In particular WR-08 (host memory is not bounded by the 64 MiB guest cap: list lifting copies every item before the 1000-item cap runs, so a hostile component that aliases one large string across many items can exhaust host memory) and WR-09 (the 64 MiB limit is per linear memory; MAX_MEMORIES = 8 allows up to 512 MiB per instance)."
    expected: "Each finding is marked fixed, deferred (with a target phase, e.g. v2 HOST-06 or Phase 7 before third-party plugins) or declined. If the user still accepts the HOST-05 isolation edge, the acceptance should be re-stated against what is actually enforced, because UAT test 2 accepted the edge on the wording 'a guest that tries to grow memory past the fixed 64 MiB per-instance cap is refused', which WR-08 and WR-09 show is narrower than the code (and plugins/README.md lines 222 and 282 say 'per instance')."
    why_human: "These were found by the re-review committed at 23:28+08:00, five minutes after the UAT session closed at 23:23+08:00, so no recorded human result covers them. They are hostile-plugin hardening and documentation-accuracy gaps, not failures of any roadmap success criterion on the normal path; whether to accept, fix or defer them is a risk decision for the user."
---

# Phase 3: Component Host Runtime Verification Report

**Phase Goal:** `fili` hosts WASM components. It loads only what the registry lists, polls each source on its schedule, persists cursors across restarts, isolates failing plugins, and produces returned items to Fluvio as `SourceItem` records
**Verified:** 2026-11-10
**Status:** human_needed
**Re-verification:** Yes. The earlier report (2026-11-09, `human_needed`) predated review-fix commits b7c2324, 60d65ec, 2e7c091 and test commits d252ecc, 7cade0b. Every truth below was checked again against the current tree on branch `fluvio-wise`; nothing was copied from the old report.

All four roadmap success criteria are backed by code and by tests I ran myself on the current tree, including the ignored real-wasm and live-Fluvio tests that the regression gate did not run. No must-have fails. The status is `human_needed` for one reason only: a review pass that finished after the UAT session found warnings that no recorded human result covers (see Human Verification Required).

## Goal Achievement

### Observable Truths (ROADMAP success criteria are the contract)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | Source and fetcher components listed and enabled in `filimon.toml` are loaded; a `.wasm` beside them but not in the registry is never instantiated | VERIFIED | `load_components` (src/plugins/host/loader.rs:74-118) iterates only `registry.sources` and `registry.fetchers`, skips `!enabled` (and sources with no loaded plugin TOML), and pushes every opened path to `LoadReport.opened`. `grep read_dir|glob|WalkDir src/plugins` finds one hit, a test helper in cursor.rs:333. Real-wasm loader tests pass: `wasm_stub_fetcher_loads_as_fetcher`, `wasm_stub_fetcher_rejected_as_source`, `wasm_stub_source_rejected_as_fetcher`, `wasm_one_bad_component_does_not_stop_the_others`, `wasm_host_report_lists_loaded_sources_and_fetchers`. |
| 2 | A test source is polled at its TOML interval; its items appear in a Fluvio topic as `SourceItem` records; after stop and restart polling resumes from the persisted cursor | VERIFIED | Interval: `run_plugin` waits `cfg.poll_interval` after a successful poll (`next_action`, scheduler.rs); fixed-delay scheduler tests in the default suite. Sink: `FluvioSink::send_batch` sends each record with key `{platform}:{id}` then flushes, error-cleared on failure (sink.rs:76-110); the production sink is only built for `TOPIC_SOCIAL_ITEMS` (sink.rs:73). Cursor: `cursors.write` runs only in the `Ok(count)` arm after `send_records` (scheduler.rs:372-391), `cursor` updated in memory afterwards. Live (run by me against the local cluster, scratch `filimon-test-*` topics only): `live_host_end_to_end_stub_to_fluvio_with_restart` PASS, `live_fluvio_sink_produces_records_in_order_with_keys` PASS. Real wasm: `wasm_restart_resumes_from_persisted_cursor`, `wasm_tracer_stub_polls_and_persists_cursor` PASS. |
| 3 | On an error or rate limit (429) the host backs off using the plugin TOML backoff settings and polls again after the window | VERIFIED | `next_action` (scheduler.rs:144-176) with `backoff::step` / `rate_limited_wait`: `initial * multiplier^n` capped at `max` from the plugin's own `HostConfig`; `rate-limited(Some(n))` waits `max(n, step)` capped; success resets; `fatal`/`config` disable; `auth-required` backs off like transient. Real wasm PASS: `wasm_rate_limited_is_backed_off_with_retry_after_not_disabled`, `wasm_transient_and_auth_required_are_backed_off`, `wasm_fatal_disables_only_that_plugin`, `wasm_bad_settings_disable_with_a_config_error`, `wasm_init_fatal_disables_plugin_before_any_poll`. Scope note: 429 reaches the host as the guest's `PollError::RateLimited`; host-side HTTP 429 detection belongs to the Phase 5 fetch layer. |
| 4 | A component that traps or errors is logged with its plugin name and backed off, while the other plugins keep polling and the `fili` process stays up | VERIFIED | Traps and host faults classify to `InstanceFatal`, the instance is discarded and counted as a transient failure; each plugin runs as its own `JoinSet` task and a task panic is caught for that plugin only (mod.rs). Real wasm PASS: `wasm_trap_is_isolated_logged_and_instance_rebuilt`, `wasm_host_task_panic_is_isolated_and_reported`, `wasm_spinning_guest_is_cancelled_on_shutdown_and_does_not_block_other_plugins`, and the new `wasm_guest_memory_growth_past_the_cap_is_refused_and_isolated`. The isolation edge was accepted by the user in UAT test 2 (see caveat in Human Verification). |

**Score:** 4/4 truths verified (0 present-but-behavior-unverified: every behavior-dependent truth has a named test that I ran and that passed). No `coincidental-reliance` flags: the spin test uses 2.5 s of wall time but that is a timing margin, not a hidden precondition.

### Deferred / Accepted Gaps (not actionable gaps)

| # | Item | Addressed In | Evidence |
|---|------|--------------|----------|
| 1 | Configurable per-plugin CPU and memory limits; a spinning guest is never terminated while the host runs; no log-rate limit | v2 HOST-06 (REQUIREMENTS.md line 100) | Accepted in UAT test 2 |
| 2 | Cross-poll item dedupe | v2 HOST-07 (REQUIREMENTS.md line 101) | Accepted in UAT test 2 |
| 3 | Host-enforced fetch allowlist/timeout; real fetch routing (`fetch` returns not-configured) | Phase 5 | ROADMAP Phase 5 goal |
| 4 | A `fili` command that runs the host; `Host` has no caller outside `src/plugins/host/` and `--state-dir` is `allow(dead_code)` (src/cli.rs:47) | Phase 4 SC4 ("The run command polls every enabled source plugin on its interval and produces items to Fluvio") | 03-CONTEXT.md scoped Phase 3 to a library API plus test harness; Phase 3 criteria are all stated at host level |
| 5 | Access token always `none` | Phase 6 | instance.rs poll call passes `None` |

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/plugins/host/{mod,engine,instance,loader,scheduler,backoff,cursor,map,sink,error}.rs` | Host runtime | VERIFIED | All present and substantive (100-1527 lines), wired: `Host::load` -> `load_components` -> `WasmtimeFactory` -> `run_plugin` -> `map_items` -> `ItemSink` -> `CursorStore`. No TBD/FIXME/XXX/TODO/HACK/todo!/unimplemented! in any phase file. |
| `plugins/fault-source` and `fault_source.wasm` | Fault-injection fixture | VERIFIED | Source present; built wasm present for fault-source, stub-source and stub-fetcher; drives the passing real-wasm tests |
| `Cargo.toml`, `rust-toolchain.toml` | wasmtime 48, toolchain 1.96, no WASI | VERIFIED | `wasmtime = { version = "48", default-features = false, ... }` (Cargo.toml:45); Cargo.lock wasmtime 48.0.8; zero `wasmtime-wasi` entries; `rustc --version` at repo root = 1.96.0 |
| `src/cli.rs` `--state-dir` / `FILI_STATE_DIR`; `src/main.rs` logging | State dir flag, stderr subscriber | VERIFIED (flag unconsumed until Phase 4) | cli.rs tests assert global flag and env; flag is dead-code-allowed by design |
| `src/streaming.rs` `TypedProducer::clear_errors` | Sticky-error recovery | VERIFIED | Used by `FluvioSink` on failure and on timeout (`send_bounded`) |
| `plugins/README.md` host section | Guarantees and limits as implemented | PARTIAL | Documents the fixed caps, but states "64 MiB per instance" (lines 222, 282) and "cancelling the host returns within seconds"; WR-09 and WR-10 say both are narrower than the code. Documentation accuracy only. |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| `Host::load` | `load_components` | registry-only iteration | WIRED | mod.rs |
| loaded `SourcePre` | `run_plugin` task per plugin | `WasmtimeFactory` + `JoinSet` | WIRED | mod.rs |
| `run_plugin` poll result | `map_items` -> `ItemSink::send_batch` | `send_records` (sink bounds itself with `SINK_TIMEOUT`) | WIRED | scheduler.rs:367-372 |
| `send_batch` success | `CursorStore::write` | commit-after-flush | WIRED | scheduler.rs:372-391 (write only in the `Ok(count)` arm) |
| persisted cursor | next `instance.poll` | `cursors.read` at task start, in-memory update after write | WIRED | scheduler.rs:305, 390 |
| plugin TOML backoff/interval | `next_action` | `PluginSpec.host` | WIRED | scheduler.rs:144 |
| `FluvioSink` | `filimon-social-items` only | `connect_social` / `TOPIC_SOCIAL_ITEMS` | WIRED | sink.rs:73 |
| `Host` | `fili` CLI command | none | NOT WIRED (by design) | Deferred to Phase 4 SC4 |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| `FluvioSink` records | `SourceItem` | real stub-source wasm poll -> `map_items` | Yes, read back from a real Fluvio topic in the live test | FLOWING |
| Cursor file | `next_cursor` bytes | guest poll response, written after sink flush | Yes, restart tests resume from the saved cursor | FLOWING |

### Behavioral Spot-Checks (run by me on the current tree)

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Real-wasm loader, backoff, isolation, restart, memory cap | `cargo test --offline -p filimon -- --ignored wasm_` | 16 passed, 0 failed, 24.7 s | PASS |
| Live Fluvio: sink order/keys, error recovery, lock-wait, missing topic, end-to-end stub to Fluvio with restart | `cargo test --offline -p filimon -- --ignored live_fluvio_sink live_host` (cluster up; tests use `filimon-test-*` scratch topics, sink.rs:202-210) | 5 passed, 0 failed, 62.2 s | PASS |
| Default suite | Regression gate supplied by the orchestrator: `cargo test` exit 0, 255 passed, 0 failed, 29 ignored (16 wasm + 5 live above are 21 of the 29) | Not re-run by me | PASS (reported) |

### Probe Execution

No `probe-*.sh` scripts are declared by or present for this phase. Step 7c not applicable.

### Requirements Coverage

Requirement IDs in PLAN frontmatter: 03-01 HOST-01..05; 03-02 HOST-03, HOST-05; 03-03 HOST-01..05; 03-04 HOST-01, HOST-02, HOST-04; 03-05 HOST-02, HOST-05; 03-06 HOST-02, HOST-03, HOST-05; 03-07 HOST-01; 03-08 HOST-04; 03-09 HOST-02; 03-10 HOST-02..05. REQUIREMENTS.md maps exactly HOST-01..05 to Phase 3 (traceability rows 132-136). No orphaned requirements; all five are claimed.

| Requirement | Source Plans | Description | Status | Evidence |
|-------------|--------------|-------------|--------|----------|
| HOST-01 | 03-01, 03-03, 03-04, 03-07 | Load only registry-listed source and fetcher components | SATISFIED | Truth 1; edge confirmed by user in UAT test 1 (pass) |
| HOST-02 | 03-01, 03-03..03-06, 03-09, 03-10 | Poll each enabled source on its interval, persist cursor, resume after restart | SATISFIED | Truth 2; live restart test passes. Caveat: WR-07 (empty `--state-dir` resolves to cwd) must be settled when Phase 4 wires the flag |
| HOST-03 | 03-01, 03-02, 03-03, 03-06, 03-10 | Per-plugin TOML poll interval and backoff on errors / 429 | SATISFIED | Truth 3 |
| HOST-04 | 03-01, 03-03, 03-04, 03-08, 03-10 | Returned items produced to Fluvio as `SourceItem` records | SATISFIED | Truth 2 live tests. Caveat: WR-04 (URL scheme), WR-05 (platform impersonation) are open hardening gaps |
| HOST-05 | 03-01, 03-02, 03-03, 03-05, 03-06, 03-10 | A trapping/erroring plugin is logged and backed off without crashing `fili` or stopping others | SATISFIED for trap, guest error, sink error, task panic, spin and memory-grow refusal | Truth 4. Caveats: WR-08/WR-09/WR-10 (hostile resource exhaustion, shutdown latency) are not covered by any recorded human result |

REQUIREMENTS.md checkboxes (lines 18-22) and traceability rows are still unticked ("Pending"); the orchestrator should update them once the human item below is settled.

### Review Findings (03-REVIEW.md: 0 critical, 7 warning, 6 info; WR-01, WR-03, WR-06 fixed)

I confirmed the fixes for WR-01 (engine.rs limits, 1000-item and 4096-byte caps), WR-03 (map.rs key bound before key build, key counted in the record cap) and WR-06 (timeout moved inside `FluvioSink::send_batch` with `clear_errors` on both failure and expiry) in the current code. The still-open ones:

| ID | Bears on | Covered by recorded human result? |
|----|----------|-----------------------------------|
| WR-02, WR-04, WR-05, WR-07, IN-01, IN-02, IN-03, IN-04 | HOST-05, HOST-04, HOST-02 | Yes: UAT test 3 lists exactly these and records `pass`. Note the UAT expected a disposition in 03-REVIEW-DISPOSITION.md, and the ledger still shows all of them `open` (13 open of 16), so the decision is not recorded in the ledger. Advisory only. |
| WR-08, WR-09, WR-10, IN-05, IN-06 | HOST-05 (hostile guest, shutdown latency), docs accuracy | No. First reported at 23:28+08:00, after UAT closed at 23:23+08:00. |

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| src/plugins/host/*, src/cli.rs, src/main.rs, src/streaming.rs, plugins/fault-source/src/lib.rs | - | TBD/FIXME/XXX/TODO/HACK/todo!/unimplemented! scan | none found | No debt-marker blocker |
| src/cli.rs | 47 | `allow(dead_code)` on `state_dir` outside tests | Info | Intentional, consumed by the Phase 4 run command |
| src/plugins/host/engine.rs | - | `fetch` / `outbound-http` return not-configured | Info | Documented interim stub for Phase 5 |

No stubs affect a roadmap truth.

### Human Verification Required

UAT coverage of the earlier report's three items (03-UAT.md, status complete, 3 passed, 0 issues, 2026-11-09T23:23+08:00, after the last source commit at 21:39+08:00): item 1 (HOST-01 edge) pass, item 2 (HOST-05 isolation edge) pass, item 3 (review triage for WR-02/04/05/07 and IN-01..04) pass. Those three are CLOSED and are not re-raised. One new item remains:

#### 1. Post-UAT review findings WR-08, WR-09, WR-10, IN-05, IN-06

**Test:** Read these five findings in 03-REVIEW.md and record a disposition for each in 03-REVIEW-DISPOSITION.md.
**Expected:** Each is fixed, deferred with a target phase, or declined. If the HOST-05 isolation edge is still accepted, restate it against what is enforced: a guest memory-grow past 64 MiB on a single memory is refused, but one instance may hold up to 8 x 64 MiB (WR-09) and host-side lifting of an aliased item list is not bounded by the guest cap (WR-08); shutdown can take up to N x 60 s with a stuck broker (WR-10), whereas plugins/README.md says it returns within seconds.
**Why human:** UAT test 2 accepted the edge on the wording "fixed 64 MiB per-instance cap". The re-review shows that wording is narrower than the code. This is a risk-acceptance call for the user, not something code inspection settles. Not a failed success criterion: the normal and tested failure modes all hold.

### Gaps Summary

No gaps: no truth failed, no artifact is missing or a stub, and no in-scope key link is unwired. The one unwired link (`Host` to a CLI command) is deliberate and owned by Phase 4 SC4. The phase goal is achieved at the host-library level, proven by 16 real-wasm tests and 5 live-cluster tests that I ran on the current tree, plus the orchestrator's 255-test default run. Status is `human_needed` solely because five review findings raised after the UAT session have no recorded human decision.

Not independently re-run by me: the default `cargo test` run (taken from the regression gate) and the `smdk build` proof that the three SmartModules still build on their nested 1.85.0 pins (a plan 03-03 build prerequisite, not a roadmap truth).

---

_Verified: 2026-11-10_
_Verifier: Claude (gsd-verifier)_
