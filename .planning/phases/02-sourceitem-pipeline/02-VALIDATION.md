---
phase: "2"
slug: "sourceitem-pipeline"
# status lifecycle: draft (seeded by plan-phase) → validated (set by validate-phase §6)
# audit-milestone §5.5 distinguishes NOT-VALIDATED (draft) from PARTIAL (validated + nyquist_compliant: false) (#2117)
status: draft
nyquist_compliant: false
wave_0_complete: false
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

Task IDs are bound to the PLAN.md files (02-01, 02-02, 02-03). Every task's full `<automated>` gate and its `<fails_when>` statement live in its plan; the commands below are the primary test filter for each row.

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| 02-01-T1 | 02-01 | 1 | PIPE-01/02 | T-02-01 | news JSON never carries `html: null` (poison record) | unit | `cargo test -p filimon models::tests::news_item_is_rawpage_compatible` | ❌ W0 (created by 02-01-T1) | ⬜ pending |
| 02-01-T1 | 02-01 | 1 | PIPE-02 | — | N/A | unit | `cargo test -p filimon streaming::tests::news_chain_matches_transforms_yaml` | ❌ W0 (created by 02-01-T1) | ⬜ pending |
| 02-01-T1 | 02-01 | 1 | PIPE-02 | T-02-02 | stream errors never print the record value | unit | `cargo test -p filimon commands::consume::tests::stream_error_omits_record_value` | ❌ W0 (created by 02-01-T1) | ⬜ pending |
| 02-01-T1 | 02-01 | 1 | PIPE-02 | T-02-01 | N/A | integration (cluster) | `cargo test -p filimon -- --ignored news_through_chain` | ❌ W0 (created by 02-01-T1) | ⬜ pending |
| 02-01-T2 | 02-01 | 1 | PIPE-01 | — | N/A | unit | `cargo test -p filimon models::tests::source_item_round_trip` | ❌ W0 (created by 02-01-T2) | ⬜ pending |
| 02-01-T2 | 02-01 | 1 | PIPE-01 | — | N/A | unit | `cargo test -p filimon models::tests::source_item_matches_wit_item` | ❌ W0 (created by 02-01-T2) | ⬜ pending |
| 02-01-T2 | 02-01 | 1 | PIPE-01 | T-02-SC | Cargo.lock package set unchanged | unit | `cargo test -p filimon models::` | ❌ W0 (created by 02-01-T2) | ⬜ pending |
| 02-01-T3 | 02-01 | 1 | PIPE-02 | T-02-04 | control chars stripped from summaries | unit + live CLI | `cargo test -p filimon output::` then the `fili file` → `fili consume` smoke in 02-01-PLAN.md | ❌ W0 (created by 02-01-T3) | ⬜ pending |
| 02-02-T1 | 02-02 | 2 | PIPE-03 | T-02-06, T-02-07 | label by topic of origin; no silent drops | unit | `cargo test -p filimon commands::consume::tests::` | ❌ W0 (created by 02-02-T1) | ⬜ pending |
| 02-02-T1 | 02-02 | 2 | PIPE-03, D-05 | T-02-09 | missing topic gives a named error, no hang | integration (cluster) | `cargo test -p filimon -- --ignored` (social_passthrough, missing_topic_is_named, news_through_chain) | ❌ W0 (created by 02-02-T1) | ⬜ pending |
| 02-02-T2 | 02-02 | 2 | PIPE-04 | T-02-08 | `html` never in output; NER input capped at 500 chars | unit | `cargo test -p filimon commands::consume::tests::ner_runs_on_social_text` | ❌ W0 (created by 02-02-T2) | ⬜ pending |
| 02-02-T3 | 02-02 | 2 | PIPE-04 | — | N/A | unit + CLI help | `cargo test -p filimon cli::tests::` | ❌ W0 (created by 02-02-T3) | ⬜ pending |
| 02-03-T1 | 02-03 | 3 | PIPE-02/03/04, D-05 | T-02-12 | README commands proven by execution | live CLI (cluster) | README operator-path run in 02-03-PLAN.md Task 1 | n/a (live) | ⬜ pending |
| 02-03-T2 | 02-03 | 3 | D-03 | — | N/A | doc grep | grep gates in 02-03-PLAN.md Task 2 | n/a (docs) | ⬜ pending |

*Status: ⬜ pending · ✅ green · ❌ red · ⚠️ flaky*

---

## Wave 0 Requirements

- [ ] `examples/fixtures/news-article.html` — HTML fixture with news terms so relevance ≥ 0.05 (no sample HTML exists today)
- [ ] Topic names passed as parameters into the consume core (e.g. `ConsumeTopics { raw_pages, social }`) so `#[ignore]` tests use scratch topics created/deleted via `admin.create`/`admin.delete::<TopicSpec>`
- [ ] Generic merge/enrich loop over `Stream<Item = Incoming>` plus a detector abstraction, so unit tests run without Fluvio or the GLiNER model

---

## Manual-Only Verifications

| Behavior | Requirement | Why Manual | Test Instructions |
|----------|-------------|------------|-------------------|
| Live NER over both kinds into one JSON array | PIPE-04 / SC4 | GLiNER model files absent locally; `--enrich-ner` silently falls back to no NER | `<human-check>` in 02-02-PLAN.md Task 2: download model, produce the `t3_ner` social sample, then `fili consume --from-beginning --enrich-ner --limit 500 --output /tmp/fili-02-sc4.json`; confirm one JSON array with news + social records, `source_type`/`platform` labels, non-empty entities |
| Live crawl through `fili produce` | PIPE-02 / SC2 | Depends on network and a live third-party site | `<human-check>` in 02-01-PLAN.md Task 3: `fili produce https://www.bbc.co.uk/news --depth 2`, then consume; records labelled `platform` `bbc.co.uk` |

The `fili file` news smoke (SC2) and the README operator path (SC2/SC3/D-05) are automated on the local cluster in 02-01-T3 and 02-03-T1.

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
