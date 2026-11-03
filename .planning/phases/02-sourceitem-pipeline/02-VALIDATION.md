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

Requirement-level map from research; task IDs are bound by the PLAN.md files.

| Task ID | Plan | Wave | Requirement | Threat Ref | Secure Behavior | Test Type | Automated Command | File Exists | Status |
|---------|------|------|-------------|------------|-----------------|-----------|-------------------|-------------|--------|
| TBD | TBD | TBD | PIPE-01 | — | N/A | unit | `cargo test -p filimon models::tests::source_item_round_trip` | ❌ W0 | ⬜ pending |
| TBD | TBD | TBD | PIPE-01 | — | N/A | unit | `cargo test -p filimon models::tests::source_item_matches_wit_item` | ❌ W0 | ⬜ pending |
| TBD | TBD | TBD | PIPE-01/02 | — | news JSON has no `html: null` (poison record) | unit | `cargo test -p filimon models::tests::news_item_is_rawpage_compatible` | ❌ W0 | ⬜ pending |
| TBD | TBD | TBD | PIPE-01 | — | N/A | unit | `cargo test -p filimon models::tests::` | ❌ W0 | ⬜ pending |
| TBD | TBD | TBD | PIPE-02 | — | N/A | unit | `cargo test -p filimon streaming::tests::news_chain_matches_transforms_yaml` | ❌ W0 | ⬜ pending |
| TBD | TBD | TBD | PIPE-02 | — | N/A | integration (cluster) | `cargo test -p filimon -- --ignored news_through_chain` | ❌ W0 | ⬜ pending |
| TBD | TBD | TBD | PIPE-03 | — | N/A | unit | `cargo test -p filimon commands::consume::tests::` | ❌ W0 | ⬜ pending |
| TBD | TBD | TBD | PIPE-03 | — | N/A | integration (cluster) | `cargo test -p filimon -- --ignored social_passthrough` | ❌ W0 | ⬜ pending |
| TBD | TBD | TBD | PIPE-04 | — | `html` never in output | unit | `cargo test -p filimon commands::consume::tests::ner_runs_on_social_text` | ❌ W0 | ⬜ pending |
| TBD | TBD | TBD | D-05 | — | missing topic → named error, no hang | integration (cluster) | `cargo test -p filimon -- --ignored` | ❌ W0 | ⬜ pending |

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
| Live NER over both kinds into one JSON array | PIPE-04 / SC4 | GLiNER model files absent locally; `--enrich-ner` silently falls back to no NER | Download model, then `fili consume --enrich-ner --limit N`; confirm one JSON array with news + social records, `source_type`/`platform` labels, non-empty entities |
| News path end-to-end smoke | PIPE-02 / SC2 | Needs running cluster + hub SmartModules | `fili file examples/fixtures/news-article.html` then `fili consume --from-beginning --limit 1`; confirm extracted article |

---

## Validation Sign-Off

- [ ] All tasks have `<automated>` verify or Wave 0 dependencies
- [ ] Sampling continuity: no 3 consecutive tasks without automated verify
- [ ] Wave 0 covers all MISSING references
- [ ] No watch-mode flags
- [ ] Feedback latency < 60s
- [ ] `nyquist_compliant: true` set in frontmatter

**Approval:** pending
