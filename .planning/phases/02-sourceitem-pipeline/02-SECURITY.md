---
phase: "2"
slug: "sourceitem-pipeline"
status: verified
# threats_open = count of OPEN threats at or above workflow.security_block_on severity (the blocking gate)
threats_open: 0
asvs_level: 1
created: "2026-11-03"
---

# Phase 2 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.
> Register source: `<threat_model>` blocks in 02-01 through 02-06 PLAN.md (authored at plan time). No `## Threat Flags` were raised in any SUMMARY.md.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| any Fluvio producer → `filimon-raw-pages` → `fili consume` | Any client with cluster access can write records that the SmartModule chain and the consumer parse | untrusted page HTML / SourceItem JSON |
| any Fluvio producer → `filimon-social-items` → `fili consume` | Social records reach the consumer with no SmartModule in between | untrusted SourceItem JSON (text, raw) |
| consumer → operator terminal | Record text printed in summaries | untrusted text (control / ANSI sequences) |
| README → operator shell | Operators paste README commands | commands / sample records |
| live run → local dev cluster | Plan 02-03 creates `filimon-social-items` and writes one record per real topic | synthetic records |

---

## Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation | Status |
|-----------|----------|-----------|----------|-------------|------------|--------|
| T-02-01 | Denial of Service | SourceItem serialization; news stream in consume.rs | medium | mitigate | `html` has `skip_serializing_if` (src/models.rs:136); `RetryMode::Disabled` (src/streaming.rs:141); news-branch reopen past a rejected record bounded by `MAX_REOPENS = 100` (consume.rs:263, review fix WR-01, cluster test good_news_after_poison_record_is_delivered); tests news_item_is_rawpage_compatible, stream_error_omits_record_value pass | closed |
| T-02-02 | Information Disclosure | `describe_stream_error`, `decode_news` | medium | mitigate | Closed by gap plan 02-04 (CR-01): diagnostics come from `describe_json_error` (consume.rs:104, serde `classify()`/line/column only; `redact_quoted` deleted); SmartModule hints described only from a closed allow-list by `module_hint_summary` (consume.rs:155, applied at 251; review fix WR-05) and other stream-error text reduced by `hint_summary` (consume.rs:174); every diagnostic printed via `print_diagnostic` → `output::sanitize_for_terminal` (consume.rs:741-746), no other non-comment `eprintln!`; tests decode_news_withholds_escaped_quote_content, decode_social_withholds_variant_value, stream_error_hint_withholds_quoted_value, diagnostic_line_strips_control_chars and cluster poison_news/poison_social pass | closed |
| T-02-03 | Information Disclosure | `OutputRecord` / JSON output | low | mitigate | `OutputRecord` has no html field (0 matches in struct); test save_json_writes_array_without_html passes; live smokes (02-01, 02-03) show no `html` key | closed |
| T-02-04 | Tampering | `print_summary` (operator terminal) | low | mitigate | `sanitize_for_terminal` (src/output.rs:49) on every record-derived string; test sanitize_strips_control_chars passes | closed |
| T-02-05 | Denial of Service | consumer start-up, missing topic/SmartModule | medium | mitigate | `FluvioStream::preflight` (src/streaming.rs:155); cluster test missing_topic_is_named passes | closed |
| T-02-SC (02-01) | Tampering | cargo dependency `chrono` | high | mitigate | `git diff 0ed7d3f -- Cargo.lock` shows no package added/removed; chrono stays 0.4.45 | closed |
| T-02-06 | Spoofing | `decode_social` | medium | mitigate | labelled by topic of origin; test social_label_comes_from_topic passes | closed |
| T-02-07 | Repudiation | social decode path | low | mitigate | undeserializable record → stderr diagnostic without value; test undeserializable_social_record_is_reported passes | closed |
| T-02-08 | Denial of Service | `ner_input` / NER engine | low | mitigate | `NER_INPUT_CHARS = 500` (src/commands/consume.rs:464); test ner_input_truncates_by_chars passes | closed |
| T-02-09 | Denial of Service | consume start-up, social topic missing | low | mitigate | preflight names topic + create command; cluster test missing_topic_is_named passes; 02-03 live run showed the error before topic creation | closed |
| T-02-10 | Tampering | `print_summary` on social text | low | mitigate | social records go through `sanitize_for_terminal` | closed |
| T-02-11 | Tampering | `raw` JSON nesting | low | accept | serde_json recursion limit 128 — see Accepted Risks | closed |
| T-02-SC (02-02) | Tampering | cargo installs | high | accept | no new crate (dev-only tokio `test-util` feature on an existing dependency; Cargo.lock unchanged) — see Accepted Risks | closed |
| T-02-12 | Tampering | README operator commands | low | mitigate | 02-03 Task 1 executed README's own commands live on the local cluster (passed twice) | closed |
| T-02-13 | Information Disclosure | README sample records | low | accept | synthetic samples, no secrets — see Accepted Risks | closed |
| T-02-14 | Denial of Service | real topics on the dev cluster | low | accept | two small records per run, no deletion — see Accepted Risks | closed |
| T-02-SC (02-03) | Tampering | package installs | high | accept | plan installs no packages — see Accepted Risks | closed |
| T-02-15 | Information Disclosure | `decode_news` / `decode_social` parse diagnostics | medium | mitigate | `describe_json_error` (consume.rs:104) used at consume.rs:196,215; `fn redact_quoted` 0 matches; tests decode_news_withholds_escaped_quote_content, decode_social_withholds_variant_value, empty_and_malformed_values_are_reported pass | closed |
| T-02-16 | Information Disclosure | `describe_stream_error` (SmartModule hint, other ErrorCode text) | medium | mitigate | SmartModule hints: `module_hint_summary` (consume.rs:155, closed allow-list of module prefixes and serde phrases since review fix WR-05; applied at consume.rs:251,255); other stream-error text: `hint_summary` + `NO_DETAIL` (consume.rs:116,174); tests stream_error_hint_withholds_quoted_value, other_stream_errors_withhold_record_value, hint_detail_truncates_by_chars, module_hint_prints_only_listed_phrases, module_hint_prefixes_match_smartmodule_sources, cluster poison_news_record_is_reported_without_value pass | closed |
| T-02-17 | Tampering | consume stderr (terminal escape injection) | medium | mitigate | `diagnostic_line` → `output::sanitize_for_terminal(message)` (consume.rs:741-742), `print_diagnostic` is the only non-comment `eprintln!` (0 others); tests diagnostic_line_strips_control_chars, cluster poison_social_record_is_reported_without_value pass | closed |
| T-02-18 | Repudiation | decode and stream paths (record accounting) | low | mitigate | zero-byte / invalid-UTF-8 / empty-variant records each give one topic+offset diagnostic; empty_and_malformed_values_are_reported and existing no-silent-drop tests pass | closed |
| T-02-19 | Spoofing | Unicode format characters in terminal lines (review IN-05) | low | accept | diagnostics carry no record text after 02-04; Cf characters in ids/summaries remain open finding IN-05 — see Accepted Risks | closed |
| T-02-SC (02-04) | Tampering | package installs | high | accept | no packages installed; Cargo.toml/Cargo.lock unchanged eb989bb..HEAD — see Accepted Risks | closed |
| T-02-20 | Repudiation | `Collected` past `MAX_DIAGNOSTICS` (gap 2, plan 02-05) | medium | mitigate | located `RecordDiagnostic` (consume.rs:57); `Collected::skip`/`note` keep offsets in per-topic `OffsetRanges` (consume.rs:164-175, `skipped.add` at 168); `consume_batch` prints `unshown_lines` via `print_diagnostic` (consume.rs:830); tests undecodable_flood_stops_at_diagnostic_budget, unshown_ranges_never_cover_written_records, located_diagnostics_match_their_message, cluster undecodable_social_flood_names_every_offset pass | closed |
| T-02-21 | Denial of Service | `Collected.unshown` memory under a decode flood | low | mitigate | `OffsetRanges` holds only `ranges: Vec<(i64, i64)>` and `count` (consume.rs:80-83); `diagnostic_budget` byte-identical to 98e70f6; `MAX_DIAGNOSTICS = 100` (consume.rs:152) | closed |
| T-02-22 | Information Disclosure | range lines (`unshown_lines`) | medium | mitigate | `unshown_lines` takes only `&BTreeMap<String, Unshown>` (consume.rs:874), offsets and counts only; only non-comment `eprintln!` is inside `print_diagnostic` (consume.rs:900); cluster test asserts no `PAGE-BODY` | closed |
| T-02-23 | Denial of Service | long stderr range line with scattered offsets | low | accept | bounded by `diagnostic_budget(limit)`; printed through the sanitizing choke point — see Accepted Risks | closed |
| T-02-SC (02-05) | Tampering | package installs | high | accept | no packages installed; Cargo.toml/Cargo.lock unchanged f1f93fc..HEAD — see Accepted Risks | closed |
| T-02-24 | Tampering | `published` in JSON output (plan 02-06) | low | mitigate | one guarded formatter `utc_rfc3339` (models.rs:199) gives None outside UTC year 0..=9999; the only non-test `to_rfc3339_opts` is inside it (models.rs:203; tests module starts at 344); tests rfc3339_to_utc_normalizes_only_rfc3339, parse_news_published_best_effort, epoch_ms_to_rfc3339_converts, chrono_writes_out_of_range_years_with_sign pass | closed |
| T-02-25 | Repudiation | `decode_social` published note | low | mitigate | out-of-range `published` keeps the record with null plus one located note (consume.rs:343); test social_published_outside_year_range_becomes_null passes | closed |
| T-02-26 | Information Disclosure | `decode_social` published note text | low | mitigate | note is fixed text plus topic and offset only (consume.rs:343); test asserts the limit values are absent | closed |
| T-02-SC (02-06) | Tampering | package installs | high | accept | no packages installed (`chrono::Datelike` from existing chrono 0.4.45); Cargo.toml/Cargo.lock unchanged — see Accepted Risks | closed |

