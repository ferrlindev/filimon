# Phase 7: Reddit & YouTube Plugins - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-11-11
**Phase:** 7-Reddit & YouTube Plugins
**Areas discussed:** Reddit auth model, Feed config & item mapping, Cursor & first-poll backlog, Rate limits/quota & fixtures

---

## Reddit auth model

| Option | Description | Selected |
|--------|-------------|----------|
| PKCE login for everything | One Phase 6 flow, no host change; run-once needs login | |
| App-only for subreddits, PKCE for user feeds | .env secrets for subreddits, login for home/saved | ✓ |
| Plugin does app-only token via fetch | No host change; stateless plugin re-requests each poll | |

**User's choice:** App-only for subreddits, PKCE for user feeds.

Follow-ups:
- Where to add client_credentials → **Host `grant` key in `[host.oauth]`** (alternative: plugin via fetch).
- Token choice when logged in → config-driven by feed (alternative: prefer user token). Made moot by the next answer.
- Conflict raised: `poll` takes one `access-token`, WIT unchanged per Phase 6 D-10. Options: **two plugin instances** (chosen), change WIT to pass both tokens, host picks one token per source.

**Notes:** Result is `reddit` (app-only) and `reddit-user` (PKCE) sharing one .wasm.

---

## Feed config & item mapping

| Question | Chosen | Alternative |
|----------|--------|-------------|
| Reddit listings | `listings` list per instance | single `listing` as in the example |
| YouTube channels | Channel IDs only + subscriptions flag | accept @handles |
| YouTube instances | `youtube` + `youtube-user` | one instance on PKCE for everything |
| Reddit item | title=title, text=selftext (link posts: text=title) | text = title + selftext |
| YouTube item | text=description, raw=snippet, no videos.list | add videos.list for stats |

---

## Cursor & first-poll backlog

| Question | Chosen | Alternative |
|----------|--------|-------------|
| First poll | One page, then incremental | empty first poll; deep backfill |
| Cursor shape | JSON map of per-feed high-water marks | page until mark found |
| Deleted posts | Compare `created_utc` plus fullname | fullname only |

---

## Rate limits, quota & fixtures

| Question | Chosen | Alternative |
|----------|--------|-------------|
| Reddit rate limits | Reactive (429 / remaining<1 → rate-limited) | also pace within a poll |
| User-Agent | Setting-driven with enforced format | secret in .env |
| YouTube quotaExceeded | transient with named message | rate-limited with retry-after; fatal |
| Crate layout / tests | plugins/reddit + plugins/youtube, logic behind a trait, native tests | test via host only |
| Fixtures | Hand-sanitised real responses | synthetic from docs |

---

## Claude's Discretion

Serde/JSON crates, module split, fixture naming, page-size details beyond first poll, 401/403 and 5xx mapping, UA version source, example TOML updates.

## Deferred Ideas

Search and comments (v2), videos.list stats, @handle resolution, gap-free paging, intra-poll pacing.
