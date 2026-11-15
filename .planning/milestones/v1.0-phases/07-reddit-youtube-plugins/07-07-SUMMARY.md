---
phase: 07-reddit-youtube-plugins
plan: 07
subsystem: plugins
tags: [reddit, wasm-component, cursor, saved-feed, home-feed, rust]

requires:
  - phase: 07-reddit-youtube-plugins
    provides: "plans 07-01 to 07-06: Reddit crate, Mark cursor, time-mark poll, response classification, compliance sweep, fixture manifest"
provides:
  - "feed (home or saved) and username settings, FeedKind (Listing, Home, Saved)"
  - "FeedState cursor value: untagged time mark or seen-fullname set (D-19), SEEN_SET_MAX = 25"
  - "home_request, saved_request, parse_saved in api.rs"
  - "poll dispatch by FeedKind: time marks for listings and home, stop-at-first-seen for saved"
  - "synthetic home_page and saved_page fixtures"
affects: [07-11 sample TOML and README (history scope, username setting), 07-12 real capture (assumptions A1, A2)]

actuals:
  tokens: 31000
  tasks: 2
  commits: 3
plan_head_before: ab7dc2739fc25b679e6a134c0f9e58e58f9c1c02
plan_head_after: e0fcc75b1c714743975cf389281e4e340e9afcfe

tech-stack:
  added: []
  patterns:
    - "Untagged serde enum per feed value keeps the old time-mark bytes valid while a second shape is added"
    - "A stored cursor value of the wrong shape for a feed is no state for it: first poll, stale value cleared"

key-files:
  created:
    - plugins/reddit/tests/fixtures/home_page.json
    - plugins/reddit/tests/fixtures/saved_page.json
  modified:
    - plugins/reddit/src/settings.rs
    - plugins/reddit/src/api.rs
    - plugins/reddit/src/cursor.rs
    - plugins/reddit/src/poll.rs
    - plugins/reddit/tests/replay.rs
    - plugins/reddit/tests/fixtures_manifest.rs
    - plugins/reddit/tests/fixtures/README.md

key-decisions:
  - "A saved or home child whose fullname is longer than 20 characters or has characters outside letters, digits and _ is treated as unusable (counted as skipped, not emitted, not remembered), so a hostile name cannot push the cursor past the host's 4096 bytes"
  - "Skipped non-t3 saved entries are counted only up to the stop point (the entries actually walked), the seen set is still taken from the whole current page"
  - "A saved page with a stored but empty seen set is state: every t3 entry of the next page is emitted"

patterns-established:
  - "User-feed 404 and 403 error texts name the feed key and the cause (settings or history scope), never the username or any body"

requirements-completed: [RDDT-02, TEST-01]

coverage:
  - id: D1
    description: "feed and username settings: exactly one of subreddits and feed, home or saved only, saved requires a validated username, listings rejected with feed"
    requirement: RDDT-02
    verification:
      - kind: unit
        ref: "plugins/reddit/src/settings.rs#feed_and_subreddits_are_mutually_exclusive_and_one_is_required, feed_accepts_home_and_saved_only, saved_requires_username_naming_the_field, username_is_validated_and_only_allowed_with_saved"
        status: pass
    human_judgment: false
  - id: D2
    description: "home feed polled by created_utc mark with the new sticky rule; saved feed polled by a seen-fullname set (old post saved later is emitted, oldest saved first, 25-name cap, empty page)"
    requirement: RDDT-02
    verification:
      - kind: integration
        ref: "plugins/reddit/tests/replay.rs#home_feed_uses_the_created_utc_mark_and_skips_stickied, saved_later_poll_emits_entries_until_the_first_already_seen_fullname, an_old_post_saved_later_is_emitted_even_though_created_utc_is_old, the_seen_set_is_capped_at_25_fullnames_and_the_cursor_fits"
        status: pass
    human_judgment: false
  - id: D3
    description: "user feeds are auth-required without a token before any request; time-mark cursor bytes unchanged"
    requirement: RDDT-02
    verification:
      - kind: integration
        ref: "plugins/reddit/tests/replay.rs#user_feed_without_a_token_is_auth_required_before_any_request, time_mark_cursor_json_layout_is_stable"
        status: pass
    human_judgment: false
  - id: D4
    description: "every request of every feed kind is official, authenticated and carries the enforced user agent; no private feed content or username in warnings or errors; manifest guards the two new fixtures"
    requirement: TEST-01
    verification:
      - kind: integration
        ref: "plugins/reddit/tests/replay.rs#every_request_is_official_authenticated_and_carries_the_enforced_user_agent, user_feed_logs_and_errors_carry_no_private_content; plugins/reddit/tests/fixtures_manifest.rs#every_required_fixture_case_exists_is_labelled_and_is_exercised"
        status: pass
    human_judgment: false
  - id: D5
    description: "Home endpoint (A1) and saved path, history scope and mixed t3/t1 kinds (A2) match real Reddit"
    verification: []
    human_judgment: true
    rationale: "Pinned only by synthetic fixtures; confirmed or corrected by the plan 07-12 real capture"

