---
phase: 02-sourceitem-pipeline
reviewed: 2026-11-03T20:51:17Z
depth: standard
files_reviewed: 9
files_reviewed_list:
  - src/models.rs
  - src/streaming.rs
  - src/commands/consume.rs
  - src/output.rs
  - src/main.rs
  - src/commands/produce.rs
  - src/cli.rs
  - Cargo.toml
  - docker-compose.yml
findings:
  critical: 1
  warning: 4
  info: 8
  total: 13
  necessity: 1
status: issues_found
---

# Phase 2: Code Review Report

**Reviewed:** 2026-11-03T20:51:17Z
**Depth:** standard
**Files Reviewed:** 9
**Status:** issues_found

## Summary

I reviewed the Phase 2 diff (`2dff9ed..HEAD`) for the nine files in scope. The phase adds the `SourceItem` envelope, `OutputRecord` mapping, the consumer-side SmartModule chain, the social topic merge, and NER enrichment. The scope also had `src/main.rs`, `src/commands/produce.rs`, `Cargo.toml`, and `docker-compose.yml`, but their Phase 2 changes are small (wiring, comments, one dependency).

Evidence I gathered:
- `cargo test -p filimon`: 102 passed, 0 failed, 3 ignored (cluster tests were not run).
- `cargo clippy -p filimon --all-targets`: the phase leaves `TypedConsumer` and its callers unused (IN-01).
- A scratch probe crate (`/tmp/redact_probe`, run offline against the same serde/serde_json) ran `redact_quoted` on real serde_json errors. It showed that record content and raw ESC bytes reach the diagnostic text (CR-01).
- I read the Fluvio SC list-filter source to check `preflight`. The topic name filter is a substring match, so the exact `t.name == *topic` re-check is needed and correct. SmartModule filtering uses exact package-key matching. No finding there.

Main concerns:
1. Diagnostics are printed to the operator's terminal without the sanitizer that protects record summaries. Their "never includes the value" redaction can be defeated (CR-01).
2. One bad record on `filimon-raw-pages` permanently stops the news branch at that offset (WR-01).
3. Failures exit 0 and overwrite the previous JSON output (WR-02).

## Narrative Findings (AI reviewer)

## Critical Issues

### CR-01: Diagnostics bypass terminal sanitization, and `redact_quoted` leaks record content

**File:** `src/commands/consume.rs:73-89, 95-136, 142-151, 459, 463, 489`
**Issue:** The threat model (02-SECURITY.md T-02-04, T-02-10) is marked closed on the claim that every record-derived string goes through `sanitize_for_terminal`. That holds for `print_summary` but not for diagnostics. They are printed raw with `eprintln!("  ✗ {message}")` at lines 459, 463 and 489, and they contain text controlled by whoever writes the record:

