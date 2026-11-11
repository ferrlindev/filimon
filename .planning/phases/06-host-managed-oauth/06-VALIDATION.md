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

Task IDs follow `6-<plan>-<task>`. Every command is the one the task's `<automated>` block runs; tests that live in a nested module (for example `plugins::host::scheduler::sched_auth::...`) are asserted by their full `... ok` line, because a bare substring filter can match nothing and still exit 0.

| Task ID | Plan | Wave | Requirement | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|-----------------|-----------|-------------------|-------------|--------|
| 6-01-01 | 06-01 | 1 | AUTH-02, AUTH-03 | Tracer: a stored token reaches `poll` from registry TOML through the provider and scheduler; WIT unchanged (D-10) | unit (fake instance) | `cargo test -p filimon plugins::` | task creates | pending |
| 6-01-02 | 06-01 | 1 | AUTH-02 | Margin boundary, re-read each poll (D-08), one login hint, `needs_login` polls with none, corrupt file disables only that plugin (D-02) | unit (paused time, injected clock) | `cargo test -p filimon plugins::` | task creates | pending |
| 6-02-01 | 06-02 | 2 | AUTH-02 | 0600 file in 0700 dir; loose perms, symlink, oversize, version, foreign plugin refused and never repaired (D-01, D-02) | unit | `cargo test -p filimon plugins::` | task creates | pending |
| 6-02-02 | 06-02 | 2 | AUTH-02 | Per-alias lock never touches the state lock; `mark_needs_login` and `delete` under the lock | unit | `cargo test -p filimon plugins::auth::store` | task creates | pending |
| 6-03-01 | 06-03 | 2 | AUTH-01 | `[host.oauth]` keys, https-only endpoints, no user-info, port, scopes, `authorize_params` collisions (D-04, D-11, D-13, D-14) | unit | `cargo test -p filimon plugins::config` | extends | pending |
| 6-03-02 | 06-03 | 2 | AUTH-01, AUTH-03 | Secret-only plugin reads `.env`; client secret never in `ResolvedSecrets` or errors (D-12) | unit | `cargo test -p filimon plugins::` | extends | pending |
| 6-04-01 | 06-04 | 2 | AUTH-01 | RFC 7636 vector; authorize URL has S256, state, redirect_uri, scopes, `authorize_params` (D-14) | unit | `cargo test -p filimon plugins::auth::pkce` | task creates | pending |
| 6-04-02 | 06-04 | 2 | AUTH-01 | Callback: success, `error=`, state mismatch, 404, 400, silent client, busy port, timeout, closed after wait; codes never logged (D-04, D-06) | unit (real loopback) | `cargo test -p filimon plugins::auth::callback` | task creates | pending |
| 6-05-01 | 06-05 | 3 | AUTH-01, AUTH-04 | Code exchange sends `code_verifier`; Basic vs body client auth; no redirects; revoke posts `token` (D-03, D-13) | unit (fake endpoint) | `cargo test -p filimon plugins::auth::client` | task creates | pending |
| 6-05-02 | 06-05 | 3 | AUTH-02 | Refresh grant and the permanent / transient / rejected classification (D-09) | unit (fake endpoint) | `cargo test -p filimon plugins::auth::client` | task creates | pending |
| 6-06-01 | 06-06 | 4 | AUTH-02 | Refresh at under 60 s, not at 60 s; rotated token kept; `invalid_grant` marks `needs_login`; lock and re-read; no lost login (D-07, D-08, D-09) | unit (injected clock) | `cargo test -p filimon plugins::auth::provider` | extends | pending |
| 6-06-02 | 06-06 | 4 | AUTH-02 | Scheduler refreshes before poll; transient failure backs off; `run-once` refreshes and persists (D-09, D-15) | unit (real time, fake endpoint) | `cargo test -p filimon` | extends | pending |
| 6-07-01 | 06-07 | 4 | AUTH-01 | Login end to end against a fake endpoint; nothing written on any failure (D-05, D-06) | unit | `cargo test -p filimon plugins::auth::login` | task creates | pending |
| 6-07-02 | 06-07 | 4 | AUTH-01 | `fili auth login` wiring, refusals, no banner, no secret printed | unit | `cargo test -p filimon` | task creates | pending |
| 6-08-01 | 06-08 | 5 | AUTH-04 | `status` per state without token values; lock-free; creates nothing; shows `needs-login` (D-09) | unit | `cargo test -p filimon` | extends | pending |
| 6-08-02 | 06-08 | 5 | AUTH-04 | `logout` deletes first, revokes best effort, warns and exits 0 on failure; later poll uses `none` (D-03) | unit (fake endpoint) | `cargo test -p filimon` | extends | pending |
| 6-09-01 | 06-09 | 6 | AUTH-03 | Recording instance sees access token, never refresh token or client secret (SC3, fake) | unit | `cargo test -p filimon plugins::host::scheduler` | extends | pending |
| 6-09-02 | 06-09 | 6 | AUTH-03 | Real `stub-source` `record_inputs` shows no refresh token or secret (SC3, real wasm) | integration (ignored) | `cargo test -p filimon -- --ignored wasm_auth` | task creates | pending |
| 6-09-03 | 06-09 | 6 | AUTH-04 | `fili auth` through the real binary; README documents the feature | binary | `cargo test -p filimon --test auth_cli` | task creates | pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

Each item is created inside the task that first needs it (tests are written with their code, so every `<automated>` command exists):
- [ ] `src/plugins/auth/` (store, pkce, callback, client, provider, login) with `#[cfg(test)]` modules: plans 06-01, 06-02, 06-04, 06-05, 06-06, 06-07
- [ ] `FakeInstance` in `host/testing.rs` records `(settings, secrets, cursor, access_token)` and takes the new `poll` argument; update `ProbeInstance` in `commands/plugins.rs`: plan 06-01
- [ ] Fake OAuth endpoint on top of `TestHttpServer` (`Reply::json`, `auth/testing.rs` helpers): plan 06-05
- [ ] `plugins/stub-source`: optional `record_inputs` setting; rebuild with `cargo component build --release`: plan 06-09
- [ ] `tests/auth_cli.rs` (arg parsing, no-`[host.oauth]` error, status on prepared files, logout without revocation): plan 06-09

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
