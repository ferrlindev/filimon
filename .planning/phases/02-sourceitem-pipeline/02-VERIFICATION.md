---
phase: 02-sourceitem-pipeline
verified: 2026-11-03T22:07:07Z
status: human_needed
score: 66/66 must-haves verified (58/58 truths, 8/8 prohibitions)
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
covered_digest: "v4:sha256:2713de90b09c0eeffb04193c39e9638c0eefc00d8054f2dbb41bae0df478077c"
behavior_unverified: 0
overrides_applied: 0
re_verification:
  previous_status: gaps_found
  previous_score: 50/52
  gaps_closed:
    - "Prohibition (02-01): MUST NOT silently drop a record the news stream delivers; no diagnostic carries the record value (page HTML)"
    - "Prohibition (02-02): MUST NOT silently drop a record the social stream delivers; diagnostic without the record value"
  gaps_remaining: []
  regressions: []
advisory:
  - finding: "NER engine error text reaches the NER diagnostic through truncate_detail only (src/commands/consume.rs enrich, src/scorer.rs:84-85 map gline-rs errors with miette!(\"{e}\")). If a gline-rs TextInput or inference error ever embedded input text, up to 200 chars of record text would reach stderr (sanitized for control characters by print_diagnostic)."
    category: security
    reason: "Raised because enrich still formats a third-party error's Display text. It is outside the decode prohibitions (a NER failure keeps the record, so it is not a drop diagnostic) and is the plan's recorded assumption A-1. It would be resolved by a test showing a gline-rs error that embeds input, or by building the NER diagnostic without e.to_string()."
    evidence_status: "none provided"
human_verification:
  - test: "Live GLiNER NER on social and news: download the GLiNER model (models/gliner_small-v2.1/tokenizer.json and onnx/model.onnx), produce the t3_ner social item from 02-02 Task 2's human-check to filimon-social-items, run `fili file examples/fixtures/news-article.html`, then `fili consume --from-beginning --enrich-ner --limit 500 --output /tmp/fili-02-sc4.json`"
    expected: "Start-up prints `NER: ✓ GLiNER loaded`. One JSON array holds news and social records, each with platform. t3_ner has non-empty entities (for example Satya Nadella, Seattle or Microsoft). The fixture news record has entities. No html key."
    why_human: "The model weights are absent on this machine, so --enrich-ner falls back to no NER (seen in this run: `NER: ✗ unavailable`). The injected-detector tests prove only the wiring."
  - test: "Live crawl: `fili produce https://www.bbc.co.uk/news --depth 2`, then `fili consume --from-beginning --limit 50`, and open output/wise_output.json"
    expected: "At least one news record with platform bbc.co.uk (no www.), id equal to url, an article block, and published RFC 3339 or null. No html key."
    why_human: "Needs network access and a live third-party site whose content changes over time."
  - test: "View README.md on GitHub"
    expected: "The mermaid flowchart renders and shows raw-pages -> SmartModule chain -> fili consume, and social-items -> fili consume with no SmartModules."
    why_human: "Visual rendering."
---

# Phase 2: SourceItem Pipeline Verification Report

**Phase Goal:** Every pipeline record travels as a unified `SourceItem`. News keeps flowing through the WISE SmartModule chain, while social items pass straight through to the consumer and land, NER-enriched, in the same JSON output
**Verified:** 2026-11-03T22:07:07Z
**Status:** human_needed
**Re-verification:** Yes, after gap-closure plan 02-04

## Summary

The gap from the previous report is closed. Two prohibitions failed last time (02-01 news and 02-02 social: "no diagnostic carries the record value"). Both now hold in the code at HEAD (`d3d4893`):

- Decode diagnostics are built from serde_json's `classify()`, `line()` and `column()` only.
- SmartModule hints are cut at the first double quote, backtick or control character.
- Every consume diagnostic goes to stderr through `print_diagnostic` → `diagnostic_line` → `output::sanitize_for_terminal`.

The prior report's exact reproduction inputs are pinned by unit tests and by 2 new cluster tests through the real chain and topic. All of them pass in my own run.

I also checked a wider set of inputs than the plan's tests use. I copied `hint_summary` verbatim into a scratch crate and fed it 15 adversarial payloads × 3 serde error forms × 3 hint prefixes. It kept 0 bytes of record content.

No regressions were found:

- Since the 02-04 base, only `src/commands/consume.rs` changed, and only in the diagnostic code and tests.
- The full unit suite passes.
- All 5 cluster tests pass.
- A live read-only consume still puts news and social records into one JSON array.

