---
phase: 02-sourceitem-pipeline
plan: 06
subsystem: pipeline
tags: [rust, chrono, rfc3339, d-09, gap-closure, wr-09]

requires:
  - phase: 02-sourceitem-pipeline
    provides: "02-05 located diagnostics (Incoming::Note(RecordDiagnostic)) in decode_social; WR-04 rfc3339_to_utc"
provides:
  - "utc_rfc3339: the single RFC 3339 formatter, None when the UTC year is outside 0000..=9999"
  - "epoch_ms_to_rfc3339, rfc3339_to_utc and parse_news_published all format through utc_rfc3339"
  - "decode_social note for an out-of-range or non-RFC 3339 published: `published is not an RFC 3339 time with a UTC year from 0000 to 9999; set to null`"
affects: [phase-02-verification, phase-03-plugin-host]

actuals:
  tokens: 3100
  tasks: 2
  commits: 4
plan_head_before: 391186e17af8e5c0d8c5d5b61a66e62fedbbe542
plan_head_after: 74ce6c3c523a100e83569e95fe4fdb091b31e0ce

tech-stack:
  added: []
  patterns:
    - "One guarded formatter for every published conversion (no direct to_rfc3339_opts outside utc_rfc3339)"

key-files:
  created: []
  modified:
    - src/models.rs
    - src/commands/consume.rs

key-decisions:
  - "Guard on the shared formatting step, not on a parse branch: every news parse branch and the epoch path can reach a signed year"
  - "Out-of-range times become null; nothing is clamped or shifted, and a social record is still written"
  - "WR-04 UTC normalization kept (gap 1 override)"
  - "REQUIREMENTS.md not marked: PIPE-01 and PIPE-03 rows read Gaps Found until phase re-verification (same handling as 02-05)"

patterns-established:
  - "utc_rfc3339(utc, precision): the only place that formats a published time"

requirements-completed: [PIPE-01, PIPE-03]

coverage:
  - id: D1
    description: "Social published at the year limits: out-of-range UTC year gives null plus one note naming topic and offset (value not echoed); in-range limits convert to milliseconds"
    requirement: PIPE-03
    verification:
      - kind: unit
        ref: "src/models.rs#models::tests::rfc3339_to_utc_normalizes_only_rfc3339"
        status: pass
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::social_published_outside_year_range_becomes_null"
        status: pass
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::invalid_social_published_becomes_null"
        status: pass
    human_judgment: false
  - id: D2
    description: "News published at the year limits on every parse branch (RFC 3339, RFC 2822, offset-less, date-only) gives null with published_raw kept; in-range limits parse at seconds precision"
    requirement: PIPE-01
    verification:
      - kind: unit
        ref: "src/models.rs#models::tests::parse_news_published_best_effort"
        status: pass
      - kind: unit
        ref: "src/models.rs#models::tests::from_article_drops_out_of_range_published"
        status: pass
    human_judgment: false
  - id: D3
    description: "Epoch path: 253_402_300_800_000 gives None, 253_402_300_799_999 gives 9999-12-31T23:59:59.999Z, also through SourceItem::social"
    requirement: PIPE-01
    verification:
      - kind: unit
        ref: "src/models.rs#models::tests::epoch_ms_to_rfc3339_converts"
        status: pass
    human_judgment: false
  - id: D4
    description: "chrono 0.4.45 baseline pinned: it writes signed years that its own RFC 3339 parser rejects; one to_rfc3339_opts call in non-test models.rs; D-01 diff gate and clippy baseline hold"
    verification:
      - kind: unit
        ref: "src/models.rs#models::tests::chrono_writes_out_of_range_years_with_sign"
        status: pass
      - kind: other
        ref: "Task 1 and Task 2 verify gates (structural greps, git diff --quiet 0ed7d3f -- smartmodules/ transforms.yaml src/scorer.rs, cargo clippy -p filimon)"
        status: pass
      - kind: unit
        ref: "cargo test -p filimon (132 passed, 0 failed, 7 ignored)"
        status: pass
    human_judgment: false

duration: 4min
completed: 2026-11-04
status: complete
---

# Phase 2 Plan 06: Published Year-Limit Guard Summary

**All three published conversions (social RFC 3339, social epoch ms, news best-effort) now format through one private `utc_rfc3339` that returns None for a UTC year outside 0000..=9999, so chrono's signed `+10000-...` / `-0001-...` output can no longer reach the JSON. This closes gap 3 / WR-09.**

## Performance

- **Duration:** 4 min
- **Started:** 2026-11-04T12:07:29Z
- **Completed:** 2026-11-04T12:11:09Z
- **Tasks:** 2
- **Files modified:** 2

## Accomplishments

- `utc_rfc3339(utc: DateTime<Utc>, precision: SecondsFormat) -> Option<String>` added. It is the only non-test `to_rfc3339_opts` call in src/models.rs. Before this plan there were 3.
- Social decode: `9999-12-31T23:00:00-05:00` and `0000-01-01T00:30:00+01:00` keep the record with `published` null. Each adds one note that names `filimon-social-items offset N`, says `set to null`, and does not include the value. `9999-12-31T23:59:59Z` still gives `9999-12-31T23:59:59.000Z`.
- News: an out-of-range time on any of the four parse branches gives null, and `article.published_raw` keeps the original string. The in-range limits `0000-01-01` and `9999-12-31` still parse.
- Epoch: the first millisecond of year 10000 gives None, and the last millisecond of 9999 still converts.
- WR-04 is kept: `2024-05-01T10:00:00+01:00` still gives `2024-05-01T09:00:00.000Z`.

## Task Commits

