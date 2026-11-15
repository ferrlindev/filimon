---
phase: 07-reddit-youtube-plugins
plan: 05
subsystem: plugins
tags: [rust, wasm-component, youtube-data-api, playlistItems, time, serde, cargo-component]

requires:
  - phase: 07-reddit-youtube-plugins
    provides: "plan 07-01 proven plugin shape (cfg-gated bindings, Http seam, native replay mock, one .wasm for both instances)"
provides:
  - "plugins/youtube crate (package youtube-source) building youtube_source.wasm for world source, no WASI imports"
  - "channels (UC ids only) to uploads playlist derivation (UC to UU), one playlistItems.list call per playlist"
  - "per-playlist publication-mark cursor, always Some(bytes), 4096-byte capacity guard"
  - "four synthetic fixtures plus the native replay suite (36 tests)"
affects: [07-08 quota and error classification, 07-09 youtube-user subscriptions, 07-11 plugin README, 07-12 real captures]

actuals:
  tokens: 33000
  tasks: 2
  commits: 3
plan_head_before: 964becf69497d827360fbe1065467f229750f784
plan_head_after: 45d3b596f239c6874bdda3c28a878a31472d84c4

tech-stack:
  added: [time 0.3 (parsing feature only), same serde/serde_json/toml/percent-encoding/wit-bindgen-rt 0.41 as the Reddit crate]
  patterns:
    - "Http trait seam with a recording mock; wasm fetch adapter compiled only for wasm32"
    - "all-or-nothing poll: any playlist error returns no items and no cursor"
    - "cursor mark carries forward when nothing newer is found; never a None cursor"

key-files:
  created:
    - plugins/youtube/Cargo.toml
    - plugins/youtube/Cargo.lock
    - plugins/youtube/rust-toolchain.toml
    - plugins/youtube/.cargo/config.toml
    - plugins/youtube/.gitignore
    - plugins/youtube/src/lib.rs
    - plugins/youtube/src/http.rs
    - plugins/youtube/src/settings.rs
    - plugins/youtube/src/api.rs
    - plugins/youtube/src/cursor.rs
    - plugins/youtube/src/poll.rs
    - plugins/youtube/src/component.rs
    - plugins/youtube/tests/replay.rs
    - plugins/youtube/tests/fixtures/playlist_page.json
    - plugins/youtube/tests/fixtures/playlist_newer.json
    - plugins/youtube/tests/fixtures/playlist_empty.json
    - plugins/youtube/tests/fixtures/playlist_placeholders.json
    - plugins/youtube/tests/fixtures/README.md
  modified: []

key-decisions:
  - "Video ids must be 1 to 11 characters of letters, digits, _ and -; anything else is treated as a placeholder, so a hostile id cannot inflate the 4096-byte cursor"
  - "Publication times are accepted only from 1970 to year 2286 (13-digit epoch milliseconds), which keeps WORST_ENTRY_BYTES (57) exact and MAX_FEEDS 71"
  - "Unknown settings keys return a fixed error text instead of serde's, which quotes the key"
  - "A single aggregated warning counts skipped entries; placeholders older than the mark are not re-counted every poll"

patterns-established:
  - "Mark = max (time, video id) over every entry newer than the old mark, placeholders included (research Pitfall 8)"
  - "parse_playlist_page reads serde_json::Value with .get(); a body without an items array is ParseError::Unreadable, never an empty page"

requirements-completed: [YTUB-01, TEST-01]

coverage:
  - id: D1
    description: "youtube plugin polls configured channels' uploads playlists with the API key header and maps entries to items (D-06, D-09, D-20)"
    requirement: "YTUB-01"
    verification:
      - kind: unit
        ref: "plugins/youtube/tests/replay.rs#first_poll_emits_one_page_for_the_channel"
        status: pass
      - kind: unit
        ref: "plugins/youtube/tests/replay.rs#request_targets_the_uploads_playlist_with_the_api_key_header"
        status: pass
      - kind: unit
        ref: "plugins/youtube/tests/replay.rs#item_mapping_follows_d09_with_d20_dates"
        status: pass
    human_judgment: false
  - id: D2
    description: "per-playlist marks: later polls emit only strictly newer videos, placeholders and odd times are survived, a failed channel fails the whole poll"
    requirement: "TEST-01"
    verification:
      - kind: unit
        ref: "plugins/youtube/tests/replay.rs#later_poll_emits_only_videos_strictly_newer_than_the_mark"
        status: pass
      - kind: unit
        ref: "plugins/youtube/tests/replay.rs#private_and_deleted_placeholders_are_skipped_but_still_advance_the_mark"
        status: pass
      - kind: unit
        ref: "plugins/youtube/tests/replay.rs#a_failed_channel_fails_the_whole_poll_with_no_items_and_no_new_cursor"
        status: pass
    human_judgment: false
  - id: D3
    description: "component builds for world source with no WASI imports"
    requirement: "YTUB-01"
    verification:
      - kind: other
        ref: "cargo component build --release && wasm-tools component targets ../../wit -w source target/wasm32-unknown-unknown/release/youtube_source.wasm"
        status: pass
    human_judgment: false

