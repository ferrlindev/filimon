---
phase: 02
review: 02-REVIEW.md
titles: json
findings:
  - id: WR-01
    severity: warning
    disposition: open
    title: "One poison record permanently halts the news branch (carried forward, open)"
  - id: WR-02
    severity: warning
    disposition: open
    title: "Consume exits 0 and overwrites prior output when streams fail (carried forward, open)"
  - id: WR-03
    severity: warning
    disposition: open
    title: "`--enrich-ner` silently ignored in streaming mode (carried forward, open)"
  - id: WR-04
    severity: warning
    disposition: open
    title: "Social `published` validated but not normalized to UTC (carried forward, open)"
  - id: WR-05
    severity: warning
    disposition: open
    title: "Content-freedom of stream diagnostics depends on an unenforced cross-crate message convention"
  - id: WR-06
    severity: warning
    disposition: open
    title: "Undecodable records do not count toward `--limit`, so a batch run is unbounded"
  - id: IN-01
    severity: info
    disposition: open
    title: "Local decode diagnostics drop safe, struct-authored detail; `hint_summary` over-truncates fluvio-authored errors"
  - id: IN-02
    severity: info
    disposition: open
    title: "Unicode format characters (bidi overrides) pass the diagnostic choke point"
  - id: IN-03
    severity: info
    disposition: open
    title: "Offset assertions use single-digit `contains`, which can pass when the offset is missing"
  - id: CR-01
    severity: critical
    disposition: fixed
    title: "Diagnostics bypass terminal sanitization, and `redact_quoted` leaks record content"
  - id: IN-04
    severity: info
    disposition: open
    title: "docker-compose: dead service, unused env var, and the documented consume command fails preflight"
  - id: IN-05
    severity: info
    disposition: open
    title: "`sanitize_for_terminal` keeps Unicode format characters"
  - id: IN-06
    severity: info
    disposition: open
    title: "`parse_news_published` misses common offset-less forms"
  - id: IN-07
    severity: info
    disposition: open
    title: "`SourceItem::social` reinterprets JSON-scalar `raw` strings"
  - id: IN-08
    severity: info
    disposition: open
    title: "Already-defended-elsewhere — empty-parent guard before `create_dir_all`"
open: 15
total: 15
recorded: 2026-11-03T22:02:57.092Z
---

# Phase 02: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| WR-01 | warning | open | - |
| WR-02 | warning | open | - |
| WR-03 | warning | open | - |
| WR-04 | warning | open | - |
| WR-05 | warning | open | - |
| WR-06 | warning | open | - |
| IN-01 | info | open | - |
| IN-02 | info | open | - |
| IN-03 | info | open | - |
| CR-01 | critical | fixed | gap plan 02-04 (commits 1e895e0, e406c7a); incremental re-review 16e529d confirms all three parts resolved |
| IN-04 | info | open | - (not in the current review) |
| IN-05 | info | open | - (not in the current review) |
| IN-06 | info | open | - (not in the current review) |
| IN-07 | info | open | - (not in the current review) |
| IN-08 | info | open | - (not in the current review) |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
