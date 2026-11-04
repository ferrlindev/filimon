---
phase: 02-sourceitem-pipeline
verified: 2026-11-04T12:45:00Z
status: gaps_found
score: 93/95 must-haves verified (80/81 truths incl. 1 override, 13/14 prohibitions)
covered_files:
  - .planning/phases/02-sourceitem-pipeline/02-01-PLAN.md
  - .planning/phases/02-sourceitem-pipeline/02-01-SUMMARY.md
  - .planning/phases/02-sourceitem-pipeline/02-02-PLAN.md
  - .planning/phases/02-sourceitem-pipeline/02-02-SUMMARY.md
  - .planning/phases/02-sourceitem-pipeline/02-03-PLAN.md
  - .planning/phases/02-sourceitem-pipeline/02-03-SUMMARY.md
  - .planning/phases/02-sourceitem-pipeline/02-04-PLAN.md
  - .planning/phases/02-sourceitem-pipeline/02-04-SUMMARY.md
  - .planning/phases/02-sourceitem-pipeline/02-05-PLAN.md
  - .planning/phases/02-sourceitem-pipeline/02-05-SUMMARY.md
  - .planning/phases/02-sourceitem-pipeline/02-06-PLAN.md
  - .planning/phases/02-sourceitem-pipeline/02-06-SUMMARY.md
  - Cargo.lock
  - Cargo.toml
  - README.md
  - src/cli.rs
  - src/commands/consume.rs
  - src/main.rs
  - src/models.rs
  - src/output.rs
  - src/streaming.rs
covered_digest: "v4:sha256:19b04d9d64cafc164adb27efdaa2173134717df8a6142c539b72934f2310a1f1"
behavior_unverified: 0
overrides_applied: 1
overrides:
  - must_have: "D-09 social side: a social `published` that is valid RFC 3339 passes through unchanged; any other value becomes null with one diagnostic naming the topic and offset"
    reason: "WR-04 (4c2845e) deliberately normalizes a valid RFC 3339 social `published` to UTC with millisecond precision. This matches README line 242 ('RFC 3339 UTC timestamp or null'), CONTEXT D-09 ('RFC 3339 or null') and the epoch-ms social path. Invalid values still become null with one diagnostic naming the topic and offset. Scope: this accepts only the UTC and millisecond normalization. It does not accept output that is not RFC 3339 at the year limits (gap 3); the Phase 02 gap-closure plans fix that."
    accepted_by: "ferrlindev"
    accepted_at: "2026-11-04T10:51:01Z"
re_verification:
  previous_status: gaps_found
  previous_score: 61/66
  trigger: "Gap-closure plans 02-05 (66075b0, 7c8db6d, e294591, 1e09a35) and 02-06 (cd7f8e8, c9a1331, 7afb8fd, 74ce6c3) changed src/commands/consume.rs and src/models.rs"
  gaps_closed:
    - "Gap 1 (02-02 D-09 social side, WR-04 UTC normalization): closed by the accepted override (f1f93fc); WR-04 is still in place, as 02-06 requires"
    - "Gap 2 (no silent drop past MAX_DIAGNOSTICS): every record past the cap is now named by topic and exact offset on a per-topic range line (02-05)"
    - "Gap 3 (D-09 year limits, news, social and epoch): every published conversion goes through utc_rfc3339, which returns None outside UTC years 0000..=9999 (02-06; closes WR-09)"
  gaps_remaining: []
  regressions:
    - "New, introduced by 02-05's located notes together with the WR-06 budget (review WR-12): a social Note counts toward the diagnostic budget and arrives before its Record, so a budget stop can fall between them. The record is then not written, but its note is kept, or its offset goes into a 'noted' range. This breaks 02-06 prohibition #2 and the 02-05 truth that notes are about written records"
