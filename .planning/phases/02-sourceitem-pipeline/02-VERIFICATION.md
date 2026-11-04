---
phase: 02-sourceitem-pipeline
verified: 2026-11-04T13:55:00Z
status: human_needed
score: 111/111 must-haves verified (93/93 truths incl. 1 override, 18/18 prohibitions)
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
  - .planning/phases/02-sourceitem-pipeline/02-07-PLAN.md
  - .planning/phases/02-sourceitem-pipeline/02-07-SUMMARY.md
  - Cargo.lock
  - Cargo.toml
  - README.md
  - src/cli.rs
  - src/commands/consume.rs
  - src/main.rs
  - src/models.rs
  - src/output.rs
  - src/streaming.rs
covered_digest: "v4:sha256:37949567cdd9c5c6e1ad300ac1eb023b49df9e4ee1cab7d748986afc51b7552d"
behavior_unverified: 0
overrides_applied: 1
overrides:
  - must_have: "D-09 social side: a social `published` that is valid RFC 3339 passes through unchanged; any other value becomes null with one diagnostic naming the topic and offset"
    reason: "WR-04 (4c2845e) deliberately normalizes a valid RFC 3339 social `published` to UTC with millisecond precision. This matches README line 242 ('RFC 3339 UTC timestamp or null'), CONTEXT D-09 ('RFC 3339 or null') and the epoch-ms social path. Invalid values still become null with one diagnostic naming the topic and offset. Scope: this accepts only the UTC and millisecond normalization. It does not accept output that is not RFC 3339 at the year limits (gap 3); the Phase 02 gap-closure plans fix that."
    accepted_by: "ferrlindev"
    accepted_at: "2026-11-04T10:51:01Z"
re_verification:
  previous_status: gaps_found
  previous_score: 93/95
  trigger: "Gap-closure plan 02-07 (d39e9a5, 972c4cc, 08452d7, eadd9b2; merged 8a85020) changed src/commands/consume.rs only"
  gaps_closed:
    - "WR-12 (Note/Record split at a stop): a social record and its notes are now one Incoming::Noted item, and notes no longer count toward the diagnostic budget. 02-06 prohibition #2 and the restated 02-05 truth #5 now hold at run level, in the single stream and in the merged stream"
  gaps_remaining: []
  regressions: []
advisory:
  - finding: "NER engine error text reaches the NER diagnostic through truncate_detail(&e.to_string()) (consume.rs enrich). If a gline-rs error ever embedded input text, up to 200 chars of record text would reach stderr (control characters stripped by print_diagnostic)."
    category: security
    reason: "Carried forward unchanged. Outside the decode prohibitions: a NER failure keeps the record. It would be resolved by building the NER diagnostic without e.to_string(), or by evidence that gline-rs errors never embed input."
    evidence_status: "none provided"
human_verification:
  - test: "Live GLiNER NER on social and news. Install models/gliner_small-v2.1/tokenizer.json and onnx/model.onnx (still absent: the live run here printed `NER: ✗ unavailable (File at models/gliner_small-v2.1/onnx/model.onnx does not exist)`). Produce the t3_ner social item, run `fili file examples/fixtures/news-article.html`, then `fili consume --from-beginning --enrich-ner --limit 500 --output /tmp/fili-02-sc4.json`"
    expected: "Start-up prints `NER: ✓ GLiNER loaded`. One JSON array holds news and social records, each with source_type and platform. t3_ner has non-empty entities. The fixture news record has entities. No html key"
    why_human: "Needs the GLiNER model files, which are not in the repository. Unit tests prove the wiring only with injected detectors"
  - test: "Live crawl: `fili produce https://www.bbc.co.uk/news --depth 2`, then `fili consume --from-beginning --limit 50`, and open output/wise_output.json"
    expected: "At least one news record with platform bbc.co.uk (no www.), id equal to url, an article block, published RFC 3339 with a 4-digit year or null. No html key"
    why_human: "Needs network access to a live site; the cluster test news_through_chain covers the chain with a local fixture only"
  - test: "Open README.md on GitHub and look at the mermaid flowchart"
    expected: "It renders and shows raw-pages -> SmartModule chain -> fili consume, and social-items -> fili consume with no SmartModules"
    why_human: "Visual rendering check"
