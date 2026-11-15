---
phase: 07-reddit-youtube-plugins
reviewed: 2026-11-11T00:00:00Z
depth: standard
files_reviewed: 40
files_reviewed_list:
  - examples/plugins/filimon.toml
  - examples/plugins/reddit-user.toml
  - examples/plugins/reddit.toml
  - examples/plugins/youtube-user.toml
  - examples/plugins/youtube.toml
  - plugins/reddit/.cargo/config.toml
  - plugins/reddit/Cargo.toml
  - plugins/reddit/rust-toolchain.toml
  - plugins/reddit/src/api.rs
  - plugins/reddit/src/component.rs
  - plugins/reddit/src/cursor.rs
  - plugins/reddit/src/http.rs
  - plugins/reddit/src/lib.rs
  - plugins/reddit/src/poll.rs
  - plugins/reddit/src/settings.rs
  - plugins/reddit/tests/fixtures_manifest.rs
  - plugins/reddit/tests/replay.rs
  - plugins/youtube/.cargo/config.toml
  - plugins/youtube/Cargo.toml
  - plugins/youtube/rust-toolchain.toml
  - plugins/youtube/src/api.rs
  - plugins/youtube/src/component.rs
  - plugins/youtube/src/cursor.rs
  - plugins/youtube/src/http.rs
  - plugins/youtube/src/lib.rs
  - plugins/youtube/src/poll.rs
  - plugins/youtube/src/settings.rs
  - plugins/youtube/tests/fixtures_manifest.rs
  - plugins/youtube/tests/replay.rs
  - src/commands/auth.rs
  - src/plugins/auth/client.rs
  - src/plugins/auth/error.rs
  - src/plugins/auth/login.rs
  - src/plugins/auth/pkce.rs
  - src/plugins/auth/provider.rs
  - src/plugins/auth/testing.rs
  - src/plugins/config.rs
  - src/plugins/mod.rs
  - tests/auth_cli.rs
  - tests/plugins_cli.rs
findings:
  critical: 0
  warning: 6
  info: 6
  total: 12
  necessity: 0
status: issues_found
---

# Phase 7: Code Review Report

**Reviewed:** 2026-11-11
**Depth:** standard
**Files Reviewed:** 40
**Status:** issues_found

## Summary

Reviewed the Reddit and YouTube plugin crates, the host `client_credentials` support (client, provider, config, CLI) and the sample registry. The security themes in the brief hold up well:

- Request URLs are built from a fixed official host plus validated, percent-encoded components.
- User-Agent and username values are charset-validated, so they cannot carry CR or LF.
- Error texts built from API bodies are fixed phrases or allow-listed reasons.
- Hostile JSON goes through `get`/`as_*` with no indexing or `unwrap`.
- Cursor size is bounded by `MAX_FEEDS`.
- Rate-limit arithmetic is saturating and clamped.
- The host never persists the app token, and the redirect and body-size caps are in place.

I found no critical defects. The warnings are robustness and design gaps:

- A rate-limited multi-feed Reddit poll can starve its later feeds.
- One bad feed can disable a whole plugin.
- YouTube's unstable subscription ordering churns cursors.
- Reddit's time-mark cursor does not bound the fullname length.
- A cached app token is never invalidated after an API 401.
- The `x-goog-api-key` header is not stripped on a cross-origin redirect (host code outside the diff, but it is the channel the plugin chose).

I did not run the code or tests.

## Warnings

### WR-01: Rate-limit handling aborts the whole poll, discards good pages, and can starve later feeds

**File:** `plugins/reddit/src/poll.rs:83-88` (and `plugins/reddit/src/settings.rs:185-190`)
**Issue:** `classify_response` returns `RateLimited` whenever `x-ratelimit-remaining < 1.0`, even on a 2xx response that holds a complete, valid page. `poll` then returns `Err` for the whole poll: no items, no cursor advance, and no progress kept for feeds already fetched. The next poll restarts at feed 1.

Reddit allows about 100 requests per 10-minute window. Settings validation allows up to `MAX_FEEDS` (61) feeds, and the sample interval is 300 s. A configuration of 50 or more feeds therefore issues over 100 requests per 600 s. Every second poll trips the limit part-way through the feed list, throws away everything fetched, and restarts from the top. The tail feeds are never completed, and the head feeds re-spend the budget each time. The rate-limit result is also reported against a response that succeeded, so the page that exhausted the budget is wasted.

**Fix:**
- Accept a 2xx page. Treat `remaining < 1` as a reason to stop issuing further requests after the current feed, and return the items and cursor collected so far (partial progress), with `RateLimited` carried only when nothing could be fetched.
- Alternatively, cap total feeds so that `feeds * (600 / poll_interval)` stays under 100, and document it.