gaps:
  - truth: "02-06 prohibition: MUST NOT drop or skip a social record because its published time is out of range: the record is still written, with published null. Also the 02-05 truth 'Notes about written records are located and kept apart ... past the cap their offsets go into the topic's noted OffsetRanges'"
    status: failed
    reason: "collect_records (consume.rs:548-573) does `seen += 1` for every Incoming::Note (line 558) and checks the budget after each item. decode_social emits [Note, Record] for a social record whose published is out of range or invalid. A budget stop can therefore land on the Note before the Record is pulled. Reproduced with a scratch copy of the crate (repo source untouched) that adds one test calling the real decode_social and collect_records. Input: budget-1 undecodable social values, then one valid social item. Limit 1, last item published `2024-05-01T09:00:00Z`: records=1. Same input with published `9999-12-31T23:00:00-05:00`: records=0, and the kept messages end with `t offset 99: published is not an RFC 3339 time with a UTC year from 0000 to 9999; set to null` and `stopped after 100 diagnostics before 1 records arrived`. With limit 50: records=0, and unshown_lines gives `t: 399 more skipped record(s), details not shown, offsets 100-498` and `t: 1 more record diagnostic(s), details not shown, offsets 499`. Offset 499 was never written. The only difference between the written and unwritten cases is the published value. So the record is not written because its published time is out of range, and the note tells the operator it was written with null. The claims-news Note has the same defect. The unit tests pass because none of them puts a Note at the budget boundary. Review WR-12 found this; the disposition is still open."
    artifacts:
      - path: "src/commands/consume.rs"
        issue: "collect_records lines 557-560 charge Incoming::Note to the diagnostic budget, so the budget check (568-572) can break between a Note and its Record"
    missing:
      - "Do not charge notes to the budget: `Ok(Some(Incoming::Note(d))) => collected.note(d),` with no `seen += 1`. Notes cannot make a run unbounded: at most two arrive per record, and records are bounded by limit. Alternative: carry notes with their record, for example Incoming::Record(OutputRecord, Vec<RecordDiagnostic>), so the two cannot be separated"
      - "Add a unit test: diagnostic_budget(1)-1 skipped items, then decode_social for an item with an out-of-range published, collected with limit 1. Expect 1 record with published null and a kept note. Repeat with limit 50 and assert unshown[topic].noted only names offsets of written records"
advisory:
  - finding: "NER engine error text reaches the NER diagnostic through truncate_detail(&e.to_string()) (consume.rs enrich). If a gline-rs error ever embedded input text, up to 200 chars of record text would reach stderr (control characters stripped by print_diagnostic)."
    category: security
    reason: "Carried forward unchanged. Outside the decode prohibitions: a NER failure keeps the record. It would be resolved by building the NER diagnostic without e.to_string(), or by evidence that gline-rs errors never embed input."
    evidence_status: "none provided"
---

# Phase 2: SourceItem Pipeline Verification Report

**Phase Goal:** Every pipeline record travels as a unified `SourceItem`. News keeps flowing through the WISE SmartModule chain, while social items pass straight through to the consumer and land, NER-enriched, in the same JSON output
**Verified:** 2026-11-04T12:45:00Z
**Status:** gaps_found
**Re-verification:** Yes. This run follows gap-closure plans 02-05 (gap 2) and 02-06 (gap 3). Gap 1 was accepted as an override.

## Summary

All three previous gaps are closed:

- **Gap 1 (social `published` UTC normalization):** accepted by override, and WR-04 is still in place.
- **Gap 2 (no silent drop past 100 diagnostics):** closed. In batch mode, past `MAX_DIAGNOSTICS`, every skipped record is now named by topic and exact offset on a per-topic range line. The flood unit test names offsets 0..=499, each exactly once. The cluster test `undecodable_social_flood_names_every_offset` passes on the local cluster.
- **Gap 3 (year limits):** closed. A single `utc_rfc3339` guard is used by `epoch_ms_to_rfc3339`, `rfc3339_to_utc` and `parse_news_published`. It is the only non-test `to_rfc3339_opts` call in models.rs (line 203). Every boundary test passes.

All 4 roadmap success criteria still hold, on the live cluster and in the tests.

