---
phase: 02-sourceitem-pipeline
plan: 01
subsystem: streaming
tags: [rust, fluvio, smartmodule, serde, chrono, sourceitem]

# Dependency graph
requires:
  - phase: 01-plugin-contract
    provides: "wit/types.wit `item` record (field list and epoch-ms `published`) that SourceItem mirrors"
provides:
  - "SourceItem envelope (news and social constructors) aligned with the WIT item record"
  - "OutputRecord/ArticleMeta unified output with no html field"
  - "NEWS_CHAIN, news_chain(), FluvioStream::topic_stream (RetryMode::Disabled) and FluvioStream::preflight"
  - "fili consume news path: preflight, chain stream, decode, limit/idle collection, NER, JSON array output"
  - "fili file and fili produce emit SourceItem::news to filimon-raw-pages"
  - "epoch_ms_to_rfc3339 and parse_news_published (D-09)"
  - "sanitize_for_terminal for printed record text"
  - "examples/fixtures/news-article.html news fixture"
affects: [02-02, 02-03, phase-03-plugin-host]

# Actuals (#2632)
actuals:
  tokens: 14346
  tasks: 3
  commits: 4
plan_head_before: 2dff9ed8803d321414911a5dd755b9956a0a610d
plan_head_after: cdcacfd6a5d1507264695f47c7dc468761fe4fa6

# Tech tracking
tech-stack:
  added: ["chrono 0.4 (direct; already 0.4.45 in Cargo.lock via fluvio)"]
  patterns:
    - "Consumer attaches the server-side SmartModule chain via ConsumerConfigExt + SmartModuleInvocationWasm::Predefined"
    - "Stream items normalised to Incoming::{Record, Diagnostic}; collection loop generic over Stream<Item = Incoming> for unit tests"
    - "Diagnostics via eprintln!, never including record values (page HTML)"
    - "Admin-API preflight that names every missing topic and SmartModule"

key-files:
  created:
    - examples/fixtures/news-article.html
  modified:
    - src/models.rs
    - src/streaming.rs
    - src/commands/consume.rs
    - src/output.rs
    - src/main.rs
    - src/commands/produce.rs
    - Cargo.toml
    - Cargo.lock

key-decisions:
  - "decode_news redacts double-quoted spans of the serde error before truncating to 200 chars, because serde_json quotes the offending string value (page content) in invalid-type errors"
  - "Task 2 RED used compile-only stubs (None-returning) so the new tests compile and fail on their assertions; Rust has no RED-evidence adapter for libtest text, so RED was classified from a libtest JUnit rerun (RUSTC_BOOTSTRAP=1 on the test binary only)"
  - "print_summary also sanitises published, category and entity kind, beyond the listed fields, since they are record-derived too"

patterns-established:
  - "News records travel as SourceItem on filimon-raw-pages; the chain sees them as RawPage { url, html }"
  - "OutputRecord is the only shape written to JSON; html cannot reach it by construction"

requirements-completed: [PIPE-01, PIPE-02]

