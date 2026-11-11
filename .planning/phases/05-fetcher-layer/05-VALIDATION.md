---
phase: "5"
slug: "fetcher-layer"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: validated
nyquist_compliant: true
wave_0_complete: true
created: "2026-11-11"
---

# Phase 5 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Reconstructed from the eight PLAN and SUMMARY files (State B). Every task carries an `<automated>` command.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in harness (`cargo test`); `tests/plugins_cli.rs` drives the `fili` binary; real-wasm and TLS tests are `#[ignore]` groups |
| **Config file** | none (Cargo.toml, rust-toolchain.toml) |
| **Quick run command** | `cargo test -p filimon plugins::host` (plus `commands::produce` or `plugins::config` for the module under change) |
| **Full suite command** | `cargo test -p filimon` (end of phase: 433 unit + 12 integration passed, 62 + 3 ignored) |
| **Wasm group** | `cargo test -p filimon -- --ignored wasm_` (needs stub-fetcher, stub-source and http-fetcher built with `cargo component build --release`; see plugins/README.md) |
| **TLS group** | `cargo test -p filimon -- --ignored tls_` (needs `openssl` on PATH) |
| **Network group** | `cargo test -p filimon -- --ignored net_` (needs internet; not part of any gate) |
| **Estimated runtime** | ~10 seconds quick/full; ~60 seconds for the wasm and TLS groups; a cold worktree adds several minutes of compile |

---

## Sampling Rate

- **After every task commit:** the task's own `cargo test` filter (plugins::host::fetch, commands::produce, …)
- **After every plan wave:** `cargo test` (build gate + full suite)
- **Before `/gsd-verify-work`:** full suite green, plus the `wasm_` and `tls_` groups
- **Max feedback latency:** ~60 seconds

---