1. **Raw control bytes reach the terminal.** serde formats `unknown variant` errors with backticks and `Display`, not `Debug`. `redact_quoted` only strips double-quoted spans, so a social record with `"source_type":"\u001b[2J..."` produces a diagnostic with a literal ESC. Probe output (real serde_json 1.x):
   `unknown variant `\u{1b}[2JPAGE-BODY`, expected `news` or `social` at line 1 column 35`
   `filimon-social-items` has no chain and is meant to be written by external source plugins, so its contents are untrusted. That makes this a terminal-escape injection path into the operator's console.
2. **Redaction is defeated by escaped quotes.** serde formats `Unexpected::Str` as `string {:?}` (serde_core `de/mod.rs:410`), so quotes inside the value appear as `\"`. `redact_quoted` toggles on every `"` and does not handle the backslash escape, so an odd number of inner quotes flips the state and leaks the rest of the value. Probe: the value `"a\"PAGE-BODY-SECRET"` decoded as an article gives `invalid type: string "…"PAGE-BODY-SECRET"`. The doc comments ("The diagnostic never includes the value itself", "No diagnostic includes the value") and the tests at lines 571-586 and 672-690 only cover values with no inner quotes.
3. `describe_stream_error` passes `e.hint` and `other.to_string()` through `truncate_detail` only. They are neither redacted nor sanitized. map-preprocess's hint is `Failed to parse RawPage: {serde error}`, which is the same error class as point 2.

**Fix:** Build parse diagnostics from serde_json's structured accessors, never its `Display`. Also sanitize every diagnostic at one choke point:
```rust
fn describe_json_error(e: &serde_json::Error) -> String {
    format!("{:?} error at line {} column {}", e.classify(), e.line(), e.column())
}
// decode_news / decode_social:
Err(e) => Incoming::Diagnostic(format!(
    "{topic} offset {offset}: not an extracted article: {}",
    describe_json_error(&e)
)),
// SmartModule hint: redact and sanitize
truncate_detail(&output::sanitize_for_terminal(&e.hint))
// print sites (459, 463, 489):
eprintln!("  ✗ {}", output::sanitize_for_terminal(&message));
```
Add tests that feed a value with an odd number of inner quotes, and a `source_type` containing `\u001b`, and assert that neither the content nor ESC appears in the message.

## Warnings

### WR-01: One poison record permanently halts the news branch

**File:** `src/streaming.rs:129-149`, `src/commands/consume.rs:153-175`
**Issue:** `topic_stream` sets `RetryMode::Disabled`, and `news_incoming` ends the branch on the first `ErrorCode`. The comment says "a retry-disabled consumer cannot move past it". `fili` itself never writes `html: null` (D-06), but `filimon-raw-pages` is an open topic: any producer, a future plugin, or a hand-run `fluvio produce` can write a record that map-preprocess rejects. After that:
- every `fili consume --from-beginning` run stops collecting news at that offset, with no way to skip it;
- `--source news` in streaming mode returns `Ok(())` and exits 0 once the only branch ends (see WR-02);
- in `both` mode the news side silently stops while social keeps printing, which looks like "no news".

**Fix:** On `ErrorCode::SmartModuleRuntimeError(e)`, report the diagnostic and reopen the news stream at `Offset::absolute(e.offset + 1)`. Bound the number of reopens to avoid a hot loop. Alternatively, expose a `--start-offset` flag so an operator can step past a known bad offset. Add a cluster test that produces an `html: null` record followed by a good one and asserts the good one is still delivered.

### WR-02: Consume reports success and overwrites prior output when streams fail

**File:** `src/commands/consume.rs:458-473, 486-492`; `src/output.rs:15-25`
**Issue:** A stream error is downgraded to an `Incoming::Diagnostic` and never affects the result:
- batch mode always calls `save_json`, so a run where the news branch failed (or every record failed to decode) truncates the previous `output/wise_output.json` to `[]` (or a partial set) and returns `Ok(())`, exit 0;
- streaming mode returns `Ok(())` when both branches end on errors.
Scripts and CI cannot tell a clean empty run from a broken pipeline. `save_json` also does `File::create` and then `write_all`, so an interrupted write leaves a truncated, invalid JSON file in place of the last good one.

**Fix:** Carry fatal stream errors separately, for example `Collected { fatal: Vec<String>, .. }` or an `Incoming::Fatal` variant. After printing, return `Err(miette!("{n} stream(s) failed"))`. Do not replace the output file when a fatal error occurred and no records were collected. Make `save_json` atomic: write to `path.with_extension("json.tmp")` in the same directory, then `std::fs::rename` it over the target.

### WR-03: `--enrich-ner` in streaming mode loads the model and silently ignores it

**File:** `src/main.rs:73-78`; `src/commands/consume.rs:418-428, 476-493`
**Issue:** `main` loads the GLiNER engine (about 1.5 GB, per the compose comments) whenever `--enrich-ner` is set. With `--limit 0` (the default), `run` calls `consume_stream`, which never receives `ner`. The user pays the load time and memory, sees "NER: ✓ GLiNER loaded", and gets records with no entities and no warning. The flag's help text says "batch mode only", but nothing enforces it.

**Fix:** In `main`, before loading:
```rust
if enrich_ner && limit == 0 {
    return Err(miette::miette!("--enrich-ner needs --limit N (batch mode)"));
}
```
At minimum, print a warning and skip `load_ner_engine` when `limit == 0`.

### WR-04: Social `published` is not normalized to UTC, contradicting the output contract

**File:** `src/commands/consume.rs:128-133`; `src/models.rs:128, 271`
**Issue:** `OutputRecord.published` is documented as "RFC 3339 (UTC)", and news dates are converted to UTC (`parse_news_published`). For social records, `decode_social` only checks `is_rfc3339(p)` and passes the string through unchanged. A plugin or hand-produced item with `"published":"2024-05-01T10:00:00+01:00"` comes out as non-UTC. Consumers that sort or compare `published` as strings across news and social records then get wrong results.

**Fix:** Normalize instead of only validating:
```rust
item.published = match item.published.as_deref() {
    None => None,
    Some(p) => match DateTime::parse_from_rfc3339(p) {
        Ok(d) => Some(d.with_timezone(&Utc).to_rfc3339_opts(SecondsFormat::Millis, true)),
        Err(_) => { out.push(/* existing diagnostic */); None }
    },
};
```
Add a test with a `+01:00` input that expects the `Z` form.

## Info

### IN-01: The phase leaves the old consumer API unused

**File:** `src/streaming.rs:105-123, 245-315, 319-359`
**Issue:** Before Phase 2, `commands/consume.rs` used `FluvioStream::consumer` and `TypedConsumer::{process, consume_batch}`. The phase replaced them with `topic_stream`. clippy now reports `TypedConsumer` never constructed; `consumer`, `send_no_key`, `PipelineCoordinator::{producer, consumer}`, and the `stream` field unused. This code also wraps the deprecated `partition_consumer`.
**Fix:** Delete `TypedConsumer`, `FluvioStream::consumer`, and the unused `PipelineCoordinator` methods and field. Alternatively, mark the intended Phase 3 users with `#[expect(dead_code, reason = ...)]`.

### IN-02: Stale commented-out code in main.rs references the removed `TOPIC_ARTICLES`

**File:** `src/main.rs:109-324`, `src/main.rs:41-50`
**Issue:** About 215 lines of commented-out `run_produce`, `run_consume`, and `run_watch` remain. The commented `run_consume` uses `TOPIC_ARTICLES`, which D-03 deleted, so it now documents a dead topic. `criteria` is unused (clippy).
**Fix:** Remove the commented blocks and `criteria`; git history keeps them.

### IN-03: `--threshold` is parsed but never applied

**File:** `src/cli.rs:43-45`
**Issue:** `fili consume` now attaches the chain itself, so an operator would reasonably expect `--threshold` to configure `filter-relevance`. Instead, `news_chain()` passes empty params (`src/streaming.rs:65`), and the module uses the hard-coded `RELEVANCE_THRESHOLD = 0.05`. The flag is silently ignored. This predates Phase 2.
**Fix:** Remove the flag, or pass it as a SmartModule param (`params: BTreeMap::from([("threshold".into(), t.to_string())])`) and read it in filter-relevance's init.

### IN-04: docker-compose: dead service, unused env var, and the documented consume command fails preflight

**File:** `docker-compose.yml:13, 103, 132-156`
**Issue:**
- `filimon-stream-processor` runs `stream --stage all --from-beginning` with `restart: unless-stopped`. `fili` has no `stream` subcommand, so this service crash-loops under the `streaming` profile.
- `OUTPUT_PATH` is never read.

Both are already tracked in `deferred-items.md`. In addition, the header usage line `docker compose run --rm filimon consume --from-beginning` now fails preflight on a fresh cluster. The default `--source both` requires `filimon-social-items`, which nothing creates (D-05). Line 13 should show the topic-create step, or pass `--source news`.
**Fix:** Act on the deferred items, and update the header usage line.

### IN-05: `sanitize_for_terminal` keeps Unicode format characters

**File:** `src/output.rs:31-39`
**Issue:** `char::is_control` covers only category Cc. Bidi overrides (U+202E, U+2066-U+2069) and zero-width characters (Cf) pass through. They can visually reorder or hide parts of a printed URL or title (Trojan-Source-style spoofing).
**Fix:** Also drop `'\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{FEFF}'`.

### IN-06: `parse_news_published` misses common offset-less forms

**File:** `src/models.rs:220`
**Issue:** `"%Y-%m-%dT%H:%M:%S"` rejects fractional seconds (`2024-05-01T10:00:00.000`) and the space-separated form (`2024-05-01 10:00:00`). Both are common in page metadata, so these dates become null.
**Fix:** Try `"%Y-%m-%dT%H:%M:%S%.f"` and `"%Y-%m-%d %H:%M:%S%.f"` (`%.f` also accepts no fraction).

### IN-07: `SourceItem::social` reinterprets JSON-scalar `raw` strings

**File:** `src/models.rs:185-187`
**Issue:** A plugin `raw` of `"42"`, `"true"`, or `"null"` is parsed into a JSON number, bool, or null. `"null"` becomes `Some(Value::Null)`, which serializes the same as `None`, so the original metadata string is lost.
**Fix:** Keep a parsed value only when it is an object or array; otherwise keep `Value::String(s)`.

### IN-08: Already-defended-elsewhere — empty-parent guard before `create_dir_all`

**File:** `src/output.rs:16-20`
**Category:** necessity
**Issue:** The `!parent.as_os_str().is_empty()` check guards against `create_dir_all("")`, but std already returns `Ok(())` for an empty path. See `library/std/src/fs.rs:3017` in the 1.85.0 toolchain: `if path == Path::new("") { return Ok(()); }`.
**Fix:** Simplify to `if let Some(parent) = path.parent() { std::fs::create_dir_all(parent)?; }`.

---

_Reviewed: 2026-11-03T20:51:17Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