coverage:
  - id: D1
    description: "SourceItem round-trips for news and social, and its JSON keys equal the WIT item fields plus source_type (plus html for news)"
    requirement: PIPE-01
    verification:
      - kind: unit
        ref: "src/models.rs#models::tests::source_item_round_trip"
        status: pass
      - kind: unit
        ref: "src/models.rs#models::tests::source_item_matches_wit_item"
        status: pass
      - kind: unit
        ref: "src/models.rs#models::tests::minimal_social_json_defaults"
        status: pass
    human_judgment: false
  - id: D2
    description: "News SourceItem is RawPage-compatible and never serializes html: null"
    requirement: PIPE-01
    verification:
      - kind: unit
        ref: "src/models.rs#models::tests::news_item_is_rawpage_compatible"
        status: pass
    human_judgment: false
  - id: D3
    description: "D-09 conversions: epoch ms to RFC 3339 (social) and best-effort news date parsing with published_raw kept"
    requirement: PIPE-01
    verification:
      - kind: unit
        ref: "src/models.rs#models::tests::epoch_ms_to_rfc3339_converts"
        status: pass
      - kind: unit
        ref: "src/models.rs#models::tests::parse_news_published_best_effort"
        status: pass
      - kind: unit
        ref: "src/models.rs#models::tests::from_article_maps_fields"
        status: pass
      - kind: unit
        ref: "src/models.rs#models::tests::social_raw_parsing"
        status: pass
    human_judgment: false
  - id: D4
    description: "fili consume attaches the unmodified three-SmartModule chain to filimon-raw-pages and returns one news OutputRecord"
    requirement: PIPE-02
    verification:
      - kind: unit
        ref: "src/streaming.rs#streaming::tests::news_chain_matches_transforms_yaml"
        status: pass
      - kind: integration
        ref: "src/commands/consume.rs#commands::consume::tests::news_through_chain (--ignored, local cluster)"
        status: pass
    human_judgment: false
  - id: D5
    description: "Collection keeps arrival order and duplicates, stops at limit, and handles an empty stream; bad values and stream errors become diagnostics without record content"
    requirement: PIPE-02
    verification:
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::collect_keeps_arrival_order_and_duplicates"
        status: pass
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::collect_stops_at_limit"
        status: pass
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::collect_empty_stream_returns_no_records"
        status: pass
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::decode_news_reports_bad_value_without_content"
        status: pass
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::stream_error_omits_record_value"
        status: pass
    human_judgment: false
  - id: D6
    description: "JSON output is an array with no html key, parent dirs created, [] for no records; terminal output strips control characters"
    requirement: PIPE-02
    verification:
      - kind: unit
        ref: "src/output.rs#output::tests::save_json_writes_array_without_html"
        status: pass
      - kind: unit
        ref: "src/output.rs#output::tests::save_json_empty_slice_writes_empty_array"
        status: pass
      - kind: unit
        ref: "src/output.rs#output::tests::sanitize_strips_control_chars"
        status: pass
    human_judgment: false
  - id: D7
    description: "SC2 live smoke: fili file fixture -> fili consume --from-beginning --limit 200 -> output/wise_output.json holds the fixture as a news record (platform local, published 2024-05-01T09:00:00Z, author Jane Reporter, relevance 0.489)"
    requirement: PIPE-02
    verification:
      - kind: e2e
        ref: "target/debug/fili file examples/fixtures/news-article.html && target/debug/fili consume --from-beginning --limit 200 && jq -e ... output/wise_output.json"
        status: pass
    human_judgment: false
  - id: D8
    description: "fili produce against a live news site yields news records with platform without www., an article block and RFC 3339 or null published"
    requirement: PIPE-02
    verification: []
    human_judgment: true
    rationale: "Depends on a live third-party site and network access (plan human-check for Task 3); the crawl result changes over time"

# Metrics
duration: 11min
completed: 2026-11-03
status: complete
---

# Phase 2 Plan 01: SourceItem Pipeline (news path) Summary

**News pages now travel as `SourceItem`s on `filimon-raw-pages`. `fili consume` runs the unmodified map-preprocess, map-extract and filter-relevance chain on that stream itself (retries disabled, preflight first) and writes a JSON array of unified `OutputRecord`s with RFC 3339 dates and no html.**

## Performance

- **Duration:** about 11 min
- **Started:** 2026-11-03T19:59:33Z
- **Completed:** 2026-11-03T20:10:52Z
- **Tasks:** 3
- **Files modified:** 9 (1 created, 8 modified)

## Accomplishments
- `SourceItem` (news and social constructors), `OutputRecord` and `ArticleMeta` in `src/models.rs`. They line up with the WIT `item` record field for field, round-trip through JSON, and never write `html: null`.
- `fili consume` attaches the server-side chain through `consumer_with_config` with `RetryMode::Disabled`. A preflight names every missing topic or SmartModule. Each record becomes an output record or a stderr diagnostic that never carries the record value. Batch mode writes `output/wise_output.json`.
- `fili file` and `fili produce` both produce `SourceItem::news` with the record key unchanged. The retired articles topic is gone from live code, and `ensure_topics` creates only `filimon-raw-pages`.
- D-09 conversions: WIT epoch ms gives RFC 3339 with milliseconds, and news dates (RFC 3339, RFC 2822, offset-less, date-only) give RFC 3339 UTC or null, with `published_raw` kept.
- The live SC2 smoke passed through the real CLI: the fixture came back as a news record with relevance 0.489.

## Task Commits

1. **Task 1 (tracer): `fili file` to SourceItem on raw-pages, chain stream in `fili consume`, JSON output** - `4b09728` (feat)
2. **Task 2 (TDD): SourceItem contract, WIT alignment, RFC 3339 conversion, social constructor**
   - RED: `eaf86b5` (test)
   - GREEN: `ce9a969` (feat)