duration: 40min
completed: 2026-11-11
status: complete
---

# Phase 7 Plan 05: YouTube source plugin Summary

**Standalone `plugins/youtube` crate that polls channel uploads playlists through `playlistItems.list` with an `x-goog-api-key` header, tracks a per-playlist publication mark, and builds to a world-conformant `youtube_source.wasm` with no WASI imports.**

## Performance

- **Duration:** about 40 min
- **Tasks:** 2 of 2
- **Commits:** 3 (feat task 1, test RED for task 2, feat GREEN for task 2)
- **Files created:** 18

## Accomplishments

- Crate skeleton copied from `plugins/reddit` (own `[workspace]`, own `Cargo.lock`, pinned toolchain), `time` added with `default-features = false, features = ["parsing"]` for RFC 3339.
- `settings.rs`: `channels` only (24-character `UC` ids, handles rejected, no value echoed), `uploads_playlist` UC to UU swap with no lookup, `FIRST_POLL_LIMIT = 10`, `later_page_limit(n) = clamp(1000 / n, 1, 50)`, and a capacity guard (`MAX_FEEDS = 71`) naming `channels`.
- `api.rs`: `playlist_request` with the key or bearer token in a header only, a typed-free `Value` page parser (no `unwrap`, `expect` or indexing on API data), `parse_rfc3339_ms`, `video_to_item` with the D-20 date rule.
- `cursor.rs`: `{"<playlist id>":[<epoch ms>,"<video id>"]}`, `encode` refuses over 4096 bytes, `retain_feeds`.
- `poll.rs`: credential choice (token, else key, else `AuthRequired` before any request), first versus later page size, strict greater-than mark filter, oldest-first order with video-id tiebreak, placeholders skipped but advancing the mark, in-poll dedupe, undecodable cursor warning, always `Some(bytes)`.
- `component.rs` (wasm32 only): captures the `api_key` secret alias at `init`, logs fixed warning text only.
- Native suite: 11 unit tests and 25 replay tests, all green with `cargo test --target x86_64-apple-darwin`.

## TDD record

- **Task 1:** written as a single `feat` commit (a new crate has no failing-test starting point without stubs). Behaviors were checked by the Task 1 tests, all green on first full run.
- **Task 2 RED (`619a8bf`):** the 14 Task 2 tests were added against the Task 1 implementation. Six failed on their planned assertions, for the intended reason: `later_poll_emits_only_videos_strictly_newer_than_the_mark` (later page size 10 instead of 50), `private_and_deleted_placeholders_are_skipped_but_still_advance_the_mark` (no mark advance, no warning), `a_json_body_without_an_items_array_is_an_unreadable_page_not_an_empty_one` (text did not say unreadable page), `an_undecodable_cursor_is_a_first_poll_with_a_warning` (no warning), `channels_have_independent_marks` (later page size), and the extra `a_video_listed_by_two_playlists_is_emitted_once` (no dedupe). Semantic assessment: each target test ran and failed at the planned assertion, no compile, fixture or zero-test fault. The `gsd_run check tdd-red-evidence` classifier was not run (the plan is `type: execute`, not `type: tdd`, and Rust produces no supported report format).
- **Task 2 GREEN (`45d3b59`):** `poll.rs` rewritten; all 36 tests pass.
- Tests for behavior that Task 1 already implemented (`a_video_published_at_the_mark_instant...`, `the_mark_is_the_max_publication_time...`, `output_is_oldest_first...`, `an_empty_playlist_page...`, `an_all_empty_first_poll...`, `marks_for_playlists_no_longer_configured...`, `hostile_shapes...`, `fetch_level_errors...`, `a_failed_channel...`, `published_at_parsing...`) were green on arrival and pin that behavior rather than drive it. No REFACTOR commit was needed.