---

# Phase 2: SourceItem Pipeline Verification Report

**Phase Goal:** Every pipeline record travels as a unified `SourceItem`. News keeps flowing through the WISE SmartModule chain, while social items pass straight through to the consumer and land, NER-enriched, in the same JSON output
**Verified:** 2026-11-04T13:55:00Z
**Status:** human_needed
**Re-verification:** Yes. This run follows gap-closure plan 02-07, which targets the one open gap (review WR-12).

## Summary

The WR-12 gap is closed, and the fix is structural rather than a guard:

- `Incoming::Note` is gone. A social record with notes is now one `Incoming::Noted { record, notes }` item (consume.rs, `pub enum Incoming`).
- `decode_social` returns exactly one `Incoming` per value. `social_incoming` no longer flattens a list.
- In `collect_records`, the `Noted` arm pushes the record first and then calls `collected.note(d)` for each note. It does not touch `seen`. Only the `Skipped` and `Fatal` arms charge the budget.

Every stop (limit, budget, idle timeout, end of stream) happens between whole stream items. So no stop can fall between a note and its record, and `Collected::note` is reached only after the record is in `records`.

I ran these checks myself rather than relying on the SUMMARY:
- the unit suite: 134 passed, 0 failed, 8 ignored;
- the cluster suite: 8 passed, including the new `social_note_at_budget_boundary_is_written`;
- every 02-07 structural gate;
- a mutation check: putting `seen += 1` back on the `Noted` arm makes `note_at_budget_boundary_keeps_its_record` fail;
- a live merged consume.

There are no regressions. All 4 roadmap success criteria still hold.

The status is `human_needed`, not `passed`, because the 3 human checks in 02-UAT.md are still pending: the live GLiNER model, the live crawl and the README diagram rendering. All automated must-haves are verified.

## Goal Achievement

### Roadmap Success Criteria

| # | Success criterion | Status | Evidence |
|---|-------------------|--------|----------|
| SC1 | `SourceItem` round-trips through JSON for news and social; fields line up with the WIT item record | VERIFIED | `models::tests::source_item_round_trip ... ok`, `models::tests::source_item_matches_wit_item ... ok` (`/tmp/verif-02r5-unit.txt`). models.rs has not changed since 02-06 |
| SC2 | `fili produce` / `fili file` still yield extracted articles through `map-preprocess → map-extract → filter-relevance` | VERIFIED (live crawl: human item 2) | Cluster `news_through_chain ... ok`, `good_news_after_poison_record_is_delivered ... ok`. Live `fili consume --from-beginning --limit 500` (exit 0) wrote 5 news records. `git diff --quiet 0ed7d3f -- smartmodules/ transforms.yaml src/scorer.rs` exits 0 |
| SC3 | A social item reaches the consumer without social-specific filtering, even below the relevance threshold | VERIFIED | Cluster `social_passthrough ... ok`; unit `low_relevance_social_is_kept ... ok`. The same live run wrote 2 social records (platform `reddit`). After 02-07, a social record with a note is also always written at the limit and budget boundaries |
| SC4 | `--enrich-ner` + `--limit`: NER on social text; both kinds in one JSON, labelled by source type and platform | VERIFIED (live model: human item 1) | `ner_runs_on_social_text`, `enriched_output_labels_and_omits_html`, `detector_error_keeps_record` all `ok`. Live `--enrich-ner --limit 500` (exit 0) printed `NER: ✗ unavailable`, the designed fallback, and wrote 7 records, all with `source_type` and `platform`. `--enrich-ner --limit 0` is rejected with the batch-mode help |

