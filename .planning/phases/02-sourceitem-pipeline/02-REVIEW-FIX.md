---
phase: 02-sourceitem-pipeline
fixed_at: 2026-11-04T00:00:00Z
review_path: .planning/phases/02-sourceitem-pipeline/02-REVIEW.md
iteration: 1
findings_in_scope: 6
fixed: 6
skipped: 0
status: all_fixed
---

# Phase 2: Code Review Fix Report

**Fixed at:** 2026-11-04
**Source review:** .planning/phases/02-sourceitem-pipeline/02-REVIEW.md
**Iteration:** 1

**Summary:**
- Findings in scope: 6 (WR-01 to WR-06; fix_scope critical_warning, so IN-01 to IN-03 were not in scope)
- Fixed: 6
- Skipped: 0

## Where the checks ran

All edits, commits and checks ran in the main checkout on branch `fluvio-wise`. No worktree was created, because the caller asked for that. You can repeat these checks from this tree:

- `cargo test -p filimon`: 125 passed, 0 failed, 6 ignored (114 before this run).
- `cargo test -p filimon -- --ignored` (local cluster, scratch topics only): 6 passed, including the new `good_news_after_poison_record_is_delivered`.
- `cargo clippy -p filimon --all-targets`: the only warning in the touched files is the existing pedantic `unreadable_literal` in a `consume.rs` test. The other warnings in `main.rs`, `models.rs` and `output.rs` are on lines this run did not change.
- D-01: `git diff --quiet 0ed7d3f -- smartmodules/ transforms.yaml src/scorer.rs` passes. The `fluvio/` checkout and `.planning/config.json` were not touched.
- Content-freedom tests still pass: `decode_news_withholds_escaped_quote_content`, `stream_error_hint_withholds_quoted_value`, `diagnostic_line_strips_control_chars`, `social_text_is_not_truncated_in_output`.

## Fixed Issues

### WR-01: One poison record permanently halts the news branch

**Files modified:** `src/commands/consume.rs`
**Commit:** 308e775
**Status:** fixed: requires human verification (state-machine logic)
**Applied fix:** A previous run wrote this code, and this run kept it after review. When the chain rejects a record, `news_incoming` gives a diagnostic without content and reopens at the next offset, at most `MAX_REOPENS` (100) times. The fluvio SPU runs the chain over whole stored batches (checked in `fluvio-spu/src/smartengine/batch.rs` `process_batch`). So a reopen that fails again before its start offset is still inside the rejected record's batch. The branch then reports the start offset as skipped and moves one offset further. Any other stream error, a failed reopen, or reaching the limit ends the branch. Five unit tests cover this. The cluster test `good_news_after_poison_record_is_delivered` passes. This run also corrected the `social_incoming` doc comment, which still described the old shared behaviour. The cluster test passes whether or not the producer puts the second record in the poison record's batch. It does not show which of the two cases happened.

### WR-02: Consume exits 0 and overwrites prior output when streams fail

**Files modified:** `src/commands/consume.rs`, `src/output.rs`
**Commit:** 9382d43
**Status:** fixed: requires human verification (exit-status and file-handling logic)
**Applied fix:** Added `Incoming::Fatal` for a stream error that ends its branch, and `Collected::failed`. The new `finish_batch` does not write the file when a stream failed before any record arrived. When records arrived before a failure, it writes them. Either way the run returns `Err`. Streaming mode returns `Err` when a branch ended on an error. `save_json` now writes a temp file next to the target, syncs it, and renames it over the target, so a failed write keeps the old file. Tests added in both files.

### WR-03: `--enrich-ner` silently ignored in streaming mode

**Files modified:** `src/commands/consume.rs`, `src/main.rs`
**Commit:** ec72c90
**Applied fix:** Added `consume::check_ner_mode`, which rejects `--enrich-ner` with `--limit 0`. `main` calls it before the NER model loads, and `consume::run` calls it again. This matches the README's "in `--limit` mode only". Test added.

### WR-04: Social `published` validated but not normalized to UTC

**Files modified:** `src/commands/consume.rs`, `src/models.rs`
**Commit:** 4c2845e
**Applied fix:** Replaced `is_rfc3339` with `rfc3339_to_utc`. It parses RFC 3339, converts to UTC, and formats with `SecondsFormat::Millis`, the same format the epoch-ms social path uses. `decode_social` stores the converted value. A value that is not RFC 3339 still becomes null with a diagnostic. Added a `+01:00` case in both test modules. Note: a social `published` given without milliseconds now gets `.000` in the output.

### WR-05: Content-freedom of stream diagnostics depends on an unenforced cross-crate message convention

**Files modified:** `src/commands/consume.rs`
**Commit:** fc99d4e
**Status:** fixed: requires human verification (security logic)
**Applied fix:** This is the consumer-side fix that D-01 allows. `SmartModuleRuntimeError` and `SmartModuleLookBackError` hints now go through `module_hint_summary`. It prints only a listed module prefix (`MODULE_HINT_PREFIXES`) and a listed `serde_json` phrase (`SERDE_HINT_PHRASES`) that it matched, or `no detail`. It never copies text from the hint. `hint_summary` keeps its quote/backtick cut, now used only for fluvio-authored errors and failed reopens. Its doc comment no longer overstates serde's quoting. A new test feeds hints that put record content before any quote. A read-only drift test uses `include_str!` on the three module sources (the files are not changed) and checks that every `eyre!` message uses a listed prefix. Two existing tests were changed: their invented hints (`"bad"`, `"bad\u{1b}[2J tail"`) now correctly give `no detail`. The truncation test now calls `hint_summary` directly.

### WR-06: Undecodable records do not count toward `--limit`, so a batch run is unbounded

**Files modified:** `src/commands/consume.rs`
**Commit:** 7330aa6
**Status:** fixed: requires human verification (loop bound logic)
**Applied fix:** `collect_records` now stops after `diagnostic_budget(limit)` diagnostics and adds a closing line. The budget is `max(limit * 10, 100)`. It keeps at most `MAX_DIAGNOSTICS` (100) and counts the rest in `Collected::suppressed`. Fatal diagnostics and the closing line are always kept. `consume_batch` prints "N more diagnostics not shown". Stopping at the budget is not treated as a failed stream. `news_branch_reopens_at_most_max_times` now reads the branch directly, because the collector's budget would stop it first. Tests added for an endless stream of bad records and for keeping a fatal diagnostic past the cap.

---

_Fixed: 2026-11-04_
_Fixer: Claude (gsd-code-fixer)_
_Iteration: 1_