## Task Commits

1. **Task 1: crate and one channel end to end** - `5244ae8` (feat)
2. **Task 2 RED: failing tests and fixtures** - `619a8bf` (test)
3. **Task 2 GREEN: later polls, placeholders, capacity** - `45d3b59` (feat)

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing critical functionality] Bound the video id and the time range**
- **Found during:** Task 1 (cursor capacity, threat T-07-23)
- **Issue:** the cursor size guard assumes an 11-character video id and a 13-digit time. A hostile or odd API value (long id, year 9999) would make `encode` fail and turn a bad entry into a `Fatal` that stops the instance.
- **Fix:** an id outside 1 to 11 characters of `[A-Za-z0-9_-]` makes the entry a placeholder; a time outside 1970 to year 2286 does not parse (entry skipped and counted).
- **Files modified:** `plugins/youtube/src/api.rs`
- **Commit:** `5244ae8`

**2. [Rule 2 - Missing critical functionality] Fixed text for unknown settings keys**
- **Found during:** Task 1 (threat T-07-20)
- **Issue:** serde's `unknown field` message quotes the operator-supplied key, which may carry CR or LF into a logged config error.
- **Fix:** `Settings::parse` returns a fixed text for that case.
- **Files modified:** `plugins/youtube/src/settings.rs`
- **Commit:** `5244ae8`

**3. [Plan sequencing] Some Task 2 behavior landed in Task 1**
- The Task 1 action already required the strict mark filter, `map_http_error` and the unit tests; the cursor accessors and `retain_feeds` were written in Task 1 as well. Task 2 added the later page size, placeholder mark advance, warnings, unreadable-page text, undecodable cursor warning, dedupe and `retain_feeds` use in `poll.rs`.

**4. [Test addition] Extra tests beyond the plan's list**
- `a_video_listed_by_two_playlists_is_emitted_once` and `only_the_official_api_host_and_playlist_items_are_ever_requested` (the plan lists the host prohibition as `verification: test`), plus a settings-level duplicate collapse test named `settings_collapse_duplicate_channels_keeping_order`.

**Total deviations:** 2 auto-added (Rule 2), 2 plan notes. No impact on scope.

## Documented limits (for the plugin README, plan 07-11)

- A channel with more new videos than one page (`min(50, 1000 / channels)`) loses the oldest overflow.
- A video published at exactly the mark instant but added after the mark is not emitted (strict greater-than).
- Counting only entries newer than the mark means an old private video does not warn on every poll.

## Verification

- `cargo test --target x86_64-apple-darwin` in `plugins/youtube`: 11 unit + 25 replay tests pass, every test named in both tasks reports `ok`. The host triple was written literally because command substitution is blocked in this sandbox.
- `cargo component build --release` and `wasm-tools component targets ../../wit -w source .../youtube_source.wasm` exit 0; the printed WIT has `filimon:plugin/fetch` and no `wasi:` import.
- Acceptance greps: `[workspace]`, `x-goog-api-key` in `api.rs`, no `key=` in `src`, no `filimon::plugin` in core files, no `thread::sleep`, `retain_feeds` in cursor and poll, `synthetic` in the fixture README: all pass.
- `git diff --quiet 75b078e -- wit Cargo.toml Cargo.lock` exits 0.

## Known Stubs

None.

## Threat Flags

None. The plugin adds no surface beyond the plan's threat model (one outbound host, `www.googleapis.com`).

## Next Phase Readiness

- Plan 07-08 can add `quotaExceeded`, 401 and malformed fixtures and replace the generic `Transient("YouTube HTTP <status>")`.
- Plan 07-09 reuses `Auth::Bearer` and `playlist_request`.
- Research assumptions A9 to A13 (live `x-goog-api-key` support, UC to UU swap) remain unconfirmed until plan 07-12.

## Self-Check: PASSED

- All 18 created files exist under `plugins/youtube/`.
- Commits `5244ae8`, `619a8bf`, `45d3b59` are ancestors of HEAD.