duration: 38min
completed: 2026-11-11
status: complete
---

# Phase 7 Plan 07: Reddit home and saved feeds Summary

**The `reddit-user` instance polls the logged-in home feed by a `created_utc` mark and saved posts by a 25-fullname seen set (D-19), answers `auth-required` without a login, and leaves every existing time-mark cursor byte-identical.**

## Performance

- **Duration:** 38 min
- **Tasks:** 2 (both TDD)
- **Files modified:** 9 (2 created)

## Accomplishments

- `settings.rs`: raw keys `feed` and `username`; `FeedKind { Listing, Home, Saved }`; feed keys `r/<sub>:<listing>`, `home`, `saved` (the key never holds the username). Exactly one of `subreddits` and `feed`; `listings` rejected with `feed`; `username` required for `saved`, rejected otherwise; username normalised by the same helper the contact validator now uses (3 to 20 of letters, digits, `_`, `-`, optional `u/` or `/u/`). No error echoes a value.
- `cursor.rs`: `FeedState` untagged enum (`Time(Mark)` serialised exactly as before, `Seen(SeenSet)` as `{"s":[..]}`), `SEEN_SET_MAX = 25`, `Cursor::seen`, `set_seen`, `clear`. `Cursor::mark` and `seen` return `None` for a value of the other shape.
- `api.rs`: `home_request` (`/new`), `saved_request` (`/user/<percent-encoded username>/saved`), `parse_saved` (children in listing order with their fullnames, comments included), shared `get_request` builder.
- `poll.rs`: dispatch by `FeedKind`. Home uses the time-mark path with the `new` sticky rule. Saved walks children in listing order, stops at the first already-seen fullname (any kind), emits the `t3` entries above it oldest-saved first, and stores the first 25 fullnames of the current page (an empty set for an empty page). A wrong-shape stored value is cleared and the feed is polled as a first poll.
- Tests: all 17 plan-named behaviors plus a wrong-shape self-heal test, cursor and api unit tests; the compliance sweep covers home and saved polls; the privacy test reads its sentinels from the fixture JSON and scans 40+ warning and error texts (success, empty, skipped comment, 401, 403, 404, 429, 500, 503, malformed, transport errors quoting the username); the manifest requires `home_page` and `saved_page`.

## Task Commits

1. **Task 1 RED:** `4c4cb07` test(07-07): failing tests for the home and saved user feeds (plus fixtures and README labels)
2. **Task 1 GREEN:** `edc519a` feat(07-07): poll the Reddit home and saved feeds for the reddit-user instance
3. **Task 2:** `e0fcc75` test(07-07): prove the user feeds stay official, authenticated and private

## TDD Gate Compliance

- RED (`4c4cb07`): 14 of the 58 replay tests failed, all with `settings: unknown field feed` (the `feed` and `username` settings did not exist), so each target test executed and failed for the missing behavior. `time_mark_cursor_json_layout_is_stable` also failed because its `home` half could not parse settings. Cargo's libtest text output is not a report format the `tdd-red-evidence` classifier supports, so the RED was assessed by hand from the real `cargo test --test replay` output (semantic assessment: target tests executed, failed on the planned missing feature, no compile, load or fixture fault). The four settings unit tests were added in the GREEN commit because they name `FeedKind`, which would have made the RED a build failure (INVALID_RED), not a failing test.
- GREEN (`edc519a`): all 34 lib, 2 manifest and 58 replay tests passed.
- Task 2 is cross-cutting proof tests over behavior Task 1 already delivered (the plan says no production changes), so there is no separate RED for it; the tests were written first and passed immediately, as intended.
- REFACTOR: none.

