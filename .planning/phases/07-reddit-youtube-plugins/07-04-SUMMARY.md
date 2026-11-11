---
phase: 07-reddit-youtube-plugins
plan: 04
subsystem: plugins/reddit
tags: [reddit, rust, wasm-component, polling, cursor, fixtures, tdd]
status: complete
requires:
  - phase: 07-reddit-youtube-plugins
    provides: "plan 07-01 tracer: reddit crate, Http seam, settings, cursor Mark, single-feed poll"
provides:
  - "Complete D-08 item mapping with counted skips, raw size cap and listing-aware sticky rule"
  - "Per-feed high-water marks across several feeds with Cursor::mark, set_mark, retain_feeds"
  - "Synthetic fixtures: hot, empty, removed, newer, marked-post-gone"
affects: [07-06 status classification, 07-07 logged-in feeds, 07-11 README, 07-12 real capture]
tech-stack:
  added: []
  patterns:
    - "parse typed-by-hand with serde_json::Value, no unwrap or indexing on API data"
    - "warnings are fixed text carrying only counts"
    - "cursor starts from the stored marks and is pruned with retain_feeds, always Some(bytes)"
key-files:
  created:
    - plugins/reddit/tests/fixtures/listing_hot.json
    - plugins/reddit/tests/fixtures/listing_empty.json
    - plugins/reddit/tests/fixtures/listing_removed.json
    - plugins/reddit/tests/fixtures/listing_newer.json
    - plugins/reddit/tests/fixtures/listing_marked_post_gone.json
  modified:
    - plugins/reddit/src/api.rs
    - plugins/reddit/src/cursor.rs
    - plugins/reddit/src/poll.rs
    - plugins/reddit/tests/replay.rs
    - plugins/reddit/tests/fixtures/README.md
key-decisions:
  - "A sticky post left out of a `new` page by D-12 is a rule, not a skip, so it is not counted in the skipped warning"
  - "created_utc is accepted only in 0..=9_999_999_999 seconds, matching the cursor size budget's 10-digit mark assumption"
  - "A child with no permalink starting with `/` is skipped and counted, since the item url is SITE_BASE plus the permalink"
  - "Emission order is (created_utc as f64, fullname); the mark keeps whole seconds"
requirements-completed: [RDDT-01, TEST-01]
duration: 15 min
completed: 2026-11-11
actuals:
  tokens: 70000
  tasks: 2
  commits: 4
commits: 4
plan_head_before: 964becf69497d827360fbe1065467f229750f784
plan_head_after: a8aebea072dff043234c0fc53b22b5f672a2ccb7
---

# Phase 7 Plan 04: Reddit subreddit-feed behavior Summary

Reddit polling is now correct over time and across feeds: every post maps to a predictable item or a counted skip, each `r/<sub>:<listing>` feed keeps its own `created_utc` mark so a deleted marked post cannot re-emit a feed, `new` drops stickies while `hot` keeps them, and no hostile body can panic the plugin.

## Task Commits

| Task | Phase | Commit | Description |
|------|-------|--------|-------------|
| 1 | RED | 1652d09 | failing replay tests and all five synthetic fixtures |
| 1 | GREEN | 6b56eff | `parse_listing(body, Listing)`, `ParsedPage`, `RAW_MAX_BYTES`, counted-skip and raw-drop warnings |
| 2 | tests | da475d1 | tests pinning multi-feed marks, order, dedupe, error mapping |
| 2 | GREEN | a8aebea | `Cursor::mark/set_mark/retain_feeds`, stable `(created_utc, fullname)` order, documented limits |

## What was built

- `api.rs`: `Listing { New, Hot }`, `ParseError::Unreadable`, `ParsedPage { posts, skipped }`, `RAW_MAX_BYTES` (256 KiB), `MAX_CREATED_UTC`. The top level must be an object with `data.children` an array; otherwise the body is unreadable (an error, never an empty page). Per child: `t3` only, non-empty `name` and `title`, a `/`-prefixed `permalink`, and a finite `created_utc` in range; everything else is counted in `skipped`. `raw` above 256 KiB is dropped and the item kept.
- `poll.rs`: passes each feed's listing to `parse_listing`, maps `ParseError` to the fixed text "Reddit answer is not a readable listing", emits one skipped-children warning and one dropped-raw warning (numbers only), starts the next cursor from the stored marks, advances a feed's mark to the greatest emitted post, prunes with `retain_feeds`, and always returns `Some(bytes)`.
- `cursor.rs`: the three `Cursor` helpers and a unit test for them.
- Fixtures (all labelled synthetic in the README) and the tests named in both tasks' behavior lists, plus unit tests for the cursor helpers and the sticky and counted-skip rules in `api.rs`.