### Gap-closure truths (02-07, 12 truths)

| # | Truth (short) | Status | Evidence |
|---|---------------|--------|----------|
| 1 | Budget boundary: cases A–D (limit 1 with the published note, the claims-news note and both notes; limit 50 with exact `unshown_lines`) | VERIFIED | `note_at_budget_boundary_keeps_its_record ... ok`. The test body asserts every value the truth lists (1 record `t3_edge`, published None, 100 messages, no `stopped after`, noted [(99,99)] / [(499,499)], skipped [(100,498)] count 399, both exact range lines). Mutation: re-adding `seen += 1` in the `Noted` arm fails it at the `diagnostics.len() == 100` assertion |
| 2 | Live tracer: 99 bad values, then an out-of-range social item on a scratch topic, limit 1, gives 1 record and the note as the 100th message | VERIFIED | Cluster `social_note_at_budget_boundary_is_written ... ok` (`/tmp/verif-02r5-cluster.txt`). It asserts no `stopped after` or `PAGE-BODY` and an empty `unshown` |
| 3 | Merged stream: neither a limit stop nor a budget stop splits a note from its record | VERIFIED | `merged_stop_never_splits_a_note_from_its_record ... ok`. It covers both runs plus the invariant check (every `set to null` note has `t3_edge` in the output) |
| 4 | A record and its notes are one item; `Incoming` has no standalone note variant | VERIFIED | `pub fn decode_social(topic: &str, offset: i64, value: &[u8]) -> Incoming {` is present. The enum has `Noted { record: OutputRecord, notes: Vec<RecordDiagnostic> }`. Non-comment `Incoming::Note(`/`Note(RecordDiagnostic)` lines: 0. `located_diagnostics_match_their_message ... ok` (Skipped / Noted with one note / Noted with two notes, claims-news first / plain Record) |
| 5 | Restated 02-05 truth #5: notes are kept or noted-ranged only after their record is in `records` | VERIFIED | `collect_records` `Noted` arm: `collected.records.push(record); for d in notes { collected.note(d); }`. `Collected::note` is called from no other arm. Tests `unshown_ranges_never_cover_written_records` and `note_at_budget_boundary_keeps_its_record` both `ok` |
| 6 | Notes outside the budget; Skipped/Fatal charged as before; pinned regions unchanged | VERIFIED | awk gate: every `seen += 1` in `collect_records` directly follows the `Skipped(` or `Fatal(` arm head, and both still have one. `diagnostic_budget` and `finish_batch` are byte-identical to 98e70f6. `impl Collected`, `impl OffsetRanges`, `unshown_lines`, `decode_news`, `news_incoming`, `diagnostic_line`, `print_diagnostic` and the four structs are identical to 578c371. I confirmed every compared region is non-empty. `undecodable_flood_stops_at_diagnostic_budget` (asserts `MAX_DIAGNOSTICS + 1` messages and the stop line), `fatal_diagnostic_is_kept_past_the_cap` and `cap_boundary_moves_only_later_offsets_into_ranges` all `ok` |
| 7 | Restated 02-05 truth #9: streaming prints notes then summary; no collector logic in `consume_stream`; one stderr choke point | VERIFIED | Order gate exits 0: in `consume_stream`, the `Noted` arm calls `print_diagnostic` before `print_summary`. `consume_stream` contains no `unshown`, `MAX_DIAGNOSTICS` or `collect_records`. The only `eprintln!` is at consume.rs:916, inside `print_diagnostic` |
| 8 | 02-06 and the gap 1 override stay in force | VERIFIED | `decode_social` still calls `rfc3339_to_utc(` and keeps both note wordings. `social_published_outside_year_range_becomes_null`, `invalid_social_published_becomes_null`, `social_label_comes_from_topic` and `rfc3339_to_utc_normalizes_only_rfc3339` all `ok` |
| 9 | Edge PIPE-03/adjacency: a stop falls before or after a record and its notes, never between them; two notes count twice and name one offset | VERIFIED | Structural (one item). Tests `merged_stop_never_splits...`, `note_at_budget_boundary...` (case C) and `unshown_ranges_never_cover_written_records` (`noted(RAW, 105, 2)` gives noted count 2, range [(105,105)]) all `ok` |
| 10 | Edge PIPE-03/ordering: arrival order kept, claims-news note first; per-source order in the merged stream | VERIFIED | `located_diagnostics_match_their_message` (notes[0] claims news, notes[1] set to null) and `per_source_order_preserved` both `ok` |
| 11 | Edge PIPE-03/empty: a note-free record is `Incoming::Record`, never an empty `Noted`; an empty stream gives nothing | VERIFIED | `decode_social` returns `Incoming::Record(record)` when `notes.is_empty()`. Tests `located_diagnostics_match_their_message` (plain case), `low_relevance_social_is_kept`, `social_with_empty_text_is_delivered` and `collect_empty_stream_returns_no_records` all `ok` |
| 12 | No regression: all named tests pass; 8 cluster tests ok; D-01; Cargo files unchanged | VERIFIED | Unit `134 passed; 0 failed; 8 ignored`. Cluster `8 passed; 0 failed`, every test listed `... ok`. D-01 diff exits 0. `git diff --quiet 578c371 -- Cargo.toml Cargo.lock` exits 0. Clippy: `Finished`, 0 warnings in consume.rs |

