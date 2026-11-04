---
phase: "2"
slug: "sourceitem-pipeline"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: true) (#2117)
status: validated
nyquist_compliant: true
wave_0_complete: true
created: "2026-11-04"
---

# Phase 2 — Validation Strategy

> Per-phase validation contract for feedback sampling during execution.
> Source: `02-RESEARCH.md` § Validation Architecture.

---

## Test Infrastructure

| Property | Value |
|----------|-------|
| **Framework** | Rust built-in test harness (`#[test]`, `#[tokio::test]`); unit tests live inside `src/` (bin-only crate) |
| **Config file** | none |
| **Quick run command** | `cargo test -p filimon` |
| **Full suite command** | `cargo test -p filimon && cargo clippy -p filimon` (never `--workspace`: pre-existing `map-preprocess::tests::test_lemmatize` failure, untouchable under D-01) |
| **Cluster tests** | `cargo test -p filimon -- --ignored` (needs local Fluvio cluster; scratch topics only) |
| **Estimated runtime** | ~6 seconds (unit); cluster tests longer |

---

## Sampling Rate

- **After every task commit:** Run `cargo test -p filimon`
- **After every plan wave:** Run `cargo test -p filimon && cargo clippy -p filimon` (plus `-- --ignored` when the cluster is up)
- **Before `/gsd-verify-work`:** Full suite must be green, plus manual SC2/SC3/SC4 smoke on the local cluster
- **Max feedback latency:** 60 seconds

---

## Per-Task Verification Map

Task IDs are bound to the PLAN.md files (02-01 through 02-06). Every task's full `<automated>` gate and its `<fails_when>` statement live in its plan; the commands below are the primary test filter for each row.

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 02-01-T1 | 02-01 | 1 | PIPE-01/02 | T-02-01 | news JSON never carries `html: null` (poison record) | unit | `cargo test -p filimon models::tests::news_item_is_rawpage_compatible` | ✅ | ✅ green |
| 02-01-T1 | 02-01 | 1 | PIPE-02 | — | N/A | unit | `cargo test -p filimon streaming::tests::news_chain_matches_transforms_yaml` | ✅ | ✅ green |
| 02-01-T1 | 02-01 | 1 | PIPE-02 | T-02-02 | stream errors never print the record value | unit | `cargo test -p filimon commands::consume::tests::stream_error_omits_record_value` | ✅ | ✅ green |
| 02-01-T1 | 02-01 | 1 | PIPE-02 | T-02-01 | N/A | integration (cluster) | `cargo test -p filimon -- --ignored news_through_chain` | ✅ | ✅ green |
| 02-01-T2 | 02-01 | 1 | PIPE-01 | — | N/A | unit | `cargo test -p filimon models::tests::source_item_round_trip` | ✅ | ✅ green |
| 02-01-T2 | 02-01 | 1 | PIPE-01 | — | N/A | unit | `cargo test -p filimon models::tests::source_item_matches_wit_item` | ✅ | ✅ green |
| 02-01-T2 | 02-01 | 1 | PIPE-01 | T-02-SC | Cargo.lock package set unchanged | unit | `cargo test -p filimon models::` | ✅ | ✅ green |
| 02-01-T3 | 02-01 | 1 | PIPE-02 | T-02-04 | control chars stripped from summaries | unit + live CLI | `cargo test -p filimon output::` then the `fili file` → `fili consume` smoke in 02-01-PLAN.md | ✅ | ✅ green |
| 02-02-T1 | 02-02 | 2 | PIPE-03 | T-02-06, T-02-07 | label by topic of origin; no silent drops | unit | `cargo test -p filimon commands::consume::tests::` | ✅ | ✅ green |
| 02-02-T1 | 02-02 | 2 | PIPE-03, D-05 | T-02-09 | missing topic gives a named error, no hang | integration (cluster) | `cargo test -p filimon -- --ignored` (social_passthrough, missing_topic_is_named, news_through_chain) | ✅ | ✅ green |
| 02-02-T2 | 02-02 | 2 | PIPE-04 | T-02-08 | `html` never in output; NER input capped at 500 chars | unit | `cargo test -p filimon commands::consume::tests::ner_runs_on_social_text` | ✅ | ✅ green |
| 02-02-T3 | 02-02 | 2 | PIPE-04 | — | N/A | unit + CLI help | `cargo test -p filimon cli::tests::` | ✅ | ✅ green |
| 02-03-T1 | 02-03 | 3 | PIPE-02/03/04, D-05 | T-02-12 | README commands proven by execution | live CLI (cluster) | README operator-path run in 02-03-PLAN.md Task 1 | n/a (live) | ✅ green |
| 02-03-T2 | 02-03 | 3 | D-03 | — | N/A | doc grep | grep gates in 02-03-PLAN.md Task 2 | n/a (docs) | ✅ green |
| 02-04-T1 | 02-04 | 3 | PIPE-03 | T-02-02 | social decode diagnostics carry no record value, ESC or backtick | unit | `cargo test -p filimon commands::consume::tests::decode_social_withholds_variant_value` | ✅ | ✅ green |
| 02-04-T1 | 02-04 | 3 | PIPE-02/03 | T-02-02, T-02-04 | every diagnostic printed via `print_diagnostic` → `sanitize_for_terminal` | unit + structural grep | `cargo test -p filimon commands::consume::tests::diagnostic_line_strips_control_chars` (+ eprintln grep gate in 02-04-PLAN.md) | ✅ | ✅ green |
| 02-04-T1 | 02-04 | 3 | PIPE-03 | T-02-02 | live social poison record → one content-free diagnostic | integration (cluster) | `cargo test -p filimon -- --ignored poison_social_record_is_reported_without_value` | ✅ | ✅ green |
| 02-04-T2 | 02-04 | 3 | PIPE-02 | T-02-02 | news decode diagnostics built from serde class/line/column only | unit | `cargo test -p filimon commands::consume::tests::decode_news_withholds_escaped_quote_content` | ✅ | ✅ green |
| 02-04-T2 | 02-04 | 3 | PIPE-02 | T-02-02 | SmartModule hint never copied: described from a closed allow-list of module prefixes and serde phrases (tightened by review fix WR-05); other stream-error text cut before first quote/backtick/control char | unit | `cargo test -p filimon commands::consume::tests::stream_error_hint_withholds_quoted_value` + `other_stream_errors_withhold_record_value` | ✅ | ✅ green |
| 02-04-T2 | 02-04 | 3 | PIPE-01/02/03 | — | edge: 200-char cut by scalar values; empty/malformed values reported | unit | `cargo test -p filimon commands::consume::tests::hint_detail_truncates_by_chars` + `empty_and_malformed_values_are_reported` | ✅ | ✅ green |
| 02-04-T2 | 02-04 | 3 | PIPE-02 | T-02-02 | live news poison record through real map-preprocess hint | integration (cluster) | `cargo test -p filimon -- --ignored poison_news_record_is_reported_without_value` | ✅ | ✅ green |
| 02-05-T1 | 02-05 | 4 | PIPE-02/03 | T-02-02 | past `MAX_DIAGNOSTICS`, every skipped record is named by topic and exact offset range; no record content kept | unit | `cargo test -p filimon commands::consume::tests::undecodable_flood_stops_at_diagnostic_budget` + `offset_ranges_are_exact` + `cap_boundary_moves_only_later_offsets_into_ranges` | ✅ | ✅ green |
| 02-05-T1 | 02-05 | 4 | PIPE-03 | T-02-02 | live social flood names every offset 0..=149 exactly once, content-free | integration (cluster) | `cargo test -p filimon -- --ignored undecodable_social_flood_names_every_offset` | ✅ | ✅ green |
| 02-05-T2 | 02-05 | 4 | PIPE-02/03 | T-02-02 | every skip path located; notes kept apart; no skipped range covers a written record | unit | `cargo test -p filimon commands::consume::tests::located_diagnostics_match_their_message` + `unshown_ranges_never_cover_written_records` + `fatal_diagnostic_is_kept_past_the_cap` | ✅ | ✅ green |
| 02-06-T1 | 02-06 | 5 | PIPE-01/03 | — | social `published` outside UTC year 0000..=9999 becomes null plus one located note | unit | `cargo test -p filimon commands::consume::tests::social_published_outside_year_range_becomes_null` + `models::tests::rfc3339_to_utc_normalizes_only_rfc3339` + `models::tests::chrono_writes_out_of_range_years_with_sign` | ✅ | ✅ green |
| 02-06-T2 | 02-06 | 5 | PIPE-01/02 | — | news and epoch `published` outside the year range become null; `published_raw` keeps the original | unit | `cargo test -p filimon models::tests::from_article_drops_out_of_range_published` + `epoch_ms_to_rfc3339_converts` + `parse_news_published_best_effort` | ✅ | ✅ green |

Post-review fixes WR-01..WR-06 (02-REVIEW-FIX.md; commits 308e775, 9382d43, ec72c90, 4c2845e, fc99d4e, 7330aa6) are not plan tasks, so they add no rows; their 18 new tests (e.g. news_branch_reopens_past_rejected_record, failed_batch_with_no_records_keeps_previous_output, enrich_ner_is_rejected_in_streaming_mode, rfc3339_to_utc_normalizes_only_rfc3339, module_hint_prefixes_match_smartmodule_sources, undecodable_flood_stops_at_diagnostic_budget, cluster good_news_after_poison_record_is_delivered) run in the same `cargo test -p filimon` sample.

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [x] `examples/fixtures/news-article.html` — HTML fixture with news terms so relevance ≥ 0.05 (no sample HTML exists today)
- [x] Topic names passed as parameters into the consume core (e.g. `ConsumeTopics { raw_pages, social }`) so `#[ignore]` tests use scratch topics created/deleted via `admin.create`/`admin.delete::<TopicSpec>`
- [x] Generic merge/enrich loop over `Stream<Item = Incoming>` plus a detector abstraction, so unit tests run without Fluvio or the GLiNER model

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Live NER over both kinds into one JSON array | PIPE-04 / SC4 | GLiNER model files absent locally; `--enrich-ner` silently falls back to no NER | `<human-check>` in 02-02-PLAN.md Task 2: download model, produce the `t3_ner` social sample, then `fili consume --from-beginning --enrich-ner --limit 500 --output /tmp/fili-02-sc4.json`; confirm one JSON array with news + social records, `source_type`/`platform` labels, non-empty entities |
| Live crawl through `fili produce` | PIPE-02 / SC2 | Depends on network and a live third-party site | `<human-check>` in 02-01-PLAN.md Task 3: `fili produce https://www.bbc.co.uk/news --depth 2`, then consume; records labelled `platform` `bbc.co.uk` |

The `fili file` news smoke (SC2) and the README operator path (SC2/SC3/D-05) are automated on the local cluster in 02-01-T3 and 02-03-T1.

---

## Validation Sign-Off

- [x] All tasks have `<automated>` verify or Wave 0 dependencies
- [x] Sampling continuity: no 3 consecutive tasks without automated verify
- [x] Wave 0 covers all MISSING references
- [x] No watch-mode flags
- [x] Feedback latency < 60s
- [x] `nyquist_compliant: true` set in frontmatter

**Approval:** validated 2026-11-04 (re-audit after gap-closure plans 02-05 and 02-06: 132 unit + 7 cluster tests green on 5caa89b; 2 live checks remain manual-only)

## Validation Audit 2026-11-03

| Metric | Count |
|---|---|
| Gaps found | 0 |
| Resolved | 0 |
| Escalated | 0 |
