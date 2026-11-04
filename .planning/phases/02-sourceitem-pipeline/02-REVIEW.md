---
phase: 02-sourceitem-pipeline
reviewed: 2026-11-04T10:31:47Z
depth: standard
files_reviewed: 4
files_reviewed_list:
  - src/commands/consume.rs
  - src/main.rs
  - src/models.rs
  - src/output.rs
findings:
  critical: 0
  warning: 5
  info: 3
  total: 8
  necessity: 1
status: issues_found
---

# Phase 2: Code Review Report (incremental re-review after review-fix iteration 1)

**Reviewed:** 2026-11-04T10:31:47Z
**Depth:** standard
**Files Reviewed:** 4
**Status:** issues_found

## Summary

Scope: diff `16e529d..HEAD` (HEAD `085efcc`) over `src/commands/consume.rs`, `src/main.rs`, `src/models.rs` and `src/output.rs`. These are the six review-fix commits 308e775 (WR-01), 9382d43 (WR-02), ec72c90 (WR-03), 4c2845e (WR-04), fc99d4e (WR-05) and 7330aa6 (WR-06).

Evidence:
- `cargo test -p filimon --offline`: 125 passed, 0 failed, 6 ignored. The cluster tests were not run in this pass.
- Read fluvio sources in `fluvio/`. `SmartModuleTransformRuntimeError::new` sets `offset = base_offset + offset_delta`, so the offset is absolute (`crates/fluvio-protocol/src/link/smartmodule.rs:32`). The chain keeps record offset deltas between modules. `process_batch` runs the chain over whole stored batches and stops at the first error (`crates/fluvio-spu/src/smartengine/batch.rs:41-135`). The client drops records below the requested start offset (`crates/fluvio/src/consumer/mod.rs:636`), so a reopen inside a batch does not deliver duplicates. These facts support the WR-01 design.
- Ran chrono 0.4.45 (the locked version) with the exact `rfc3339_to_utc` body. `9999-12-31T23:00:00-05:00` gives `+10000-01-01T04:00:00.000Z`, and `0000-01-01T00:30:00+01:00` gives `-0001-12-31T23:30:00.000Z` (see WR-09).

Five new findings are warnings. WR-07 shows that the WR-06 budget creates a new way to get the WR-02 failure: when every item is a diagnostic, a batch run exits 0 and overwrites the last good output with `[]`. For `--limit` 1 to 10, the budget also makes the MAX_REOPENS failure unreachable. WR-08: the reopen cap counts over the whole run and charges one reopen for every record in a rejected batch, so a long streaming run loses its news branch for good. WR-09: valid RFC 3339 input can produce output that is not RFC 3339. WR-10: `save_json` writes through a temp path that is easy to predict and follows symlinks.

### Prior findings in these files