## Decisions Made

See `key-decisions` above. The plan's stated behaviors were implemented as written.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing critical functionality] Fullname bound for the seen set**
- **Found during:** Task 1
- **Issue:** The plan caps the seen set at 25 names but not the length of a name. A hostile or malformed response could carry very long `name` values and push the cursor over the host's 4096-byte cap, which makes the host disable the plugin (Pitfall 2, T-07-32 class).
- **Fix:** `parse_saved` accepts a fullname only when it is 1 to 20 characters of letters, digits and `_`; a child without an acceptable name is counted as skipped and is neither emitted nor remembered.
- **Files modified:** plugins/reddit/src/api.rs (with a unit test)
- **Commit:** edc519a

**2. [Rule 1 - Bug] User-feed 404 and 403 texts named the wrong cause**
- **Found during:** Task 1
- **Issue:** The 404 text said "check the subreddits setting" and the 403 text said "the subreddit may be private or banned", which are wrong for `home` and `saved`.
- **Fix:** `classify_response` takes a `user_feed` flag and names the feed and username settings for 404 and only the `history` scope for 403 on user feeds. Subreddit texts are unchanged.
- **Files modified:** plugins/reddit/src/poll.rs
- **Commit:** edc519a

**3. [Rule 3 - Blocking] Existing tests used the old `Cursor` map shape**
- **Found during:** Task 1
- **Issue:** `Cursor(BTreeMap<String, Mark>)` became `BTreeMap<String, FeedState>`, so four existing test sites that did `cursor.0.get(..)` or `cursor.0.insert(.., Mark)` no longer compiled.
- **Fix:** they use `Cursor::mark` and `set_mark`; no assertion changed.
- **Files modified:** plugins/reddit/src/cursor.rs (tests), plugins/reddit/tests/replay.rs
- **Commit:** edc519a

**Total deviations:** 3 auto-fixed (1 missing critical, 1 bug, 1 blocking). **Impact:** none on scope; the first closes a cursor-size hole the plan's cap did not cover.

## Issues Encountered

- `cargo fmt --check` reports diffs in `fixtures_manifest.rs` and `replay.rs` lines from earlier plans, so the crate was not auto-formatted (that would have touched unrelated code). New code was formatted by hand to the surrounding style.

## Known Stubs

None. The two new fixtures are intentionally synthetic and labelled so in `tests/fixtures/README.md`; plan 07-12 holds the real capture.

## Limitations (documented for plan 07-11)

- An un-saved then re-saved post whose fullname is still in the seen set is not re-emitted.
- If every remembered saved fullname is un-saved before the next poll, the whole page is emitted again (the host dedups by item id).
- Flagged assumptions A1 (home endpoint) and A2 (saved path, `history` scope, mixed `t3` and `t1` kinds) stay unconfirmed until plan 07-12.

## Threat Flags

None. No new network endpoint beyond the planned `oauth.reddit.com` paths; the username is validated and percent-encoded before it reaches a path (T-07-29), warnings and errors are fixed text (T-07-30), no dependency was added (T-07-SC: `git diff 75b078e -- wit Cargo.toml Cargo.lock` is empty).

## Verification

- `cargo test --target x86_64-apple-darwin` in `plugins/reddit`: 34 lib, 2 manifest, 59 replay tests pass, every test named in both tasks reports `ok`.
- `cargo component build --release` exits 0; `wasm-tools component targets ../../wit -w source ...reddit_source.wasm` exits 0; no `wasi:` import in the component WIT.
- All Task 1 and Task 2 acceptance greps pass.

## Self-Check: PASSED

- FOUND: plugins/reddit/tests/fixtures/home_page.json, plugins/reddit/tests/fixtures/saved_page.json
- FOUND commits: 4c4cb07, edc519a, e0fcc75 (all ancestors of HEAD)
