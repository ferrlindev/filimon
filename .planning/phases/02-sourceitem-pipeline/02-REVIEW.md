---
phase: 02-sourceitem-pipeline
reviewed: 2026-11-03T22:01:23Z
depth: standard
files_reviewed: 1
files_reviewed_list:
  - src/commands/consume.rs
findings:
  critical: 0
  warning: 6
  info: 3
  total: 9
  necessity: 0
status: issues_found
---

# Phase 2: Code Review Report (incremental re-review after 02-04)

**Reviewed:** 2026-11-03T22:01:23Z
**Depth:** standard
**Files Reviewed:** 1
**Status:** issues_found

## Summary

Scope: `src/commands/consume.rs` only, diff `7b012a5..HEAD` (HEAD `66d515e`). Gap-closure plan 02-04 changed only this file. The earlier full review covered 9 files. Its findings in other files (IN-01 to IN-08, and the `main.rs`/`models.rs`/`output.rs` parts of WR-03 and WR-04) were not re-reviewed here and stay as recorded.

Evidence:
- `cargo test -p filimon --offline`: 109 passed, 0 failed, 5 ignored. The two new cluster tests (`poison_*_record_is_reported_without_value`) are `#[ignore]` and were **not** run.
- `cargo clippy -p filimon --all-targets`: only a pedantic `unreadable_literal` on test line 770 in this file.
- Read the fluvio-protocol 0.50.1 `ErrorCode` Display strings and `SmartModuleTransformRuntimeError` (the hint is the module's eyre report formatted with `{:?}`).
- Read every error construction in `smartmodules/*/src/lib.rs`: map-preprocess:172 and :182, map-extract:386 and :416, filter-relevance:43 (bare `?`). Their input structs (`RawPage`, `PreprocessedBundle`, `ExtractedArticle`) use only derived `Deserialize` with no custom error text.

### Prior findings in this file

| Prior ID | Status | Evidence |
|---|---|---|
| CR-01 (1): raw control bytes in diagnostics | **Resolved** | All three print sites now go through `print_diagnostic` → `diagnostic_line` → `output::sanitize_for_terminal` (lines 486-492, 504-512, 527). Test `diagnostic_line_strips_control_chars` covers this. |
| CR-01 (2): `redact_quoted` defeated by escaped quotes | **Resolved** | `redact_quoted` was deleted. `decode_news` and `decode_social` now use `describe_json_error`, which reads only `classify()`, `line()` and `column()` and never the Display text (lines 76-85, 122-125, 140-143). The escaped-quote reproduction is now a test case (lines 672-703). |
| CR-01 (3): SmartModule hint passed through raw | **Resolved for the current modules** | `hint_summary` cuts at the first `"`, `` ` `` or Cc character (lines 102-112). The current modules emit only `<fixed prefix>: <serde text>`, so the kept text is module-authored. The guarantee depends on an unenforced convention in other crates; see WR-05. Unicode format characters still pass; see IN-02. |
| WR-01: one poison record halts the news branch | **Open** (unchanged) | Lines 185-203, 209-229 |
| WR-02: success exit and output overwrite on stream failure | **Open** (unchanged) | Lines 481-501, 521-530 |
| WR-03: `--enrich-ner` ignored in streaming mode | **Open** (unchanged in this file) | Lines 446-456 |
| WR-04: social `published` not normalized to UTC | **Open** (unchanged) | Lines 153-158 |

New in this pass: WR-05 (the hint guarantee rests on a convention), WR-06 (diagnostics do not bound a `--limit` run), IN-01 to IN-03.

## Narrative Findings (AI reviewer)

## Warnings

### WR-01: One poison record permanently halts the news branch (carried forward, open)

**File:** `src/commands/consume.rs:185-203` (also `src/streaming.rs:129-149`)
**Issue:** 02-04 did not change this. `news_incoming` sets `failed = true` on the first `ErrorCode` and ends the branch. The consumer uses `RetryMode::Disabled`. The new `poison_news_record_is_reported_without_value` cluster test confirms the behavior: a single string-valued record at offset 0 of `filimon-raw-pages` makes `--source news` collect nothing, and every later `--from-beginning` run stops at the same offset. `filimon-raw-pages` is an open topic.
**Fix:** As before. On `SmartModuleRuntimeError(e)`, emit the diagnostic and reopen at `Offset::absolute(e.offset + 1)`, with a bounded number of reopens. Alternatively, add a `--start-offset` flag. Add a cluster test that produces a poison record followed by a good one, and assert the good one is delivered.

### WR-02: Consume exits 0 and overwrites prior output when streams fail (carried forward, open)

**File:** `src/commands/consume.rs:481-501, 521-530`
**Issue:** 02-04 did not change this. A stream error is still only an `Incoming::Diagnostic`. `consume_batch` always calls `save_json` and returns `Ok(())`, so a failed run replaces the last good `wise_output.json` with `[]`. `consume_stream` returns `Ok(())` when both branches end on errors.
**Fix:** As before. Mark fatal stream errors separately (for example an `Incoming::Fatal` variant), return `Err` after printing, skip writing the file when a fatal error occurred and no records were collected, and make `save_json` write to a temp file and then rename it.

### WR-03: `--enrich-ner` silently ignored in streaming mode (carried forward, open)

**File:** `src/commands/consume.rs:446-456` (and `src/main.rs:73-78`)
**Issue:** 02-04 did not change this. `run` still receives `ner` and, when `limit == 0`, drops it without a warning.
**Fix:** Reject or warn on `ner.is_some() && limit == 0` at the top of `run`, or in `main` before the model loads.

### WR-04: Social `published` validated but not normalized to UTC (carried forward, open)

**File:** `src/commands/consume.rs:153-158`
**Issue:** 02-04 did not change this. A value such as `2024-05-01T10:00:00+01:00` passes `is_rfc3339` and is emitted as-is. This breaks the documented "RFC 3339 (UTC)" output contract that news records follow.
**Fix:** Parse with `DateTime::parse_from_rfc3339`, convert to `Utc`, and reformat with `SecondsFormat::Millis`. On a parse error, keep the existing null-plus-diagnostic path. Add a `+01:00` test case.

### WR-05: Content-freedom of stream diagnostics depends on an unenforced cross-crate message convention

**File:** `src/commands/consume.rs:90-112, 163-179`
**Issue:** `hint_summary` keeps all text before the first `"`, `` ` `` or Cc character. That text is content-free only because every current SmartModule error happens to be `eyre!("<fixed text>: {}", serde_err)` (map-preprocess:172 and :182, map-extract:386 and :416) or a bare serde error (filter-relevance:43). Nothing enforces this:
- A natural future edit such as `eyre!("Failed to parse page {}: {}", raw_page.url, e)` in map-extract, or any `eyre!("... {}", field)` placed before a quote, would put record content (a URL, a title, page text) into the operator diagnostic. Every consume.rs test would still pass, because the tests build hints by hand (lines 777, 788, 793).
- No test in `smartmodules/*` pins the error-message shape.
- The doc comment at lines 95-98 states as fact that "serde puts every record-controlled piece of its text after a double quote or a backtick". That is not quite true. `invalid length N, expected ...` prints the record-derived length bare. It is only a number, but the comment overstates the invariant that the security claim rests on.

The 02-SECURITY.md closure of T-02-04/T-02-10 now rests on this convention, not on a property of the code.
**Fix:** Do not let the hint decide what is printed. Choose the detail from a closed allowlist:
```rust
const KNOWN_HINT_PREFIXES: &[&str] = &[
    "Failed to parse RawPage",
    "Failed to parse PreprocessedBundle",
    "Failed to serialize bundle",
    "Failed to serialize article",
];
fn hint_summary(text: &str) -> String {
    KNOWN_HINT_PREFIXES
        .iter()
        .find(|p| text.starts_with(*p))
        .map_or_else(|| NO_DETAIL.to_string(), |p| (*p).to_string())
}
```
Alternatively, keep the current cut and add a unit test in each SmartModule crate that feeds a poison record and asserts that the error `Display` begins with a fixed literal prefix that is free of record content. Also correct the doc comment so it does not overstate serde's guarantee.

### WR-06: Undecodable records do not count toward `--limit`, so a batch run is unbounded

**File:** `src/commands/consume.rs:243-261, 481-488`
**Issue:** `collect_records` stops only when `limit` *records* arrive, the stream ends, or nothing arrives within `IDLE_TIMEOUT`. Decode diagnostics are pushed to `collected.diagnostics` without limit, and they are printed only after collection ends. The `filimon-social-items` topic has no chain and is fed by external plugins. If its producer writes undecodable values at least once every 5 s, `fili consume --limit 1 --source social` never returns, its memory grows with every bad record, and it prints nothing while it runs. With `--from-beginning`, a backlog of N bad records costs N reads and buffers N diagnostics before the run can end. The new cluster test comment (lines 1439-1440) records this "ends on the idle timeout" behavior as intended, but nothing bounds it. This was not flagged in the prior review, and 02-04 did not touch it.
**Fix:** Bound the work as well as the result. For example, cap retained diagnostics and count the rest:
```rust
const MAX_DIAGNOSTICS: usize = 100;
Ok(Some(Incoming::Diagnostic(m))) => {
    if collected.diagnostics.len() < MAX_DIAGNOSTICS { collected.diagnostics.push(m) }
    else { collected.suppressed += 1 }
    if collected.diagnostics.len() + collected.suppressed >= limit.saturating_mul(10).max(MAX_DIAGNOSTICS) { break; }
}
```
Also print a "N more diagnostics suppressed" line, or print diagnostics as they arrive in batch mode.

## Info

### IN-01: Local decode diagnostics drop safe, struct-authored detail; `hint_summary` over-truncates fluvio-authored errors

**File:** `src/commands/consume.rs:76-85, 177`
**Issue:** `describe_json_error` reduces every `Data` error to "wrong JSON shape". The likely cause of a news decode failure (host/module schema drift, for example `missing field `named_entities``) becomes undiagnosable. The design is also inconsistent. The prefix-before-quote/backtick cut is trusted for SmartModule hints, which contain the same serde text, but rejected for local serde errors, where it would keep content-free phrases such as `missing field`, `invalid type: string`, or `unknown variant`. In the other direction, `hint_summary(&other.to_string())` on fluvio's own `ErrorCode` Display cuts useful operator detail that is not record content. For example, `SmartModule is invalid: {error}` and `Other error: {0}` lose everything after the first quote.
**Fix:** In `describe_json_error`, append the content-free phrase, for example `format!("{class} ({}) at line {} column {}", hint_summary(&e.to_string()), e.line(), e.column())`. Remove the trailing ` at line ...` first, or accept the duplicate. For the `other` arm, consider printing the fluvio Display through `sanitize_for_terminal` only, since `ErrorCode` variants other than `SmartModuleRuntimeError` and `SmartModuleLookBackError` do not carry record values.

### IN-02: Unicode format characters (bidi overrides) pass the diagnostic choke point

**File:** `src/commands/consume.rs:104, 328, 505-507`
**Issue:** `diagnostic_line` uses `output::sanitize_for_terminal`, which removes only `char::is_control` (Cc), and `hint_summary` also cuts only at Cc. The NER diagnostic embeds the plugin-controlled `record.id` (line 328). An id containing U+202E or U+2066-U+2069 can visually reorder the rest of the stderr line. This is the prior IN-05 (`src/output.rs:31-39`), which now also applies to the diagnostic path that 02-04 calls the single sanitized place.
**Fix:** Fix IN-05 in `sanitize_for_terminal` by also dropping `'\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'`. Add a `diagnostic_line` test case with U+202E.

### IN-03: Offset assertions use single-digit `contains`, which can pass when the offset is missing

**File:** `src/commands/consume.rs:635, 662, 902, 994`
**Issue:** `message.contains('7')`, `contains('9')`, `contains('5')` and `contains('3')` also match digits in `line 1 column N` or other text. The tests would still pass if the offset were dropped from the diagnostic. Other new tests use the stricter `contains("offset 3")` form (lines 710, 717, 733).
**Fix:** Assert `message.contains("offset 7")` and so on, consistent with lines 710-733.

---

_Reviewed: 2026-11-03T22:01:23Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
