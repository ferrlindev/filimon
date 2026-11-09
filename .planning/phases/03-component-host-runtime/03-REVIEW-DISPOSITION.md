---
phase: 03
review: 03-REVIEW.md
titles: json
findings:
  - id: WR-02
    severity: warning
    disposition: open
    title: "Epoch ticker thread spawn failure is silently swallowed, which removes the anti-wedge guarantee"
  - id: WR-04
    severity: warning
    disposition: open
    title: "Item URL scheme is not restricted"
  - id: WR-05
    severity: warning
    disposition: open
    title: "Guest-chosen `platform` can impersonate another plugin or platform, and the key encoding is ambiguous"
  - id: WR-07
    severity: warning
    disposition: open
    title: "An empty `--state-dir` / `FILI_STATE_DIR` silently resolves to the current directory"
  - id: WR-08
    severity: warning
    disposition: open
    title: "The per-poll item cap does not bound host memory: list lifting copies every item before `map_items` runs"
  - id: WR-09
    severity: warning
    disposition: open
    title: "The \"64 MiB per instance\" ceiling is per memory, so one instance can hold up to 512 MiB"
  - id: WR-10
    severity: warning
    disposition: open
    title: "Sink sends are never raced against shutdown and lock waiting is unbounded, so shutdown can take N x 60 s"
  - id: IN-01
    severity: info
    disposition: open
    title: "`clean_guest_text` leaves Unicode line separators and bidi controls in place"
  - id: IN-02
    severity: info
    disposition: open
    title: "Invalid `RUST_LOG` is discarded silently and per-item drop warnings are unbounded"
  - id: IN-03
    severity: info
    disposition: open
    title: "The state-file reader treats a missing `cursor` key as a first poll"
  - id: IN-04
    severity: info
    disposition: open
    title: "The core isolation tests are all `#[ignore]` and depend on a manual fixture build"
  - id: IN-05
    severity: info
    disposition: open
    title: "Items over the per-poll cap are lost permanently while an oversize cursor disables the plugin"
  - id: IN-06
    severity: info
    disposition: open
    title: "Misleading variable name in the `grow` fixture mode"
open: 13
total: 13
recorded: 2026-11-09T17:38:21.058Z
---

# Phase 03: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| WR-02 | warning | open | - |
| WR-04 | warning | open | - |
| WR-05 | warning | open | - |
| WR-07 | warning | open | - |
| WR-08 | warning | open | - |
| WR-09 | warning | open | - |
| WR-10 | warning | open | - |
| IN-01 | info | open | - |
| IN-02 | info | open | - |
| IN-03 | info | open | - |
| IN-04 | info | open | - |
| IN-05 | info | open | - |
| IN-06 | info | open | - |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