One new defect blocks a `passed` verdict. It comes from the interaction of 02-05's located notes with the WR-06 budget (review WR-12, still open). A social note counts toward the diagnostic budget and arrives before its record. A budget stop between the two leaves the record unwritten, even though its note says the record was "set to null". I reproduced this deterministically. It breaks 02-06 prohibition #2, which this round's own plan wrote. The fix is one line plus a test.

## Goal Achievement

### Roadmap Success Criteria

| # | Success criterion | Status | Evidence |
|---|-------------------|--------|----------|
| SC1 | `SourceItem` round-trips through JSON for news and social; fields line up with the WIT item record | VERIFIED | `source_item_round_trip` and `source_item_matches_wit_item` pass (`/tmp/verif-02r4-unit.txt`). 02-06 changes only how `published` is formatted, not the envelope |
| SC2 | `fili produce` / `fili file` still yield extracted articles through `map-preprocess → map-extract → filter-relevance` | VERIFIED (live crawl: human item 2) | Cluster `news_through_chain ... ok` and `good_news_after_poison_record_is_delivered ... ok`. Live `fili consume --from-beginning --limit 500` (exit 0) returned 5 news records. `git diff --quiet 0ed7d3f -- smartmodules/ transforms.yaml src/scorer.rs` exits 0 |
| SC3 | A social item reaches the consumer without social-specific filtering, even below the relevance threshold | VERIFIED | Cluster `social_passthrough ... ok`. The same live run returned 2 social records (`t3_fili02`, platform `reddit`) |
| SC4 | `--enrich-ner` + `--limit`: NER on social text; both kinds in one JSON, labelled by source type and platform | VERIFIED (live model: human item 1) | `ner_runs_on_social_text`, `enriched_output_labels_and_omits_html` and `detector_error_keeps_record` pass. Live `--enrich-ner --limit 500` (exit 0, `NER: ✗ unavailable`, the designed fallback) wrote 7 records, all with `source_type` and `platform`, and 0 `html` keys. `--enrich-ner --limit 0` is rejected before connecting |

### Gap-closure truths (02-05, 14 truths)

| # | Truth (short) | Status | Evidence |
|---|---------------|--------|----------|
| 1 | Past the cap, every skipped record is named by topic and exact offset; flood with limit 50 names 0..=99 kept and `100-499` on the range line | VERIFIED | `undecodable_flood_stops_at_diagnostic_budget ... ok`. Code: `Collected::skip` (consume.rs:144-150), `unshown_lines` (874-892), printed in `consume_batch` (827-832) |
| 2 | Live tracer: 150 bad social values give 100 kept messages and `{topic}: 50 more skipped record(s), details not shown, offsets 100-149` | VERIFIED | Cluster `undecodable_social_flood_names_every_offset ... ok` |
| 3 | `OffsetRanges` are exact (checked add; `5-7, 9, 3`, count 6); no skipped range covers a written record | VERIFIED | `offset_ranges_are_exact`, `unshown_ranges_never_cover_written_records` pass. Code at consume.rs:79-120 matches the truth exactly |
| 4 | Every skip path gives a located `Skipped` whose message starts `{topic} offset {offset}:`; the in-batch step carries the start offset | VERIFIED | `located_diagnostics_match_their_message ... ok`. Code: decode_news 292, decode_social 315, news_incoming 446-468 |
| 5 | Notes about written records are located and kept apart; past the cap their offsets go into noted ranges, never into skipped ranges | FAILED (edge, WR-12) | The mechanism holds (`unshown_ranges_never_cover_written_records ... ok`). The premise does not: at a budget stop, a Note can describe a record that was never written. Scratch probe, limit 50: `t: 1 more record diagnostic(s) ... offsets 499` with records=0. See the gap |
| 6 | Restated 02-01 news decode truth holds past the cap | VERIFIED | `decode_news_reports_bad_value_without_content`, `stream_error_omits_record_value`, `located_diagnostics_match_their_message` and `unshown_ranges_never_cover_written_records` pass |
| 7 | WR-06 limits kept: `diagnostic_budget` and `finish_batch` are byte-identical to 98e70f6; `MAX_DIAGNOSTICS` = 100; a suppressed record keeps only its offset | VERIFIED | Region diffs against 98e70f6 are identical for both functions. `const MAX_DIAGNOSTICS: usize = 100` is at line 153. `OffsetRanges` has only `ranges` and `count`. `unshown_lines(&BTreeMap<String, Unshown>)`. No `suppressed`, `Incoming::Diagnostic` or `more diagnostics not shown` remains |
| 8 | One stderr choke point | VERIFIED | The only `eprintln!` is at consume.rs:900, inside `print_diagnostic`. The range lines print through it (831) |
| 9 | Streaming mode unchanged, with no cap or range logic | VERIFIED | `consume_stream` (904-926) prints `Skipped`/`Note` messages directly. No collector logic |
| 10 | Edge adjacency: touching offsets merge, gaps split, a written record splits a range, two notes count twice and name one offset, topics stay separate | VERIFIED | `offset_ranges_are_exact` and `unshown_ranges_never_cover_written_records` pass |
| 11 | Edge empty: with 0 or exactly 100 skipped records, `unshown` is empty; the 101st record gives `offsets 100` | VERIFIED | `collect_empty_stream_returns_no_records` and `cap_boundary_moves_only_later_offsets_into_ranges` pass |
| 12 | Edge ordering: kept messages first, then range lines by topic name, the skipped line before the noted line | VERIFIED | `unshown_ranges_never_cover_written_records ... ok`. BTreeMap iteration order is in the code |
| 13 | No regression; all seven cluster tests pass | VERIFIED | Unit `132 passed; 0 failed; 7 ignored`. Cluster `7 passed; 0 failed`, every test listed `... ok` |
| 14 | D-01 unchanged | VERIFIED | `git diff --quiet 0ed7d3f -- smartmodules/ transforms.yaml src/scorer.rs` exits 0 |