### Earlier plan truths (02-01..02-06, 81 truths, regression check)

| Plan | Truths | Status | Notes |
|------|--------|--------|-------|
| 02-01 | 19 | 19/19 VERIFIED | Only consume.rs changed since the last run. The news decode, news branch and the pinned regions are byte-identical to 578c371 |
| 02-02 | 17 | 16 VERIFIED + 1 PASSED (override) | Gap 1 override carried forward. Social pass-through, topic labelling and NER tests all `ok` |
| 02-03 | 5 | 5/5 VERIFIED | README.md, docker-compose.yml and .planning/codebase are unchanged since 578c371 |
| 02-04 | 13 | 13/13 VERIFIED | output.rs, main.rs, cli.rs and streaming.rs are unchanged. The single stderr choke point holds (consume.rs:916) |
| 02-05 | 14 | 14/14 VERIFIED | Truth #5 was FAILED last run. It is now VERIFIED through its restatement in 02-07 (02-07 truth 5). As 02-07-PLAN "Superseded wording" asks, the old literal `Incoming::Note(` gate and truth #9's wording are checked through the restated truths 02-07 #5 and #7, not literally |
| 02-06 | 9 | 9/9 VERIFIED | Year guard and WR-04 unchanged. models.rs is untouched since 02-06 |

I took every test name cited in the seven plans (`tests::<name>`, 70 distinct names) and checked it in the two run logs. 69 report `... ok`. The 70th is `test_lemmatize`, which appears only as the `--workspace` exclusion note and is not a filimon test.

**Score:** 93/93 truths (92 VERIFIED + 1 PASSED override; 0 present but behavior-unverified)

### Prohibitions (18)

