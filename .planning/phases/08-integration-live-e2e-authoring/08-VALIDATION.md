---
phase: "08"
slug: integration-live-e2e-authoring
status: draft
nyquist_compliant: false
wave_0_complete: false
created: "2026-11-12"
---

# Phase 08 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in harness (`cargo test`), `#[tokio::test]` |
| **Config file** | none; `[dev-dependencies] tokio` with `test-util` already in `Cargo.toml` |
| **Quick run command** | `cargo test -p filimon e2e_replay_ -- --nocapture` |
| **Full suite command** | `cargo test -p filimon` (plus the `--ignored` groups in `plugins/README.md`) |
| **Estimated runtime** | ~30 seconds quick (warm build cache, running cluster) |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p filimon e2e_replay_ -- --nocapture`
- **After every plan wave:** Run `cargo test -p filimon` and `scripts/verify-authoring.sh`
- **Before `/gsd-verify-work`:** Full suite must be green with a reachable Fluvio cluster (a skip is not a pass: record "not run")
- **Max feedback latency:** 30 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 08-XX-XX | — | — | TEST-02 | — | Reddit + YouTube replay lands exact `SourceItem`s in a scratch Fluvio topic | integration (needs cluster, skips otherwise) | `cargo test -p filimon e2e_replay_ -- --nocapture` | ❌ W0 | ⬜ pending |
| 08-XX-XX | — | — | TEST-02 | — | Missing cluster skips with a clear message | integration | same, run with Fluvio stopped (manual) | ❌ W0 | ⬜ pending |
| 08-XX-XX | — | — | TEST-03 | — | Same body against real APIs; skips with `skipped: missing <KEY>` | integration | `cargo test -p filimon e2e_credentialed_ -- --nocapture` | ❌ W0 | ⬜ pending |
| 08-XX-XX | — | — | TEST-04 | — | Enriched social + news JSON from a real run | manual-only (D-07) | `scripts/live-e2e.sh` | ❌ W0 | ⬜ pending |
| 08-XX-XX | — | — | WIT-03 | — | Out-of-repo plugin builds against only the WIT and runs in an unmodified `fili` | script | `scripts/verify-authoring.sh` | ❌ W0 | ⬜ pending |
| 08-XX-XX | — | — | DOCS-01 | — | Template builds in-repo as-is and via guide steps | build | `(cd plugins/template && cargo component build --release)` and the script | ❌ W0 | ⬜ pending |
| 08-XX-XX | — | — | DOCS-02 | — | Fetcher built by following the guide serves a source's requests | script | `scripts/verify-authoring.sh` (fetcher leg) | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

*Task IDs are filled in by the planner once plans exist.*

---

## Wave 0 Requirements

- [ ] `plugins/replay-fetcher/` crate (and its built `.wasm` via helper)
- [ ] `plugins/template/` crate
- [ ] `src/plugins/host/integration_tests.rs` plus `mod` line in `src/plugins/host/mod.rs`
- [ ] Extended registry writer in `src/plugins/host/testing.rs`
- [ ] `scripts/verify-authoring.sh`, `scripts/live-e2e.sh`
- [ ] `docs/plugin-guide.md`
- [ ] `.env.example` additions, `plugins/README.md` updates, `.gitignore` entry for the live-E2E work dir
- [ ] Framework install: none

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Live E2E: `fili` polls real Reddit + YouTube, consumer writes NER-enriched social JSON beside news | TEST-04 | Needs real credentials, a GLiNER model download and a local cluster (D-07) | Run `scripts/live-e2e.sh`; commit the sanitised sample and transcript under the phase dir |
| Authoring proof with the unmodified `fili` binary | WIT-03 | The user runs it once as the SC4 proof (D-12) | Run `scripts/verify-authoring.sh` |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 30s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
