---
phase: 02-sourceitem-pipeline
reviewed: 2026-11-04T13:33:36Z
depth: standard
files_reviewed: 1
files_reviewed_list:
  - src/commands/consume.rs
findings:
  critical: 0
  warning: 2
  info: 6
  total: 8
  necessity: 1
status: issues_found
---

# Phase 2: Code Review Report (re-review after gap-closure plan 02-07)

**Reviewed:** 2026-11-04T13:33:36Z
**Depth:** standard
**Files Reviewed:** 1
**Status:** issues_found

## Summary

Scope: `src/commands/consume.rs` at HEAD `d5bfb44`, diff base `9f38a1c`. The source commits since that base are only the four 02-07 commits: d39e9a5 and 972c4cc (Task 1: notes no longer charged to the diagnostic budget), and 08452d7 and eadd9b2 (Task 2: `Incoming::Noted { record, notes }`). They were merged in 8a85020. 02-07-PLAN.md, 02-07-SUMMARY.md and 02-REVIEW-DISPOSITION.md were read for intent. No source file was modified.

Evidence:
- `cargo test -p filimon --offline`: 134 passed, 0 failed, 8 ignored. The cluster tests (`#[ignore]`) were not run.
- `grep` across `src/` and `tests/`: `Incoming`, `decode_social` and `collect_records` have no users outside consume.rs. The return-type change of `decode_social` (`Vec<Incoming>` to `Incoming`) breaks no other caller.
- futures-util 0.3.31 (Cargo.lock), `stream/select.rs:52-54` and `select_with_strategy.rs:32-35`: `select` alternates starting with the left (news) stream. This matters for IN-15.

**WR-12 is resolved.** Each stop path was traced:
- **Limit stop.** `while collected.records.len() < limit` (consume.rs:564) is checked only between items. The `Noted` arm (571-576) pushes the record before any of its notes.
- **Budget stop.** `seen += 1` now happens only in the `Skipped` (567-570) and `Fatal` (577-581) arms. The check at 584 therefore runs only after an item that gives no record, never inside a `Noted` item.
- **Idle and end-of-stream stops.** These come from `stream.next()` (565), between whole items.
- **Merged stream.** `decode_social` (319-359) returns exactly one item per value. `social_incoming` (510-528) no longer flattens, so `stream::select` cannot interleave a news item between a note and its record.
- **Noted ranges.** A noted range now always names a pushed record. `Collected::note` is reached only from the `Noted` arm, after `records.push`. Also, `finish_batch` (866-884) can refuse to write only when `records` is empty, which cannot happen once a `Noted` item has arrived.
- **Streaming mode** (933-938) prints the notes and then the summary from the same item.
- **Tests.** `note_at_budget_boundary_keeps_its_record` reproduces the exact WR-12 trace (cases A-D), and `merged_stop_never_splits_a_note_from_its_record` covers both merged-stream splits.

The diff adds no correctness or security defect. Content-freedom is unchanged: the `Noted` notes are the same fixed-format strings as before, and every one is printed through `print_diagnostic` (915-917). Two new Info items follow from the change. The budget stop line and the `diagnostic_budget` doc still say "diagnostics", but notes are no longer counted (IN-14). One new test asserts exact counts that depend on futures-util's left-first polling order (IN-15). WR-08, WR-11, IN-10, IN-11, IN-12 and IN-13 are still present and are carried forward with updated line numbers.

### Prior findings in this file

