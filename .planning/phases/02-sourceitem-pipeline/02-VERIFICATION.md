---
phase: 02-sourceitem-pipeline
verified: 2026-11-03T20:56:32Z
status: gaps_found
score: 50/52 must-haves verified (45/45 truths, 5/7 prohibitions)
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
covered_digest: "v4:sha256:5bf3c31ad7cd51e38a9813edd20bdb61f64d1c797e50edb3a3d6070cbfa6a4b0"
behavior_unverified: 0
overrides_applied: 0
gaps:
  - truth: "Prohibition (02-01): MUST NOT silently drop a record the news stream delivers. Every record either becomes an output record or produces a stderr diagnostic naming the topic and offset, and no diagnostic carries the record value (page HTML). Prohibition (02-02): same for the social stream, 'without the record value'."
    status: failed
    reason: "decode_news and decode_social build diagnostics from serde_json's Display text, run through redact_quoted. redact_quoted toggles on every double quote and ignores backslash escapes, so a value with an escaped inner quote leaks the rest of the value. serde's unknown-variant errors use backticks, which redact_quoted never touches, so a social record's source_type value (including a raw ESC byte) reaches the diagnostic verbatim. Diagnostics are printed with eprintln! at consume.rs:459, 463 and 489 without sanitize_for_terminal. Reproduced deterministically (see Behavioral Spot-Checks). The existing tests (decode_news_reports_bad_value_without_content, undeserializable_social_record_is_reported) only use values with no inner quotes, so they pass while the prohibition is violated. Same root cause as code review CR-01 and SECURITY T-02-02 (reopened)."
    artifacts:
      - path: "src/commands/consume.rs"
        issue: "redact_quoted (lines 73-89) is not escape-aware and does not handle backtick-quoted spans; decode_news (95-103) and decode_social (111-120) feed it serde Display text; describe_stream_error (142-151) passes e.hint through truncate_detail only; eprintln! at 459/463/489 prints diagnostics unsanitized"
    missing:
      - "Build parse diagnostics from serde_json structured accessors (e.classify(), e.line(), e.column()) instead of Display text, or make redaction escape-aware and cover backtick spans"
      - "Route every diagnostic through output::sanitize_for_terminal at one choke point before eprintln!"
      - "Regression tests: a news value with an odd number of escaped inner quotes (e.g. \"a\\\"PAGE-BODY\") and a social record whose source_type contains \\u001b, asserting neither the content nor ESC appears in the diagnostic"
---

# Phase 2: SourceItem Pipeline Verification Report

**Phase Goal:** Every pipeline record travels as a unified `SourceItem`. News keeps flowing through the WISE SmartModule chain, while social items pass straight through to the consumer and land, NER-enriched, in the same JSON output
**Verified:** 2026-11-03T20:56:32Z
**Status:** gaps_found
**Re-verification:** No — initial verification

## Summary

All four roadmap success criteria hold in the codebase, confirmed by running the code. The full unit suite passes (102 passed, 3 ignored). The 3 cluster tests pass on the local Fluvio cluster. A read-only `fili consume` run over the real topics put news and social records into one JSON array.

One plan-level, test-tier prohibition fails in both 02-01 and 02-02. A diagnostic must not carry the record value, but with crafted input it does. This is the same defect as code review CR-01. It does not stop the pipeline from working, but it is a must-have that is false in the code, so the status is `gaps_found`. If the project accepts the non-blocking disposition already recorded for T-02-02, an override is suggested at the end.

## Goal Achievement

### Roadmap Success Criteria

