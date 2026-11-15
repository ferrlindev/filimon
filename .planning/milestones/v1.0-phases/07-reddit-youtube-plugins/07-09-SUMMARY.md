---
phase: 07-reddit-youtube-plugins
plan: 09
subsystem: plugins/youtube
tags: [youtube, subscriptions, quota, oauth, privacy, tdd]
requires:
  - phase: 07-reddit-youtube-plugins
    provides: "07-05 poll loop and cursor, 07-08 classify_youtube and fixture manifest"
provides:
  - "subscriptions and subscriptions_limit settings with capacity guard"
  - "subscriptions_request and parse_subscriptions_page (mine=true, order=relevance, bearer only)"
  - "quota_units_per_poll tested formula"
  - "combined explicit plus subscribed playlist polling with a page cap and subscribed-404 skip"
affects: [07-11, 07-12]
actuals:
  tokens: 14000
  tasks: 2
  commits: 4
tech-stack:
  added: []
  patterns: [endpoint-routed recording mock, privacy sentinels read from a fixture, hard page cap on API-supplied tokens]
key-files:
  created:
    - plugins/youtube/tests/fixtures/subscriptions_page.json
  modified:
    - plugins/youtube/src/settings.rs
    - plugins/youtube/src/api.rs
    - plugins/youtube/src/poll.rs
    - plugins/youtube/tests/replay.rs
    - plugins/youtube/tests/fixtures_manifest.rs
    - plugins/youtube/tests/fixtures/README.md
key-decisions:
  - "Settings values stay untyped (toml Value) so subscriptions and subscriptions_limit errors name the field and never echo the value (serde type errors quote it)."
  - "An empty subscriptions list still costs one subscriptions.list call, which the formula counts as zero; documented on quota_units_per_poll and not tested as a formula case."
  - "nextPageToken longer than 512 characters is treated as the end of the list so a hostile token cannot bloat the next url."
  - "The later-poll page size uses the worst-case playlist count (explicit channels plus subscriptions_limit), so it is 20 for a default subscriptions-only poll."
requirements-completed: [YTUB-02, YTUB-03, TEST-01]
duration: ~35 min
completed: 2026-11-11
status: complete
commits: 4
plan_head_before: ab7dc2739fc25b679e6a134c0f9e58e58f9c1c02
plan_head_after: 4ed693def63e0278c5f8c06e811c37d1864194c7
---

# Phase 7 Plan 09: YouTube subscriptions Summary

The `youtube-user` instance can now list the logged-in user's subscriptions (`subscriptions.list?mine=true&order=relevance`, bearer token only) and poll each subscribed channel's uploads playlist, bounded by `subscriptions_limit` and a hard page cap, with a tested quota formula and no subscription data in any warning or error.

## What was built

- `settings.rs`: `subscriptions` (bool) and `subscriptions_limit` (1 to `MAX_FEEDS`, default 50, only with `subscriptions = true`); `channels` is optional when subscriptions are on but one source is required; explicit channels plus the limit must fit `MAX_FEEDS` (error names `subscriptions_limit`); `Settings::subscriptions()`, `Settings::max_playlists()`, `quota_units_per_poll` and the constants `DEFAULT_SUBSCRIPTIONS_LIMIT`, `SUBSCRIPTIONS_PAGE_SIZE`, `MAX_SUBSCRIPTION_PAGES`. `valid_channel_id` is now `pub(crate)` so the response parser applies the same 24-character `UC` rule.
- `api.rs`: `subscriptions_request`, `parse_subscriptions_page` and `SubscriptionsPage`; `classify_youtube` takes `Option<&str>` so a 404 on the list call is the generic `Transient("YouTube HTTP 404")` and a 404 for a named channel stays the config error.
- `poll.rs`: `collect_subscriptions` (pagination to the limit, page cap with a fixed warning), combined deduplicated playlist list (explicit first), token required in subscription mode with no api-key fallback, subscribed-404 skip counted in one fixed-text warning; marks of channels that leave the window are dropped by the existing `retain_feeds`.
- Fixture `subscriptions_page.json` (synthetic, labelled in the README) and 21 new tests (8 unit, 13 replay) plus the manifest entry.

## TDD record

- Task 1 RED (`6e2ff7d`): 8 new unit tests failed on their planned assertions against compile-clean stubs (the settings still rejected the new keys, the formula returned 0, the request builder returned an empty url, the parser returned `Err`). The hostile-shape test passed against the stub, as its stub returns `Err(Unreadable)`. Semantic assessment: failures are behavior assertions, not compile or fixture faults.
- Task 1 GREEN (`58fe93c`): settings, builder, parser, formula; 22 unit tests pass.
- Task 2 RED (`c4b488c`): 13 new replay tests failed on assertions against the unchanged poll (subscription settings parsed but no requests were made).
- Task 2 GREEN (`4ed693d`): poll flow; 22 unit + 1 manifest + 52 replay tests pass.
- No REFACTOR commit. Rust has no RED-commit path gate (tests live beside source), so the `test(...)` commits touch `src/` stubs as well as tests.

## Verification

- `cargo test --target x86_64-apple-darwin` in `plugins/youtube`: 22 unit, 1 manifest, 52 replay tests pass; every test named in both tasks reports `ok`.
- `cargo component build --release` and `wasm-tools component targets ../../wit -w source` exit 0; no `wasi:` import in the component WIT.
- Acceptance greps pass; `git diff --quiet 75b078e -- wit Cargo.toml Cargo.lock` exits 0; no `thread::sleep` in `src`.

## Deviations from Plan

None - plan executed exactly as written. Notes:

- The plan listed `Option<bool>` and `Option<u32>` raw fields; they are `Option<toml::Value>` instead, validated by hand, so no error text echoes a rejected value (the existing `channels` key already did this). Same behavior, stricter privacy.
- `quota_formula_matches_the_requests_actually_made` covers accounts with at least one subscription; an empty account makes one call the formula counts as zero (documented on the function).
- Task 1 introduced the `classify_youtube` signature change inside its RED scaffold commit so the new `None` test could compile; the callers and tests from 07-08 were updated in the same commit.

## Known Stubs

None.

## Threat Flags

None. T-07-37 to T-07-41 are mitigated by the tests named in the plan; no new endpoint, auth path or trust boundary beyond them.

## Self-Check: PASSED

- `plugins/youtube/tests/fixtures/subscriptions_page.json` exists.
- Commits `6e2ff7d`, `58fe93c`, `c4b488c`, `4ed693d` are ancestors of HEAD.
