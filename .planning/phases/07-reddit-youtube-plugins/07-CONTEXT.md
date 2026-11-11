# Phase 7: Reddit & YouTube Plugins - Context

**Gathered:** 2026-11-11
**Status:** Ready for planning

<domain>
## Phase Boundary

Two reference `.wasm` source components (Reddit, YouTube), built separately from `fili` against the published WIT, stream real content through the host using official APIs only. Each is proven by offline fixture unit tests (no network, no keys). Covers RDDT-01..03, YTUB-01..03, TEST-01. Compiled-plugin integration in the host, live E2E, and the authoring guide are Phase 8.

</domain>

<decisions>
## Implementation Decisions

### Reddit auth model (RDDT-01, RDDT-02)
- **D-01:** Two auth modes, split by **plugin instance**, not by token choice inside one instance. Source `reddit` is **app-only** (subreddit listings, `.env` client_id/client_secret, no login). Source `reddit-user` is **PKCE login** (`fili auth login reddit-user`) for the home feed or saved posts. Both instances load the **same `.wasm`** with different plugin TOMLs. — **Reversibility:** costly — the instance names and TOML shape are what operators register in `filimon.toml`.
- **D-02:** The WIT is **unchanged** (`poll` keeps one `access-token`, Phase 1 D-07 / Phase 6 D-10). Each instance receives exactly one token. The user rejected a two-token WIT bump.
- **D-03:** The host gains a **`client_credentials` grant** in `[host.oauth]`, e.g. `grant = "client_credentials"` (default stays authorization-code PKCE). The host fetches, caches and refreshes the app token using the `client_secret_env` secret and hands the plugin only an access token. The plugin never sees client_id/secret for the token exchange. This **extends Phase 6 D-11** and touches Phase 6 host code inside Phase 7. — **Reversibility:** costly — published plugin TOML shape that authors write against.
- **D-04:** `reddit-user` with no stored login returns `auth-required` (Phase 3 D-03 backoff; Phase 6 D-09 points the user at `fili auth login`).

### Feed config (RDDT-01, RDDT-02, YTUB-01, YTUB-02)
- **D-05:** Reddit `[settings]` for the app-only instance: `subreddits = [...]` and `listings = ["new"]` (list; values `new`, `hot`; default `new`). This replaces the singular `listing` in `examples/plugins/reddit.toml`. The user instance sets `feed = "home"` or `"saved"` instead of subreddits.
- **D-06:** YouTube `[settings]`: `channels = ["UC..."]` **channel IDs only**. The plugin derives the uploads playlist (`UC` → `UU`) with no lookup call. `@handle` values are rejected with a `config` error naming the field. Subscriptions are `subscriptions = true` in the **`youtube-user`** instance (PKCE login); the `youtube` instance uses the API key and never needs login.
- **D-07:** Same instance pattern for YouTube: `youtube` (API key, channels) and `youtube-user` (PKCE, subscriptions), one `.wasm` for both.

### Item mapping
- **D-08:** Reddit item: `title` = post title; `text` = `selftext`, and for link posts `text` = the title so it is never empty. `url` = permalink for self posts (external link kept in `raw`). `author`, `published` from `created_utc` (convert to epoch ms). `raw` = the full API post object. `platform = "reddit"`, `id` = fullname.
- **D-09:** YouTube item: `title` = video title; `text` = description; `url` = watch URL; `author` = channel title; `published` = `publishedAt` (epoch ms); `raw` = the playlist-item `snippet`. **No `videos.list` call** (no stats), so each page costs 1 unit. `platform = "youtube"`, `id` = video id.

### Cursor and first poll
- **D-10:** First poll (cursor `none`) returns **one page** per feed: Reddit limit 25 (override `first_poll_limit`), YouTube 10 per playlist. Later polls return only items newer than the cursor. Not an empty first poll, not a deep backfill.
- **D-11:** The cursor is a **JSON map of per-feed high-water marks** (opaque bytes to the host, Phase 1 D-01). Reddit: feed key to `{newest fullname, created_utc}`. YouTube: playlist id to `{newest publishedAt, videoId}`. Each poll fetches **one page** and filters to items newer than the mark. A feed with more than a page of new items loses the oldest overflow; this limit is documented. — **Reversibility:** costly — stored cursor bytes are what restarts depend on; a layout change needs a migration or a reset.
- **D-12:** Reddit filtering compares **`created_utc`**, not only the fullname, so a deleted/removed marked post cannot cause a full re-emit. Stickied posts are skipped on `new` and kept on `hot`. Consequence to document: `hot` only emits posts newer than the mark, so it is not a ranked re-poll.