### Gap-closure truths (02-06, 9 truths)

| # | Truth (short) | Status | Evidence |
|---|---------------|--------|----------|
| 1 | `rfc3339_to_utc` gives None outside UTC years 0..=9999; in-range limits convert, cut to milliseconds | VERIFIED | `rfc3339_to_utc_normalizes_only_rfc3339 ... ok`. Code at models.rs:216-219 |
| 2 | Social decode at the year limits: one record with published null plus one note that does not echo the value; `9999-12-31T23:59:59Z` gives `.000Z` with no note | VERIFIED (decode level) | `social_published_outside_year_range_becomes_null ... ok`. At run level, see the gap |
| 3 | `parse_news_published` gives None outside the range on all four branches; in-range limits parse | VERIFIED | `parse_news_published_best_effort ... ok`. Code at models.rs:225-245 |
| 4 | `from_article` with an out-of-range date gives null and keeps `published_raw` | VERIFIED | `from_article_drops_out_of_range_published ... ok` |
| 5 | Epoch side: 253_402_300_800_000 gives None; ...799_999 gives `9999-12-31T23:59:59.999Z` | VERIFIED | `epoch_ms_to_rfc3339_converts ... ok` |
| 6 | The guard is proven necessary (chrono 0.4.45 baseline) | VERIFIED | `chrono_writes_out_of_range_years_with_sign ... ok` |
| 7 | One guard: a single non-test `to_rfc3339_opts` call, inside `utc_rfc3339` | VERIFIED | grep finds line 203 (helper) and line 794 (inside the tests module). All three callers use `utc_rfc3339(` |
| 8 | Gap 1 override respected: WR-04 not reverted | VERIFIED | `invalid_social_published_becomes_null` and `rfc3339_to_utc_normalizes_only_rfc3339` pass. Live social `published` = `2024-10-27T03:33:20.123Z` |
| 9 | No regression; D-01 | VERIFIED | See 02-05 #13 and #14 |

### Earlier plan truths (02-01..02-04, 54 truths, regression check)