| Prior ID | Status | Evidence |
|---|---|---|
| WR-01: one poison record halts the news branch | **Resolved** (residual limits in WR-08) | `news_incoming` (consume.rs:302-367) reopens at `e.offset + 1`. When a reopened stream fails again before its start offset, it steps one offset at a time through the rejected batch. This matches the SPU's whole-batch processing and the client's start-offset filter, both cited above. |
| WR-02: success exit and output overwrite on stream failure | **Resolved for stream errors** (budget bypass in WR-07) | `Incoming::Fatal` → `Collected::failed` (consume.rs:430-434). `finish_batch` skips the write when the run failed with no records (720-726). `consume_stream` returns `Err` (771-773). `save_json` now writes a temp file and renames it (output.rs:25-42). |
| WR-03: `--enrich-ner` ignored in streaming mode | **Resolved** | `check_ner_mode` runs at main.rs:73, before the model loads. A second call at consume.rs:633 is redundant (WR-11). |
| WR-04: social `published` not normalized to UTC | **Resolved** (edge case in WR-09) | `rfc3339_to_utc` (models.rs:202-208) is used at consume.rs:226-232. Tested with a `+01:00` input. |
| WR-05: hint content-freedom rests on a convention | **Resolved** | `module_hint_summary` (consume.rs:155-166) returns only `&'static` list entries or `NO_DETAIL` and never copies hint text. `SmartModuleLookBackError` takes the same path (253-256). Content-freedom no longer depends on how the modules word their messages. The drift test only affects how useful the diagnostics are (IN-10). |
| WR-06: diagnostics do not bound a `--limit` run | **Resolved** (side effect in WR-07) | `diagnostic_budget` limits the reads, and `MAX_DIAGNOSTICS` limits the memory (consume.rs:71-90, 415-446). Suppressed diagnostics are counted and printed (700-702). |
| IN-01: local decode diagnostics drop safe detail; `hint_summary` over-truncates fluvio errors | **Open** (unchanged) | `describe_json_error` consume.rs:104-113 is unchanged, and the `other` arm still uses `hint_summary` (257). |
| IN-02: bidi/format characters pass the diagnostic choke point | **Open** (unchanged) | `diagnostic_line` consume.rs:741-743 calls `sanitize_for_terminal`, which is unchanged (output.rs:49-57). |
| IN-03: single-digit offset assertions | **Open** (unchanged) | consume.rs:894 `contains('7')`, :921 `contains('9')`, :1403 `contains('5')`, :1418 `contains('9')`, :1495 `contains('3')`. |
| IN-05: `sanitize_for_terminal` keeps Unicode format characters | **Open** (unchanged) | output.rs:49-57 |
| IN-06: `parse_news_published` misses offset-less forms | **Open** (unchanged) | models.rs, not touched by the fix commits |
| IN-07: `SourceItem::social` reinterprets JSON-scalar `raw` strings | **Open** (unchanged) | models.rs, not touched by the fix commits |
| IN-08: empty-parent guard before `create_dir_all` | **Open** (unchanged) | output.rs:19-23 |
| CR-01 | **Resolved** (as previously recorded) | No regression: every diagnostic still passes through `print_diagnostic` (consume.rs:697-706, 764-768), and every new message is built only from fixed text, offsets, topic names and list entries. |

IN-04 (docker-compose) is not in this file scope and stays as recorded.

## Narrative Findings (AI reviewer)

## Warnings

### WR-07: Diagnostic-budget stop counts as a clean run, so a run that got only errors overwrites the last good output with `[]` and exits 0

**File:** `src/commands/consume.rs:437-443, 720-737` (with `263`, `77-79`)
**Issue:** When `seen >= budget`, `collect_records` adds a "stopped after" note but leaves `collected.failed` false. `finish_batch(&[], false, path)` then calls `save_json` with an empty slice, replaces the prior `wise_output.json` with `[]`, prints a run summary, and returns `Ok(())`. The test `undecodable_flood_stops_at_diagnostic_budget` asserts `!collected.failed`, and the fix report confirms this is intended. This is the same overwrite-and-exit-0 failure WR-02 fixed, reached a different way:
- A social topic full of undecodable values gives `[]` and exit 0, so the last good file is lost.
- News: for any `limit <= 10` the budget is 100. The news branch emits one `Diagnostic` per reopen. After `MAX_REOPENS` (100) reopens it emits a 101st `Diagnostic`, then the `Fatal` closing line. The collector stops at the 100th diagnostic, so it never sees the `Fatal`. With `--limit 1..10`, a news topic with 100 or more rejected records therefore never fails the run. It always exits 0 with `[]` written. The fix report's note that `news_branch_reopens_at_most_max_times` must "read the branch directly, because the collector's budget would stop it first" describes this unreachability.
**Fix:** Treat a budget stop as a failure when it produced no records, and make the budget leave room for the reopen cap:
```rust
fn diagnostic_budget(limit: usize) -> usize {
    limit.saturating_mul(10).max(MAX_DIAGNOSTICS).max(MAX_REOPENS + 2)
}
// in collect_records, at the budget stop:
if seen >= budget {
    collected.exhausted = true; // new field
    collected.note(format!(...), true);
    break;
}
// in finish_batch:
if (failed || exhausted) && records.is_empty() { return Err(...not written...) }
```
Add a test: an endless `Diagnostic` stream into `finish_batch` must leave a seeded file unchanged and return `Err`.

