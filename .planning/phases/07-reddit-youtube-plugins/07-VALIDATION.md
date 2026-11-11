---
phase: "7"
slug: "reddit-youtube-plugins"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
status: draft
nyquist_compliant: false
wave_0_complete: false
created: "2026-11-11"
---

# Phase 7 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test harness (`cargo test`); plugin crates run natively on the host triple; host crate `cargo test -p filimon` |
| **Config file** | per-crate `Cargo.toml` + `.cargo/config.toml` (forces wasm target, so every plugin test needs the explicit `--target <host triple>`) |
| **Quick run command** | `(cd plugins/reddit && cargo test --target "$(rustc -vV \| sed -n 's/^host: //p')")` and the same in `plugins/youtube` |
| **Full suite command** | both plugin native suites, then `cargo test -p filimon plugins::` and `cargo test -p filimon --test plugins_cli --test auth_cli`, then `cargo component build --release` + `wasm-tools component targets` per plugin |
| **Estimated runtime** | ~60–180 seconds |

---

## Sampling Rate

- **After every task commit:** Run the touched crate's native `cargo test --target <host>`, or `cargo test -p filimon plugins::auth` for host work
- **After every plan wave:** Run the full suite command
- **Before `/gsd-verify-work`:** Full suite must be green
- **Max feedback latency:** 180 seconds

---

## Per-Task Verification Map

Seeded from 07-RESEARCH.md "Validation Architecture". Task IDs are filled in once plans exist.

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| TBD | TBD | TBD | RDDT-01 | — | Listing parse, cursor high-water mark, no re-emit of deleted marked post | unit (mock `Http`) | `cd plugins/reddit && cargo test --target <host> listing cursor` | ❌ W0 | ⬜ pending |
| TBD | TBD | TBD | RDDT-02 | — | `none` token → `auth-required`; saved/home feed settings validated | unit | `cargo test --target <host> user_feed` | ❌ W0 | ⬜ pending |
| TBD | TBD | TBD | RDDT-03 | — | 429 / `x-ratelimit-remaining` < 1 → `rate-limited`; UA format enforced; missing contact is a `config` error | unit | `cargo test --target <host> rate_limit user_agent` | ❌ W0 | ⬜ pending |
| TBD | TBD | TBD | YTUB-01 | — | `UC`→`UU` derivation; `@handle` rejected naming `channels` | unit | `cd plugins/youtube && cargo test --target <host> playlist` | ❌ W0 | ⬜ pending |
| TBD | TBD | TBD | YTUB-02 | — | Subscriptions pagination, limit clamp, `none` token → `auth-required` | unit | `cargo test --target <host> subscriptions` | ❌ W0 | ⬜ pending |
| TBD | TBD | TBD | YTUB-03 | — | Only `playlistItems`/`subscriptions` URLs requested; `quotaExceeded` → named `transient` | unit | `cargo test --target <host> quota` | ❌ W0 | ⬜ pending |
| TBD | TBD | TBD | RDDT-01/02, YTUB-01/02 | — | Cursor ≤ 4096 bytes at the configured cap; oversize config rejected at `init` | unit | `cargo test --target <host> cursor_size` | ❌ W0 | ⬜ pending |
| TBD | TBD | TBD | TEST-01 | — | Every D-18 fixture exists and is exercised with no network or env vars | unit/integration | `cargo test --target <host>` in each crate | ❌ W0 | ⬜ pending |
| TBD | TBD | TBD | D-03 (host) | — | `client_credentials` grant: validation, Basic auth token request, in-memory cache, `login` refuses, `status` shows app-only | unit | `cargo test -p filimon plugins::config plugins::auth` and `--test auth_cli` | ✅ extend | ⬜ pending |
| TBD | TBD | TBD | Samples | — | New sample TOMLs load; `.env.example` covers every declared secret | unit/CLI | `cargo test -p filimon plugins::tests` and `--test plugins_cli` | ✅ edit | ⬜ pending |
| TBD | TBD | TBD | Conformance | — | Built `.wasm` matches world `source`, imports only `filimon:plugin/*` | build | `wasm-tools component targets ../../wit -w source target/wasm32-unknown-unknown/release/<crate>.wasm` | n/a | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `plugins/reddit/` and `plugins/youtube/` crate skeletons (Cargo.toml, toolchain pin, `.cargo/config.toml`, `.gitignore`, cfg-gated `lib.rs`) copied from `plugins/stub-source`
- [ ] `tests/fixtures/` sets with a `README.md` recording provenance and capture recipes
- [ ] Recording mock `Http` helper and fixture loader in each crate's tests
- [ ] Host test scaffolding for `client_credentials` extends existing helpers; no new framework

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Real API fixture capture | TEST-01 (D-18) | Needs Reddit and Google credentials that are not available to automation | Run the curl recipes in 07-RESEARCH.md; sanitise tokens and PII; commit under `tests/fixtures/` |
| `reddit` app-only read of `/r/{sub}/new` | RDDT-01 | Unconfirmed in docs (A3); needs one live call | First capture confirms the token can read the listing |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 180s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