### Rate limits, quota, User-Agent (RDDT-03, YTUB-03)
- **D-13:** Reddit rate limiting is **reactive**. On HTTP 429, or a response whose `X-Ratelimit-Remaining` is below 1, return `rate-limited(Some(X-Ratelimit-Reset secs))`; the host backoff (Phase 3 D-01) does the waiting. The plugin is stateless and does not pace across polls or within one poll.
- **D-14:** The User-Agent is **setting-driven with an enforced format**. `[settings] user_agent_contact = "u/yourname"` is required; the plugin sends `filimon:<alias>:<version> (by /u/<contact>)`. A missing contact is a `config` error naming that field.
- **D-15:** YouTube `quotaExceeded` (HTTP 403, reason `quotaExceeded`) returns `transient` with a named message: "YouTube quota exceeded (quotaExceeded): daily Data API quota exhausted; resets at midnight Pacific". The host backoff retries and the plugin recovers after the reset without a restart. Never a silent empty poll (SC4). Only low-cost endpoints (`playlistItems.list`, `subscriptions.list`); **no `search.list`**.

### Crate layout and fixtures (TEST-01)
- **D-16:** Crates at **`plugins/reddit`** and **`plugins/youtube`**, each with its own empty `[workspace]`, own `Cargo.lock`, own toolchain pin, built with `cargo component` (like the stubs, Phase 1 D-16). Each builds **one** `.wasm` used by both of its instances.
- **D-17:** Parsing, cursor and error-mapping logic is plain Rust behind a small **`Http`-style trait**. Native `cargo test` replays fixtures through a mock of that trait; the wasm `fetch` import is a thin adapter. No network, no keys.
- **D-18:** Fixtures are **real API responses captured once, hand-sanitised** (tokens and PII stripped, trimmed) and committed under each crate's `tests/fixtures/`, with a short note on how they were recorded. Required cases: normal page, empty page, self post vs link post, deleted/removed post, 429 with rate-limit headers, `quotaExceeded` 403, 401/auth failure, malformed JSON.

### Claude's Discretion
- JSON/serde crate choices in the plugins, exact module split, and fixture file naming.
- Page-size parameters beyond the first-poll values, and exact `raw` serialisation.
- Mapping of 401/403 auth failures to `auth-required` vs `transient`, and of 5xx to `transient`.
- Plugin version string source for the User-Agent (crate version).
- Updating `examples/plugins/` (`reddit.toml`, `youtube.toml`, `filimon.toml`) and adding `reddit-user` / `youtube-user` examples.

### Carried forward
- One-token `poll(cursor, access-token)`; `auth-required` is backoff (Phase 1 D-07, Phase 3 D-03, Phase 6 D-10).
- Backoff semantics, cursor commit after flush, at-least-once delivery (Phase 3 D-01, D-02, D-07).
- Secrets per plugin alias only, redacted (Phase 1 D-12); `client_secret_env` never reaches the plugin (Phase 6 D-12).
- Items pass to `filimon-social-items` unchanged by the SmartModules (Phase 2 D-01); `published` is epoch ms (Phase 1 D-03).
- Plugins target `wasm32-unknown-unknown` with no WASI imports; all HTTP via the `fetch` import with the host allowlist.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Contract and config
- `wit/types.wit` — `item`, `poll-response`, `poll-error`, `request`/`response`, `fetch-error` the plugins code against
- `wit/worlds.wit` — `source` world (`init(settings, secrets)`, `poll(cursor, access-token)`)
- `src/plugins/config.rs` — `[host.oauth]` parsing and validation; where the `grant` key is added (D-03)
- `examples/plugins/filimon.toml`, `examples/plugins/reddit.toml`, `examples/plugins/youtube.toml` — registry and TOML shapes to update