| Prior ID | Status | Evidence |
|---|---|---|
| WR-12: budget stop between a social note and its record | **Resolved** by 02-07 | consume.rs:564-590 (`Noted` arm pushes record first; notes not charged to `seen`); 319-359 one item per value; 510-528 no flatten; tests at 1650-1730 and 1942-2000. |
| WR-08: `MAX_REOPENS` counts over the whole run | **Open** (carried below) | consume.rs:408, 441, 477, 486: `reopens` is never reset, and in-batch steps (458-466) still increase it. |
| WR-11: second `check_ner_mode` in `run` | **Open** (carried below) | consume.rs:779; main.rs:73 already checks. |
| IN-10: brittle prefix drift test | **Open** (carried below) | consume.rs:1292-1313. |
| IN-11: failed reopen still says "skipped" | **Open** (carried below) | consume.rs:485, 492-498. |
| IN-12: fatal lines use up the record-diagnostic cap | **Open** (carried below) | consume.rs:170-187 still compare `self.diagnostics.len()`. |
| IN-13: reopen-limit line says 100, but 101 are reported as skipped | **Open** (carried below) | consume.rs:477-483. |
| IN-03: single-digit offset assertions | **Open** (not re-raised; not in this re-review's carry list) | consume.rs:1123 `contains('9')`, 2124 `contains('3')`. The 02-07 tests use full `"{topic} offset {n}:"` prefixes. |
| WR-07, WR-10, IN-06, IN-07, IN-09 | Not in scope | Out of scope (WR-07) or in files not reviewed here (models.rs, output.rs). |

## Narrative Findings (AI reviewer)

## Warnings

### WR-08: `MAX_REOPENS` counts over the whole run, and each record left in a rejected batch costs one reopen (carried forward, unchanged)

**File:** `src/commands/consume.rs:408, 441, 458-466, 477, 486`
**Issue:** 02-07 did not touch this code. `b.reopens` starts at 0 (441), is never reset when the branch delivers a record (449), and goes up once per reopen (486), in-batch steps included (458-466). A streaming run (`--limit 0`) that meets 101 rejected records over its whole life loses the news branch for good: the closing line at 479-482 becomes `Incoming::Fatal`, and `consume_stream` then exits with an error once the social branch ends. A single poison record followed by 100 or more records in the same stored batch has the same effect, because each in-batch step costs one reopen.
**Fix:** Reset the counter when a record gets through, and do not charge in-batch steps to the cap (or give them their own per-batch cap):
```rust
let code = match b.stream.as_mut()?.next().await? {
    Ok(incoming) => {
        if matches!(incoming, Incoming::Record(_)) {
            b.reopens = 0;
        }
        return Some((incoming, b));
    }
    Err(code) => code,
};
// ...
let in_batch = matches!(b.start, Some(start) if e.offset < start);
// ...
if !in_batch {
    b.reopens += 1;
}
```
Add unit tests for both cases: 101 rejections separated by good records, and 101 or more in-batch steps.

### WR-11: Already-defended-elsewhere: second `check_ner_mode` inside `consume::run` (carried forward, unchanged)

**File:** `src/commands/consume.rs:779`
**Category:** necessity
**Issue:** The crate is a binary only. `main.rs:73` already calls `commands::consume::check_ner_mode(enrich_ner, limit)?` before the model loads, and `ner` is `Some` only when `enrich_ner` is set (main.rs:74). So `run` can never receive `ner.is_some() && limit == 0`, and the check at consume.rs:779 cannot fail.
**Fix:** Remove the call at consume.rs:779. Or keep a single check in `run`, pass it the flag, and drop the one in main.rs.

## Info

### IN-14: The budget stop line and the `diagnostic_budget` doc still say "diagnostics", but notes are no longer counted (new, from 02-07)

**File:** `src/commands/consume.rs:160-164, 584-587`
**Issue:** Since 972c4cc, `seen` counts only `Skipped` and `Fatal` items, while notes are still pushed into `collected.diagnostics` or the noted ranges. Two texts were not updated:
- The doc on `diagnostic_budget` (160-161) still says "Most diagnostics a batch run for `limit` records reads before it stops".
- The stop line still prints `stopped after {seen} diagnostics before {limit} records arrived`.

Example: `--limit 10`, 9 social records that each carry a note, and 100 undecodable values. The run prints 100 kept messages (9 notes and 91 skips), then "9 more skipped record(s)", then "stopped after 100 diagnostics". The operator counts 109 diagnostics against a line that says 100. The collect_records doc (544-549) was rewritten correctly. 02-07-SUMMARY says the `diagnostic_budget` region was left byte-identical on purpose, but the text it pins is now wrong.
**Fix:** Change the doc to "Most skipped and fatal items a batch run for `limit` records reads before it stops". Change the stop line to `stopped after {seen} skipped or failed records before {limit} records arrived`. Update the tests that match `stopped after` if they pin the full text.

### IN-15: `merged_stop_never_splits_a_note_from_its_record` asserts counts that depend on futures-util's polling order (new, from 02-07)

**File:** `src/commands/consume.rs:1977-1999`
**Issue:** The budget case asserts exactly 100 kept messages and that the last one is `filimon-social-items offset 49:`. That holds only because `stream::select` polls the left (news) stream first and then alternates (futures-util 0.3.31, `select.rs:52-54`, `PollNext::default() == Left`). With right-first polling, the `Noted` item arrives after 98 skips. The record then fills the limit with 99 messages, and the test fails even though the invariant it protects (no note without its record) still holds. A futures-util upgrade, or swapping the arguments of `merge_incoming`, would break this test for a reason unrelated to WR-12.
**Fix:** Assert the invariant rather than the interleaving: the record `t3_edge` is present, every message containing `set to null` has its record, no `stopped after` line appears, and `collected.diagnostics.len()` is 99 or 100. Or build the interleaving by hand with `iter(vec![...])` instead of `merge_incoming`, so the order is explicit.

### IN-12: Fatal and budget-stop lines use up the record-diagnostic message cap (carried forward, unchanged)

**File:** `src/commands/consume.rs:155-158, 170-187, 191-193`
**Issue:** `skip` and `note` compare `self.diagnostics.len()` against `MAX_DIAGNOSTICS`, but `diagnostics` also holds the `keep_always` fatal lines. Each fatal line kept before the cap is reached cuts one record diagnostic from the 100 kept messages. This contradicts the doc on `MAX_DIAGNOSTICS` ("Most record diagnostics a batch run keeps as messages").
**Fix:** Count kept record diagnostics in a separate `kept_records: usize` field and compare that against `MAX_DIAGNOSTICS`. Or reword the doc to "most diagnostic messages, fatal ones included".

### IN-13: The reopen-limit closing line says it skipped 100 records, but 101 are reported as skipped (carried forward, unchanged)

**File:** `src/commands/consume.rs:477-483`
**Issue:** When the limit is reached, the 101st rejected record is still sent as `Incoming::Skipped` (478). The closing fatal line then says `stopped after skipping {MAX_REOPENS} records`, one fewer than the number reported. The test at `news_branch_reopens_at_most_max_times` confirms this: it expects `MAX_REOPENS + 2` diagnostics, which are `MAX_REOPENS + 1` skipped plus the closing line.
**Fix:** Word the line as `stopped after {MAX_REOPENS} reopens; {} records rejected` with `MAX_REOPENS + 1`, or count the records actually sent.

### IN-10: SmartModule prefix drift test is narrow and brittle (carried forward, unchanged)

**File:** `src/commands/consume.rs:1292-1313`
**Issue:** The test only matches `eyre!("` with the literal on the same line, and it asserts that the number of matches equals `MODULE_HINT_PREFIXES.len()`. A multi-line `eyre!(` call, `bail!`, `wrap_err` or `.context(...)` would be missed. Two messages that share one prefix would fail the count even though both are covered.
**Fix:** Match `eyre!|bail!|ensure!|wrap_err|context` across lines, and check that every found message is covered by a listed prefix rather than comparing counts.

### IN-11: A failed reopen reports the rejected record as "skipped" right before the branch ends (carried forward, unchanged)

**File:** `src/commands/consume.rs:485, 492-498`
**Issue:** `located(format!("{message}; skipped"))` is built before `(b.reopen)(next)` runs. If the reopen fails, the next item is the fatal "could not reopen at offset {next}". No later read moves past the record, so "skipped" overstates what happened.
**Fix:** Build the item after the reopen result is known. In the `Err(err)` arm, send `located(message)` without the `; skipped` suffix, as the reopen-limit path does.

---

_Reviewed: 2026-11-04T13:33:36Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
