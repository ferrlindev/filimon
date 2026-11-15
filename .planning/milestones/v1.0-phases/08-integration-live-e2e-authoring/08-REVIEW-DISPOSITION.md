---
phase: 08
review: 08-REVIEW.md
titles: json
findings:
  - id: CR-01
    severity: critical
    disposition: open
    title: "`live-e2e.sh run` accepts a stale `last_poll` from a previous run as \"every source polled\""
  - id: CR-02
    severity: critical
    disposition: open
    title: "Registry-less `fili produce` panics (known issue, `src/crawler.rs`)"
  - id: WR-01
    severity: warning
    disposition: open
    title: "A failed or blocked `run` destroys the committed evidence"
  - id: WR-02
    severity: warning
    disposition: open
    title: "Relative `sanitise` / `check-evidence` arguments resolve against the repo root, not the caller's directory"
  - id: WR-03
    severity: warning
    disposition: open
    title: "Preflight demands the Reddit component even when Reddit is waived"
  - id: WR-04
    severity: warning
    disposition: open
    title: "Selftest does not exercise e-mail redaction in `.text`"
  - id: WR-05
    severity: warning
    disposition: open
    title: "Sanitiser is a denylist and `check-evidence` has no PII scan"
  - id: WR-06
    severity: warning
    disposition: open
    title: "Sample provenance cannot show the records are from this run"
  - id: WR-07
    severity: warning
    disposition: open
    title: "`errexit` is disabled throughout `run`, so setup failures are silent"
  - id: WR-08
    severity: warning
    disposition: open
    title: "`verify-authoring.sh` \"unassigned\" assertion passes on any failure"
  - id: WR-09
    severity: warning
    disposition: open
    title: "\"Repository tree unchanged\" check is too weak to prove its claim"
  - id: WR-10
    severity: warning
    disposition: open
    title: "Script and docs claim \"no network\" and \"nothing is run\", neither of which holds"
  - id: WR-11
    severity: warning
    disposition: open
    title: "Template comment says the host de-duplicates by `id`; the docs say it does not"
  - id: WR-12
    severity: warning
    disposition: open
    title: "Credentialed live tests hit real APIs under a plain `cargo test`; failures lose all diagnostics"
  - id: WR-13
    severity: warning
    disposition: open
    title: "Built-in crawler disables TLS certificate verification"
  - id: IN-01
    severity: info
    disposition: open
    title: "Dead `Cargo.lock` line in `.gitignore`"
  - id: IN-02
    severity: info
    disposition: open
    title: "`is_stale` is mtime-only and ignores toolchain and config inputs"
  - id: IN-03
    severity: info
    disposition: open
    title: "Selftest step 8 greps the script's own source text"
  - id: IN-04
    severity: info
    disposition: open
    title: "Debug formatting is used to write TOML strings"
  - id: IN-05
    severity: info
    disposition: open
    title: "Transcript drain relies on `sleep 1`"
  - id: IN-06
    severity: info
    disposition: open
    title: "Secret pattern file survives interrupts"
  - id: IN-07
    severity: info
    disposition: open
    title: "A failed topic delete is dropped when the test body already failed"
  - id: IN-08
    severity: info
    disposition: open
    title: "Shell dotenv parser does not follow the repository parser as claimed"
  - id: IN-09
    severity: info
    disposition: open
    title: "Token-marker scan can reject legitimate news content"
open: 24
total: 24
recorded: 2026-11-15T06:21:05.448Z
---

# Phase 08: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| CR-01 | critical | open | - |
| CR-02 | critical | open | - |
| WR-01 | warning | open | - |
| WR-02 | warning | open | - |
| WR-03 | warning | open | - |
| WR-04 | warning | open | - |
| WR-05 | warning | open | - |
| WR-06 | warning | open | - |
| WR-07 | warning | open | - |
| WR-08 | warning | open | - |
| WR-09 | warning | open | - |
| WR-10 | warning | open | - |
| WR-11 | warning | open | - |
| WR-12 | warning | open | - |
| WR-13 | warning | open | - |
| IN-01 | info | open | - |
| IN-02 | info | open | - |
| IN-03 | info | open | - |
| IN-04 | info | open | - |
| IN-05 | info | open | - |
| IN-06 | info | open | - |
| IN-07 | info | open | - |
| IN-08 | info | open | - |
| IN-09 | info | open | - |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