### WR-08: `MAX_REOPENS` counts over the whole run, and each record left in a rejected batch costs one reopen

**File:** `src/commands/consume.rs:261-263, 333-351`
**Issue:** `b.reopens` only goes up. It does not reset when a later record is delivered, so the cap counts rejected records over the branch's whole lifetime, not consecutive ones. In streaming mode (`--limit 0`, a long-running process on an open topic), the 101st rejected record ends the news branch for good, even if days of good records came in between. The process keeps running and prints only social records. `consume_stream` returns `Err` only after both branches end, so in practice the operator gets one stderr line and a silent news outage. Second, the in-batch case (`e.offset < start`, lines 334-341) moves forward one offset per reopen. Each step opens a new consumer stream and uses one unit of the cap. A single poison record followed by 100 or more records in the same stored batch ends the branch, whatever happens in later batches. The SPU feeds the chain whole batches (`fluvio-spu/src/smartengine/batch.rs:52-71`). The cluster test (`good_news_after_poison_record_is_delivered`) only covers a batch with at most one record after the poison.
**Fix:** Count consecutive rejections, not total ones, and do not charge the in-batch steps to the cap:
```rust
Ok(incoming) => {
    if matches!(incoming, Incoming::Record(_)) { b.reopens = 0; }
    return Some((incoming, b));
}
...
let in_batch = matches!(b.start, Some(start) if e.offset < start);
if !in_batch { b.reopens += 1; }
```
A rejected batch has a finite size, so the in-batch walk still ends. If the network cost of the walk matters, you can bound it separately, for example with a larger per-batch cap. Add unit tests for these cases: 101 rejected records with good records between them must not end the branch, and 150 in-batch steps after a single rejection must not end it.

### WR-09: `rfc3339_to_utc` can turn valid RFC 3339 input into output that is not RFC 3339

**File:** `src/models.rs:202-208` (used at `src/commands/consume.rs:226-232`)
**Issue:** After converting to UTC, the year can fall outside `0000..=9999`. chrono's `write_rfc3339` then writes a signed, five-digit year (`chrono-0.4.45/src/format/formatting.rs:511-518`). Reproduced with the locked chrono version:
- `9999-12-31T23:00:00-05:00` → `+10000-01-01T04:00:00.000Z`
- `0000-01-01T00:30:00+01:00` → `-0001-12-31T23:30:00.000Z`

`filimon-social-items` is fed by external plugins. Such a value passes as a "valid" timestamp without a diagnostic and breaks the documented "RFC 3339 (UTC)" output contract for any downstream parser. The existing `epoch_ms_to_rfc3339` (models.rs:196-200) has the same exposure for very large epoch values.
**Fix:**
```rust
pub fn rfc3339_to_utc(s: &str) -> Option<String> {
    let utc = DateTime::parse_from_rfc3339(s).ok()?.with_timezone(&Utc);
    (0..=9999).contains(&utc.year())
        .then(|| utc.to_rfc3339_opts(SecondsFormat::Millis, true))
}
```
Do the same in `epoch_ms_to_rfc3339`. Add both boundary inputs to `rfc3339_to_utc_normalizes_only_rfc3339`, expecting `None`.

### WR-10: `save_json` writes through a predictable temp path that follows symlinks