### Existing plugins and host
- `plugins/README.md` — build prerequisites, `cargo component` layout, stub conventions
- `plugins/stub-source/` and `plugins/http-fetcher/` — Cargo.toml, toolchain pin and component structure to copy
- `src/plugins/host/` — poll loop, token provider (Phase 6), backoff; the `client_credentials` grant plugs in here
- `src/plugins/secrets.rs`, `src/plugins/error.rs` — secret redaction and diagnostics patterns

### Prior decisions
- `.planning/phases/01-plugin-contract-configuration/01-CONTEXT.md` — D-01 (cursor bytes), D-02/D-03 (item shape, epoch ms), D-04 (errors), D-05/D-06 (settings, secrets), D-16/D-17 (stub layout)
- `.planning/phases/02-sourceitem-pipeline/02-CONTEXT.md` — D-01, D-08 (social path and `SourceItem` mapping)
- `.planning/phases/03-component-host-runtime/03-CONTEXT.md` — D-01..D-08 (backoff, errors, state)
- `.planning/phases/06-host-managed-oauth/06-CONTEXT.md` — D-07..D-15 (token provider, `[host.oauth]` keys, refresh policy)

### Requirements and roadmap
- `.planning/REQUIREMENTS.md` — RDDT-01..03, YTUB-01..03, TEST-01
- `.planning/ROADMAP.md` §Phase 7 — success criteria SC1-SC5
- `.planning/STATE.md` §Blockers/Concerns — the Reddit OAuth question settled by D-01..D-03

### External (researcher to verify; not yet read)
- Reddit API docs (OAuth2, rate limits, `/r/{sub}/new`, `/saved`, API rules on User-Agent) and YouTube Data API v3 docs (`playlistItems.list`, `subscriptions.list`, quota costs, `quotaExceeded`)

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `plugins/stub-source` and `plugins/http-fetcher`: working `cargo component` crates (own workspace, toolchain pin) to copy as the Reddit/YouTube skeletons.
- Phase 6 token provider and `[host.oauth]` loader: base for adding the `client_credentials` grant.
- `fault-source` and stub fixtures in the host tests: patterns for exercising `rate-limited`/`transient` errors.

### Established Patterns
- Plugin parses its own `[settings]` TOML in `init` (Phase 1 D-05); secrets arrive as alias/value pairs.
- Host owns allowlist, timeout, TLS; plugins hold no state, so state lives only in the cursor.
- Errors are typed `poll-error` variants; config problems name field and plugin.

### Integration Points
- `examples/plugins/*.toml` (add `reddit-user`, `youtube-user`, `grant`, `listings`).
- Phase 6 token code in `src/plugins/` (grant type); poll loop supplies `access-token`.
- Phase 8 integration tests will load these compiled components.

</code_context>

<specifics>
## Specific Ideas

- Open for research: the host's request validation (Phase 5) must permit the plugin to set `User-Agent` (and `Authorization`) headers. Confirm before relying on D-14.
- Open for research: whether the registry allows two sources pointing at the same `.wasm` (D-01, D-07) and whether alias-keyed state/auth files stay distinct (cursor and token files are per alias, so likely yes).
- Open for research: Reddit's `client_credentials` grant for installed vs script app types, and whether `application-only` tokens can read `/r/{sub}/new` on `oauth.reddit.com`.
- Open for research: YouTube `subscriptions.list` pagination/quota cost and how new uploads are derived from subscriptions with low quota (one `playlistItems.list` per subscribed channel can be expensive).

</specifics>

<deferred>
## Deferred Ideas

- RDDT-04/05 (Reddit search, comments) and YTUB-04/05 (YouTube search, comments) — already v2 in REQUIREMENTS.md.
- `videos.list` stats and duration in `raw` — deferred; D-09 keeps quota at 1 unit per page.
- Resolving YouTube `@handle` to channel ID — deferred; IDs only for now.
- Paging until the cursor mark is found (gap-free for busy feeds) — deferred; D-11 accepts documented overflow loss.
- Intra-poll rate-limit pacing — deferred; D-13 is reactive only.

</deferred>

---

*Phase: 07-reddit-youtube-plugins*
*Context gathered: 2026-11-11*
