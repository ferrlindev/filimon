---
phase: 02-sourceitem-pipeline
reviewed: 2026-11-04T12:19:37Z
depth: standard
files_reviewed: 2
files_reviewed_list:
  - src/commands/consume.rs
  - src/models.rs
findings:
  critical: 0
  warning: 3
  info: 4
  total: 7
  necessity: 1
status: issues_found
---

# Phase 2: Code Review Report (re-review after gap-closure plans 02-05 and 02-06)

**Reviewed:** 2026-11-04T12:19:37Z
**Depth:** standard
**Files Reviewed:** 2
**Status:** issues_found

## Summary

Scope: `src/commands/consume.rs` and `src/models.rs` at HEAD `4799bbd`, diff base `6d61d72`. The source commits are 66075b0..1e09a35 (02-05: located record diagnostics and exact per-topic offset ranges past `MAX_DIAGNOSTICS`) and cd7f8e8..74ce6c3 (02-06: the `utc_rfc3339` year guard). As instructed, WR-07 (a budget stop does not mark the run failed) is out of scope under plan 02-05 and is not raised again. The `fluvio/` checkout was read but not modified.

Evidence:
- `cargo test -p filimon --offline`: 132 passed, 0 failed, 7 ignored. The cluster tests were not run.
- `fluvio/crates/fluvio-spu/src/smartengine/batch.rs:39-135`: `process_batch` returns the successes before an error together with that error. So the records before a rejected offset in the same batch are still delivered. The `news_incoming` in-batch reasoning (consume.rs:407-411) still holds.
- The only `eprintln!` in consume.rs is at line 900, inside `print_diagnostic`. Every `unshown_lines` line passes through it (consume.rs:830-832). Range lines are built only from the topic map key, a count and integer offsets, so content-freedom (CR-01) holds.
- `src/output.rs` is unchanged since 98e70f6. That file is not in scope, so WR-10 and IN-09 stay as recorded.

02-06 is sound. All three date paths (`rfc3339_to_utc`, `epoch_ms_to_rfc3339`, `parse_news_published`) go through `utc_rfc3339` (models.rs:199-245). The 0000 and 9999 boundaries are tested on both sides, and every in-range output is re-parsed as RFC 3339. 02-05 locates every skip path: `decode_news` (consume.rs:292), `decode_social` (315) and every `news_incoming` skip (460-475). One new warning: a social `Note` counts toward the diagnostic budget and arrives before its `Record`. A budget stop can therefore fall between the two, so a note (kept, or named in a "noted" range) describes a record that was never written. This breaks the documented invariant "noted names records that were written" (WR-12). WR-08 and WR-11 are unchanged and carried forward.

### Prior findings in these files

| Prior ID | Status | Evidence |
|---|---|---|
| WR-07: budget stop is a clean run | **Open, out of scope (plan 02-05)** | `collect_records` consume.rs:568-572 still does not set `failed`. `finish_batch` 850-868 is unchanged. |
| WR-08: `MAX_REOPENS` counts over the whole run | **Open** (carried below) | consume.rs:438-439, 467, 476: `reopens` is never reset and goes up on in-batch steps too. |
| WR-09: `rfc3339_to_utc` can emit non-RFC 3339 | **Resolved** by 02-06 | `utc_rfc3339` models.rs:199-204 is used at 213, 220 and 244. Tests at models.rs:514-554, 556-608, 735-801 and consume.rs:1917-1956. |
| WR-10: predictable temp path in `save_json` | Not in scope (output.rs) | output.rs is unchanged since 98e70f6. |
| WR-11: second `check_ner_mode` in `run` | **Open** (carried below) | consume.rs:763. |
| IN-03: single-digit offset assertions | **Open** | consume.rs:1077 `contains('9')`, 1894 `contains('3')`. The new 02-05 and 02-06 tests use full `"{topic} offset {n}:"` prefixes. |
| IN-06 / IN-07 (models.rs) | **Open** (unchanged) | models.rs:228-245 and 185-187. |
| IN-09 | Not in scope (output.rs) | |
| IN-10: brittle prefix drift test | **Open** (carried below) | consume.rs:1246-1267. |
| IN-11: failed reopen still says "skipped" | **Open** (carried below) | consume.rs:475-489. |
| CR-01 | **Resolved**, no regression | See the evidence above. |

## Narrative Findings (AI reviewer)

## Warnings

### WR-12: A budget stop can fall between a social note and its record, so a note describes a record that was never written

