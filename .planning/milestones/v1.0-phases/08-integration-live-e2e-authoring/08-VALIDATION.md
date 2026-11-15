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
| 08-01-01 | 01 | 1 | TEST-02 | T-08-01, T-08-02 | Reddit replay through the replay fetcher lands the exact `SourceItem`s in a scratch Fluvio topic; replay fetcher logs fixed text only | integration (needs cluster, skips otherwise) | `FILI_TEST_REQUIRE_FLUVIO=1 cargo test -p filimon e2e_replay_reddit` | ❌ W0 | ⬜ pending |
| 08-01-02 | 01 | 1 | TEST-02 | T-08-01 | Reddit and YouTube replay in one host run; same assertion on a Fluvio sink and an in-memory sink; staleness rule | integration + unit | `FILI_TEST_REQUIRE_FLUVIO=1 cargo test -p filimon e2e_replay_` | ❌ W0 | ⬜ pending |
| 08-01-02 | 01 | 1 | TEST-02 | — | Missing cluster skips with a clear message | manual (`<human-check>`: stop the cluster, run `cargo test -p filimon e2e_replay_ -- --nocapture`) | same, run with Fluvio stopped (manual) | ❌ W0 | ⬜ pending |
| 08-04-01 | 04 | 2 | TEST-03 | T-08-14, T-08-17 | Gate names the first missing key with `skipped: missing <KEY>`; env layering; literal `.env` values | unit (no cluster, no network) | `env -u REDDIT_CLIENT_ID -u REDDIT_CLIENT_SECRET -u YOUTUBE_API_KEY -u REDDIT_USER_AGENT_CONTACT cargo test -p filimon e2e_credentialed_` | ❌ W0 | ⬜ pending |
| 08-04-02 | 04 | 2 | TEST-03 | T-08-14, T-08-15, T-08-16 | Same host body against real APIs with credentials; skips cleanly without them; no secret in the temp registry | integration (credential-gated) | `env -u REDDIT_CLIENT_ID -u REDDIT_CLIENT_SECRET -u YOUTUBE_API_KEY -u REDDIT_USER_AGENT_CONTACT FILI_TEST_ENV_FILE=/dev/null cargo test -p filimon e2e_credentialed_ -- --nocapture` (skip path); live path is run by 08-06-03 | ❌ W0 | ⬜ pending |
| 08-04-03 | 04 | 2 | TEST-02, TEST-03 | — | Test groups, env vars, replay-fetcher and template documented in `plugins/README.md` | doc gate | grep chain in the task's verify block | ❌ W0 | ⬜ pending |
| 08-03-01 | 03 | 1 | TEST-04 | T-08-10, T-08-13 | Preflight names unmet prerequisites; D-09 stop with exit 3; no secret printed | script | `ENV_FILE=<empty file> scripts/live-e2e.sh preflight` (exit 3) | ❌ W0 | ⬜ pending |
| 08-03-02 | 03 | 1 | TEST-04 | T-08-10 | Sanitiser, secret scan, evidence check and waiver rule proven on synthetic data | script (hermetic) | `scripts/live-e2e.sh selftest` | ❌ W0 | ⬜ pending |
| 08-06-03 | 06 | 3 | TEST-04, TEST-03 | T-08-22, T-08-23 | Enriched social + news JSON from a real run; credentialed tests pass with real keys | manual-only (D-07) | `scripts/live-e2e.sh run`, then `scripts/live-e2e.sh check-evidence <evidence dir>` | ❌ W0 | ⬜ pending |
| 08-02-01 | 02 | 1 | WIT-03, DOCS-01 | T-08-08 | Template builds in-repo as-is; out-of-repo build against only the WIT runs in an unmodified `fili` | build + script | `(cd plugins/template && cargo component build --release)` and `scripts/verify-authoring.sh` | ❌ W0 | ⬜ pending |
| 08-02-02 | 02 | 1 | WIT-03 | T-08-06, T-08-07 | Remote distribution path (sparse clone at a ref), cleanup and refusal guards | script (hermetic remote via a throwaway bare clone) | `WIT_REPO=file://<bare clone> WIT_REF=<tag> scripts/verify-authoring.sh` (command in the task) | ❌ W0 | ⬜ pending |
| 08-05-02 | 05 | 2 | DOCS-02 | T-08-18 | Fetcher built by following the guide serves a source's requests; unmatched 404; assignment removed gives no canned answer | script | `scripts/verify-authoring.sh` (fetcher leg) | ❌ W0 | ⬜ pending |
| 08-05-03 | 05 | 2 | DOCS-01, DOCS-02 | T-08-18, T-08-19 | Guide headings, tag line, identical sparse-checkout command, trust statement, no dead links | doc gate | grep and link-check commands in the task's verify block | ❌ W0 | ⬜ pending |
| 08-07-01 | 07 | 3 | WIT-03 | T-08-26 | Tag exists locally and on origin; the proof runs from the tag | script (network) | `WIT_REF=<tag> scripts/verify-authoring.sh` | ❌ W0 | ⬜ pending |
| 08-07-02 | 07 | 3 | WIT-03 | — | The user runs the authoring proof once (SC4, D-12) | manual-only | `scripts/verify-authoring.sh` run by the user; transcripts committed | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

*Task IDs are `08-<plan>-<task>`; filled in by the planner on 2026-11-12. Task 08-01-03 (build-tool policy), 08-05-01 (tag name and walkthrough shape) and 08-06-02 (Reddit waiver) are decision checkpoints with nothing to run.*

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