3. **Task 3: crawler producer, news fixture, terminal sanitising, live SC2 smoke** - `cdcacfd` (feat)

**Plan metadata:** recorded in the docs commit that adds this SUMMARY.

## Files Created/Modified
- `src/models.rs`: SourceType, SourceItem (news/social), platform_from_url, epoch_ms_to_rfc3339, parse_news_published, ArticleMeta, OutputRecord::from_article, 10 contract tests
- `src/streaming.rs`: NEWS_CHAIN, news_chain(), FluvioStream::topic_stream, FluvioStream::preflight; articles-topic constant removed; chain-order test
- `src/commands/consume.rs`: rewritten news consume path (DEFAULT_OUTPUT, ConsumeTopics, Incoming, Collected, decode_news, describe_stream_error, collect_records, collect_from_topics), 5 unit tests and 1 `#[ignore]` cluster test
- `src/output.rs`: save_json over OutputRecord with parent-dir creation, print_summary over OutputRecord, sanitize_for_terminal, 4 tests
- `src/main.rs`: run_file sends SourceItem::news; header comment rewritten; commented-out blocks untouched
- `src/commands/produce.rs`: crawler sends SourceItem::news keyed by target URL
- `Cargo.toml` / `Cargo.lock`: chrono 0.4 direct dependency (no new crate; chrono stays 0.4.45)
- `examples/fixtures/news-article.html`: news fixture with og:title, author and article:published_time meta

## TDD Gate Compliance (Task 2)

- **RED:** `eaf86b5`. Nine new tests were written in `models::tests`. `SourceItem::social`, `epoch_ms_to_rfc3339` and `parse_news_published` got compile-only stubs (social leaves published and raw None; the converters return None), so the tests compile and fail on their assertions, not on a build error. `cargo test -p filimon models::` showed 5 failures (`epoch_ms_to_rfc3339_converts`, `parse_news_published_best_effort`, `social_raw_parsing`, `from_article_maps_fields`, `source_item_round_trip`), all `assertion left == right failed`, `left: None`. The other 5 passed because they cover behaviour Task 1 or serde defaults already supply (`platform_from_url_rules`, `minimal_social_json_defaults`, `from_article_keeps_boundary_relevance`, `source_item_matches_wit_item`, `news_item_is_rawpage_compatible`).
  - The classifier refused the libtest text report (`INVALID_RED invalid_record`, unsupported format). Following tdd.md, the target was rerun with a supported reporter: `RUSTC_BOOTSTRAP=1 target/debug/deps/fili-<hash> models::tests::epoch_ms_to_rfc3339_converts -Z unstable-options --format junit`, with report path, run start and modification time recorded. `gsd-tools check tdd-red-evidence` returned `RED_EVIDENCE_OK (target_test_failed)`, 1 test, 1 failure, matched `models::tests#epoch_ms_to_rfc3339_converts`.
  - **Semantic assessment:** the target ran and failed on the planned assertion (expected `Some("2024-10-27T03:33:20.123Z")`, got `None` from the stub), not on setup, collection or compile errors.
- **GREEN:** `ce9a969`. All 10 `models::tests::*` pass. The full suite gives `test result: ok. 77 passed; 0 failed; 1 ignored`.
- **REFACTOR:** none needed.

## Verification Results

- `cargo test -p filimon`: `test result: ok. 79 passed; 0 failed; 1 ignored` (59 baseline + 20 new; the ignored one is the cluster test).
- `cargo test -p filimon -- --ignored news_through_chain`: `test result: ok. 1 passed`. The scratch topic `filimon-test-raw-<nanos>` was deleted afterwards (`fluvio topic list` shows only `filimon-raw-pages` and the retired `filimon-articles`, which was left alone).
- Task 1 grep gate: the main.rs non-comment filter and `git grep` for the retired topic outside main.rs both print nothing (exit 1). `consumer_with_config`, `RetryMode::Disabled`, `SourceItem::news` and `skip_serializing_if` are present.
- D-01: `git diff --quiet 0ed7d3f -- smartmodules/ transforms.yaml` exits 0, and `git status --porcelain` is empty for those paths.
- Clippy gates: there are no warnings in src/commands/consume.rs or src/output.rs. src/models.rs has 4 warnings, all pre-existing at HEAD. Non-dead-code warnings: streaming.rs 2 and main.rs 2, both at baseline. Task 3's produce.rs shows 4 pre-existing warnings (lines shifted by one), so none are new.
- Task 2: the Cargo.lock `name = ` set is identical to commit 0ed7d3f, and chrono is 0.4.45.
- Task 3 live smoke: `fili file examples/fixtures/news-article.html` then `fili consume --from-beginning --limit 200` produced 2 records (the README sample already on the topic, plus the fixture). The jq assertion printed `true`, and `grep -c '"html"' output/wise_output.json` printed 0. output/wise_output.json is gitignored.
- The smoke wrote one fixture record to the real `filimon-raw-pages`, which is the plan's SC2 acceptance path.

