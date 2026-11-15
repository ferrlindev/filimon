---
phase: "04"
slug: "plugin-cli"
status: verified
# threats_open = count of OPEN threats at or above workflow.security_block_on severity (the blocking gate)
threats_open: 0
asvs_level: 1
created: "2026-11-10"
---

# Phase 04 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| status/registry files to terminal and `list` | File-derived strings printed in tables and JSON | status text, plugin names |
| guest to host (status, items, errors) | Guest-controlled text stored and printed | item fields, error strings |
| operator signals to host | SIGINT/SIGTERM must lead to a bounded stop | process signals |
| `fili run` / `run-once --produce` to production topic | Writes to `filimon-social-items` | items |
| CLI args (`<name>`, `--state-dir`) to registry/filesystem | Attacker- or typo-controlled text | names, paths |
| secrets to guest | Leave host only through `init` | secret values |
| tests to cluster | Must never touch production topic | topic writes |

---

## Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation | Status |
|-----------|----------|-----------|----------|-------------|------------|--------|
| T-4-01 | Tampering | terminal escapes via `list` | high | mitigate | `sanitize_for_terminal`/`clean_guest_text` (cursor.rs:118-156, plugins.rs:118-239) | closed |
| T-4-02 | Tampering | status write alters cursor | high | mitigate | separate `<alias>.status.json`, `write_status` (cursor.rs:312-328) | closed |
| T-4-03 | DoS | oversize/garbage status file | medium | mitigate | `MAX_STATUS_BYTES` cap (cursor.rs:131-141) | closed |
| T-4-04 | Tampering | blank `--state-dir` | medium | mitigate | `non_blank_path`, `require_state_dir` | closed |
| T-4-05 | Tampering/DoS | `list` mutates/blocks on lock | medium | mitigate | lock-free read-only `read_status` | closed |
| T-4-06 | DoS | shutdown behind stuck broker | high | mitigate | 10 s grace + abort (host/mod.rs:238-291) | closed |
| T-4-07 | Tampering | replay after abort | low | accept | at-least-once; see Accepted Risks | closed |
| T-4-08 | Repudiation | abort reported as clean | medium | mitigate | `aborted at shutdown` reported (host/mod.rs:294-322) | closed |
| T-4-09 | Tampering | stdout corrupted | medium | mitigate | banner skipped, logs to stderr | closed |
| T-4-10 | Tampering | name used as path | high | mitigate | `is_valid_name` filter (config.rs:430-476) | closed |
| T-4-11 | Tampering | escapes in status file | high | mitigate | same sanitizing as T-4-01 | closed |
| T-4-12 | Info disclosure | secrets in `check` output | high | mitigate | alias names only; redacted Debug | closed |
| T-4-13 | EoP | guest runs during `check` | high | mitigate | compile/instantiate_pre only (loader.rs:155-196) | closed |
| T-4-14 | Tampering | injection via `<name>` | high | mitigate | exact match, sanitized echo (plugins.rs:366-396) | closed |
| T-4-15 | Tampering | `check` mutates state | medium | mitigate | no state-dir dependency | closed |
| T-4-16 | DoS | hostile component in `check` | medium | accept | see Accepted Risks | closed |
| T-4-17 | Tampering | dry run mutates / cursor before send | high | mitigate | `commit_once` order (once.rs:108-122) | closed |
| T-4-18 | Tampering | item text escapes | high | mitigate | `clean_guest_text` + serde | closed |
| T-4-19 | Info disclosure | secrets in output/status/errors | high | mitigate | single `expose()` feeding `init` (see WR-03 residual) | closed |
| T-4-20 | Tampering | two producers one cursor | medium | mitigate | lock before poll | closed |
| T-4-21 | Tampering | oversize cursor persisted | medium | mitigate | `oversize_cursor_outcome` | closed |
| T-4-22 | DoS | spinning guest in `run-once` | medium | mitigate | poll raced vs Ctrl-C | closed |
| T-4-23 | Tampering | test writes to production topic | high | mitigate | in-memory sinks only | closed |
| T-4-24 | Tampering | test/CLI writes to production topic | high | mitigate | `run_with_sink` generic; scratch topics | closed |
| T-4-25 | DoS | Ctrl-C/SIGTERM hang | high | mitigate | `forward_signals` cancels tokens — but CR-01: `println!` at run.rs:301/308 precedes `shutdown.cancel()`/`abort.cancel()` and panics on broken stdout, leaving later signals swallowed | closed (accepted risk AR-04-04) |
| T-4-26 | Repudiation | silent load failure | medium | mitigate | failures printed, errors on none loaded | closed |
| T-4-27 | Tampering | two `fili` one cursor dir | medium | mitigate | exclusive lock `StateDirLocked` | closed |
| T-4-28 | Tampering | empty `--state-dir` in `run` | medium | mitigate | clap + `CursorStore::open` | closed |
| T-4-29 | Info disclosure | secrets in startup/summary text | medium | mitigate | names/counts/cleaned reasons only | closed |
| T-4-SC | Tampering | supply chain (x6, one per plan) | high | accept | Cargo.toml/Cargo.lock unchanged; see Accepted Risks | closed |

*Status: open · closed · open — below high threshold (non-blocking)*
*Severity: critical > high > medium > low — only open threats at or above workflow.security_block_on count toward threats_open*
*Disposition: mitigate (implementation required) · accept (documented risk) · transfer (third-party)*

### Notes on T-4-25 (open)

- Auditor evidence: `src/commands/run.rs:295-310` still prints before cancelling; unchanged since review (04-REVIEW-DISPOSITION.md lists CR-01 `open`). Same hazard at run.rs:258 (before handlers installed).
- UAT test 2 (piped `fili run`) was reported `pass` by the user, but it is a single manual run of a race and no code changed; no test covers a failing stdout.
- Fix (~5 lines): cancel tokens first; use `writeln!(std::io::stdout(), ...).ok()` instead of `println!`; add a failing-stdout test.

### Residual warnings (not registered)

- WR-03: guest error text is cleaned/clipped but not redacted for secret values (touches T-4-19, T-4-29).
- IN-03 unbounded `read_cursor_file`; IN-04 `write_diagnostics` unsanitized; WR-02 `ok:true` recorded on cursor write failure.

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-04-01 | T-4-07 | Cursor written only after sink returns (D-07); an abort re-sends at most the last batch (at-least-once, HOST-07 dedupe deferred) | plan 04-02 (plan-time disposition) | 2026-11-10 |
| AR-04-02 | T-4-16 | `check` uses the same wasmtime compile path as `fili run`, bounded by Phase 3 caps; malformed component is a normal `ComponentLoad` error | plan 04-04 (plan-time disposition) | 2026-11-10 |
| AR-04-03 | T-4-SC (x6) | No package installed or added; `Cargo.toml`/`Cargo.lock` unchanged over the phase range | plans 04-01..04-06 (plan-time disposition) | 2026-11-10 |

| AR-04-04 | T-4-25 (CR-01) | User accepted the risk after the audit showed `println!` precedes `shutdown.cancel()` in `forward_signals` (run.rs:301/308); a broken stdout pipe can swallow later SIGINT/SIGTERM. Basis: UAT test 2 (piped `fili run`) passed manually; code NOT fixed, no failing-stdout test. Recommended follow-up: cancel tokens first, `writeln!(..).ok()`, add test. | user (chat, 2026-11-10) | 2026-11-10 |

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-11-10 | 35 | 35 | 0 (T-4-25 accepted) | gsd-security-auditor (L1, source-level; `cargo test` not run) |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-11-10 (T-4-25 accepted by user)