Every must-have is verified. The status is `human_needed` only because 3 checks remain that the plans deferred to a human: live GLiNER, a live crawl, and README rendering.

## Goal Achievement

### Roadmap Success Criteria

| # | Success criterion | Status | Evidence |
|---|-------------------|--------|----------|
| SC1 | `SourceItem` round-trips through JSON for news and social, and its fields line up with the WIT item record | VERIFIED | `models::tests::source_item_round_trip` and `source_item_matches_wit_item` pass in `/tmp/verif-02r-unit.txt`. `src/models.rs` is unchanged since the prior verification |
| SC2 | `fili produce <url>` and `fili file <paths>` still yield extracted articles through `map-preprocess → map-extract → filter-relevance` | VERIFIED (live crawl: human item 2) | Cluster test `news_through_chain` passes. A live read-only `fili consume --from-beginning --limit 500 --enrich-ner --output /tmp/verif-02r-e2e.json` returned the fixture record: platform `local`, published `2024-05-01T09:00:00Z`, author `Jane Reporter`, relevance 0.4893. `git diff --quiet 0ed7d3f -- smartmodules/ transforms.yaml src/scorer.rs` exits 0 |
| SC3 | A social `SourceItem` reaches the consumer without social-specific filtering, even below the news relevance threshold | VERIFIED | Cluster test `social_passthrough` passes. The live run returned `t3_fili02` (`lol cat`, article null). The social branch still opens with `topic_stream(topics.social, offset, Vec::new())` |
| SC4 | With `--enrich-ner` and `--limit`, NER runs on social text, and both kinds go to one JSON output labelled with source type and platform | VERIFIED (live model: human item 1) | `ner_runs_on_social_text`, `enriched_output_labels_and_omits_html` and `detector_error_keeps_record` pass. The live run wrote 7 records (5 news, 2 social) into one array, each with `source_type` and `platform`, and 0 `html` keys. The model is absent, so it fell back as designed |

### Plan Truths

| Plan | Truths | Status | Evidence |
|------|--------|--------|----------|
| 02-01 | 19 | 19/19 VERIFIED (regression check) | All named 02-01 tests pass within `test result: ok. 109 passed; 0 failed; 5 ignored`. `news_through_chain` passes. D-01 diff exits 0. `src/models.rs`, `src/streaming.rs`, `src/main.rs`, `src/commands/produce.rs`, `src/output.rs` are unchanged since the 02-04 base (`git diff --stat eb989bb HEAD` shows consume.rs only) |
| 02-02 | 17 | 17/17 VERIFIED (regression check) | All named 02-02 tests pass. `social_passthrough` and `missing_topic_is_named` pass. `fili consume --help` lists `--source <SOURCE>` and `--output <OUTPUT>`. The live run merges both kinds |
| 02-03 | 5 | 5/5 VERIFIED (regression check) | README, the codebase map and docker-compose.yml are unchanged since the prior verification. The live run still returns the README sample record `t3_fili02` |
| 02-04 | 13 | 13/13 VERIFIED (full check) | See the table below |

