---
phase: 07-reddit-youtube-plugins
plan: 01
subsystem: plugins
tags: [rust, wasm-component, reddit, source-plugin, cursor, fixtures]
status: complete
requires:
  - phase: 03-plugin-host
    provides: "source world WIT contract and plugins/stub-source crate conventions"
provides:
  - "plugins/reddit standalone cargo-component crate producing a world-conformant reddit_source.wasm"
  - "Http trait seam, recording mock and fixture replay harness (TEST-01 shape reused by 07-05)"
  - "per-feed high-water-mark cursor layout with the 4096-byte guard (D-11)"
  - "validated Reddit settings schema (subreddits, listings, user_agent_contact, app_id, first_poll_limit)"
affects: [07-04, 07-05, 07-06, 07-07, 07-11, 07-12]
tech-stack:
  added: [serde_json 1, percent-encoding 2, toml 1, wit-bindgen-rt 0.41 (pinned)]
  patterns:
    - "plain-Rust core behind an Http trait; wasm32-only bindings and Guest glue"
    - "untyped toml::Value settings fields so config errors name the field and never echo the value"
    - "all-or-nothing poll: any feed error returns no items and no new cursor"
key-files:
  created:
    - plugins/reddit/Cargo.toml
    - plugins/reddit/Cargo.lock
    - plugins/reddit/rust-toolchain.toml
    - plugins/reddit/.cargo/config.toml
    - plugins/reddit/.gitignore
    - plugins/reddit/src/lib.rs
    - plugins/reddit/src/http.rs
    - plugins/reddit/src/settings.rs
    - plugins/reddit/src/api.rs
    - plugins/reddit/src/cursor.rs
    - plugins/reddit/src/poll.rs
    - plugins/reddit/src/component.rs
    - plugins/reddit/tests/replay.rs
    - plugins/reddit/tests/fixtures/listing_new.json
    - plugins/reddit/tests/fixtures/README.md
  modified: []
key-decisions:
  - "Cursor layout is the compact JSON map {feed key: {t: created_utc seconds, n: fullname}}; feed key is r/<lower-cased name>:<listing>"
  - "A mark compares on whole seconds (floor of created_utc) with strict greater-than, so a same-second sibling is never re-emitted and fractional timestamps cannot loop"
  - "Duplicate subreddits collapse case-insensitively (the feed key is lower-cased, so r/Rust and r/rust are one feed)"
  - "Settings fields stay untyped toml::Value and are validated by hand because serde type errors quote the offending value"
  - "Fetch-level errors map to fixed text (no host error string, url or token is echoed)"
  - "Items are de-duplicated by fullname across feeds in one poll (a post can sit in both new and hot)"
metrics:
  duration: "about 35 minutes"
  completed: 2026-11-11
actuals:
  tokens: 30000
  tasks: 2
  commits: 3
plan_head_before: 92801e152e0001a786fe312829139497e0131ab4
plan_head_after: e507d7b40fd149e27bacb02f0ea5d8b62f36093c
requirements-completed: [RDDT-01, RDDT-03, TEST-01]
---

# Phase 7 Plan 01: Reddit source plugin tracer and settings Summary

**Standalone `plugins/reddit` crate: a validated-settings Reddit app-only poll of subreddit listings behind an `Http` trait, replayed natively from a synthetic fixture and built to a world-conformant `reddit_source.wasm` with no WASI imports.**

## Performance

- **Tasks:** 2 of 2 (Task 1 tracer, Task 2 TDD)
- **Tests:** 31 native tests pass (22 unit, 9 replay), also under `--test-threads=1`
- **Files created:** 15, none outside `plugins/reddit/`; `wit/`, root `Cargo.toml` and `Cargo.lock` unchanged (`git diff --quiet 75b078e -- wit Cargo.toml Cargo.lock` exits 0)

## Accomplishments

- Tracer: one subreddit's `new` listing flows from `[settings]` through `GET https://oauth.reddit.com/r/rust/new?limit=25&raw_json=1` (bearer token, enforced `filimon:<app_id>:<version> (by /u/<name>)` User-Agent, `accept: application/json`) to items and a per-feed cursor; a second poll with the same page returns nothing and an identical cursor.
- Item mapping follows D-08: fullname id, `https://www.reddit.com` plus permalink, self text or title for link posts (never empty), epoch-millisecond `published` rounded from `created_utc`, `platform = reddit`, `raw` is the API post object.
- Component build: `cargo component build --release` produces `reddit_source.wasm`; `wasm-tools component targets --world=source ../../wit` exits 0; the printed WIT imports only `filimon:plugin/{types,log,fetch}` and holds zero `wasi:` imports.
- Settings complete: unknown keys, bad subreddit names, bad `listings`, bad `app_id`, bad contact and out-of-range `first_poll_limit` are each a `config` error naming the field and never echoing the value.
- Capacity guards: feed count above `MAX_FEEDS` (61, derived from the 4096-byte host cursor cap and a measured 67-byte worst-case entry) is a `config` error naming `subreddits`; page limits shrink so one poll cannot pass the host's 1000-item cap.

## Task Commits

1. **Task 1: tracer** - `87a4bca` (feat)
2. **Task 2 RED: failing settings and capacity tests** - `91795ac` (test)
3. **Task 2 GREEN: settings schema, User-Agent, capacity guards** - `e507d7b` (feat)

