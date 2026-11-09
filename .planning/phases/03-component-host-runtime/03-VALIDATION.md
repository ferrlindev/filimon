---
phase: "3"
slug: "component-host-runtime"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: draft
nyquist_compliant: false
wave_0_complete: false
created: "2026-11-09"
---

# Phase 3 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in harness; tokio `#[tokio::test(start_paused = true)]` (`test-util` present) |
| **Config file** | none (Cargo.toml) — Wave 0 adds toolchain pins |
| **Quick run command** | `cargo test plugins::host` |
| **Full suite command** | `cargo test` |
| **Wasm fixture tests** | `cargo test plugins::host -- --ignored wasm_` (needs built fixtures) |
| **Live Fluvio tests** | `cargo test plugins::host -- --ignored live_` (needs cluster + topic `filimon-social-items`) |
| **Estimated runtime** | ~60 seconds (quick/full); ignored groups extra |

---

## Sampling Rate

- **After every task commit:** Run `cargo test plugins::host`
- **After every plan wave:** Run `cargo test`
- **Before `/gsd-verify-work`:** Full `cargo test`, then `-- --ignored wasm_`, then (cluster up) `-- --ignored live_` — all green. Plain `cargo test` does not exercise real wasm; report ignored groups separately.
- **Max feedback latency:** 90 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 3-xx-xx | TBD | TBD | HOST-01 | T-3-xx | Only enabled registry entries opened; WASI-importing / wrong-world component rejected | unit + `wasm_` | `cargo test plugins::host::loader` | ❌ W0 | ⬜ pending |
| 3-xx-xx | TBD | TBD | HOST-02 | T-3-xx | Cursor committed only after flush; corrupt file disables only that plugin | unit + `wasm_` restart | `cargo test plugins::host::scheduler` / `plugins::host::cursor` | ❌ W0 | ⬜ pending |
| 3-xx-xx | TBD | TBD | HOST-03 | — | Backoff sequence, cap, reset, `retry_after` max rule | unit (paused time) | `cargo test plugins::host::backoff` | ❌ W0 | ⬜ pending |
| 3-xx-xx | TBD | TBD | HOST-04 | T-3-xx | Guest items validated (id/url/size); epoch-ms → RFC 3339; key `{platform}:{id}` | unit + `live_` | `cargo test plugins::host::map` / `plugins::host::sink` | ❌ W0 | ⬜ pending |
| 3-xx-xx | TBD | TBD | HOST-05 | T-3-xx | Trap/error logged with plugin name; others keep polling; process stays up | unit (fake factory, log capture) + `wasm_` | `cargo test plugins::host::scheduler` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*
*Task IDs are filled in by the planner once plans exist.*

---

## Wave 0 Requirements

- [ ] Toolchain checkpoint (human action) and Cargo.toml changes (RESEARCH Open Question 1)
- [ ] `plugins/fault-source/` fixture crate + README entry; build both stubs and the fixture
- [ ] `src/plugins/host/` skeleton with `ItemSink` trait, `VecSink`, fake `InstanceFactory`
- [ ] Log-capture helper (scoped `fmt` subscriber into a shared buffer)
- [ ] `TypedProducer::clear_errors`, `init_logging`, `--state-dir` flag, `.gitignore` entry

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Toolchain change (`rustup override unset`) | HOST-01..05 (build prerequisite) | Machine-local setting; needs user consent | Run `rustup override unset` in the repo, then `cargo +1.96.0 check` |
| Live Fluvio produce | HOST-04 | Local cluster not confirmed running | `fluvio cluster start`, create `filimon-social-items`, run `-- --ignored live_` |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 90s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