```rust
// poll.rs: after a successful, parsed feed
let exhausted = parse_header_f64(&response.headers, "x-ratelimit-remaining")
    .is_some_and(|r| r < 1.0);
// ... finish processing this feed, then:
if exhausted { rate_limited_after = Some(reset); break; }
```

### WR-02: One failing feed disables the entire plugin (or wedges it) instead of skipping the feed

**File:** `plugins/reddit/src/poll.rs:92-101`, `plugins/youtube/src/api.rs:312-325`, `plugins/youtube/src/poll.rs:204-215`
**Issue:** The host disables a plugin permanently on `PollError::Config` or `Fatal` (`src/plugins/host/scheduler.rs:176-179`). The plugins map per-feed conditions to those classes:

- A single subreddit answering 404 (banned or removed) gives `Config`. The poll aborts, every other subreddit stops, and the plugin is disabled until restart.
- An explicit YouTube channel whose uploads playlist 404s does the same, as does a 403 `forbidden` or `insufficientPermissions` on one playlist.
- Conversely, a subscribed channel answering 403 with a reason such as `playlistItemsNotAccessible` falls to `Transient("YouTube HTTP 403 (...)")`. That abort is retried every cycle for as long as the channel stays in the 50-subscription window, so one inaccessible subscription blocks every other channel's items indefinitely.

The code already skips a 404 for subscribed channels, which shows the intent. It is applied inconsistently.

**Fix:**
- For a multi-feed poll, a per-feed 403 or 404 should be skipped, counted and warned about for subscribed feeds. Consider the same for explicit feeds, while keeping the first-ever poll strict so a typo still surfaces.
- At minimum, extend `subscription_only` skipping to 403 on a playlist request, and reserve `Config` for failures that apply to the whole poll (key invalid, API not enabled, 401 scope).

### WR-03: `order=relevance` subscription window is unstable, so marks are dropped and videos re-emitted

**File:** `plugins/youtube/src/api.rs:214`, `plugins/youtube/src/poll.rs:176-180`
**Issue:** With `subscriptions_limit` below the user's subscription count (very common; the default is 50), the polled set is the first N subscriptions in `order=relevance`. Relevance order is not stable between polls. A channel that leaves the window loses its mark through `next.retain_feeds(&playlist_ids)`. When it re-enters, it is a "first poll" again (`FIRST_POLL_LIMIT = 10`) and re-emits up to 10 already-delivered videos. Items are keyed only by `platform:id`, and the host has no dedup (`src/plugins/host/map.rs`, `sink.rs`), so the downstream stream gets duplicates every time a boundary channel flaps.

**Fix:** Use a deterministic order (`order=alphabetical`, or `unread`) so membership only changes when subscriptions change. Alternatively, retain marks for channels outside the current window (bounded by `MAX_FEEDS`) instead of dropping them.

### WR-04: Reddit time-mark cursor stores an unbounded API-supplied fullname

**File:** `plugins/reddit/src/api.rs:164-176`, `plugins/reddit/src/poll.rs:257-264`, `plugins/reddit/src/cursor.rs:16-19`
**Issue:** `parse_post` accepts any non-empty `name` (no length or charset check), and `poll` stores `post.name` as `Mark.n`. `WORST_ENTRY_BYTES`, `MAX_FEEDS` and the test `cursor_size_at_the_configured_cap_fits_the_host_limit` assume `n` is at most 13 bytes (`t3_` plus 10). The saved feed has `valid_fullname` (at most 20 characters, alphanumeric or `_`), but time-mark feeds do not. A response with one very long `name` makes `cursor::encode` exceed 4096 bytes. `poll` returns `Fatal`, the host disables the plugin, and the oversized post stays on the page so a restart would hit it again.

The data crosses a trust boundary (network), and the code comment on `MAX_FULLNAME_LEN` shows the author knew the risk for `saved`.

**Fix:** Apply the same validation in `parse_post`, so a bad post is counted as skipped instead of reaching the cursor or an item id.

```rust
// api.rs, parse_post
let name = text_of(data, "name");
if !valid_fullname(&name) || title.is_empty() || ... { return None; }
```

Then update `WORST_ENTRY_BYTES` to the 20-character bound, or keep 13 and tighten `valid_fullname` for `t3_` ids.

### WR-05: Cached app-only token is never invalidated when the API rejects it

**File:** `src/plugins/auth/provider.rs:173-216` (interaction with `plugins/reddit/src/poll.rs:91`)
**Issue:** For `client_credentials`, `app_token` serves the cached token until `expires_at - 60`. Nothing invalidates it if the API answers 401. The Reddit plugin maps 401 to `AuthRequired`, which the scheduler treats as a backoff, with the same cached token passed again on the next poll. The token is dropped only at expiry, about 24 h for Reddit. If the app is revoked, its secret rotated, or the token otherwise invalidated, the plugin fails repeatedly for up to a day even though a fresh token could be fetched immediately.

