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
> Register source: `<threat_model>` blocks in 02-01, 02-02 and 02-03 PLAN.md (authored at plan time). No `## Threat Flags` were raised in any SUMMARY.md.

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
| T-02-01 | Denial of Service | SourceItem serialization; news stream in consume.rs | medium | mitigate | `html` has `skip_serializing_if` (src/models.rs:136); `RetryMode::Disabled` (src/streaming.rs:141); tests news_item_is_rawpage_compatible, stream_error_omits_record_value pass | closed |
| T-02-02 | Information Disclosure | `describe_stream_error`, `decode_news` | medium | mitigate | record value never printed; serde-quoted text scrubbed (02-01 deviation 1); tests stream_error_omits_record_value, decode_news_reports_bad_value_without_content pass | closed |
| T-02-03 | Information Disclosure | `OutputRecord` / JSON output | low | mitigate | `OutputRecord` has no html field (0 matches in struct); test save_json_writes_array_without_html passes; live smokes (02-01, 02-03) show no `html` key | closed |
| T-02-04 | Tampering | `print_summary` (operator terminal) | low | mitigate | `sanitize_for_terminal` (src/output.rs:31) on every record-derived string; test sanitize_strips_control_chars passes | closed |
| T-02-05 | Denial of Service | consumer start-up, missing topic/SmartModule | medium | mitigate | `FluvioStream::preflight` (src/streaming.rs:155); cluster test missing_topic_is_named passes | closed |
| T-02-SC (02-01) | Tampering | cargo dependency `chrono` | high | mitigate | `git diff 0ed7d3f -- Cargo.lock` shows no package added/removed; chrono stays 0.4.45 | closed |
| T-02-06 | Spoofing | `decode_social` | medium | mitigate | labelled by topic of origin; test social_label_comes_from_topic passes | closed |
| T-02-07 | Repudiation | social decode path | low | mitigate | undeserializable record → stderr diagnostic without value; test undeserializable_social_record_is_reported passes | closed |
| T-02-08 | Denial of Service | `ner_input` / NER engine | low | mitigate | `NER_INPUT_CHARS = 500` (src/commands/consume.rs:251); test ner_input_truncates_by_chars passes | closed |
| T-02-09 | Denial of Service | consume start-up, social topic missing | low | mitigate | preflight names topic + create command; cluster test missing_topic_is_named passes; 02-03 live run showed the error before topic creation | closed |
| T-02-10 | Tampering | `print_summary` on social text | low | mitigate | social records go through `sanitize_for_terminal` | closed |
| T-02-11 | Tampering | `raw` JSON nesting | low | accept | serde_json recursion limit 128 — see Accepted Risks | closed |
| T-02-SC (02-02) | Tampering | cargo installs | high | accept | no new crate (dev-only tokio `test-util` feature on an existing dependency; Cargo.lock unchanged) — see Accepted Risks | closed |
| T-02-12 | Tampering | README operator commands | low | mitigate | 02-03 Task 1 executed README's own commands live on the local cluster (passed twice) | closed |
| T-02-13 | Information Disclosure | README sample records | low | accept | synthetic samples, no secrets — see Accepted Risks | closed |
| T-02-14 | Denial of Service | real topics on the dev cluster | low | accept | two small records per run, no deletion — see Accepted Risks | closed |
| T-02-SC (02-03) | Tampering | package installs | high | accept | plan installs no packages — see Accepted Risks | closed |

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

*Accepted risks do not resurface in future audit runs.*

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-11-03 | 17 | 17 | 0 | execute-phase verify:post (secure-phase, L1 grep depth; auditor skipped per short-circuit rule) |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-11-03