**File:** `src/output.rs:28-35`
**Issue:** The temp path is `<dir>/.<name>.tmp-<pid>`, which is easy to predict, and it is opened with `File::create`. That call follows symlinks and truncates an existing target. `--output` accepts any path, and `src/cli.rs:193` even uses `/tmp/x.json` as its example. In a shared writable directory, another local user can create `/tmp/.x.json.tmp-<pid>` as a symlink to a file the victim owns. The run then truncates that file and fills it with JSON, and `rename` moves the symlink, not the victim's file. macOS has no `protected_symlinks` setting to block this. A stale temp file left by a killed run is also silently reused. `tempfile` is already a normal dependency (Cargo.toml:27).
**Fix:**
```rust
let dir = match path.parent() { Some(p) if !p.as_os_str().is_empty() => p, _ => Path::new(".") };
let mut tmp = tempfile::NamedTempFile::new_in(dir)?;   // O_EXCL, random name
tmp.write_all(json.as_bytes())?;
tmp.as_file().sync_all()?;
tmp.persist(path).map_err(|e| e.error)?;               // rename; temp removed on drop if this fails
```
The two existing `save_json` tests still apply. The leftover-temp check needs no change, because `NamedTempFile` removes the file when it is dropped.

### WR-11: Already-defended-elsewhere: second `check_ner_mode` inside `consume::run`

**File:** `src/commands/consume.rs:633`
**Category:** necessity
**Issue:** The rejected state (`ner.is_some() && limit == 0`) cannot reach `run`. The crate is binary-only (Cargo.toml:14-16, no `[lib]`). The only caller is `main.rs:79` (`run_consume`, re-exported at `src/commands/mod.rs:5`). `main.rs:73` already returns `Err` for `enrich_ner && limit == 0`, and `ner` is `Some` only when `enrich_ner` is true (main.rs:74-78). The inner check also tests a different predicate (`ner.is_some()`, not the flag), so the two checks do not even guard the same thing.
**Fix:** Remove the call at consume.rs:633, or move the only check into `run` and drop the one in `main`. Then `run` would need the flag rather than `Option<&NerEngine>`, and the model load would have to happen after the check.

## Info

### IN-09: Atomic `save_json` changes the output file's identity and does not make the rename durable

**File:** `src/output.rs:32-42`
**Issue:** The rename (a) replaces an output path that is a symlink with a regular file instead of writing through the link, (b) resets permissions to `0666 & ~umask`, so a file the operator had made `0600` becomes world-readable under a default umask, and (c) does not `fsync` the parent directory. After a crash, the rename can be lost even though the data was synced. None of this is documented in the new doc comment.
**Fix:** Copy the old file's permissions to the temp file before the rename (`std::fs::metadata(path).map(|m| m.permissions())`), then `File::open(dir)?.sync_all()` after the rename. Document that a symlinked output path is replaced, or resolve it with `std::fs::canonicalize` first.

### IN-10: SmartModule prefix drift test is narrow and brittle

**File:** `src/commands/consume.rs:1087-1107`
**Issue:** The test only finds `eyre!("` with the literal on the same line. It misses `bail!`, `ensure!`, `.wrap_err(...)`, `.context(...)`, an `eyre!(` whose literal starts on the next line, and bare `?` sites like filter-relevance:43. It also asserts `found == MODULE_HINT_PREFIXES.len()`. A second call site that reuses an existing prefix would fail the test even though it is covered, and removing one call site while adding an unlisted one in a macro the test misses would still pass. Since WR-05, this test only protects how useful the diagnostics are, not content-freedom, so this is Info.
**Fix:** Match on a multi-line regex over `eyre!|bail!|ensure!|wrap_err|context`, and assert set inclusion (every found prefix is listed) rather than equal counts.

### IN-11: A failed reopen reports the rejected record as "skipped" right before the branch ends

**File:** `src/commands/consume.rs:357-365`
**Issue:** When `(b.reopen)(next)` fails, the branch still yields `"{message}; skipped"` and then the fatal "could not reopen" line. No later read moves past the record, so "skipped" suggests the stream went on when it did not. The MAX_REOPENS path (344-350) correctly leaves out the suffix.
**Fix:** Yield `Incoming::Diagnostic(message)` without the `; skipped` suffix in the `Err(err)` arm, as the MAX_REOPENS path does.

---

_Reviewed: 2026-11-04T10:31:47Z_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