| Plan | Truths | Status | Notes |
|------|--------|--------|-------|
| 02-01 | 19 | 19/19 VERIFIED | The two previously FAILED truths now hold: news decode diagnostics (gap 2, closed by 02-05) and D-09 news side (gap 3, closed by 02-06). Live output: every non-null `published` has a 4-digit year |
| 02-02 | 17 | 16 VERIFIED + 1 PASSED (override) | D-09 social side is covered by the gap 1 override (ferrlindev, 2026-11-04T10:51:01Z) |
| 02-03 | 5 | 5/5 VERIFIED | README.md, docker-compose.yml and .planning/codebase are unchanged since 98e70f6 |
| 02-04 | 13 | 13/13 VERIFIED | output.rs, main.rs, cli.rs and streaming.rs are unchanged since 98e70f6. 02-04 #12's literal "5 passed" is out of date (now 7 cluster tests, all ok), as noted last time |

Every test named in the six plans reports `ok`: 62 distinct names, 55 in the unit run and 7 in the cluster run. `test_lemmatize` appears only as a workspace exclusion note.

**Score:** 80/81 truths (79 VERIFIED + 1 PASSED override; 0 present but behavior-unverified)

### Prohibitions (14)

| Plan | Prohibition | Status | Evidence |
|------|-------------|--------|----------|
| 02-01 | No news `html` in output or summary | VERIFIED | `save_json_writes_array_without_html` passes. Live output has 0 `html` keys |
| 02-01 / 02-04 #1 / 02-05 #1 | No silent drop of a news record; past the cap, the topic's range line is the diagnostic | VERIFIED | Range lines (02-05). Content-freedom tests and the live poison test pass |
| 02-02 | Do not label by the claimed `source_type` | VERIFIED | `social_label_comes_from_topic` passes |
| 02-02 / 02-04 #2 / 02-05 #2 | No silent drop of a social record; no social filtering | VERIFIED (with WR-12 note) | Holds as written: in the WR-12 case the record does get a stderr diagnostic naming its topic and offset (its note). That note misdescribes the record as written, which is the gap below |
| 02-02 | Do not alter social title or text | VERIFIED | `social_text_is_not_truncated_in_output` passes |
| 02-02 | No `html` for either kind | VERIFIED | Tests pass. Live output has 0 `html` keys |
| 02-03 | Document no topic, command or flag that `fili` lacks | VERIFIED | README unchanged |
| 02-04 #3 | Do not sanitize or truncate record text on its way to the JSON | VERIFIED | `save_json_keeps_original_text` passes. output.rs unchanged |
| 02-05 #3 | No skipped range names an offset that was not skipped | VERIFIED | `unshown_ranges_never_cover_written_records` passes. In the WR-12 probe, offset 499 went to `noted`; `skipped` stayed exactly `100-498` |
| 02-06 #1 | Do not clamp or shift an out-of-range time into 0000..=9999 | VERIFIED | The guard returns None. All boundary tests pass |
| 02-06 #2 | Do not drop or skip a social record because its published time is out of range: the record is still written, with published null | **FAILED (edge, WR-12)** | Scratch probe: same input, in-range published gives records=1; out-of-range published gives records=0 at the budget boundary, and the kept note says `set to null` |
| 02-06 #3 | Do not revert WR-04 | VERIFIED | See 02-06 truth #8 |

All prohibitions are test-tier and each has a wired, passing test. The single failure is a counterexample at run level that the tests do not cover, not missing enforcement at decode level.

### Required Artifacts

| Artifact set | Status | Details |
|--------------|--------|---------|
| 02-01 (8), 02-02 (5), 02-03 (4), 02-04 (1) | VERIFIED | `verify.artifacts`: 8/8, 5/5, 4/4, 1/1 |
| 02-05: src/commands/consume.rs (`pub struct OffsetRanges`) | VERIFIED | 1/1. Substantive (2535 lines) and wired: `skip`/`note` → `unshown` → `unshown_lines` → `print_diagnostic` |
| 02-06: src/models.rs (`fn utc_rfc3339`), src/commands/consume.rs (`UTC year from 0000 to 9999`) | VERIFIED | 2/2. Called from all three formatters and from decode_social |

