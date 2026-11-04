---
phase: 02
review: 02-REVIEW.md
titles: json
findings:
  - id: WR-08
    severity: warning
    disposition: open
    title: "`MAX_REOPENS` counts over the whole run, and each record left in a rejected batch costs one reopen (carried forward, unchanged)"
  - id: WR-11
    severity: warning
    disposition: open
    title: "Already-defended-elsewhere: second `check_ner_mode` inside `consume::run` (carried forward, unchanged)"
  - id: IN-14
    severity: info
    disposition: open
    title: "The budget stop line and the `diagnostic_budget` doc still say \"diagnostics\", but notes are no longer counted (new, from 02-07)"
  - id: IN-15
    severity: info
    disposition: open
    title: "`merged_stop_never_splits_a_note_from_its_record` asserts counts that depend on futures-util's polling order (new, from 02-07)"
  - id: IN-12
    severity: info
    disposition: open
    title: "Fatal and budget-stop lines use up the record-diagnostic message cap (carried forward, unchanged)"
  - id: IN-13
    severity: info
    disposition: open
    title: "The reopen-limit closing line says it skipped 100 records, but 101 are reported as skipped (carried forward, unchanged)"
  - id: IN-10
    severity: info
    disposition: open
    title: "SmartModule prefix drift test is narrow and brittle (carried forward, unchanged)"
  - id: IN-11
    severity: info
    disposition: open
    title: "A failed reopen reports the rejected record as \"skipped\" right before the branch ends (carried forward, unchanged)"
  - id: WR-12
    severity: warning
    disposition: open
    title: "A budget stop can fall between a social note and its record, so a note describes a record that was never written"
  - id: WR-07
    severity: warning
    disposition: open
    title: "Diagnostic-budget stop counts as a clean run, so a run that got only errors overwrites the last good output with `[]` and exits 0"
  - id: WR-09
    severity: warning
    disposition: open
    title: "`rfc3339_to_utc` can turn valid RFC 3339 input into output that is not RFC 3339"
  - id: WR-10
    severity: warning
    disposition: open
    title: "`save_json` writes through a predictable temp path that follows symlinks"
  - id: IN-09
    severity: info
    disposition: open
    title: "Atomic `save_json` changes the output file's identity and does not make the rename durable"
  - id: WR-01
    severity: warning
    disposition: fixed
    title: "One poison record permanently halts the news branch"
  - id: WR-02
    severity: warning
    disposition: fixed
    title: "Consume exits 0 and overwrites prior output when streams fail"
  - id: WR-03
    severity: warning
    disposition: fixed
    title: "`--enrich-ner` silently ignored in streaming mode"
  - id: WR-04
    severity: warning
    disposition: fixed
    title: "Social `published` validated but not normalized to UTC"
  - id: WR-05
    severity: warning
    disposition: fixed
    title: "Content-freedom of stream diagnostics depends on an unenforced cross-crate message convention"
  - id: WR-06
    severity: warning
    disposition: fixed
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
open: 21
total: 28
recorded: 2026-11-04T13:34:46.261Z
---

# Phase 02: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| WR-08 | warning | open | - |
| WR-11 | warning | open | - |
| IN-14 | info | open | - |
| IN-15 | info | open | - |
| IN-12 | info | open | - |
| IN-13 | info | open | - |
| IN-10 | info | open | - |
| IN-11 | info | open | - |
| WR-12 | warning | open | - (not in the current review) |
| WR-07 | warning | open | - (not in the current review) |
| WR-09 | warning | open | - (not in the current review) |
| WR-10 | warning | open | - (not in the current review) |
| IN-09 | info | open | - (not in the current review) |
| WR-01 | warning | fixed | 02-REVIEW-FIX.md (not in the current review) |
| WR-02 | warning | fixed | 02-REVIEW-FIX.md (not in the current review) |
| WR-03 | warning | fixed | 02-REVIEW-FIX.md (not in the current review) |
| WR-04 | warning | fixed | 02-REVIEW-FIX.md (not in the current review) |
| WR-05 | warning | fixed | 02-REVIEW-FIX.md (not in the current review) |
| WR-06 | warning | fixed | 02-REVIEW-FIX.md (not in the current review) |
| IN-01 | info | open | - (not in the current review) |
| IN-02 | info | open | - (not in the current review) |
| IN-03 | info | open | - (not in the current review) |
| CR-01 | critical | fixed | gap plan 02-04 (commits 1e895e0, e406c7a); incremental re-review 16e529d confirms all three parts resolved (not in the current review) |
| IN-04 | info | open | - (not in the current review) |
| IN-05 | info | open | - (not in the current review) |
| IN-06 | info | open | - (not in the current review) |
| IN-07 | info | open | - (not in the current review) |
| IN-08 | info | open | - (not in the current review) |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