| # | Success criterion | Status | Evidence |
|---|-------------------|--------|----------|
| SC1 | `SourceItem` (source type, platform, id, url, title, text, author, published, raw) round-trips through JSON for news and social, and its fields line up with the WIT item record | VERIFIED | `src/models.rs:111-187` defines `SourceItem` with those fields. `models::tests::source_item_round_trip` and `source_item_matches_wit_item` pass (the WIT test reads `wit/types.wit` with `include_str!`) |
| SC2 | `fili produce <url>` and `fili file <paths>` still yield extracted articles at the consumer through `map-preprocess → map-extract → filter-relevance` | VERIFIED (produce live crawl needs a human) | `main.rs` `run_file` and `produce.rs:71-73` both send `SourceItem::news`. `streaming.rs` `news_chain()` attaches the 3 SmartModules through `consumer_with_config`. Cluster test `news_through_chain` passes. A live read-only `fili consume --from-beginning --limit 500` returned the fixture record: news, platform `local`, published `2024-05-01T09:00:00Z`, author `Jane Reporter`, relevance 0.489 |
| SC3 | A social `SourceItem` reaches the consumer without social-specific filtering, even when its text is below the news relevance threshold | VERIFIED | `consume.rs:358-362` opens the social stream with `topic_stream(topics.social, offset, Vec::new())`. Cluster test `social_passthrough` passes: news `lol cat` is dropped by the chain, while social `lol cat` (t3_lol) is delivered. The live run returned `t3_fili02` (`lol cat`, article null) |
| SC4 | With `--enrich-ner` and `--limit`, NER runs on social text, and both kinds go to the same JSON output labelled with source type and platform | VERIFIED (live GLiNER run needs a human) | `consume_batch` (`consume.rs:462`) calls `enrich(records, ner as &dyn EntityDetector)` on all records. `ner_input` gives title + newline + text for social. Tests `ner_runs_on_social_text`, `enriched_output_labels_and_omits_html` and `detector_error_keeps_record` pass. The live run wrote 5 news and 2 social records into one array, each with `source_type` and `platform`. The GLiNER model is absent, so it fell back to no NER as designed |

### Plan Truths

| Plan | Truths | Status | Evidence |
|------|--------|--------|----------|
| 02-01 | 19 (SC2 smoke, D-01, D-02, D-03 code, D-06, D-07, D-08, D-09 both sides, SC1, preflight, producers, diagnostics, terminal sanitising, 5 edge truths, suite green) | 19/19 VERIFIED | All 20 named tests pass in `/tmp/verif-02-unit.txt`. `git diff --quiet 0ed7d3f -- smartmodules/ transforms.yaml` exits 0. The `git grep` for the retired topic in `src/` (excluding main.rs) exits 1, and the filter for non-comment lines in main.rs exits 1. `ensure_topics` lists only `TOPIC_RAW_PAGES`. No `deny_unknown_fields` under smartmodules/. The live fixture record matches every SC2 field |
| 02-02 | 17 (SC3, D-01 social, D-04, D-05, SC4, D-07/D-08/D-09 social, `--source`/`--output`, D-01 unchanged, 9 edge/idle truths) | 17/17 VERIFIED | All 21 named tests pass, including the 3 cli tests, `required_inputs_follow_source_filter` and `idle_timeout_stops_collection`. All 3 cluster tests pass (`social_passthrough`, `missing_topic_is_named`, `news_through_chain`). `fili consume --help` lists `--source <SOURCE>` (both/news/social, default both) and `--output` (default `output/wise_output.json`). `src/scorer.rs` is unchanged from 0ed7d3f |
| 02-03 | 5 (README operator path, D-05 at the CLI, D-03 docs, README topology and flags, codebase map) | 5/5 VERIFIED | `filimon-articles` count is 0 in README, ARCHITECTURE, INTEGRATIONS and docker-compose. `subscribe to articles` count is 0. `filimon-social-items` appears in all 3 map files and in README (8 times). The README social sample line (`t3_fili02`) matches the records now on `filimon-social-items`, and my consume run returned them. D-05 at the CLI: `run → consume_batch → collect_from_topics → open_incoming → preflight(...)?`, with the error propagated to `main` (non-zero exit). The preflight error text is proven by the `missing_topic_is_named` cluster test |

