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

Filled in by the planner against the final task IDs (all commands run from the repo root; test filters use `-- --exact` where a single test is named):

| Task | Requirement | Behavior | Test Type | Automated Command | File Exists | Status |
|------|-------------|----------|-----------|-------------------|-------------|--------|
| 01-01 T1 | WIT-01 | `wit/` parses; `source` world exports `init`,`poll`, imports `log`,`fetch`; stub source builds from `../../wit` and conforms to `source`; no `wasi:` imports | integration | `wasm-tools component wit wit/` + greps, `(cd plugins/stub-source && cargo component build --release) && wasm-tools component targets wit -w source plugins/stub-source/target/wasm32-unknown-unknown/release/stub_source.wasm` | ❌ W0 | ⬜ pending |
| 01-01 T2 | WIT-02 | `fetcher` world exports `fetch`, imports outbound-http; stub fetcher conforms; stub source rejected against `fetcher`; one package line across `wit/*.wit` | integration | `(cd plugins/stub-fetcher && cargo component build --release) && wasm-tools component targets wit -w fetcher plugins/stub-fetcher/target/wasm32-unknown-unknown/release/stub_fetcher.wasm && ! wasm-tools component targets wit -w fetcher plugins/stub-source/target/wasm32-unknown-unknown/release/stub_source.wasm` | ❌ W0 | ⬜ pending |
| 01-02 T1 | CONF-01 | registry + plugin TOML + `.env` load into typed config; paths relative to registry dir; disabled sources not read | unit | `cargo test --bin fili plugins::config::tests::` | ❌ W0 | ⬜ pending |
| 01-02 T2 | CONF-02 | plugin receives only its aliased secrets; process env overrides `.env`; empty/case-mismatched secrets are errors; `Secret(***)` redaction; no process-env mutation | unit | `cargo test --bin fili plugins::` + `test -d src/plugins && ! grep -rnE 'set_var\|remove_var' src/plugins/` | ❌ W0 | ⬜ pending |
| 01-03 T1 | CONF-03 | unknown fetcher → diagnostic naming `sources.<name>.fetcher` with registry span | unit | `cargo test --bin fili plugins::config::tests::unknown_fetcher -- --exact` | ❌ W0 | ⬜ pending |
| 01-03 T2 | CONF-03 | missing/malformed registry, missing plugin file, disabled fetcher, name rules, duplicate names | unit | `cargo test --bin fili plugins::config::tests::` | ❌ W0 | ⬜ pending |
| 01-03 T3 | CONF-03 | malformed plugin TOML / unknown key / zero interval / missing secret / bad allowlist / bad backoff; collect-all with deterministic order (D-14); rendered report omits secret values | unit | `cargo test --bin fili plugins::` | ❌ W0 | ⬜ pending |
| 01-04 T1 | CONF-01, CONF-02 | committed `examples/plugins/` sample loads; per-plugin secret isolation (SC3); `.env.example` matches declared env vars | unit | `cargo test --bin fili plugins::tests::` | ❌ W0 | ⬜ pending |
| 01-04 T2 | CONF-01 (D-13) | `--registry` / `FILI_REGISTRY`, default `filimon.toml`, global flag, feeds the loader | unit | `cargo test --bin fili cli::tests::` | ❌ W0 | ⬜ pending |

Clippy is scoped to new code (the in-progress root crate has pre-existing warnings): `cargo clippy --bin fili --message-format=short 2> /tmp/fili-clippy.txt && ! grep -E '^src/plugins/' /tmp/fili-clippy.txt`. The full-crate `-D warnings` form in "Full suite command" above would fail on pre-existing warnings outside this phase.

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