*Status: open · closed · open — below high threshold (non-blocking)*
*Severity: critical > high > medium > low — only open threats at or above workflow.security_block_on count toward threats_open*
*Disposition: mitigate (implementation required) · accept (documented risk) · transfer (third-party)*

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-02-01 | T-02-11 | serde_json's default recursion limit (128) bounds `raw` nesting; adequate at ASVS L1 | plan 02-02 threat model | 2026-11-03 |
| AR-02-02 | T-02-SC (02-02) | No package installed; only a dev-only feature flag on the existing tokio dependency | plan 02-02 threat model | 2026-11-03 |
| AR-02-03 | T-02-13 | README samples are synthetic (example.com, made-up reddit id) | plan 02-03 threat model | 2026-11-03 |
| AR-02-04 | T-02-14 | Live run appends two small records per run to the dev cluster; retired `filimon-articles` left untouched | plan 02-03 threat model | 2026-11-03 |
| AR-02-05 | T-02-SC (02-03) | Docs-only plan; no package installs | plan 02-03 threat model | 2026-11-03 |
| AR-02-06 | T-02-19 | Diagnostics no longer carry record text; residual Unicode format characters (Cf) in ids/summaries tracked as review finding IN-05, out of scope for the gap | plan 02-04 threat model | 2026-11-04 |
| AR-02-07 | T-02-SC (02-04) | No package installed; serde_json and fluvio are existing dependencies, Cargo.lock unchanged | plan 02-04 threat model | 2026-11-04 |
| AR-02-08 | T-02-23 | Range line length bounded by `diagnostic_budget(limit)` (≤ 400 entries at default `--limit 50`), operator-chosen limit, sanitized output | plan 02-05 threat model | 2026-11-04 |
| AR-02-09 | T-02-SC (02-05) | No package installed; `BTreeMap` is std, Cargo.toml/Cargo.lock unchanged | plan 02-05 threat model | 2026-11-04 |
| AR-02-10 | T-02-SC (02-06) | No package installed; `chrono::Datelike` from existing chrono 0.4.45 | plan 02-06 threat model | 2026-11-04 |

*Accepted risks do not resurface in future audit runs.*

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-11-03 | 17 | 17 | 0 | execute-phase verify:post (secure-phase, L1 grep depth; auditor skipped per short-circuit rule) |
| 2026-11-03 | 17 | 16 | 1 (medium, non-blocking) | reopened T-02-02 after code review CR-01 |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-11-04 (re-audit after gap plans 02-05 and 02-06: 9 new register entries T-02-20..T-02-26 plus two T-02-SC verified at L1 grep depth on 5673a3e; all 32 entries closed)

## Security Audit 2026-11-03

| Metric | Count |
|---|---|
| Threats found | 23 |
| Closed | 23 |
| Open | 0 |

## Security Audit 2026-11-04

| Metric | Count |
|---|---|
| Threats found | 32 |
| Closed | 32 |
| Open | 0 |