**Score:** 45/45 truths verified (0 present but behavior-unverified)

### Prohibitions

| Plan | Prohibition | Tier | Status | Evidence |
|------|-------------|------|--------|----------|
| 02-01 | MUST NOT write news `html` into the JSON output or terminal summary | test | VERIFIED | `OutputRecord` has no html field. `save_json_writes_array_without_html` passes. My live output contained 0 `"html"` keys |
| 02-01 | MUST NOT silently drop a news record; no diagnostic carries the record value | test | FAILED | Records are not dropped silently, but diagnostics do carry record content. The probe reproduced a leak of `PAGE-BODY-SECRET` (see Spot-Checks) |
| 02-02 | MUST NOT label by the claimed `source_type` (topic of origin) | test | VERIFIED | `from_social` hard-codes `SourceType::Social`. `social_label_comes_from_topic` passes |
| 02-02 | MUST NOT silently drop a social record; diagnostic without the record value | test | FAILED | Same root cause. A social `source_type` value, including a raw ESC byte, appears verbatim in the diagnostic |
| 02-02 | MUST NOT alter social title or text in the output (the 500-char cut is NER input only) | test | VERIFIED | `social_text_is_not_truncated_in_output` passes. `ner_input` returns a new String |
| 02-02 | MUST NOT write `html` into the output for either kind | test | VERIFIED | `from_social_drops_html_and_labels_social` and `enriched_output_labels_and_omits_html` pass. The live output has 0 `"html"` keys |
| 02-03 | MUST NOT document a topic, command or flag `fili` does not have | test | VERIFIED | The README flags table matches `fili consume --help`. The topic names match `TOPIC_RAW_PAGES` and `TOPIC_SOCIAL_ITEMS` |

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/models.rs` | SourceType, SourceItem, ArticleMeta, OutputRecord, conversions, from_social, is_rfc3339 | VERIFIED | Substantive (654 lines, 13 tests). Used by consume.rs, main.rs, produce.rs |
| `src/streaming.rs` | NEWS_CHAIN, news_chain, topic_stream, preflight, TOPIC_SOCIAL_ITEMS | VERIFIED | `consumer_with_config` + `RetryMode::Disabled`. Preflight lists topics and SmartModules through the admin API |
| `src/commands/consume.rs` | decode, merge, collect, enrich, EntityDetector, source filter | VERIFIED | 1187 lines. The run path is wired from `main.rs` |
| `src/output.rs` | save_json, print_summary, sanitize_for_terminal | VERIFIED | Called from consume_batch and consume_stream |
| `src/cli.rs` | SourceFilter, DEFAULT_OUTPUT, `--source`/`--output` | VERIFIED | Shown in `--help` |
| `src/main.rs`, `src/commands/produce.rs` | Both producers send `SourceItem::news` | VERIFIED | Record keys unchanged (url, target.url) |
| `examples/fixtures/news-article.html` | News fixture with published_time meta | VERIFIED | Its record came out of the live chain with relevance 0.489 |
| `Cargo.toml` | chrono 0.4 direct dependency | VERIFIED | The Cargo.lock `name =` set is identical to 0ed7d3f |
| README.md, codebase map (3 files) | Operator path and topology | VERIFIED | See 02-03 truths |

`verify.artifacts`: 02-01 8/8, 02-02 5/5, 02-03 4/4 (exit 0 each).

### Key Link Verification

`verify.key-links`: 02-01 5/5, 02-02 4/4, 02-03 2/2, all WIRED. I also traced `main.rs` Consume arm → `run_consume(from_beginning, limit, source, output, ner.as_ref())` → `consume_batch` → `collect_from_topics` → `open_incoming` → `preflight` + `topic_stream` (news with `news_chain()`, social with `Vec::new()`) → `merge_incoming` (`stream::select`) → `collect_records` → `enrich` → `save_json`.

### Data-Flow Trace (Level 4)

| Artifact | Data | Source | Real data | Status |
|----------|------|--------|-----------|--------|
| output JSON (`save_json`) | `records` | Fluvio `consumer_with_config` streams on the real topics | Yes: 5 news + 2 social records from a live run | FLOWING |
| `OutputRecord.entities` | detector output | `NerEngine` via `EntityDetector` | Wiring proven with injected detectors. The real model is absent on this machine | FLOWING (wiring); live model needs a human |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Full unit suite (run once) | `cargo test -p filimon` | `test result: ok. 102 passed; 0 failed; 3 ignored`. 41/41 named phase tests `... ok` | PASS |
| Cluster tests (scratch topics) | `cargo test -p filimon -- --ignored` | `test result: ok. 3 passed`. Scratch topics cleaned up afterwards; only filimon-raw-pages, filimon-social-items and filimon-articles remain | PASS |
| Merged live consume (read-only, output to /tmp) | `fili consume --from-beginning --limit 500 --enrich-ner --output /tmp/verif-02-e2e.json` | exit 0. Reads raw-pages through the 3 SmartModules and social-items with none. 7 records: 5 news, 2 social. Fixture record fields match SC2. `t3_fili02` is social/reddit/`lol cat`/article null. `"html"` count 0 | PASS |
| CLI flags | `fili consume --help` | `--source <SOURCE>` with both/news/social [default: both], `--output` [default: output/wise_output.json] | PASS |
| D-01 SmartModules untouched | `git diff --quiet 0ed7d3f -- smartmodules/ transforms.yaml src/scorer.rs` | exit 0 | PASS |
| Diagnostic redaction (CR-01) | Scratch crate `/tmp/verif02probe` with `redact_quoted` copied verbatim from consume.rs:73-89 and serde_json 1.x | news value `"a\"PAGE-BODY-SECRET"` gives `invalid type: string "…"PAGE-BODY-SECRET"` (leak=true). Social `source_type` `\u001b[2JPAGE-BODY` gives ``unknown variant `\u{1b}[2JPAGE-BODY` `` (esc=true, leak=true) | FAIL |