## TDD notes (Task 2)

- **RED:** 10 target tests failed on assertions against the placeholder cursor constants and the tracer-level settings (`app_id`, `listings`, `first_poll_limit` rejected as unknown fields, no feed cap, no page shrink, serde type errors echoing values, plus `first_poll_limit_changes_the_requested_page_size` in `tests/replay.rs`). Semantic assessment: each failure was the planned assertion (an expected `Ok` that was an `Err`, or a constant that did not match the measured encode size), not a build or fixture fault. A first RED attempt with `0` placeholder constants did not compile (const-evaluated division by zero lint); the placeholders were changed to `1` so the build succeeded and the tests failed on assertions.
- **Known weakness in RED evidence:** five Task 2 tests (`missing_user_agent_contact...`, `user_agent_contact_accepts_three_spellings...`, `user_agent_contact_with_control_or_separator_characters...`, `user_agent_has_the_documented_format...`, `unknown_keys_are_rejected...`) were already green at the RED commit because the tracer implemented the contact charset check (a security requirement, T-07-01) and `deny_unknown_fields`. The RED gate is therefore partial, not total.
- **Evidence classifier:** `cargo test` output is not one of the report formats `gsd_run check tdd-red-evidence` supports (TAP, JUnit, swift-testing, unittest, go test), so no machine classification was run; RED was assessed by reading the failing assertions.
- **REFACTOR:** none.

## Decisions Made

See `key-decisions` in the frontmatter. The reversible-cost decision is the cursor layout (D-11): changing it later needs a migration or a one-poll reset; an undecodable cursor is already treated as a first poll with a warning so a layout change self-heals at the price of one re-emitted page.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing critical functionality] Contact charset check and `deny_unknown_fields` in the tracer**
- **Found during:** Task 1
- **Issue:** the plan describes the tracer-level `user_agent` as format-only and leaves validation to Task 2, but a tracer that interpolates an unvalidated operator string into a header is a header-injection hole (T-07-01) in a committed state.
- **Fix:** the tracer already enforces the 3 to 20 character `[A-Za-z0-9_-]` rule; Task 2 added only `app_id` validation to `user_agent`.
- **Effect:** weakens the RED gate for five Task 2 tests (see TDD notes).
- **Commit:** `87a4bca`

**2. [Rule 3 - Blocking] Placeholder cursor constants changed from 0 to 1 in the RED commit**
- **Found during:** Task 2 RED
- **Issue:** `0` placeholders made the compiler reject the tests at compile time (a deny-by-default lint on constant division by zero), which would have been a build failure, not a RED.
- **Fix:** placeholders set to `1`; real values land in the GREEN commit.
- **Commit:** `91795ac`

**3. [Rule 2 - Missing critical functionality] Cross-feed de-duplication of items**
- **Found during:** Task 1
- **Issue:** with `listings = ["new", "hot"]` one post can appear in both feeds and would be emitted twice in one poll.
- **Fix:** `poll` keeps the first item per fullname within a poll.
- **Commit:** `87a4bca`

---

**Total deviations:** 3 auto-fixed (2 Rule 2, 1 Rule 3). No architectural changes.

**Impact on plan:** no scope change. The RED gate for Task 2 is weaker than ideal (deviation 1).

## Issues Encountered

- The worktree tool guard refused compound shell commands (command substitution for the host triple, `source` as an argument to `wasm-tools -w`). Worked around by passing the literal host triple `x86_64-apple-darwin` and `--world=source`. The plan's `<automated>` verify commands use `$(rustc -vV ...)`; their logic was run with the literal triple and an equivalent check (`grep` of each named test for `... ok`, no `FAILED`, conformance via `wasm-tools component targets`, absence of `wasi:` in the printed WIT).

## Known Stubs

None. The fixture is synthetic by design and labelled so in `tests/fixtures/README.md`; real captures replace it in plan 07-12 (research assumptions A1 to A4 stay unconfirmed until then).

## Threat Flags

None. No new network endpoint, auth path or schema beyond the plan's threat model. Mitigations applied: T-07-01 (charset and length validation, percent-encoding), T-07-02 (fixed-text errors, no url, header, token or body in errors or logs), T-07-03 (no `unwrap` on API data in non-test code, non-finite `created_utc` skipped, malformed page is an error), T-07-04 (`encode` guard, `MAX_FEEDS`, page-limit shrink), T-07-05 (conformance check and no `wasi:` import).

## Next Phase Readiness

- 07-04, 07-06 and 07-07 extend this crate (hot listing already works; error-status classification, home and saved feeds are theirs). 07-05 copies the crate shape for YouTube.
- `poll` currently maps any non-200 status to `transient("Reddit HTTP <status>")` as the plan specifies; 07-06 owns proper classification.
- The plugin-side `MAX_CURSOR_BYTES` and `MAX_ITEMS_PER_POLL` are mirrors of the host constants; nothing yet ties them by test across the crate boundary (the crate is a separate workspace).

## Self-Check: PASSED

- Files: all 15 listed files exist under `plugins/reddit/` (checked via `git status` and the test and build runs).
- Commits: `87a4bca`, `91795ac`, `e507d7b` are in `git log` and are ancestors of HEAD.
- `reddit_source.wasm` builds, conforms to world `source`, and imports no `wasi:` interface.