### Key Link Verification

`verify.key-links`: 02-01 5/5, 02-02 4/4, 02-03 2/2, 02-04 4/4, 02-05 4/4, 02-06 2/2. All WIRED.

### Data-Flow Trace (Level 4)

| Artifact | Data | Source | Real data | Status |
|----------|------|--------|-----------|--------|
| Output JSON (`save_json`) | `records` | Fluvio: filimon-raw-pages (3 SmartModules) + filimon-social-items (none) | Yes: 5 news + 2 social in the live run | FLOWING |
| stderr kept diagnostics + range lines | `Collected.diagnostics`, `Collected.unshown` | decode_* / news_incoming → collect_records → consume_batch → print_diagnostic | Yes: the cluster flood test asserts the exact range line from real topic offsets | FLOWING |
| `published` | `utc_rfc3339` | chrono parse of record values | Yes: live values `2024-10-27T03:33:20.123Z`; no signed years | FLOWING |
| `OutputRecord.entities` | detector output | NerEngine via EntityDetector | Wiring is proven with injected detectors; the model is absent here | FLOWING (wiring); live model is human item 1 |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Full unit suite (once) | `cargo test -p filimon` | exit 0, `132 passed; 0 failed; 7 ignored` | PASS |
| Cluster suite (once) | `cargo test -p filimon -- --ignored` | exit 0, `7 passed; 0 failed`, all 7 listed `ok` | PASS |
| Plan-cited tests | grep of the 62 names in saved output | all `ok` | PASS |
| Live merged consume (read-only, /tmp) | `fili consume --from-beginning --limit 500 --output /tmp/verif-02r4-e2e.json` | exit 0; 7 records (5 news, 2 social); platforms local/example.com/reddit; 0 html; every published 4-digit year | PASS |
| Live NER fallback | `fili consume --from-beginning --enrich-ner --limit 500 --output /tmp/verif-02r4-ner.json` | exit 0; `NER: ✗ unavailable`; 7 labelled records | PASS |
| WR-03 rejection | `fili consume --enrich-ner --limit 0` | `× --enrich-ner works only in batch mode (--limit N)` | PASS |
| WR-12 budget boundary | scratch copy in /tmp/verif-wr12 + one test (`verifier_wr12`) using the real `decode_social`/`collect_records` | in-range: records=1; out-of-range: records=0 with a kept `set to null` note; limit 50: `noted` offsets 499, records=0 | FAIL (gap) |
| D-01 | `git diff --quiet 0ed7d3f -- smartmodules/ transforms.yaml src/scorer.rs` | exit 0 | PASS |
| Commits exist | `verify.commits` on the 8 plan commits | all valid | PASS |

### Probe Execution

Step 7c: no PLAN declares a probe script, and no `scripts/*/tests/probe-*.sh` exists. Not applicable.

### Requirements Coverage

| Requirement | Source Plans | Description | Status | Evidence |
|-------------|--------------|-------------|--------|----------|
| PIPE-01 | 02-01, 02-06 | Unified `SourceItem` envelope covers news and social | SATISFIED | SC1. The year guard closes the D-09 output edge |
| PIPE-02 | 02-01, 02-03, 02-04, 02-05 | News continues through the WISE chain | SATISFIED | SC2. Every skipped news record is named past the cap |
| PIPE-03 | 02-02, 02-03, 02-04, 02-05, 02-06 | Social items pass through, no social filtering | SATISFIED (edge gap) | SC3 holds and no social filtering exists. The WR-12 gap is an interaction between batch budget and notes, not filtering, but it can leave a social record unwritten at the budget boundary |
| PIPE-04 | 02-02, 02-03 | NER on social text; same JSON, labelled by source | SATISFIED (live model: human item 1) | SC4 |