| Plan | Prohibition | Status | Evidence |
|------|-------------|--------|----------|
| 02-01 | No news `html` in output or summary | VERIFIED | `save_json_writes_array_without_html` ok. Live output: 0 `html` keys |
| 02-01 / 02-04 #1 / 02-05 #1 | No silent drop of a news record | VERIFIED | Range lines and content-freedom tests ok. Cluster poison tests ok |
| 02-02 | Do not label by the claimed `source_type` | VERIFIED | `social_label_comes_from_topic` ok; case B of the boundary test (claims news, labelled Social) |
| 02-02 / 02-04 #2 / 02-05 #2 | No silent drop of a social record; no social filtering | VERIFIED | `low_relevance_social_is_kept`, `social_passthrough` ok. The WR-12 caveat from last run is gone |
| 02-02 | Do not alter social title or text | VERIFIED | `social_text_is_not_truncated_in_output` ok |
| 02-02 | No `html` for either kind | VERIFIED | Tests ok. Live output: 0 |
| 02-03 | Document no topic, command or flag that `fili` lacks | VERIFIED | README unchanged |
| 02-04 #3 | Do not sanitize or truncate record text on its way to the JSON | VERIFIED | `save_json_keeps_original_text` ok; output.rs unchanged |
| 02-05 #3 | No skipped range names an offset that was not skipped | VERIFIED | `unshown_ranges_never_cover_written_records` ok; case D skipped stays exactly [(100,498)] |
| 02-06 #1 | Do not clamp an out-of-range time | VERIFIED | Guard returns None; boundary tests ok |
| 02-06 #2 | Do not drop a social record because its published is out of range | VERIFIED (was FAILED) | Boundary test cases A, C, D and the cluster tracer each write the record with published None at the budget boundary. The merged test covers the limit and budget stops |
| 02-06 #3 | Do not revert WR-04 | VERIFIED | Case B: `2024-05-01T10:00:00+01:00` gives `2024-05-01T09:00:00.000Z` |
| 02-07 #1 | No kept, printed or noted-ranged note whose record is not in the output | VERIFIED | Structural (no standalone note variant) plus `note_at_budget_boundary...`, `merged_stop_never_splits...`, `social_note_at_budget_boundary_is_written` all ok |
| 02-07 #2 | No social record dropped because it carries a note, at run level | VERIFIED | Same tests. Records = 1 in every boundary case |
| 02-07 #3 | Do not loosen the work limit on undecodable values | VERIFIED | `undecodable_flood_stops_at_diagnostic_budget` ok (stops at budget, `MAX_DIAGNOSTICS + 1` messages). `diagnostic_budget` and `finish_batch` are byte-identical to 98e70f6 |
| 02-07 #4 | Do not revert WR-04 or the 02-06 year guard on the social path | VERIFIED | `rfc3339_to_utc(` is still in `decode_social`; 02-06 and WR-04 tests ok |

All prohibitions are test-tier, and each has a wired, passing test. None is flagged.

### Required Artifacts

| Artifact set | Status | Details |
|--------------|--------|---------|
| 02-01 (8), 02-02 (5), 02-03 (4), 02-04 (1), 02-05 (1), 02-06 (2) | VERIFIED | `verify.artifacts`: 8/8, 5/5, 4/4, 1/1, 1/1, 2/2 |
| 02-07: src/commands/consume.rs (`Noted {`) | VERIFIED | `verify.artifacts` 1/1. Substantive (enum variant, decode, collector and streaming arms, 3 new tests) and wired (decode_social → social_incoming → merge_incoming → collect_records / consume_stream) |

### Key Link Verification

`verify.key-links`: 02-01 5/5, 02-02 4/4, 02-03 2/2, 02-04 4/4, 02-05 4/4, 02-06 2/2, 02-07 3/3. All WIRED. `Incoming`, `decode_social` and `collect_records` have no users outside src/commands/consume.rs (`grep -rn` over src).

### Data-Flow Trace (Level 4)

