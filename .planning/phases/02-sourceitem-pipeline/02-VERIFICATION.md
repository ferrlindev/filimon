---
phase: 02-sourceitem-pipeline
verified: 2026-11-04T10:55:00Z
status: gaps_found
score: 61/66 must-haves verified (55/58 truths, 6/8 prohibitions)
covered_files:
  - .planning/codebase/ARCHITECTURE.md
  - .planning/codebase/INTEGRATIONS.md
  - .planning/codebase/STRUCTURE.md
  - .planning/phases/02-sourceitem-pipeline/02-01-PLAN.md
  - .planning/phases/02-sourceitem-pipeline/02-01-SUMMARY.md
  - .planning/phases/02-sourceitem-pipeline/02-02-PLAN.md
  - .planning/phases/02-sourceitem-pipeline/02-02-SUMMARY.md
  - .planning/phases/02-sourceitem-pipeline/02-03-PLAN.md
  - .planning/phases/02-sourceitem-pipeline/02-03-SUMMARY.md
  - .planning/phases/02-sourceitem-pipeline/02-04-PLAN.md
  - .planning/phases/02-sourceitem-pipeline/02-04-SUMMARY.md
  - Cargo.lock
  - Cargo.toml
  - README.md
  - docker-compose.yml
  - examples/fixtures/news-article.html
  - src/cli.rs
  - src/commands/consume.rs
  - src/commands/produce.rs
  - src/main.rs
  - src/models.rs
  - src/output.rs
  - src/streaming.rs
covered_digest: "v4:sha256:5e9a01a761138f8cced1439506fa42b583f0bd167501593b8fa4f7e8d813971a"
behavior_unverified: 0
overrides_applied: 0
re_verification:
  previous_status: human_needed
  previous_score: 66/66
  trigger: "Six review-fix commits (308e775 WR-01, 9382d43 WR-02, ec72c90 WR-03, 4c2845e WR-04, fc99d4e WR-05, 7330aa6 WR-06) changed src/commands/consume.rs, src/models.rs, src/output.rs and src/main.rs after the previous report"
  gaps_closed: []
  gaps_remaining: []
  regressions:
    - "02-02 truth D-09 social side: a valid RFC 3339 social `published` no longer passes through unchanged (4c2845e, WR-04)"
    - "02-01 truth (news decode diagnostics) and the news/social no-silent-drop prohibitions (02-01, 02-02, restated in 02-04): past 100 diagnostics a batch run only counts a bad record, with no topic or offset (7330aa6, WR-06)"
    - "D-09 output contract: social `published` can now be non-RFC 3339 for valid RFC 3339 input at the year limits (4c2845e, review WR-09). The news side has had the same defect since ce9a969; this run found it"
