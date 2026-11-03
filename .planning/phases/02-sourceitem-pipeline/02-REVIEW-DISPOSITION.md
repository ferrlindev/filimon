---
phase: 02
review: 02-REVIEW.md
titles: json
findings:
  - id: CR-01
    severity: critical
    disposition: open
    title: "Diagnostics bypass terminal sanitization, and `redact_quoted` leaks record content"
  - id: WR-01
    severity: warning
    disposition: open
    title: "One poison record permanently halts the news branch"
  - id: WR-02
    severity: warning
    disposition: open
    title: "Consume reports success and overwrites prior output when streams fail"
  - id: WR-03
    severity: warning
    disposition: open
    title: "`--enrich-ner` in streaming mode loads the model and silently ignores it"
  - id: WR-04
    severity: warning
    disposition: open
    title: "Social `published` is not normalized to UTC, contradicting the output contract"
  - id: IN-01
    severity: info
    disposition: open
    title: "The phase leaves the old consumer API unused"
  - id: IN-02
    severity: info
    disposition: open
    title: "Stale commented-out code in main.rs references the removed `TOPIC_ARTICLES`"
  - id: IN-03
    severity: info
    disposition: open
    title: "`--threshold` is parsed but never applied"
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
open: 13
total: 13
recorded: 2026-11-03T20:52:59.537Z
---

# Phase 02: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| CR-01 | critical | open | - |
| WR-01 | warning | open | - |
| WR-02 | warning | open | - |
| WR-03 | warning | open | - |
| WR-04 | warning | open | - |
| IN-01 | info | open | - |
| IN-02 | info | open | - |
| IN-03 | info | open | - |
| IN-04 | info | open | - |
| IN-05 | info | open | - |
| IN-06 | info | open | - |
| IN-07 | info | open | - |
| IN-08 | info | open | - |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