#### 02-04 truths (full 3-level check)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | News decode: escaped-quote values (1 and 3 inner quotes, article-shaped `relevance_score`) give one diagnostic with topic, offset, `line 1 column`, and no `PAGE-BODY`, `"` or backtick | VERIFIED | `decode_news_withholds_escaped_quote_content ... ok`. The test uses the prior report's exact input `"a\"PAGE-BODY-SECRET"` and asserts all the listed properties |
| 2 | Social decode: `source_type` with U+001B or backticks gives 0 records and one diagnostic with no `PAGE-BODY`, ESC or backtick | VERIFIED | `decode_social_withholds_variant_value ... ok`. Asserts `records.is_empty()`, `diagnostics.len() == 1`, topic, offset, `line 1 column`, and none of the 3 banned substrings |
| 3 | Parse diagnostics come only from `classify()`/`line()`/`column()` through `describe_json_error`, used by both decoders | VERIFIED | `describe_json_error` (consume.rs:76-85) has a 4-arm match with no wildcard and never reads Display. Both decoders call `describe_json_error(&e)` (lines 124, 142). No `{e}` or `e.to_string()` in either decoder. `redact_quoted` count is 0 |
| 4 | SmartModule hint keeps only the text before the first `"`, backtick or control character, max 200 chars, `no detail` when empty | VERIFIED | `hint_summary` (consume.rs:102-112) and `describe_stream_error` (`hint_summary(&e.hint)`, line 175). `stream_error_hint_withholds_quoted_value ... ok`. It covers the map-preprocess hint shape with eyre Location, url `424242`, ESC and the empty and leading-quote hints |
| 5 | Other stream errors follow the same rule; `SmartModuleNotFound` keeps the module name | VERIFIED | `other_stream_errors_withhold_record_value ... ok` (look_back `Value: PAGE-BODY`, `Other` with ESC, NotFound name kept) |
| 6 | Live news chain: poison record on a scratch raw-pages topic gives 0 records and one diagnostic with `offset 0`, `SmartModule error`, no `PAGE-BODY-SECRET` | VERIFIED | `poison_news_record_is_reported_without_value ... ok` in `/tmp/verif-02r-cluster.txt`, through the real map-preprocess hint |
| 7 | Live social topic: ESC `source_type` gives 0 records, one diagnostic, no `PAGE-BODY`, printed line has no control char | VERIFIED | `poison_social_record_is_reported_without_value ... ok` |
| 8 | One stderr choke point: only non-comment `eprintln!` in consume.rs is inside `print_diagnostic`; `diagnostic_line` strips ESC/CR/LF | VERIFIED | `grep eprintln! src/commands/consume.rs` has one hit: line 511, in `print_diagnostic`. 4 `print_diagnostic(` lines (definition plus batch decode, batch NER and streaming). `diagnostic_line_strips_control_chars ... ok` |
| 9 | Encoding edge: the 200-char cut counts chars (東, 🦀); invalid UTF-8 gives a content-free diagnostic | VERIFIED | `hint_detail_truncates_by_chars ... ok`. `empty_and_malformed_values_are_reported ... ok` (the `\xFFPAGE-BODY` case) |
| 10 | Empty edge (news): zero-byte value gives `truncated JSON at line 1 column 0`; empty or leading-quote hint gives `no detail` | VERIFIED | `empty_and_malformed_values_are_reported`, `stream_error_hint_withholds_quoted_value` |
| 11 | Empty edge (social): zero-byte value and empty `source_type` each give 0 records and 1 diagnostic | VERIFIED | `empty_and_malformed_values_are_reported` |
| 12 | No regression: 5 named existing tests pass; unit suite `ok`; `--ignored` gives `5 passed` | VERIFIED | All 5 named tests `... ok`. `test result: ok. 109 passed; 0 failed; 5 ignored`. `test result: ok. 5 passed; 0 failed` |
| 13 | D-01: smartmodules/, transforms.yaml, src/scorer.rs unchanged from 0ed7d3f | VERIFIED | `git diff --quiet 0ed7d3f -- smartmodules/ transforms.yaml src/scorer.rs` exits 0 |

**Score:** 58/58 truths verified (0 present but behavior-unverified)

### Prohibitions

All are test-tier. Each one has wired enforcement through a passing test, so none falls to the fail-closed default.