gaps:
  - truth: "02-02 D-09 social side: a social `published` that is valid RFC 3339 passes through unchanged; any other value becomes null with one diagnostic naming the topic and offset"
    status: failed
    reason: "Since 4c2845e (WR-04), decode_social replaces `published` with rfc3339_to_utc(published). That function converts to UTC and reformats with millisecond precision. `2024-05-01T09:00:00Z` becomes `2024-05-01T09:00:00.000Z`, `2024-05-01T10:00:00+01:00` becomes `2024-05-01T09:00:00.000Z`, and `...20.123456Z` loses its microseconds. The test the truth cites (invalid_social_published_becomes_null) now asserts the conversion. The change looks intentional: it matches README line 242 (`RFC 3339 UTC timestamp or null`). The truth's text is now out of date."
    artifacts:
      - path: "src/commands/consume.rs"
        issue: "decode_social lines 226-233 call rfc3339_to_utc instead of passing the value through"
      - path: "src/models.rs"
        issue: "is_rfc3339, which 02-02's artifact entry names, was replaced by rfc3339_to_utc (lines 202-208)"
    missing:
      - "Either accept the deviation with an override (suggested below), or restore pass-through and drop UTC normalization. The override is recommended because the code matches README and D-09"
  - truth: "No silent drop (02-01 truth 'a record the news stream yields that is not an ExtractedArticle becomes one stderr diagnostic naming the topic and offset ... neither is silently skipped'; prohibitions 02-01 #2 and 02-02 #2, restated as 02-04 #1 and #2: 'Every record either becomes an output record or produces a stderr diagnostic naming the topic and offset')"
    status: failed
    reason: "Since 7330aa6 (WR-06), Collected::note keeps only MAX_DIAGNOSTICS (100) non-fatal diagnostics. Each later one only increments `suppressed`, and consume_batch prints one line, `N more diagnostics not shown`, with no topic or offset. In batch mode, an undecodable record after the 100th diagnostic yields neither an output record nor a diagnostic naming its topic and offset. The budget is max(limit*10, 100), so `--limit 50` can count up to 400 records this way. undecodable_flood_stops_at_diagnostic_budget asserts this: diagnostics.len() + suppressed == 501. Content-freedom still holds, and the run is not fully silent because the count is printed. The literal per-record contract does not hold. Streaming mode (--limit 0) is not capped and still prints every diagnostic."
    artifacts:
      - path: "src/commands/consume.rs"
        issue: "Collected::note (lines 81-90) and the suppressed-count line in consume_batch (lines 700-702) drop the per-record topic/offset diagnostic past the cap"
    missing:
      - "Either accept the bounded-memory trade-off with an override, or keep the per-record contract cheaply: for suppressed diagnostics, record topic and offset, for example a per-topic first..last offset range plus count in the `N more diagnostics not shown` line"
  - truth: "D-09 output contract (02-01 truth 'D-09 news side: `published` is RFC 3339 UTC or null'; 02-02 D-09 social side; README line 242; CONTEXT D-09 'RFC 3339 or null')"
    status: partial
    reason: "chrono formats years outside 0000..=9999 with a sign and more than 4 digits. Valid RFC 3339 input at the year limits therefore produces output that is not RFC 3339. Probe with the locked chrono 0.4.45: rfc3339_to_utc('9999-12-31T23:00:00-05:00') = '+10000-01-01T04:00:00.000Z' and rfc3339_to_utc('0000-01-01T00:30:00+01:00') = '-0001-12-31T23:30:00.000Z', and neither parses back as RFC 3339. parse_news_published produces the same output (seconds precision). Social: introduced by 4c2845e. Before it, valid input passed through unchanged and stayed valid. Review WR-09 records this and it is still open. News: present since ce9a969. Practical impact is low, but the contract is checkably broken."
    artifacts:
      - path: "src/models.rs"
        issue: "rfc3339_to_utc (lines 202-208) and parse_news_published (lines 210-230) do not check that the UTC year is in 0..=9999 before formatting"
    missing:
      - "Return None (null plus the existing diagnostic on the social path) when the UTC year is outside 0..=9999, in both functions, and add the two year-limit inputs to rfc3339_to_utc_normalizes_only_rfc3339 and parse_news_published_best_effort"
advisory:
  - finding: "NER engine error text reaches the NER diagnostic through truncate_detail(&e.to_string()) (consume.rs enrich, lines 510-515). If a gline-rs error ever embedded input text, up to 200 chars of record text would reach stderr (control characters stripped by print_diagnostic)."
    category: security
    reason: "Carried forward unchanged from the previous report. Outside the decode prohibitions: a NER failure keeps the record. It would be resolved by building the NER diagnostic without e.to_string(), or by evidence that gline-rs errors never embed input."
    evidence_status: "none provided"
---

# Phase 2: SourceItem Pipeline Verification Report

**Phase Goal:** Every pipeline record travels as a unified `SourceItem`. News keeps flowing through the WISE SmartModule chain, while social items pass straight through to the consumer and land, NER-enriched, in the same JSON output
**Verified:** 2026-11-04T10:55:00Z
**Status:** gaps_found
**Re-verification:** Yes. The previous report (human_needed, 66/66) went stale after six review-fix commits.

## Summary

The phase goal still holds at the roadmap level:

- All 4 roadmap success criteria are verified.
- All 4 requirements are satisfied.
- The full unit suite passes: 125 passed, 0 failed.
- All 6 cluster tests pass on the local Fluvio cluster.
- A live read-only consume merged 5 news and 2 social records into one JSON array, with no `html` key.

The review-fix commits did change behavior that three plan contracts describe in exact words. Those contracts no longer hold as written:

1. **Social `published` is no longer passed through unchanged.** It is converted to UTC with milliseconds (WR-04). This looks intentional and matches README. An override is suggested.
2. **Past 100 diagnostics, a batch run only counts a bad record, without its topic or offset** (WR-06). This breaks the literal "every record ... produces a diagnostic naming the topic and offset" wording of one 02-01 truth and the two no-silent-drop prohibitions. It is an intentional memory bound. Choose an override or a small fix.
3. **`published` can be non-RFC 3339 for valid RFC 3339 input at the year limits.** On the social side WR-04 introduced this (review WR-09, still open). The news side has had the same pre-existing defect. A real fix is needed: a year-range guard in two functions.

Items 1 and 2 are wording-versus-intent gaps. Item 3 is a small code defect against the locked D-09 contract.

## Goal Achievement

### Roadmap Success Criteria

| # | Success criterion | Status | Evidence |
|---|-------------------|--------|----------|
| SC1 | `SourceItem` round-trips through JSON for news and social; fields line up with the WIT item record | VERIFIED | `models::tests::source_item_round_trip` and `source_item_matches_wit_item` pass in `/tmp/verif-02r3-unit.txt`. The WR-04 change touches only decode_social's `published`, not SourceItem serialization |
| SC2 | `fili produce` / `fili file` still yield extracted articles through `map-preprocess → map-extract → filter-relevance` | VERIFIED (live crawl: human item 2) | Cluster `news_through_chain ... ok`. Live `fili consume --from-beginning --limit 500 --output /tmp/verif-02r3-e2e.json` (exit 0) returned the fixture record: platform `local`, published `2024-05-01T09:00:00Z`, author `Jane Reporter`, relevance 0.4893. `git diff --quiet 0ed7d3f -- smartmodules/ transforms.yaml src/scorer.rs` exits 0. WR-01 improves this path: the new cluster test `good_news_after_poison_record_is_delivered` passes |
| SC3 | A social item reaches the consumer without social-specific filtering, even below the news relevance threshold | VERIFIED | Cluster `social_passthrough ... ok`. The live run returned `t3_fili02` (`lol cat`, article null). The social stream still opens with `topic_stream(topics.social, offset, Vec::new())` (consume.rs:584-586) |
| SC4 | With `--enrich-ner` and `--limit`, NER runs on social text, and both kinds land in one JSON output labelled with source type and platform | VERIFIED (live model: human item 1) | `ner_runs_on_social_text`, `enriched_output_labels_and_omits_html` and `detector_error_keeps_record` pass. Live `--enrich-ner --limit 500` (exit 0) wrote 7 records (2 social, 5 news), each with `source_type` and `platform`, and 0 `html` keys. The model is absent here (`NER: ✗ unavailable`), so it fell back as designed. WR-03 now rejects `--enrich-ner --limit 0` before the model loads (seen live) |

### Plan Truths

Every truth was checked against the current code. The truths that describe behavior changed by 308e775..7330aa6 were re-read clause by clause.

| Plan | Truths | Status | Notes |
|------|--------|--------|-------|
| 02-01 | 19 | 17/19 VERIFIED, 2 FAILED | FAILED: the news decode diagnostics truth (gap 2) and D-09 news side (gap 3, year limits). All 19 named tests pass |
| 02-02 | 17 | 16/17 VERIFIED, 1 FAILED | FAILED: D-09 social side (gap 1; gap 3 also applies) |
| 02-03 | 5 | 5/5 VERIFIED | README, codebase map and docker-compose.yml are unchanged since the previous report. The live run returns the README sample `t3_fili02` byte-for-byte (its `published` is already UTC with milliseconds, so WR-04 does not change it) |
| 02-04 | 13 | 13/13 VERIFIED | See the notes below for truths whose mechanism changed |

The headline "55/58" counts the 4 roadmap SCs plus 54 plan truths. The D-09 social truth is counted once, under gap 1.

#### Changed-behavior truths, checked in detail