### Probe Execution

Step 7c: no probe scripts are declared in the PLANs, and no `scripts/*/tests/probe-*.sh` exist for this phase. Not applicable.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| PIPE-01 | 02-01 | Unified `SourceItem` envelope covers news and social | SATISFIED | SC1 evidence |
| PIPE-02 | 02-01, 02-03 | News continues through the WISE SmartModule chain | SATISFIED | SC2 evidence, `news_through_chain`, live run |
| PIPE-03 | 02-02, 02-03 | Social items pass through with no social-specific filtering | SATISFIED | SC3 evidence, `social_passthrough` |
| PIPE-04 | 02-02, 02-03 | Consumer runs NER on social text; same JSON output, labelled by source | SATISFIED (live model needs a human) | SC4 evidence |

No orphaned requirements. REQUIREMENTS.md maps exactly PIPE-01..04 to Phase 2, and every one is claimed by at least one plan.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| src/commands/consume.rs | 73-120, 142-151, 459/463/489 | Diagnostics built from serde Display text, with redaction that does not handle escapes and no terminal sanitising | BLOCKER (violates a must-have prohibition) | Record content and terminal escape sequences from untrusted topics reach stderr |
| src/commands/consume.rs | 128-133 | Social `published` checked with `is_rfc3339` but not normalised to UTC; README:242 says "RFC 3339 UTC" (review WR-04) | WARNING | Mixed offsets in the output. The README claim is inaccurate for social records. The plan truth only says "passes through unchanged", so no must-have fails |
| src/commands/consume.rs | 153-175, 458-473 | First stream error ends the branch, and the run still exits 0 and overwrites the output file (review WR-01, WR-02) | WARNING | A poison record on raw-pages stops news at that offset. CI cannot tell an empty run from a failure. This was a deliberate design choice (A7) |
| src/main.rs | 73-78 | `--enrich-ner` loads the model even in streaming mode (`--limit 0`), where it is ignored (review WR-03) | WARNING | Wasted load. The help text says "batch mode only" |
| src/streaming.rs | TypedConsumer etc. | Dead code left behind (review IN-01) | INFO | Clippy dead-code notices |

