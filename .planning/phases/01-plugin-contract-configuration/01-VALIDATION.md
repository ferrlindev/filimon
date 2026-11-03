---
phase: "1"
slug: "plugin-contract-configuration"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: draft
nyquist_compliant: false
wave_0_complete: false
created: "2026-11-03"
---

# Phase 1 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Source: `01-RESEARCH.md` § Validation Architecture.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in `#[test]` (unit tests inside the `fili` bin crate; no lib target, no `tests/` dir) + `wasm-tools` / `cargo component` gate for WIT and stubs |
| **Config file** | none — Wave 0 installs `cargo-component@0.21.1` and `wasm-tools@1.263.0` |
| **Quick run command** | `cargo test --bin fili plugins::` |
| **Full suite command** | `cargo test --bin fili && cargo clippy --bin fili -- -D warnings` plus the WIT/stub gate below |
| **WIT/stub gate** | `wasm-tools component wit wit/ >/dev/null && (cd plugins/stub-source && cargo component build --release) && (cd plugins/stub-fetcher && cargo component build --release) && wasm-tools component targets wit -w source plugins/stub-source/target/wasm32-unknown-unknown/release/stub_source.wasm && wasm-tools component targets wit -w fetcher plugins/stub-fetcher/target/wasm32-unknown-unknown/release/stub_fetcher.wasm` |
| **Estimated runtime** | ~15 seconds (config tests); stub builds longer on first run (dependency download) |

---

## Sampling Rate

- **After every task commit:** Run `cargo test --bin fili plugins::` (config tasks) or the WIT/stub gate (WIT tasks)
- **After every plan wave:** Run the full suite + WIT/stub gate
- **Before `/gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 120 seconds (excluding one-time tool installs)

---

## Per-Task Verification Map

Filled in by the planner/executor against the final task IDs. Requirement → test map from research:

| Requirement | Behavior | Test Type | Automated Command | File Exists | Status |
|-------------|----------|-----------|-------------------|-------------|--------|
| WIT-01 | `wit/` parses; `source` world exports `init`,`poll`, imports `log`,`fetch` | smoke | `wasm-tools component wit wit/ \| grep -E 'export (init\|poll)\|import (log\|fetch)'` | ❌ W0 | ⬜ pending |
| WIT-01 | stub source builds from `../../wit` and conforms to `source` | integration | `cd plugins/stub-source && cargo component build --release && wasm-tools component targets ../../wit -w source target/wasm32-unknown-unknown/release/stub_source.wasm` | ❌ W0 | ⬜ pending |
| WIT-02 | `fetcher` world exports `fetch`, imports outbound-http; stub conforms | integration | same as above with `-w fetcher` on `stub_fetcher.wasm` | ❌ W0 | ⬜ pending |
| WIT-01/02 | negative: stub source does NOT satisfy `fetcher` | smoke | `! wasm-tools component targets wit -w fetcher <stub_source.wasm>` | ❌ W0 | ⬜ pending |
| CONF-01 | sample registry + plugin TOMLs load into typed config; paths relative to registry dir | unit | `cargo test --bin fili plugins::config::tests::loads_sample_registry` | ❌ W0 | ⬜ pending |
| CONF-02 | plugin receives only its aliased secrets; process env overrides `.env` | unit | `cargo test --bin fili plugins::secrets::tests` | ❌ W0 | ⬜ pending |
| CONF-03 | missing plugin TOML / malformed TOML / missing secret / unknown fetcher each name plugin + field/key | unit | `cargo test --bin fili plugins::config::tests` | ❌ W0 | ⬜ pending |
| CONF-03 | multiple problems across plugins all reported (D-14) | unit | `cargo test --bin fili plugins::config::tests::collects_all_problems` | ❌ W0 | ⬜ pending |
| CONF-02 (security) | rendered diagnostics never contain secret values | unit | `cargo test --bin fili plugins::` (redaction test) | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] Install `cargo-component@0.21.1` and `wasm-tools@1.263.0` (`cargo +stable install <tool>@<ver> --locked`)
- [ ] `wit/{types,host,worlds}.wit`
- [ ] `plugins/stub-source`, `plugins/stub-fetcher` (Cargo.toml with own `[workspace]`, `rust-toolchain.toml`, `.cargo/config.toml` targeting `wasm32-unknown-unknown`)
- [ ] Root `Cargo.toml` `[workspace] exclude = ["plugins"]`; `.gitignore` adds `plugins/*/target/` (and `bindings.rs` decision)
- [ ] `src/plugins/` module with `#[cfg(test)]` tests using `tempfile::TempDir` fixtures (never committed `.env` files)
- [ ] `examples/plugins/` sample `filimon.toml`, plugin TOMLs, `.env.example`; a test loads this exact sample (SC3)

---

## Manual-Only Verifications

*All phase behaviors have automated verification.*

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 120s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