| Truth | Status | Evidence |
|-------|--------|----------|
| 02-01: news decode error becomes one stderr diagnostic naming topic and offset; stream error keeps at most 200 chars of the hint, never the record value; neither silently skipped | FAILED (as written) | Content-freedom holds. Per-record reporting does not hold past `MAX_DIAGNOSTICS` in batch mode (gap 2) |
| 02-01: D-09 news side, `published` is RFC 3339 UTC or null | FAILED (edge) | `parse_news_published` gives `+10000-01-01T04:00:00Z` for `9999-12-31T23:00:00-05:00` (my probe with chrono 0.4.45). Pre-existing since ce9a969 (gap 3). The listed parse forms and null fallback work (`parse_news_published_best_effort ... ok`) |
| 02-01 / 02-02: Edge PIPE-02/empty and PIPE-04/empty, a stream with no records writes `[]` | VERIFIED | `finish_batch` writes when `failed` is false, whatever the record count (consume.rs:720-738). `clean_empty_batch_writes_empty_array` and `save_json_empty_slice_writes_empty_array` pass. WR-02 skips the write only when a stream failed with no records |
| 02-02: D-09 social side, valid RFC 3339 passes through unchanged | FAILED | Converted to UTC with milliseconds (gap 1). Probe: `2024-05-01T09:00:00Z` becomes `2024-05-01T09:00:00.000Z` |
| 02-04 #4: SmartModule hint keeps only text before the first `"`, backtick or control char, ≤200 chars, or `no detail`; the map-preprocess hint keeps `Failed to parse RawPage` and drops `PAGE-BODY-SECRET`, `424242`, `Location` | VERIFIED (stricter mechanism) | `describe_stream_error` now calls `module_hint_summary(&e.hint)` (consume.rs:251). Its output is either a literal start of the hint made only of listed, quote-free phrases, or `no detail`, so the "only text before the first quote" ceiling holds. The test asserts the message ends with `SmartModule error: Failed to parse RawPage: invalid type` and drops all listed tokens (`stream_error_hint_withholds_quoted_value ... ok`). Note: the hint `bad\u{1b}…`, which used to keep `bad`, now gives `no detail`. That is within "only" |
| 02-04 #5: look_back `Value: PAGE-BODY` and `Other` with ESC are content-free; `SmartModuleNotFound` keeps the module name | VERIFIED | look_back goes through `module_hint_summary` (line 255). Other errors go through `hint_summary` (line 257). `other_stream_errors_withhold_record_value ... ok` |
| 02-04 #6: live poison news record gives 0 records and exactly one diagnostic with `offset 0`, `SmartModule error`, no secret | VERIFIED | Cluster `poison_news_record_is_reported_without_value ... ok`. The test still asserts `diagnostics.len() == 1`. The diagnostic now ends `; skipped` because WR-01 reopens at offset 1 and then hits the idle timeout |
| 02-04 #8: one stderr choke point; the only non-comment `eprintln!` is in `print_diagnostic` | VERIFIED | `grep eprintln! src/commands/consume.rs` has one hit, line 747, in `print_diagnostic`. The new `N more diagnostics not shown` line and the Fatal diagnostics print through `print_diagnostic` (lines 698, 701, 705, 764, 767). `diagnostic_line_strips_control_chars ... ok` |
| 02-04 #9: 200-char cut counts chars | VERIFIED | `hint_summary` still truncates by chars. `hint_detail_truncates_by_chars` now calls it directly. SmartModule hints are no longer truncated because no hint text is copied |
| 02-04 #12: no regression; `--ignored` prints `test result: ok. 5 passed` | VERIFIED (count drift) | The 5 named tests pass. The cluster run prints `test result: ok. 6 passed; 0 failed`: the original 5 plus the new `good_news_after_poison_record_is_delivered`. The literal "5" is out of date, not a regression |
| 02-04 #13: D-01 unchanged | VERIFIED | `git diff --quiet 0ed7d3f -- smartmodules/ transforms.yaml src/scorer.rs` exits 0. The WR-05 drift test only reads module sources through `include_str!` |