## TDD note

Task 1 followed RED then GREEN: at 1652d09 six replay tests failed on their intended assertions (sticky rule, skipped-child count, oversize raw, unreadable listing, hostile shapes), not on build errors. Task 2 had no RED gap: the plan 07-01 tracer plus Task 1 already satisfied every Task 2 behavior (all 13 tests passed on first run, the "unexpected green" case). They were investigated, found to be genuinely satisfied by existing code, and committed as `test(07-04)` regression pins before the helper refactor (`feat(07-04)`). No `type: tdd` gate or `workflow.tdd_mode` was configured, so no gate compliance section applies.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing critical] Permalink required for a usable post**
- **Found during:** Task 1
- **Issue:** the item `url` is `SITE_BASE` plus the permalink; a child without one would emit a bare-host url.
- **Fix:** a missing or non-`/`-prefixed permalink makes the child a counted skip.
- **Files modified:** plugins/reddit/src/api.rs
- **Commit:** 6b56eff

**2. [Rule 1 - Correction of test input] `created_utc` of `1e400`**
- **Found during:** Task 1
- **Issue:** the plan says a child with `1e400` is skipped while the others still map, but serde_json rejects the number at parse time (out of range), so the whole body is unreadable.
- **Fix:** the per-child skip test uses `1e300` and `10000000000` (parseable, out of range); `1e400` is asserted as `Err(Unreadable)` without a panic and is also in the poll-level hostile-shapes test.
- **Files modified:** plugins/reddit/src/api.rs, plugins/reddit/tests/replay.rs
- **Commit:** 6b56eff

**3. [Rule 3 - Blocking] The two Task 2 fixtures were committed with Task 1**
- **Found during:** Task 1
- **Issue:** the fixtures README lists all five fixtures and they were authored together.
- **Fix:** `listing_newer.json` and `listing_marked_post_gone.json` ship in 1652d09 rather than the Task 2 commit.
- **Commit:** 1652d09

**Total deviations:** 3 (one missing-critical addition, one test-input correction, one commit-grouping). **Impact:** none on behavior or scope.

## Verification

- `cargo test --target <host>`: all tests pass, every test named in both tasks reports `ok` (`/tmp/p7-04-verify.sh 1` and `2` both print `VERIFY_OK`).
- `cargo component build --release` exits 0 and `wasm-tools component targets ../../wit -w source ...reddit_source.wasm` exits 0 (the component still conforms).
- `git diff --quiet 75b078e -- wit Cargo.toml Cargo.lock` exits 0; `plugins/reddit/Cargo.toml` and `Cargo.lock` are unchanged (no dependency added).
- Acceptance greps: `RAW_MAX_BYTES` in api.rs, `retain_feeds` in cursor.rs and poll.rs, fixtures non-empty, README names `listing_removed` and `synthetic`, no `thread::sleep` under `plugins/reddit/src`.

## Known Stubs

None.

## Threat Flags

None. No new network endpoint, auth path, file access or schema surface; T-07-15 to T-07-19 mitigations are implemented and tested.

## Flagged assumptions carried forward

- A `t3` child always has `name`, `title` and a numeric `created_utc` (research A3) is pinned only by synthetic fixtures until plan 07-12.
- A same-second late arrival is not emitted (strict whole-second filter), a feed with more new posts than one page loses the oldest overflow, and `hot` is not a ranked re-poll; all three are documented on `poll` for the plan 07-11 README.

## Self-Check: PASSED

- Created files exist: five fixtures and this SUMMARY.
- Commits 1652d09, 6b56eff, da475d1 and a8aebea are ancestors of HEAD.