| Artifact | Data | Source | Real data | Status |
|----------|------|--------|-----------|--------|
| Output JSON (`save_json`) | `records` | Fluvio: filimon-raw-pages (3 SmartModules) + filimon-social-items (none) | Yes: 5 news + 2 social in the live run (`/tmp/verif-02r5-e2e.json`) | FLOWING |
| stderr kept diagnostics + range lines | `Collected.diagnostics`, `Collected.unshown` | decode_* / news_incoming → collect_records → consume_batch → print_diagnostic | Yes: the cluster tracer asserts the note from a real scratch topic as message 100 | FLOWING |
| Social record notes | `Incoming::Noted.notes` | decode_social → collect_records `Noted` arm → `Collected::note` | Yes: cluster test `social_note_at_budget_boundary_is_written` | FLOWING |
| `OutputRecord.entities` | detector output | NerEngine via EntityDetector | Wiring proven with injected detectors; model absent here | FLOWING (wiring); live model is human item 1 |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Full unit suite (once) | `cargo test -p filimon` | exit 0, `134 passed; 0 failed; 8 ignored` | PASS |
| Cluster suite (once) | `cargo test -p filimon -- --ignored` | exit 0, `8 passed; 0 failed`, all 8 listed `ok` | PASS |
| Plan-cited tests | grep of the 70 names in the saved logs | 69 ok; `test_lemmatize` is the exclusion note | PASS |
| 02-07 structural gates (Task 2 gates 3 and 4) | the plan's verify commands, run verbatim | both exit 0 | PASS |
| Mutation: notes charged again | scratch copy (`git archive HEAD`, /tmp, since removed) with `seen += 1` added to the `Noted` arm; `cargo test -p filimon note_at_budget_boundary_keeps_its_record` | `FAILED` at `assert_eq!(collected.diagnostics.len(), 100)` | PASS (test is discriminating) |
| TDD order | `git show --stat` of d39e9a5 / 08452d7 (tests only, +147 / +61) before 972c4cc / eadd9b2 (code) | RED commits precede GREEN | PASS |
| Clippy | `cargo clippy -p filimon --message-format=short` | `Finished`; 0 lines for src/commands/consume.rs | PASS |
| Live merged consume (read-only, /tmp) | `fili consume --from-beginning --limit 500 --output /tmp/verif-02r5-e2e.json` (binary built 21:27:54, after consume.rs 21:27:38) | exit 0; 7 records (5 news, 2 social); platforms reddit/example.com/local; 0 html; every published has a 4-digit year | PASS |
| Live NER fallback | `fili consume --from-beginning --enrich-ner --limit 500 --output /tmp/verif-02r5-ner.json` | exit 0; `NER: ✗ unavailable`; 7 labelled records | PASS |
| WR-03 rejection | `fili consume --enrich-ner --limit 0` | rejected with `add --limit N to enrich a batch of N records` | PASS |
| D-01 | `git diff --quiet 0ed7d3f -- smartmodules/ transforms.yaml src/scorer.rs` | exit 0 | PASS |
| Commits exist | `verify.commits` d39e9a5 972c4cc 08452d7 eadd9b2 3450ebb 883c8f3 8a85020 | 7/7 valid | PASS |

### Probe Execution

Step 7c: no PLAN declares a probe script, and no `scripts/*/tests/probe-*.sh` exists. Not applicable.

### Requirements Coverage

| Requirement | Source Plans | Description | Status | Evidence |
|-------------|--------------|-------------|--------|----------|
| PIPE-01 | 02-01, 02-06 | Unified `SourceItem` envelope covers news and social | SATISFIED | SC1 |
| PIPE-02 | 02-01, 02-03, 02-04, 02-05 | News continues through the WISE chain | SATISFIED (live crawl: human item 2) | SC2 |
| PIPE-03 | 02-02, 02-03, 02-04, 02-05, 02-06, 02-07 | Social items pass through, no social filtering | SATISFIED | SC3. WR-12 closed: a noted social record is always written |
| PIPE-04 | 02-02, 02-03, 02-07 | NER on social text; same JSON, labelled by source | SATISFIED (live model: human item 1) | SC4 |