The other truths (02-01 D-01/D-02/D-03/D-06/D-07/D-08, SC1 social epoch-ms, encoding/adjacency/ordering edges, missing-topic preflight; 02-02 D-01/D-04/D-05/D-07/D-08, NER input cut, source filter, idle timeout, same-id and ordering edges; 02-04 #1-#3, #7, #10, #11) touch code these commits did not change, or the change does not affect their clauses. Each truth's named test passes, and every one of the 51 test names cited in the four plans reports `ok`.

**Score:** 55/58 truths verified (0 present but behavior-unverified)

### Prohibitions

| Plan | Prohibition | Status | Evidence |
|------|-------------|--------|----------|
| 02-01 | MUST NOT write news `html` into the JSON output or terminal summary | VERIFIED | `save_json_writes_array_without_html` passes. Live output has 0 `html` keys |
| 02-01 (= 02-04 #1) | MUST NOT silently drop a news record; every record becomes output or a diagnostic naming topic and offset; no diagnostic carries the record value | FAILED (as written) | Content-freedom: VERIFIED (`decode_news_withholds_escaped_quote_content`, `module_hint_prints_only_listed_phrases`, the live poison test). Per-record naming: fails past 100 diagnostics in batch mode (gap 2). The WR-01 batch-step path names each skipped offset (`offset 1: stored in one batch with rejected offset 0 ...; skipped`, asserted in `good_news_after_poison_record_is_delivered`) |
| 02-02 | MUST NOT label by the claimed `source_type` | VERIFIED | `social_label_comes_from_topic` passes |
| 02-02 (= 02-04 #2) | MUST NOT silently drop a social record; diagnostic naming topic and offset, without the value; no social-specific filtering | FAILED (as written) | Same cap as above (gap 2). Content-freedom and no social filtering hold (`decode_social_withholds_variant_value`, the live poison social test, `social_passthrough`) |
| 02-02 | MUST NOT alter social title or text in the output | VERIFIED | `social_text_is_not_truncated_in_output` passes. Note: WR-04 alters `published`, which is a separate field and outside this prohibition |
| 02-02 | MUST NOT write `html` into the output for either kind | VERIFIED | `from_social_drops_html_and_labels_social` and `enriched_output_labels_and_omits_html` pass. Live output has 0 `html` keys |
| 02-03 | MUST NOT document a topic, command or flag `fili` lacks | VERIFIED | README unchanged. `--enrich-ner ... in --limit mode only` (README:231) is now enforced by `check_ner_mode` |
| 02-04 #3 | MUST NOT sanitize, truncate or change record text on its way to the JSON output | VERIFIED | `save_json_keeps_original_text` passes. The atomic temp-and-rename `save_json` serializes the same records. `sanitize_for_terminal` is not called in `save_json` |

All prohibitions are test-tier and each has a wired, passing test. The two failures are contradicted by the tests themselves (`undecodable_flood_stops_at_diagnostic_budget`), not by missing enforcement.

### Required Artifacts

| Artifact | Status | Details |
|----------|--------|---------|
| 02-01 (8) | VERIFIED | `verify.artifacts` 8/8 |
| 02-02 (5) | VERIFIED (with note) | `verify.artifacts` 5/5. The `src/models.rs` entry still says it provides `is_rfc3339`, which is now `rfc3339_to_utc` (gap 1) |
| 02-03 (4) | VERIFIED | `verify.artifacts` 4/4 |
| 02-04 (1) | VERIFIED | `verify.artifacts` 1/1. consume.rs is 2050 lines and holds `describe_json_error`, `hint_summary`, `module_hint_summary`, `NO_DETAIL`, `diagnostic_line`, `print_diagnostic` |

### Key Link Verification

`verify.key-links`: 02-01 5/5, 02-02 4/4, 02-03 2/2, 02-04 4/4, all WIRED.

One note: the 02-04 link "describe_stream_error reduces the SmartModule hint with hint_summary(&e.hint)" matches only because `hint_summary\(&e\.hint\)` is a substring of `module_hint_summary(&e.hint)` (consume.rs:251). The intent holds, and the hint is reduced by a stricter allow-list function. The plan's wording is out of date.

### Data-Flow Trace (Level 4)

| Artifact | Data | Source | Real data | Status |
|----------|------|--------|-----------|--------|
| Output JSON (`save_json`) | `records` | Fluvio streams: filimon-raw-pages (3 SmartModules) and filimon-social-items (none) | Yes: 5 news and 2 social records in the live run | FLOWING |
| stderr diagnostics | `Incoming::Diagnostic` / `Incoming::Fatal` | `decode_*`, `describe_stream_error`, `news_incoming` → `collect_records` → `print_diagnostic` | Yes: the cluster poison tests produce real chain diagnostics | FLOWING (capped at 100 per batch run; see gap 2) |
| `OutputRecord.entities` | detector output | `NerEngine` via `EntityDetector` | Wiring proven with injected detectors. The model is absent here | FLOWING (wiring); live model is human item 1 |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Full unit suite (run once) | `cargo test -p filimon` | exit 0, `test result: ok. 125 passed; 0 failed; 6 ignored` | PASS |
| Cluster tests (run once) | `cargo test -p filimon -- --ignored` | exit 0, `test result: ok. 6 passed; 0 failed` (missing_topic_is_named, news_through_chain, poison_social…, poison_news…, social_passthrough, good_news_after_poison_record_is_delivered) | PASS |
| All plan-cited tests | grep of the 51 cited test names in the saved output | All 51 `... ok` | PASS |
| Live merged consume (read-only, /tmp output) | `fili consume --from-beginning --limit 500 --output /tmp/verif-02r3-e2e.json` | exit 0; 7 records (news 5, social 2); platforms local/reddit/example.com; 0 `html` | PASS |
| Live NER batch fallback | `fili consume --from-beginning --enrich-ner --limit 500 --output /tmp/verif-02r3-ner.json` | exit 0; `NER: ✗ unavailable`; 7 records written | PASS |
| WR-03 rejection | `fili consume --enrich-ner --limit 0` | `Error: × --enrich-ner works only in batch mode (--limit N)`, before connecting | PASS |
| Social `published` pass-through | scratch crate, `rfc3339_to_utc` verbatim, chrono =0.4.45 | `2024-05-01T09:00:00Z → 2024-05-01T09:00:00.000Z`; `…+01:00 → …09:00:00.000Z`; `…20.123456Z → …20.123Z` | FAIL (gap 1) |
| D-09 year limits | same scratch crate, `rfc3339_to_utc` and `parse_news_published` verbatim | `9999-12-31T23:00:00-05:00 → +10000-01-01T04:00:00(.000)Z`, `0000-01-01T00:30:00+01:00 → -0001-12-31T23:30:00(.000)Z`; neither parses back as RFC 3339 | FAIL (gap 3) |
| D-01 | `git diff --quiet 0ed7d3f -- smartmodules/ transforms.yaml src/scorer.rs` | exit 0 | PASS |

### Probe Execution

Step 7c: no PLAN declares a probe script, and no `scripts/*/tests/probe-*.sh` exists. Not applicable.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| PIPE-01 | 02-01 | Unified `SourceItem` envelope covers news and social | SATISFIED | SC1. Gap 3 affects the output `published` format, not the envelope |
| PIPE-02 | 02-01, 02-03, 02-04 | News continues through the WISE SmartModule chain | SATISFIED | SC2. WR-01 keeps news flowing past a rejected record. Gap 2 is about the wording of the diagnostics contract |
| PIPE-03 | 02-02, 02-03, 02-04 | Social items pass through with no social-specific filtering | SATISFIED | SC3. No social filtering exists |
| PIPE-04 | 02-02, 02-03 | NER on social text; same JSON output, labelled by source | SATISFIED (live model: human item 1) | SC4 |

No orphaned requirements. REQUIREMENTS.md maps exactly PIPE-01..04 to Phase 2, and each one is claimed by at least one plan. The traceability rows (REQUIREMENTS.md:148-151) still read `Gaps Found`.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| src/commands/consume.rs | 437-443, 720-738 | Stopping at the diagnostic budget is not a failure, so an all-error batch run overwrites the last good output with `[]` and exits 0 (review WR-07, open) | WARNING | Reintroduces the WR-02 failure mode for decode-error floods. No must-have states otherwise |
| src/commands/consume.rs | 263, 344-351 | `MAX_REOPENS` counts over the whole run, and each record in a rejected batch costs one reopen (review WR-08, open) | WARNING | A long streaming run can lose its news branch for good. The run then exits non-zero |
| src/output.rs | 25-42 | `save_json` temp path is predictable (`.<name>.tmp-<pid>`) and `File::create` follows symlinks (review WR-10, open) | WARNING | Local-attacker file clobber in shared directories |
| src/commands/consume.rs | 633 | Second `check_ner_mode` call, redundant after main.rs:73 (review WR-11) | INFO | None |
| src/models.rs | 202-230 | Year-range guard missing in both RFC 3339 formatters | BLOCKER (gap 3) | Breaks D-09 "RFC 3339 or null" at the year limits |

Debt markers (TBD/FIXME/XXX/TODO/HACK): none in consume.rs, models.rs, output.rs or main.rs.

### Advisory (New Scope, Unevidenced)

| # | Finding | Category | Why advisory |
|---|---------|----------|--------------|
| 1 | The NER diagnostic still formats the gline-rs error's Display text (`truncate_detail(&e.to_string())`, consume.rs:510-515). Carried forward unchanged | security | No deterministic evidence that gline-rs embeds input. A NER failure keeps the record, so this is outside the drop prohibitions |

### Human Verification Required (carried forward, still pending in 02-UAT.md)

These three items are still open. They do not set the status here because gaps take precedence. Run them after the gaps are closed.

1. **Live GLiNER NER on social and news.** Download `models/gliner_small-v2.1/tokenizer.json` and `onnx/model.onnx`. Produce the `t3_ner` item. Run `fili file examples/fixtures/news-article.html`, then `fili consume --from-beginning --enrich-ner --limit 500 --output /tmp/fili-02-sc4.json`. Expected: `NER: ✓ GLiNER loaded`, non-empty entities for `t3_ner` and for the fixture, one array, no `html`. The weights are absent here (`onnx/model.onnx` does not exist).
2. **Live crawl.** Run `fili produce https://www.bbc.co.uk/news --depth 2`, then `fili consume --from-beginning --limit 50`. Expected: news records with platform `bbc.co.uk`, `id == url`, an `article` block, `published` RFC 3339 or null, no `html`. Needs network access.
3. **README mermaid diagram renders on GitHub.** This is a visual check.

### Suggested overrides

Gaps 1 and 2 look intentional. Each review fix changed behavior on purpose, and the plan wording was never updated. To accept them, add these to the frontmatter:

```yaml
overrides:
  - must_have: "D-09 social side: a social `published` that is valid RFC 3339 passes through unchanged; any other value becomes null with one diagnostic naming the topic and offset"
    reason: "WR-04 (4c2845e) normalizes valid RFC 3339 to UTC with millisecond precision, matching README's 'RFC 3339 UTC timestamp or null' and the epoch-ms social path; invalid values still become null with a diagnostic"
    accepted_by: "{name}"
    accepted_at: "{ISO timestamp}"
  - must_have: "MUST NOT silently drop a record the news stream delivers. Every record either becomes an output record or produces a stderr diagnostic naming the topic and offset, and no diagnostic carries the record value (page HTML)"
    reason: "WR-06 (7330aa6) caps kept diagnostics at 100 per batch run to bound memory; later ones are counted and reported as 'N more diagnostics not shown'. Streaming mode is uncapped."
    accepted_by: "{name}"
    accepted_at: "{ISO timestamp}"
  - must_have: "MUST NOT silently drop a record the social stream delivers. Every record either becomes an output record or produces a stderr diagnostic naming the topic and offset, without the record value, and no social-specific filtering exists (PIPE-03)"
    reason: "Same WR-06 diagnostic cap as the news prohibition"
    accepted_by: "{name}"
    accepted_at: "{ISO timestamp}"
  - must_have: "A record the news stream yields that is not an ExtractedArticle becomes one stderr diagnostic naming the topic and offset; a stream error becomes one diagnostic with the offset and at most 200 characters of the SmartModule hint, never the record value; neither is silently skipped"
    reason: "Same WR-06 diagnostic cap"
    accepted_by: "{name}"
    accepted_at: "{ISO timestamp}"
```

Gap 3 needs a code fix, not an override: the output can break the locked D-09 contract.

### Gaps Summary

All three gaps have one cause: the review-fix iteration changed behavior without updating the plan contracts, or, for gap 3, without guarding a formatter edge.

- **Gap 1 (WR-04, social `published`).** The truth says "passes through unchanged". The code converts to UTC with milliseconds. Recommended: override, because the code matches README and D-09.
- **Gap 2 (WR-06, diagnostic cap).** Past 100 diagnostics, a batch run names no topic or offset for a dropped record. This affects one 02-01 truth and both no-silent-drop prohibitions. Recommended: override, or a small fix that reports per-topic offset ranges for suppressed diagnostics.
- **Gap 3 (D-09 year limits, news and social).** `rfc3339_to_utc` and `parse_news_published` can emit `+10000-…` or `-0001-…`, which is not RFC 3339. Recommended: return None when the UTC year is outside 0..=9999, and add both inputs to the existing tests. This also closes review WR-09.

The core pipeline is intact and verified live and by tests: one `SourceItem` envelope, news through the unchanged chain, social pass-through, merged NER-ready JSON. After the gaps are resolved, the three human checks remain.

---

_Verified: 2026-11-04T10:55:00Z_
_Verifier: Claude (gsd-verifier)_