**File:** `src/commands/consume.rs:326-348, 553-573` (docs at 70-71, 127-129)
**Issue:** `decode_social` returns `[Note, ..., Record]`. `social_incoming` flattens these into separate stream items, and `merge_incoming` can interleave news items between them. `collect_records` counts every `Note` toward `seen` (557-560) and runs the budget check after each item (568-572). So the run can stop right after a `Note`, before its `Record` arrives. Trace with `--limit 1` (budget 100): 99 undecodable social values, then a value with `"published":"yesterday"` at offset 99. The `Note` brings `seen` to 100 and is kept as the 100th message (`offset 99: published is not an RFC 3339 time ...; set to null`). The loop then breaks, and the record is never collected. With `--limit 50` and the stop at item 500, the offset goes into `unshown[topic].noted` and prints as "more record diagnostic(s)". Both outcomes contradict `Incoming::Note` ("a record that still gives an output record", line 70) and `Unshown::noted` ("records that were written", line 128), and break the 02-05 truth that notes concern written records. The operator sees a published value nulled on a record that is missing from the file, and nothing marks that record as unread. None of the tests feeds a `Note` into the budget boundary.
**Fix:** Notes cannot make a run unbounded: at most two arrive per record, and records are bounded by `limit`. So do not charge them to the budget:
```rust
Ok(Some(Incoming::Note(d))) => collected.note(d), // no `seen += 1`
```
Alternatively, carry notes with their record (`Incoming::Record(OutputRecord, Vec<RecordDiagnostic>)`) so they cannot be separated. Add a test: 99 `skipped(...)` items, then `noted("t", 99)` and a record, collected with limit 1. It must yield the record and no dangling note.

### WR-08: `MAX_REOPENS` counts over the whole run, and each record left in a rejected batch costs one reopen (carried forward, unchanged)

**File:** `src/commands/consume.rs:438-439, 467-476`
**Issue:** The code is the same as in the previous review. `b.reopens` is never reset when a record is delivered (line 439), and the in-batch step (449-456) charges the cap once per offset (476). A streaming run that sees 101 rejected records over its whole lifetime loses the news branch for good. So does a single poison record followed by 100 or more records in the same stored batch. Since 02-05, each in-batch step is also reported as a separate `Skipped` record, so the cap now also limits how many in-batch records get named before the branch ends.
**Fix:** As before: reset `b.reopens = 0` on `Ok(Incoming::Record(_))`, and do not count the in-batch steps (`Some(start) if e.offset < start`) toward `MAX_REOPENS`, or give them their own larger per-batch cap. Add unit tests for both cases.

### WR-11: Already-defended-elsewhere: second `check_ner_mode` inside `consume::run` (carried forward, unchanged)

**File:** `src/commands/consume.rs:763`
**Category:** necessity
**Issue:** The code is unchanged from the previous review. The crate is binary-only, and `main.rs` already rejects `--enrich-ner` with `--limit 0` before the model loads. `ner` is `Some` only when that flag is set. So the state `ner.is_some() && limit == 0` cannot reach `run`.
**Fix:** Remove the call at consume.rs:763, or keep a single check in `run` and pass it the flag.

## Info

### IN-12: Fatal and budget-stop lines use up the record-diagnostic message cap

**File:** `src/commands/consume.rs:149-152, 164-187`
**Issue:** `skip` and `note` compare `self.diagnostics.len()` against `MAX_DIAGNOSTICS`. `diagnostics` also holds the messages from `keep_always` (fatal lines). Each fatal line kept before the cap is reached therefore cuts one record diagnostic from the "100 kept messages", and pushes that record into the range line. For example, if the social branch fails at once and news then floods, only 99 news messages are kept. No record goes unnamed, but the behavior contradicts the doc on `MAX_DIAGNOSTICS` ("Most record diagnostics a batch run keeps as messages") and the 02-05 statement that "a flood keeps exactly MAX_DIAGNOSTICS + 1 messages".
**Fix:** Keep a separate `kept_records: usize` counter and compare that counter against `MAX_DIAGNOSTICS`. Or reword the doc to "most diagnostic messages, fatal ones included".

### IN-13: The reopen-limit closing line says it skipped 100 records, but 101 are reported as skipped

**File:** `src/commands/consume.rs:467-473`
**Issue:** When the limit is reached, the 101st rejected record is now sent as `Incoming::Skipped` (line 468). In batch mode past the cap, it is counted in "N more skipped record(s)". The test at consume.rs:1685 confirms this: skipped offsets run `0..=MAX_REOPENS`. The closing fatal line still says `stopped after skipping {MAX_REOPENS} records`, which is one fewer than the run reported.
**Fix:** Word the closing line as `stopped after {MAX_REOPENS} reopens; {MAX_REOPENS + 1} records rejected`, or count the records actually sent.

### IN-10: SmartModule prefix drift test is narrow and brittle (carried forward, unchanged)

**File:** `src/commands/consume.rs:1246-1267`
**Issue:** The test only matches `eyre!("` with the literal on the same line, and it asserts an exact count. The details are unchanged from the previous review.
**Fix:** Match `eyre!|bail!|ensure!|wrap_err|context` across lines, and assert set inclusion rather than equal counts.

### IN-11: A failed reopen reports the rejected record as "skipped" right before the branch ends (carried forward, unchanged)

**File:** `src/commands/consume.rs:475-489`
**Issue:** `"{message}; skipped"` is built before `(b.reopen)(next)` runs. If the reopen fails, the next line is the fatal "could not reopen". No later read moves past the record.
**Fix:** In the `Err(err)` arm, send the message without the `; skipped` suffix, as the reopen-limit path does.

---

_Reviewed: 2026-11-04T12:19:37Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