## Decisions Made
- `decode_news` redacts double-quoted spans of the serde error (`"…"`) before truncating to 200 characters. serde_json quotes the offending string value in `invalid type` errors, so the plain error text could have carried page content into a diagnostic.
- `describe_stream_error` truncates the Display of other ErrorCode variants to 200 characters as well, as defence in depth.
- `print_summary` sanitises published, category and entity kind too, not only the listed fields.
- Doc comments use "chain module" or backticked names in place of bare "SmartModule". clippy pedantic `doc_markdown` treats bare `SmartModule` as an unquoted identifier, and this keeps models.rs at its baseline count.
- The crate keeps MSRV 1.85, so consume.rs uses nested `if let` rather than let-chains (stable only from 1.88).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] Serde error text could leak record content into diagnostics**
- **Found during:** Task 1 (decode_news)
- **Issue:** The plan said to include up to 200 characters of the serde error. serde_json's `invalid type: string "..."` errors embed the offending value, which can be page HTML. That breaks the prohibition "no diagnostic carries the record value" (T-02-02).
- **Fix:** `redact_quoted` replaces every double-quoted span with `"…"` before truncation. `decode_news_reports_bad_value_without_content` now also covers a JSON-string value `"<html>PAGE-BODY</html>"`.
- **Files modified:** src/commands/consume.rs
- **Verification:** the unit test passes and the diagnostic omits `PAGE-BODY`
- **Committed in:** 4b09728

**2. [Rule 2 - Missing Critical] Extra test that the JSON keeps the original text**
- **Found during:** Task 3
- **Issue:** The plan states that the JSON keeps the original text (unsanitised), but no test pinned it.
- **Fix:** added `output::tests::save_json_keeps_original_text`
- **Files modified:** src/output.rs
- **Committed in:** cdcacfd

**3. [Rule 3 - Blocking] Clippy doc_markdown on new doc comments**
- **Found during:** Task 1 clippy gate
- **Issue:** New doc comments with bare `SmartModule` / `serde_json` pushed models.rs above its 4-warning gate, and added warnings to consume.rs (which must have 0) and streaming.rs.
- **Fix:** reworded or backticked those doc comments
- **Files modified:** src/models.rs, src/streaming.rs, src/commands/consume.rs
- **Committed in:** 4b09728

---

**Total deviations:** 3 auto-fixed (2 missing critical, 1 blocking)
**Impact on plan:** All three tighten the plan's own prohibitions or gates. There is no scope change.

## Issues Encountered
- The worktree sandbox rejected compound shell commands that mix `git` with redirects or chained checks, and writes into `.git/worktrees/<id>/`. Verify gates were run as plain, separate commands with the same checks. The plan-commit ledger file could not be written, so `plan_head_before` is the spawn base 2dff9ed, which the orchestrator verified at spawn.
- RED-evidence classification needed a JUnit rerun because libtest text output is unsupported (see TDD Gate Compliance).

## User Setup Required

None. No external service configuration is required.

## Next Phase Readiness
- Plans 02-02 and 02-03 can build on `ConsumeTopics`, `Incoming`, `collect_records`, `collect_from_topics`, `FluvioStream::preflight` and `SourceItem::social`.
- PIPE-02 is also declared by 02-03, so it was not marked complete in REQUIREMENTS.md yet (shared-ID gate). Only PIPE-01 was marked.
- Human check still open (end-of-phase): `fili produce https://www.bbc.co.uk/news --depth 2` then `fili consume --from-beginning --limit 50` against the live site.

## Self-Check: PASSED

- All 9 key files exist on disk.
- Commits 4b09728, eaf86b5, ce9a969, cdcacfd and 0c3c91a are ancestors of HEAD.

---
*Phase: 02-sourceitem-pipeline*
*Completed: 2026-11-03*