| Plan | Prohibition | Status | Evidence |
|------|-------------|--------|----------|
| 02-01 | MUST NOT write news `html` into the JSON output or terminal summary | VERIFIED | `save_json_writes_array_without_html` passes. Live output has 0 `html` keys |
| 02-01 (restated in 02-04) | MUST NOT silently drop a news record; no diagnostic carries the record value | VERIFIED (was FAILED) | Not dropped silently: each decode or stream error gives exactly one diagnostic naming the topic and offset (unit and cluster tests). No record value: news diagnostics come from `describe_json_error` and `hint_summary`. The reproduction is pinned by `decode_news_withholds_escaped_quote_content` and the live `poison_news_record_is_reported_without_value`. The independent probe found 0 leaks |
| 02-02 | MUST NOT label by the claimed `source_type` | VERIFIED | `social_label_comes_from_topic` passes |
| 02-02 (restated in 02-04) | MUST NOT silently drop a social record; diagnostic without the record value | VERIFIED (was FAILED) | `decode_social_withholds_variant_value`, the live `poison_social_record_is_reported_without_value` and `empty_and_malformed_values_are_reported` pass. The prior leak (backtick unknown-variant text, raw ESC) is gone: `describe_json_error` never reads Display, and the print path sanitizes |
| 02-02 | MUST NOT alter social title or text in the output | VERIFIED | `social_text_is_not_truncated_in_output` passes |
| 02-02 | MUST NOT write `html` into the output for either kind | VERIFIED | `from_social_drops_html_and_labels_social` and `enriched_output_labels_and_omits_html` pass. Live output has 0 `html` keys |
| 02-03 | MUST NOT document a topic, command or flag `fili` does not have | VERIFIED | README and the codebase map are unchanged. `--help` still lists the documented flags |
| 02-04 | MUST NOT sanitize, truncate or change record text on its way to the JSON output | VERIFIED | `save_json_keeps_original_text` passes (ESC and newline kept in the JSON). `sanitize_for_terminal` is called only in `print_summary` helpers, the NER diagnostic id, and `diagnostic_line`, never in `save_json` |

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/commands/consume.rs` | `describe_json_error`, `hint_summary`, `NO_DETAIL`, `diagnostic_line`, `print_diagnostic`, tests | VERIFIED | 1492 lines. All 5 helpers are present and called from the decode, stream-error and print paths. 7 new unit tests and 2 new cluster tests |
| 02-01..02-03 artifacts (models.rs, streaming.rs, output.rs, cli.rs, main.rs, produce.rs, fixture, Cargo.toml, README, codebase map) | As in the prior report | VERIFIED (regression) | Unchanged since the 02-04 base |

`verify.artifacts`: 02-01 8/8, 02-02 5/5, 02-03 4/4, 02-04 1/1 (exit 0 each).

### Key Link Verification

`verify.key-links`: 02-01 5/5, 02-02 4/4, 02-03 2/2, 02-04 4/4, all WIRED (exit 0 each). 02-04 links:

| From | To | Via | Status |
|------|----|-----|--------|
| consume.rs | output.rs | `diagnostic_line` → `output::sanitize_for_terminal(message)` | WIRED (line 506) |
| consume.rs | consume.rs | `decode_news` / `decode_social` → `describe_json_error(&e)` | WIRED (lines 124, 142) |
| consume.rs | consume.rs | `describe_stream_error` → `hint_summary(&e.hint)` | WIRED (line 175) |
| consume.rs | consume.rs | `consume_batch` (decode and NER) and `consume_stream` → `print_diagnostic` | WIRED (lines 487, 491, 527) |

### Data-Flow Trace (Level 4)

| Artifact | Data | Source | Real data | Status |
|----------|------|--------|-----------|--------|
| Output JSON (`save_json`) | `records` | Fluvio streams on filimon-raw-pages (3 SmartModules) and filimon-social-items (none) | Yes: 5 news and 2 social records from the live run | FLOWING |
| stderr diagnostics | `Incoming::Diagnostic` | `decode_*` / `describe_stream_error` → `print_diagnostic` | Yes: the cluster tests produced real chain diagnostics | FLOWING |
| `OutputRecord.entities` | detector output | `NerEngine` via `EntityDetector` | Wiring proven with injected detectors. The model is absent here | FLOWING (wiring); live model is human item 1 |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Full unit suite (run once) | `cargo test -p filimon` | exit 0, `test result: ok. 109 passed; 0 failed; 5 ignored`. All 19 sampled named tests `... ok` | PASS |
| Cluster tests | `cargo test -p filimon -- --ignored` | exit 0, `test result: ok. 5 passed; 0 failed`. Scratch topics cleaned up; only the 3 project topics remain | PASS |
| Live merged consume (read-only, output to /tmp) | `fili consume --from-beginning --limit 500 --enrich-ner --output /tmp/verif-02r-e2e.json` | exit 0. 7 records (news 5, social 2). Platforms local/reddit/example.com. 0 `html` keys | PASS |
| Independent leak probe (hint_summary copied verbatim from HEAD) | `cargo run` in `/tmp/verif02rprobe` | `TOTAL_LEAKS=0` over 15 payloads (escaped quotes, backtick variants, numbers, floats, bools, trailing text, invalid escape, raw ESC) × 3 error forms × 3 prefixes | PASS |
| Prior leak reproduction (CR-01) | `decode_news_withholds_escaped_quote_content`, `decode_social_withholds_variant_value` | Both pass with the prior report's exact inputs | PASS (was FAIL) |
| Structural gates | grep on consume.rs | `redact_quoted` count 0. Single `eprintln!` at line 511 in `print_diagnostic`. No Display formatting in decoders | PASS |
| Clippy (consume.rs) | `cargo clippy -p filimon --message-format=short` | 0 warnings in `src/commands/consume.rs` | PASS |

### Probe Execution

Step 7c: no probe scripts are declared in the PLANs, and no `scripts/*/tests/probe-*.sh` exists. Not applicable.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| PIPE-01 | 02-01 | Unified `SourceItem` envelope covers news and social | SATISFIED | SC1 evidence |
| PIPE-02 | 02-01, 02-03, 02-04 | News continues through the WISE SmartModule chain | SATISFIED | SC2 evidence. The news prohibition now holds |
| PIPE-03 | 02-02, 02-03, 02-04 | Social items pass through with no social-specific filtering | SATISFIED | SC3 evidence. The social prohibition now holds |
| PIPE-04 | 02-02, 02-03 | NER on social text; same JSON output, labelled by source | SATISFIED (live model is human item 1) | SC4 evidence |

No orphaned requirements. REQUIREMENTS.md maps exactly PIPE-01..04 to Phase 2, and each one is claimed by at least one plan. The traceability table (REQUIREMENTS.md:148-151) still reads `Gaps Found` for all four. That is a tracking update for the orchestrator, not a code gap.

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| src/commands/consume.rs | (decode_social) | Social `published` is checked as RFC 3339 but not normalised to UTC. README says "RFC 3339 UTC" (review WR-04) | WARNING (carried forward, unchanged) | No must-have fails. The plan truth says "passes through unchanged" |
| src/commands/consume.rs | news_incoming / consume_batch | The first stream error ends the branch. The run exits 0 and overwrites the output (WR-01, WR-02) | WARNING (carried forward; design choice A7) | A poison record on raw-pages stops news at that offset. It is now reported without content |
| src/main.rs | NER load | `--enrich-ner` loads the model in streaming mode, where it is unused (WR-03) | WARNING (carried forward) | Wasted load |
| src/streaming.rs | TypedConsumer etc. | Dead code (IN-01) | INFO | Clippy notices outside consume.rs |

Debt markers (TBD/FIXME/XXX): none in `src/commands/consume.rs`, the only file modified since the prior verification.

### Advisory (New Scope, Unevidenced)

| # | Finding | Category | Why advisory |
|---|---------|----------|--------------|
| 1 | The NER diagnostic still formats the gline-rs error's Display text (`truncate_detail(&e.to_string())`). It would leak input only if gline-rs embeds input in its errors (assumption A-1). The line is control-char sanitized by `print_diagnostic` | security | New scope, no deterministic evidence. It is outside the decode prohibitions: a NER failure keeps the record and is not a drop diagnostic |

### Human Verification Required

#### 1. Live GLiNER NER on social and news

**Test:** Download the GLiNER model (`models/gliner_small-v2.1/tokenizer.json`, `onnx/model.onnx`). Produce the `t3_ner` social item from 02-02 Task 2's human-check to `filimon-social-items`. Run `fili file examples/fixtures/news-article.html`. Then run `fili consume --from-beginning --enrich-ner --limit 500 --output /tmp/fili-02-sc4.json`.
**Expected:**
- Start-up prints `NER: ✓ GLiNER loaded`.
- `t3_ner` has non-empty `entities` (for example Satya Nadella, Seattle or Microsoft).
- The fixture news record has entities.
- One array holds both kinds.
- No `html` key.

**Why human:** The model weights are absent on this machine. This run printed `NER: ✗ unavailable`. The injected-detector tests prove only the wiring.

#### 2. Live `fili produce` crawl

**Test:** `fili produce https://www.bbc.co.uk/news --depth 2`, then `fili consume --from-beginning --limit 50`.
**Expected:**
- News records with platform `bbc.co.uk` (no `www.`).
- `id` == `url`.
- An `article` block.
- `published` RFC 3339 or null.
- No `html` key.

**Why human:** Needs network access and a live third-party site whose content changes over time.

#### 3. README mermaid diagram

**Test:** View README.md on GitHub.
**Expected:** The flowchart renders and shows raw-pages → chain → `fili consume`, and social-items → `fili consume` with no SmartModules.
**Why human:** Visual rendering.

### Gaps Summary

There are no gaps. Plan 02-04 closed the single prior root cause:

- It deleted the quote-toggle redactor, which did not handle escapes.
- It built parse diagnostics from serde_json's structured accessors.
- It cut SmartModule and other stream-error text at the first double quote, backtick or control character.
- It routed every consume diagnostic through one sanitizing stderr choke point.

Both failed prohibitions (02-01, 02-02) now hold. Evidence comes from passing unit tests on the exact reproduction inputs, 2 live cluster tests through the real chain and topic, and an independent 0-leak probe. Every roadmap success criterion and all 58 plan truths are verified. The phase is ready to proceed once the 3 human checks above are done. Separately, the REQUIREMENTS.md traceability rows for PIPE-01..04 can move from `Gaps Found` to complete.

---

_Verified: 2026-11-03T22:07:07Z_
_Verifier: Claude (gsd-verifier)_