1. **Task 1 (tracer): social published at the year limit → guarded rfc3339_to_utc → null plus one located note**
   - RED `cd7f8e8` (test)
   - GREEN `c9a1331` (feat)
   - Tracer gate: interactive run, `end-of-phase` mode, verify is automated only. All three verify commands were re-run and exited 0, then the plan moved on to Task 2.
2. **Task 2: news and epoch conversions through the same guard**
   - RED `7afb8fd` (test)
   - GREEN `74ce6c3` (feat)

No REFACTOR commits were needed.

## TDD Gate Compliance

Each task has a `test(02-06)` commit followed by a `feat(02-06)` commit that descends from it.

RED evidence came from libtest's JUnit reporter, the same method 02-05 used. The test binary from `cargo test -p filimon --no-run` was run with `RUSTC_BOOTSTRAP=1 <bin> <targets> -Z unstable-options --format junit`. Each record was checked with `gsd-tools check tdd-red-evidence` and used the `Class#method` target form:

| Task | Target test | Verdict | Semantic assessment |
|------|-------------|---------|---------------------|
| 1 | `models::tests#rfc3339_to_utc_normalizes_only_rfc3339` | RED_EVIDENCE_OK | Failed on the planned assertion: `rfc3339_to_utc("9999-12-31T23:00:00-05:00")` was `Some("+10000-01-01T04:00:00.000Z")`, expected None. No build, load or fixture fault. |
| 1 | `commands::consume::tests#social_published_outside_year_range_becomes_null` | RED_EVIDENCE_OK | Failed on the `published` None assertion at offset 7: it was `Some("+10000-01-01T04:00:00.000Z")`. |
| 2 | `models::tests#epoch_ms_to_rfc3339_converts` | RED_EVIDENCE_OK | Failed on `epoch_ms_to_rfc3339(253_402_300_800_000)`: it was `Some("+10000-01-01T00:00:00.000Z")`, expected None. |
| 2 | `models::tests#parse_news_published_best_effort` | RED_EVIDENCE_OK | Failed on the first new case: `9999-12-31T23:00:00-05:00` gave `Some("+10000-01-01T04:00:00Z")`. The loop stops at its first failure, so the RFC 2822, offset-less and date-only cases were not reached at RED. The plan's probe at plan time shows each of them gives a signed year without the guard, and they pass after GREEN. |
| 2 | `models::tests#from_article_drops_out_of_range_published` | RED_EVIDENCE_OK | Failed on the `published` None assertion: it was `Some("+10000-01-01T04:00:00Z")`. |

`chrono_writes_out_of_range_years_with_sign` passed at RED on purpose. It records why the guard exists and was not used as a RED target.

## Files Created/Modified

- `src/models.rs`: adds the `Datelike` import and `utc_rfc3339`. `epoch_ms_to_rfc3339`, `rfc3339_to_utc` and `parse_news_published` now format through it, and their doc comments are updated. Tests: `chrono_writes_out_of_range_years_with_sign` and `from_article_drops_out_of_range_published` are new. `rfc3339_to_utc_normalizes_only_rfc3339`, `epoch_ms_to_rfc3339_converts` and `parse_news_published_best_effort` are extended, including a round-trip check with `parse_from_rfc3339` on every Some result.
- `src/commands/consume.rs`: the `decode_social` published note has new wording and its doc comment is updated. The new test is `social_published_outside_year_range_becomes_null`.

## Decisions Made

- The guard sits on the shared formatting step, as the plan specifies. One helper covers all three conversions.
- REQUIREMENTS.md is not marked complete. The PIPE-01 and PIPE-03 traceability rows read `Gaps Found` until phase re-verification. 02-05 handled them the same way, and the orchestrator and verifier own that update.

## Deviations from Plan

None. The plan was executed as written.

Note: the test runs used `CARGO_TARGET_DIR=/Users/jferrolino/Devs/portfolio/filimon/target`, which the environment notes allow, so the build cache was reused. Verify commands were run verbatim from helper scripts under the gitignored `target/red/`, because the worktree isolation hook rejects inline `for` loops.

## Issues Encountered

- The first classifier run returned `no_target_test_failure` because the target was written in the `module::test` form. With JUnit evidence the target must use the `<classname>#<method>` form, so the records were rebuilt. The reports were not changed.

## Verification

- `cargo test -p filimon`: 132 passed, 0 failed, 7 ignored. All 6 tests named in Task 1 and all 12 named in Task 2 print `... ok`.
- Structural gates: the three conversions call `utc_rfc3339(`, and the non-test part of models.rs has exactly one `to_rfc3339_opts`, inside `utc_rfc3339`.
- D-01: `git diff --quiet 0ed7d3f -- smartmodules/ transforms.yaml src/scorer.rs` exits 0.
- Clippy: no warnings in src/commands/consume.rs. src/models.rs has only the 4 baseline lines (86, 32, 94, 94).
- No cluster path was changed, so the `--ignored` suite was not needed.

## User Setup Required

None.

## Next Phase Readiness

- Gap 3 (WR-09) is closed in code. Phase 2 re-verification should run next. The code-review gate updates WR-09 in 02-REVIEW-DISPOSITION.md.
- A-2 holds: the Phase 3 host builds social items through `SourceItem::social`, so they get the epoch guard with no further change.

## Self-Check: PASSED

- FOUND: src/models.rs (contains `fn utc_rfc3339`)
- FOUND: src/commands/consume.rs (contains `UTC year from 0000 to 9999`)
- FOUND commits: cd7f8e8, c9a1331, 7afb8fd, 74ce6c3 (all ancestors of HEAD)

---
*Phase: 02-sourceitem-pipeline*
*Completed: 2026-11-04*