No orphaned requirements. REQUIREMENTS.md maps exactly PIPE-01..04 to Phase 2, and each is claimed by at least one plan. The traceability rows (REQUIREMENTS.md:148-151) still read `Gaps Found`.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| src/commands/consume.rs | 557-560, 568-572 | Note charged to the budget; a stop can split a Note from its Record (WR-12) | BLOCKER | Breaks 02-06 prohibition #2 and the 02-05 notes-about-written-records truth |
| src/commands/consume.rs | 438-476 | `MAX_REOPENS` counts over the whole run, and each in-batch record costs one reopen (WR-08, open) | WARNING | No must-have states otherwise. A long streaming run can lose the news branch, and that run exits non-zero |
| src/commands/consume.rs | 548-573, 850-868 | A budget stop is not a failure (WR-07) | WARNING | Out of scope by 02-05's explicit decision |
| src/commands/consume.rs | 144-161 | Fatal lines use up the 100-message record cap (IN-12) | INFO | No record goes unnamed. Doc wording only |
| src/commands/consume.rs | 467-473 | Reopen-limit line says 100 skipped; 101 are reported (IN-13) | INFO | Wording only |
| src/commands/consume.rs | 763 | Redundant second `check_ner_mode` (WR-11) | INFO | None |
| src/output.rs | 25-42 | Predictable temp path that follows symlinks (WR-10) | WARNING | Unchanged; outside this round |

Debt markers (TBD/FIXME/XXX/TODO/HACK/PLACEHOLDER): none in consume.rs or models.rs.

Re-verification evidence gate: the WR-12 blocker is new scope, but consume.rs was modified after the previous `verified:` timestamp (by 02-05 and 02-06), and a deterministic, reproducible probe backs it. It is therefore blocking, not advisory.

### Advisory (New Scope, Unevidenced)

| # | Finding | Category | Why advisory |
|---|---------|----------|--------------|
| 1 | NER diagnostic formats the gline-rs error's Display text (`truncate_detail(&e.to_string())`). Carried forward unchanged | security | No deterministic evidence that gline-rs embeds input. A NER failure keeps the record |

### Human Verification Required (carried forward, still pending in 02-UAT.md)

These items do not set the status, because the gap takes precedence. Run them after the gap is closed.

1. **Live GLiNER NER on social and news.** Install `models/gliner_small-v2.1/tokenizer.json` and `onnx/model.onnx` (still absent here). Produce the `t3_ner` item. Run `fili file examples/fixtures/news-article.html`, then `fili consume --from-beginning --enrich-ner --limit 500 --output /tmp/fili-02-sc4.json`. Expected: `NER: ✓ GLiNER loaded`, non-empty entities for `t3_ner` and for the fixture, one array, no `html`.
2. **Live crawl.** Run `fili produce https://www.bbc.co.uk/news --depth 2`, then `fili consume --from-beginning --limit 50`. Expected: news records with platform `bbc.co.uk`, `id == url`, an `article` block, `published` RFC 3339 with a 4-digit year or null, no `html`. Needs network access.
3. **README mermaid diagram renders on GitHub.** This is a visual check.

### Gaps Summary

There is one gap, with one root cause and a one-line fix.

- **WR-12 (Note/Record split at the budget stop).** `collect_records` charges `Incoming::Note` to the diagnostic budget. `decode_social` emits the note before the record, so a stop can land between them. The record is then neither written nor reported as skipped, and its note (kept, or in a "noted" range) says the published value was "set to null" on a record the output file does not contain. Two must-haves are affected:
  - 02-06 prohibition #2, written in this same round: "the record is still written, with published null".
  - 02-05 truth #5: notes are about written records.

  Recommended fix: remove `seen += 1` from the `Note` arm (consume.rs:558) and add the boundary test described in the gap. Notes are bounded by two per record, so the budget's purpose (bounding a run of undecodable values) is unaffected. An override is possible but not recommended: it would accept a contradiction of a prohibition written in this same gap-closure round.

Everything else is verified, by tests, by the cluster suite and live: one `SourceItem` envelope, news through the unchanged chain, social pass-through, merged NER-ready JSON, every dropped record named past the cap, and RFC 3339 output at the year limits.

---

_Verified: 2026-11-04T12:45:00Z_
_Verifier: Claude (gsd-verifier)_
