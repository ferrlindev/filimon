---
phase: "6"
slug: "host-managed-oauth"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: draft
nyquist_compliant: false
wave_0_complete: false
created: "2026-11-11"
---

# Phase 6 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Seeded from 06-RESEARCH.md `## Validation Architecture`; task IDs are assigned once the plans exist.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in harness (`cargo test`); `tokio` `test-util` for paused time; in-repo `TestHttpServer`, `FakeFactory`/`FakeInstance` |
| **Config file** | none (Cargo.toml `[dev-dependencies] tokio` with `test-util`) |
| **Quick run command** | `cargo test -p filimon plugins::auth` |
| **Full suite command** | `cargo test -p filimon` |
| **Wasm group** | `cargo test -p filimon -- --ignored wasm_` (needs the rebuilt `stub-source` fixture; see plugins/README.md) |
| **Lint gate** | `cargo clippy --all-targets` (pedantic is `warn`; keep it clean) |
| **Estimated runtime** | ~10 seconds quick/full; ~60 seconds for the wasm group |

---

## Sampling Rate

- **After every task commit:** `cargo test -p filimon plugins::auth` plus the touched module's tests
- **After every plan wave:** `cargo test -p filimon` and `cargo clippy --all-targets`
- **Before `/gsd-verify-work`:** full suite green, then `cargo test -p filimon -- --ignored wasm_` with the rebuilt `stub-source` fixture
- **Max feedback latency:** ~60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 6-XX-XX | TBD | TBD | AUTH-01 | — | RFC 7636 vector; authorize URL has S256, state, redirect_uri, scopes, `authorize_params` (D-14) | unit | `cargo test -p filimon plugins::auth::pkce` | ❌ W0 | ⬜ pending |
| 6-XX-XX | TBD | TBD | AUTH-01 | — | Callback: success, `error=`, state mismatch, wrong path, oversize/garbage request, timeout; codes never logged | unit (loopback) | `cargo test -p filimon plugins::auth::callback` | ❌ W0 | ⬜ pending |
| 6-XX-XX | TBD | TBD | AUTH-01 | — | Code exchange sends `code_verifier`; Basic vs body client auth; token file written 0600 | unit | `cargo test -p filimon plugins::auth::client` | ❌ W0 | ⬜ pending |
| 6-XX-XX | TBD | TBD | AUTH-01 | — | `[host.oauth]` https-only, secret-only plugin reads `.env`, unknown key and `authorize_params` collisions rejected naming plugin + field | unit | `cargo test -p filimon plugins::config` | ✅ extends | ⬜ pending |
| 6-XX-XX | TBD | TBD | AUTH-02 | — | Refresh at <60 s left, not at ≥60 s; rotated refresh token kept; transient failure keeps polling; `invalid_grant` → `needs_login` kept on disk | unit (injected clock) | `cargo test -p filimon plugins::auth::provider` | ❌ W0 | ⬜ pending |
| 6-XX-XX | TBD | TBD | AUTH-02 | — | Token file re-read each poll; concurrent refresh vs login does not clobber | unit | `cargo test -p filimon plugins::auth::store` | ❌ W0 | ⬜ pending |
| 6-XX-XX | TBD | TBD | AUTH-02 | — | Corrupt or looser-than-0600 file disables only that plugin; refresh failure maps to transient backoff | unit (paused time) | `cargo test -p filimon plugins::host::scheduler` | ✅ extends | ⬜ pending |
| 6-XX-XX | TBD | TBD | AUTH-02 | — | `run-once` uses the token provider (D-15) | unit | `cargo test -p filimon commands::plugins` | ✅ extends | ⬜ pending |
| 6-XX-XX | TBD | TBD | AUTH-03 | — | Recording instance sees access token; refresh-token and client-secret sentinels absent | unit | `cargo test -p filimon plugins::host::scheduler::sched_auth` | ❌ W0 | ⬜ pending |
| 6-XX-XX | TBD | TBD | AUTH-03 | — | Real wasm `stub-source` `record_inputs` shows no refresh token or secret | integration (ignored) | `cargo test -p filimon -- --ignored wasm_auth` | ❌ W0 | ⬜ pending |
| 6-XX-XX | TBD | TBD | AUTH-04 | — | `status` per state without token values, works while the state lock is held | unit + binary | `cargo test -p filimon commands::auth` / `cargo test --test auth_cli` | ❌ W0 | ⬜ pending |
| 6-XX-XX | TBD | TBD | AUTH-04 | — | `logout` deletes file; revoke posts `token`; revoke failure warns, exits 0; later poll uses `none` | unit (fake endpoint) | `cargo test -p filimon commands::auth` | ❌ W0 | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `src/plugins/auth/` (store, pkce, callback, client, provider) with `#[cfg(test)]` modules
- [ ] `FakeInstance` in `host/testing.rs` records `(settings, secrets, cursor, access_token)` and takes the new `poll` argument; update `ProbeInstance` in `commands/plugins.rs`
- [ ] Fake OAuth server helper on top of `TestHttpServer` (scripted replies per path, request capture)
- [ ] `plugins/stub-source`: optional `record_inputs` setting; rebuild with `cargo component build --release`
- [ ] `tests/auth_cli.rs` (arg parsing, no-`[host.oauth]` error, status on prepared files, logout without revocation)

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Real `fili auth login` against a live provider | AUTH-01 | Needs a registered provider app and a browser | Register an app, set `[host.oauth]` and `.env`, run `fili auth login <plugin>`, then `fili auth status` |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