## Per-Task Verification Map

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 5-01-01 | 01 | 1 | FETCH-01 | T-5-01..06 | A source's `fetch` is answered by a fresh instance of its assigned fetcher | wasm | `cargo test -p filimon -- --ignored wasm_tracer_fetch wasm_concurrent_fetches wasm_denied_host` | ✅ | ✅ green |
| 5-01-02 | 01 | 1 | FETCH-01 | T-5-01..06 | A source whose fetcher cannot load is not loaded (strict loading) | wasm | `cargo test -p filimon -- --ignored wasm_host_run_routes wasm_host_load_skips` | ✅ | ✅ green |
| 5-01-03 | 01 | 1 | FETCH-02 | T-5-01..06 | Exact-host allowlist, http(s) only, one deadline | unit | `cargo test -p filimon plugins::host::fetch` | ✅ | ✅ green |
| 5-02-01 | 02 | 1 | FETCH-03 | T-5-07 | Default fetcher builds from the WIT alone, no `wasi:` imports | build | `(cd plugins/http-fetcher && cargo component build --release)` then `wasm-tools component wit …http_fetcher.wasm` | ✅ | ✅ green |
| 5-02-02 | 02 | 1 | FETCH-03 | T-5-07 | Default fetcher is a registry entry that loads through the real binary | integration | `cargo test -p filimon plugins::tests` and `fili --registry examples/plugins/filimon.toml plugins check http` | ✅ | ✅ green |
| 5-03-01 | 03 | 2 | FETCH-02, FETCH-03 | T-5-08..13 | Host-owned egress: one shared client, certificate validation on | wasm | `cargo test -p filimon plugins::host::fetch::tests::egress` and `-- --ignored wasm_tracer_default_fetcher` | ✅ | ✅ green |
| 5-03-02 | 03 | 2 | FETCH-02 | T-5-08..13 | Redirects checked on every hop, credentials stripped across origins, one deadline | unit | `cargo test -p filimon plugins::host::fetch::tests::egress` | ✅ | ✅ green |
| 5-04-01 | 04 | 3 | FETCH-02 | T-5-14, T-5-15 | Hostile requests refused before any connection (host/framing headers, size limits) | unit | `cargo test -p filimon plugins::host::fetch` | ✅ | ✅ green |
| 5-04-02 | 04 | 3 | FETCH-02 | T-5-14, T-5-15 | Header and body fidelity; host logs carry no secrets | unit | `cargo test -p filimon plugins::host::fetch` | ✅ | ✅ green |
| 5-05-01 | 05 | 3 | FETCH-03 | T-5-27, T-5-16, T-5-17 | Self-signed, wrong-name and untrusted certificates rejected, also through the real default fetcher | tls | `cargo test -p filimon -- --ignored tls_` | ✅ | ✅ green |
| 5-05-02 | 05 | 3 | FETCH-03 | T-5-27 | Always-on guard: no invalid-certificate switch on the fetch path | unit | `cargo test -p filimon plugins::host::https_tests::the_fetch_path_has_no_invalid_certificate_switch` | ✅ | ✅ green |
| 5-06-01 | 06 | 3 | FETCH-01 | T-5-18..20 | Changing only the assigned fetcher changes who serves the request | wasm | `cargo test -p filimon -- --ignored wasm_changing_the_assigned_fetcher` | ✅ | ✅ green |
| 5-06-02 | 06 | 3 | FETCH-02 | T-5-18..20 | A custom fetcher cannot escape the allowlist or the timeout | wasm | `cargo test -p filimon -- --ignored wasm_default_fetcher_is wasm_custom_fetcher_cannot wasm_slow_server_times_out wasm_spinning_custom wasm_a_slow_call` | ✅ | ✅ green |
| 5-06-03 | 06 | 3 | FETCH-01 | T-5-18..20 | `plugins run-once` routes fetch like `fili run`; unloadable fetcher reported by name | unit + wasm + integration | `cargo test -p filimon commands::plugins`, `-- --ignored wasm_run_once`, `--test plugins_cli` | ✅ | ✅ green |
| 5-07-01 | 07 | 4 | FETCH-04 | T-5-21..25 | `[crawler]` table loads; a page is fetched through the named fetcher | unit + wasm | `cargo test -p filimon plugins::config`, `plugins::host::fetch`, `-- --ignored wasm_tracer_crawler` | ✅ | ✅ green |
| 5-07-02 | 07 | 4 | FETCH-04 | T-5-21..25 | `fili produce` picks its fetcher from the registry; a bad `[crawler]` is an error, never a fallback | unit + wasm | `cargo test -p filimon commands::produce`, `-- --ignored wasm_crawl_visits` | ✅ | ✅ green |
| 5-07-03 | 07 | 4 | FETCH-04, FETCH-02 | T-5-21..25 | Crawler allowlist, redirect and timeout rules hold on real wasm; one policy for sources and crawler | wasm | `cargo test -p filimon -- --ignored wasm_crawler wasm_a_hung_server wasm_source_and_crawler` | ✅ | ✅ green |
| 5-08-01 | 08 | 5 | FETCH-01..04 | T-5-26 | READMEs describe the fetcher layer as built; stale interim text removed | docs check | heading and phrase greps in the plan's `<automated>` block | ✅ | ✅ green |
| 5-08-02 | 08 | 5 | FETCH-01..04 | T-5-26 | WIT comments and sample `[crawler]`; full-phase sweep | sweep | `cargo test -p filimon`, `-- --ignored wasm_`, `-- --ignored tls_`, `plugins check http` | ✅ | ✅ green |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

Evidence at phase close (orchestrator, merged tree): `cargo build` exit 0; `cargo test` 433 passed + 12 passed, 0 failed; `cargo test -- --ignored wasm_ tls_` 46 passed + 3 passed, 0 failed.

---

## Wave 0 Requirements

Existing infrastructure covers all phase requirements: the Rust harness, the stub fixtures and the local test server (`TestHttpServer`, `OpensslServer`) were built inside the plans' first tasks.

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| With `[crawler]` configured, `fili produce <url>` fetches pages through the fetcher and articles still reach the consumer (roadmap criterion 4) | FETCH-04 | Needs a live local Fluvio cluster | Start the cluster, set `[crawler] fetcher = "http"` in the registry, run `fili produce <url>` then `fili consume`; articles must appear |
| Interrupting `fili produce` mid-crawl stops cleanly | FETCH-04 | Needs a live cluster and a signal | Run `fili produce <url>` with `[crawler]` set, press Ctrl-C; the process exits without a hang |
| Public-internet certificate behaviour (`net_` group) | FETCH-03 | Needs internet access; kept out of every gate | `cargo test -p filimon -- --ignored net_` |
| A TLS-intercepting corporate proxy CA is not trusted | FETCH-03 | Environment-specific; documented assumption WD-11 | Run behind such a proxy and confirm the fetch is refused; the READMEs say so |

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 60s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** approved 2026-11-11