No orphaned requirements: REQUIREMENTS.md maps exactly PIPE-01..04 to Phase 2, and each is claimed by at least one plan. Bookkeeping note: the traceability rows PIPE-01 and PIPE-02 (REQUIREMENTS.md:148-149) still read `Gaps Found`, and their checkboxes (lines 46-47) are unchecked. 02-07 updated only PIPE-03/04, the IDs it claims. This is stale documentation, not a code gap. The orchestrator should update it when the phase completes.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| src/commands/consume.rs | `merged_stop_never_splits_a_note_from_its_record` | Exact-count assertions depend on futures-util 0.3.31 polling news first (review IN-15; plan A-1) | INFO | The test could break on a dependency upgrade for a reason unrelated to WR-12. The invariant assertion and the fix do not depend on the order. Cargo.lock pins it |
| src/commands/consume.rs | budget stop line / `diagnostic_budget` doc | Still says "diagnostics", although notes no longer count (review IN-14) | INFO | Wording only. The doc is pinned byte-identical by 02-05 #7, and the exemption is documented on `collect_records` |
| src/commands/consume.rs | news branch | `MAX_REOPENS` counts over the whole run (WR-08, open) | WARNING | Carried forward; no must-have states otherwise |
| src/commands/consume.rs | `collect_records` | A budget stop is not a failure (WR-07) | WARNING | Out of scope by 02-05's explicit decision |
| src/output.rs | 25-42 | Predictable temp path that follows symlinks (WR-10) | WARNING | Unchanged; outside this round |
| src/commands/consume.rs | — | IN-12, IN-13, WR-11 | INFO | Carried forward, wording/redundancy only |

Debt markers (TBD/FIXME/XXX/TODO/HACK/PLACEHOLDER): none in src/commands/consume.rs. Re-verification evidence gate: no new Blocker was raised. The carried-forward gap is closed with deterministic evidence: named tests, a cluster test and a mutation check.

### Advisory (New Scope, Unevidenced)

| # | Finding | Category | Why advisory |
|---|---------|----------|--------------|
| 1 | NER diagnostic formats the gline-rs error's Display text (`truncate_detail(&e.to_string())`). Carried forward unchanged | security | No deterministic evidence that gline-rs embeds input. A NER failure keeps the record |

### Human Verification Required (still pending in 02-UAT.md)

### 1. Live GLiNER NER on social and news

**Test:** Install `models/gliner_small-v2.1/tokenizer.json` and `onnx/model.onnx` (still absent). Produce the `t3_ner` social item, run `fili file examples/fixtures/news-article.html`, then `fili consume --from-beginning --enrich-ner --limit 500 --output /tmp/fili-02-sc4.json`.
**Expected:** `NER: ✓ GLiNER loaded`. One JSON array of news and social records, each with source_type and platform. `t3_ner` and the fixture have non-empty entities. No `html`.
**Why human:** Needs model files that are not in the repository.

### 2. Live crawl

**Test:** `fili produce https://www.bbc.co.uk/news --depth 2`, then `fili consume --from-beginning --limit 50`.
**Expected:** News records with platform `bbc.co.uk`, `id == url`, an `article` block, `published` RFC 3339 with a 4-digit year or null, no `html`.
**Why human:** Needs network access to a live site.

### 3. README mermaid diagram renders on GitHub

**Test:** Open README.md on GitHub.
**Expected:** The flowchart renders: raw-pages → SmartModule chain → fili consume, and social-items → fili consume with no SmartModules.
**Why human:** Visual check.

### Gaps Summary

No gaps remain. WR-12 was the single open gap. 02-07 closed it in two ways:
- It removed the possibility of a split: a record and its notes form one `Incoming::Noted` item, consumed whole.
- It stopped charging notes to the diagnostic budget, which stays bounded because each note comes with a record and records are bounded by `limit`.

The work limit on undecodable values, the message cap, the offset ranges, content-freedom, the single stderr choke point, streaming output, WR-04 and the 02-06 year guard are all unchanged. Byte-identical region diffs and passing tests show this.

The phase goal is achieved in code, tests and the live cluster. The remaining items are the three human checks above.

---

_Verified: 2026-11-04T13:55:00Z_
_Verifier: Claude (gsd-verifier)_