Debt markers (TBD/FIXME/XXX): none in any file modified by this phase.

### Human Verification Required

These are recorded for after the gap is closed. Status is `gaps_found`, so they are not yet routed to UAT.

#### 1. Live GLiNER NER on social and news

**Test:** Download the GLiNER model (`models/gliner_small-v2.1/tokenizer.json`, `onnx/model.onnx`). Produce the `t3_ner` social item from 02-02 Task 2's human-check to `filimon-social-items`. Run `fili file examples/fixtures/news-article.html`. Then run `fili consume --from-beginning --enrich-ner --limit 500 --output /tmp/fili-02-sc4.json`.
**Expected:** `NER: ✓ GLiNER loaded`. The `t3_ner` record has non-empty `entities` (for example Satya Nadella, Seattle or Microsoft). The fixture news record has entities. No `html` key.
**Why human:** The model weights are absent on this machine. `--enrich-ner` falls back silently. The injected-detector tests prove only the wiring.

#### 2. Live `fili produce` crawl

**Test:** `fili produce https://www.bbc.co.uk/news --depth 2`, then `fili consume --from-beginning --limit 50`.
**Expected:** News records with platform `bbc.co.uk` (no `www.`), `id` == `url`, an `article` block, and `published` RFC 3339 or null. No `html` key.
**Why human:** Needs network access and a live third-party site, and the result changes over time.

#### 3. README mermaid diagram readability

**Test:** View README.md on GitHub.
**Expected:** The flowchart renders and shows raw-pages → chain → `fili consume`, and social-items → `fili consume` with no SmartModules.
**Why human:** Visual rendering.

### Gaps Summary

The phase goal is achieved functionally. Records travel as `SourceItem`. News flows through the unmodified 3-SmartModule chain attached by `fili consume`. Social items bypass the chain on `filimon-social-items`. Both land in one JSON array, labelled and NER-enriched through the same `enrich` pass. Every roadmap success criterion has passing behavioral evidence, including the live cluster tests and a live consume run.

One root cause fails two test-tier prohibitions (02-01 and 02-02). A diagnostic must never carry the record value, but `redact_quoted` is not escape-aware, and serde's backtick-quoted unknown-variant text bypasses it. The diagnostics also reach stderr without `sanitize_for_terminal`. The plan's tests miss this because their inputs contain no inner quotes. The fix is narrow and lives in `src/commands/consume.rs`: build diagnostics from structured serde errors, sanitise them at one choke point, and add the two regression tests. No later roadmap phase covers it, so it is not deferred.

**This may be intentional to accept.** The security audit already reopened T-02-02 as medium and non-blocking. If the project owner agrees, add this to this file's frontmatter and re-run verification:

```yaml
overrides:
  - must_have: "MUST NOT silently drop a record the news stream delivers. Every record either becomes an output record or produces a stderr diagnostic naming the topic and offset, and no diagnostic carries the record value (page HTML)"
    reason: "Leak is limited to <=200 chars of crafted record content on the operator's own stderr; tracked as CR-01 / T-02-02 (medium, non-blocking) for a follow-up fix"
    accepted_by: "{name}"
    accepted_at: "{ISO timestamp}"
  - must_have: "MUST NOT silently drop a record the social stream delivers. Every record either becomes an output record or produces a stderr diagnostic naming the topic and offset, without the record value, and no social-specific filtering exists (PIPE-03)"
    reason: "Same root cause as CR-01 / T-02-02; tracked for a follow-up fix"
    accepted_by: "{name}"
    accepted_at: "{ISO timestamp}"
```

---

_Verified: 2026-11-03T20:56:32Z_
_Verifier: Claude (gsd-verifier)_