**Fix:** Give `TokenProvider` an `invalidate_app_token()` that clears the cache. Have the scheduler call it when a poll returns `AuthRequired` for a client-credentials plugin, so the next cycle fetches a new token.

### WR-06: `x-goog-api-key` is not dropped on a cross-origin redirect

**File:** `src/plugins/host/fetch.rs:393-395` (outside the reviewed diff; relied on by `plugins/youtube/src/api.rs:59-63`)
**Issue:** The host strips only `authorization`, `cookie` and `proxy-authorization` when a redirect changes origin. The YouTube plugin deliberately sends its API key in `x-goog-api-key` (to keep it out of URLs), and that header survives any redirect that the allowlist permits. `check_url` allows both `http` and `https`, so a redirect from `https://www.googleapis.com` to `http://www.googleapis.com` (a different origin, same allowed host) would forward the key in clear text. The practical likelihood is low, since Google does not redirect that way, but the key is the plugin's long-lived secret and the strip list is the only defence.

**Fix:** Add `x-goog-api-key` (and other well-known credential headers such as `x-api-key`) to the cross-origin drop list. Better, refuse an https-to-http redirect in `check_url` for the redirect path.

## Info

### IN-01: Stale `#[allow(dead_code)]` on `OAuthConfig::user_agent`

**File:** `src/plugins/config.rs:318-322`
**Issue:** The allow says "the token client reads it from plan 07-03", but `src/plugins/auth/client.rs:128-131` now reads `oauth.user_agent`. The suppression hides future genuine dead-code warnings.
**Fix:** Remove the attribute.

### IN-02: `build_oauth` re-implements every rule of `oauth_problems`

**File:** `src/plugins/config.rs:1117-1167` and `src/plugins/config.rs:1194-1295`
**Issue:** The validity rules (https endpoints, `client_id`, `redirect_port`, `user_agent`, grant-specific keys) are written twice. `build_oauth` runs only after `oauth_problems` returned empty, so its checks are redundant. A rule changed in one place can diverge silently: for example, `build_oauth` returns `None` on its own and the source then loads with `oauth = None` and no diagnostic.
**Fix:** Have `build_oauth` trust the validated section (using `expect` with the invariant) or derive both from one validator that returns the typed config or the problems.

### IN-03: Credential-carrying types derive `Debug`

**File:** `plugins/reddit/src/http.rs:6`, `plugins/youtube/src/http.rs:6`, `plugins/youtube/src/api.rs:49`
**Issue:** `HttpRequest` (headers with `authorization` or `x-goog-api-key`) and `Auth<'a>` (the raw key or token) derive `Debug`. Nothing prints them today, but a future `{:?}`, a `tracing` field, or a failed `assert_eq!` on a request would emit the credential. The host side redacts (`AccessToken`, `Secret`, `Verifier`), and both plugins state a never-log-credentials rule.
**Fix:** Implement `Debug` by hand to redact the credential header values and the `Auth` payload.

### IN-04: Reddit settings errors echo the unknown key; the YouTube plugin deliberately does not

**File:** `plugins/reddit/src/settings.rs:210`
**Issue:** `format!("settings: {}", e.message())` includes serde's "unknown field `<key>`" text, so an operator-supplied key containing control characters (a quoted TOML key can) reaches the error. The module header says errors never repeat the rejected value, and the YouTube plugin strips this text for exactly that reason. The host sanitizes guest text (`clean_guest_text`), so this is inconsistency rather than a hole, and `unknown_keys_are_rejected_naming_the_key` locks the behaviour in.
**Fix:** Either align Reddit with YouTube, or correct the module comment and keep the test.

### IN-05: Fixture "is exercised" check is a substring match

**File:** `plugins/reddit/tests/fixtures_manifest.rs:72-75`, `plugins/youtube/tests/fixtures_manifest.rs:57-60`
**Issue:** `replay.contains("\"<stem>\"")` is satisfied by a comment, a dead helper, or an unrelated string. It does not show that a test loads the fixture and asserts on it.
**Fix:** Require a call shape such as `fixture("<stem>")` inside a `#[test]` body, or drop the claim of "exercised".

### IN-06: `quota_units_per_poll` is public production code used only by tests

**File:** `plugins/youtube/src/settings.rs:31-35`
**Issue:** The function is `pub`, so it raises no dead-code warning, but its only callers are the unit test in `settings.rs` and `tests/replay.rs`. The plugin never reports or enforces quota. It also documents that it undercounts subscription mode for accounts with no subscriptions.
**Fix:** Move it under `#[cfg(test)]`, or use it (for example, log the estimated cost at `init`) so the formula stays tied to behaviour.

---

_Reviewed: 2026-11-11_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
