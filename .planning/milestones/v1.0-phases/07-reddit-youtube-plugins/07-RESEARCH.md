# Phase 7: Reddit & YouTube Plugins - Research

**Researched:** 2026-11-11
**Domain:** WASM component source plugins (Rust, `cargo component`, `wasm32-unknown-unknown`) over the Reddit Data API and YouTube Data API v3, plus a small host extension (OAuth `client_credentials` grant)
**Confidence:** MEDIUM overall. HIGH on host integration points and the build/test pattern (read in repo and executed this session). MEDIUM on API behaviour (official docs fetched, but nothing run live; no credentials in this environment). LOW on a handful of Reddit/YouTube runtime quirks, all listed in the Assumptions Log.

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Reddit auth model (RDDT-01, RDDT-02)**
- **D-01:** Two auth modes, split by **plugin instance**, not by token choice inside one instance. Source `reddit` is **app-only** (subreddit listings, `.env` client_id/client_secret, no login). Source `reddit-user` is **PKCE login** (`fili auth login reddit-user`) for the home feed or saved posts. Both instances load the **same `.wasm`** with different plugin TOMLs. Reversibility: costly, the instance names and TOML shape are what operators register in `filimon.toml`.
- **D-02:** The WIT is **unchanged** (`poll` keeps one `access-token`, Phase 1 D-07 / Phase 6 D-10). Each instance receives exactly one token. The user rejected a two-token WIT bump.
- **D-03:** The host gains a **`client_credentials` grant** in `[host.oauth]`, e.g. `grant = "client_credentials"` (default stays authorization-code PKCE). The host fetches, caches and refreshes the app token using the `client_secret_env` secret and hands the plugin only an access token. The plugin never sees client_id/secret for the token exchange. This **extends Phase 6 D-11** and touches Phase 6 host code inside Phase 7. Reversibility: costly, published plugin TOML shape that authors write against.
- **D-04:** `reddit-user` with no stored login returns `auth-required` (Phase 3 D-03 backoff; Phase 6 D-09 points the user at `fili auth login`).

**Feed config (RDDT-01, RDDT-02, YTUB-01, YTUB-02)**
- **D-05:** Reddit `[settings]` for the app-only instance: `subreddits = [...]` and `listings = ["new"]` (list; values `new`, `hot`; default `new`). This replaces the singular `listing` in `examples/plugins/reddit.toml`. The user instance sets `feed = "home"` or `"saved"` instead of subreddits.
- **D-06:** YouTube `[settings]`: `channels = ["UC..."]` **channel IDs only**. The plugin derives the uploads playlist (`UC` to `UU`) with no lookup call. `@handle` values are rejected with a `config` error naming the field. Subscriptions are `subscriptions = true` in the **`youtube-user`** instance (PKCE login); the `youtube` instance uses the API key and never needs login.
- **D-07:** Same instance pattern for YouTube: `youtube` (API key, channels) and `youtube-user` (PKCE, subscriptions), one `.wasm` for both.

**Item mapping**
- **D-08:** Reddit item: `title` = post title; `text` = `selftext`, and for link posts `text` = the title so it is never empty. `url` = permalink for self posts (external link kept in `raw`). `author`, `published` from `created_utc` (convert to epoch ms). `raw` = the full API post object. `platform = "reddit"`, `id` = fullname.
- **D-09:** YouTube item: `title` = video title; `text` = description; `url` = watch URL; `author` = channel title; `published` = `publishedAt` (epoch ms); `raw` = the playlist-item `snippet`. **No `videos.list` call** (no stats), so each page costs 1 unit. `platform = "youtube"`, `id` = video id.

**Cursor and first poll**
- **D-10:** First poll (cursor `none`) returns **one page** per feed: Reddit limit 25 (override `first_poll_limit`), YouTube 10 per playlist. Later polls return only items newer than the cursor. Not an empty first poll, not a deep backfill.
- **D-11:** The cursor is a **JSON map of per-feed high-water marks** (opaque bytes to the host, Phase 1 D-01). Reddit: feed key to `{newest fullname, created_utc}`. YouTube: playlist id to `{newest publishedAt, videoId}`. Each poll fetches **one page** and filters to items newer than the mark. A feed with more than a page of new items loses the oldest overflow; this limit is documented. Reversibility: costly, stored cursor bytes are what restarts depend on; a layout change needs a migration or a reset.
- **D-12:** Reddit filtering compares **`created_utc`**, not only the fullname, so a deleted/removed marked post cannot cause a full re-emit. Stickied posts are skipped on `new` and kept on `hot`. Consequence to document: `hot` only emits posts newer than the mark, so it is not a ranked re-poll.

**Rate limits, quota, User-Agent (RDDT-03, YTUB-03)**
- **D-13:** Reddit rate limiting is **reactive**. On HTTP 429, or a response whose `X-Ratelimit-Remaining` is below 1, return `rate-limited(Some(X-Ratelimit-Reset secs))`; the host backoff (Phase 3 D-01) does the waiting. The plugin is stateless and does not pace across polls or within one poll.
- **D-14:** The User-Agent is **setting-driven with an enforced format**. `[settings] user_agent_contact = "u/yourname"` is required; the plugin sends `filimon:<alias>:<version> (by /u/<contact>)`. A missing contact is a `config` error naming that field.
- **D-15:** YouTube `quotaExceeded` (HTTP 403, reason `quotaExceeded`) returns `transient` with a named message: "YouTube quota exceeded (quotaExceeded): daily Data API quota exhausted; resets at midnight Pacific". The host backoff retries and the plugin recovers after the reset without a restart. Never a silent empty poll (SC4). Only low-cost endpoints (`playlistItems.list`, `subscriptions.list`); **no `search.list`**.

**Crate layout and fixtures (TEST-01)**
- **D-16:** Crates at **`plugins/reddit`** and **`plugins/youtube`**, each with its own empty `[workspace]`, own `Cargo.lock`, own toolchain pin, built with `cargo component` (like the stubs, Phase 1 D-16). Each builds **one** `.wasm` used by both of its instances.
- **D-17:** Parsing, cursor and error-mapping logic is plain Rust behind a small **`Http`-style trait**. Native `cargo test` replays fixtures through a mock of that trait; the wasm `fetch` import is a thin adapter. No network, no keys.
- **D-18:** Fixtures are **real API responses captured once, hand-sanitised** (tokens and PII stripped, trimmed) and committed under each crate's `tests/fixtures/`, with a short note on how they were recorded. Required cases: normal page, empty page, self post vs link post, deleted/removed post, 429 with rate-limit headers, `quotaExceeded` 403, 401/auth failure, malformed JSON.

### Claude's Discretion
- JSON/serde crate choices in the plugins, exact module split, and fixture file naming.
- Page-size parameters beyond the first-poll values, and exact `raw` serialisation.
- Mapping of 401/403 auth failures to `auth-required` vs `transient`, and of 5xx to `transient`.
- Plugin version string source for the User-Agent (crate version).
- Updating `examples/plugins/` (`reddit.toml`, `youtube.toml`, `filimon.toml`) and adding `reddit-user` / `youtube-user` examples.

### Deferred Ideas (OUT OF SCOPE)
- RDDT-04/05 (Reddit search, comments) and YTUB-04/05 (YouTube search, comments) - already v2 in REQUIREMENTS.md.
- `videos.list` stats and duration in `raw` - deferred; D-09 keeps quota at 1 unit per page.
- Resolving YouTube `@handle` to channel ID - deferred; IDs only for now.
- Paging until the cursor mark is found (gap-free for busy feeds) - deferred; D-11 accepts documented overflow loss.
- Intra-poll rate-limit pacing - deferred; D-13 is reactive only.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| RDDT-01 | Reddit plugin polls new/hot posts from configured subreddits via the official API | App-only token via host `client_credentials` grant (confirmed for web/script apps); `GET https://oauth.reddit.com/r/{sub}/{new,hot}`; listing shape, `raw_json=1`, stickied/`kind` filtering (Standard Stack, Pitfalls 4-6, Code Examples) |
| RDDT-02 | With a logged-in user, Reddit plugin polls home feed / saved posts | `reddit-user` instance with `[host.oauth]` code flow, scopes `read` + `history`, `duration=permanent`; saved needs username and a different cursor rule (Tension T1) |
| RDDT-03 | Honours Reddit rate-limit headers, compliant User-Agent | `X-Ratelimit-*` headers (float-tolerant parse), UA format from Reddit API rules, host passes `user-agent` and `authorization` (confirmed), host token-call UA gap (Open Item O5) |
| YTUB-01 | Polls new uploads from configured channels via uploads playlists | `playlistItems.list` 1 unit/call, UC to UU derivation (undocumented convention, Tension T3), key sent as `x-goog-api-key` header |
| YTUB-02 | With a logged-in user, polls new uploads from subscriptions | `subscriptions.list?mine=true` (1 unit/page of 50) then one `playlistItems.list` per channel; quota arithmetic and cursor-size cap (Open Item O4, Pitfall 3) |
| YTUB-03 | Stays within daily quota; surfaces quota errors clearly | Costs verified; `quotaExceeded`/`dailyLimitExceeded`/`rateLimitExceeded` error reasons verified; named transient error (Code Examples) |
| TEST-01 | Unit tests against recorded fixtures, no network or keys | Verified native-test pattern: cfg-gated bindings + `cargo test --target <host triple>` (executed this session); mock `Http` trait; fixture set and capture recipes |
</phase_requirements>

## Summary

Phase 7 has two halves. The larger half is two new `cargo component` crates (`plugins/reddit`, `plugins/youtube`) whose logic is plain Rust behind an `Http` trait, so the whole poll/cursor/error pipeline is unit-testable natively with fixtures. The smaller but riskier half is a host change (D-03): a `client_credentials` grant in `[host.oauth]`, which touches `src/plugins/config.rs`, `src/plugins/auth/{client,provider,login}.rs`, `src/commands/auth.rs` and the sample-registry tests. Every open item in CONTEXT is now resolved: the host already permits `user-agent` and `authorization`, the registry already allows two sources on one `.wasm`, Reddit's `client_credentials` works for web/script apps (not installed apps) with HTTP Basic, and YouTube subscription polling costs one `playlistItems.list` per subscribed channel per poll (the dominant quota and cursor-size constraint).

Build and test mechanics are settled by experiment: a crate with `crate-type = ["cdylib", "rlib"]`, the generated `bindings` module gated by `#[cfg(target_arch = "wasm32")]`, builds a conforming component with `cargo component build --release` and runs native unit and integration tests with `cargo test --target x86_64-apple-darwin`. A bare `cargo test` fails because the crate's `.cargo/config.toml` forces the wasm target (exit status 126), so the planner must use the explicit host-triple command everywhere.

Five findings need the planner's or user's attention (details in Tensions and Open Questions): (1) the 4096-byte host cursor cap and the 1000-item per-poll cap bound how many subscriptions/feeds a plugin can safely track; (2) D-12's `created_utc` high-water mark is semantically wrong for the Reddit `saved` feed (ordered by save time); (3) the UC-to-UU uploads-playlist derivation is an undocumented community convention; (4) the host's token-endpoint calls send `fili/<version>` as User-Agent, which does not satisfy Reddit's required format, and D-14's `<alias>` is not knowable inside the plugin; (5) Reddit closed self-service API access in November 2025, so the user may be unable to obtain credentials for D-18 real captures and Phase 8 live runs.

**Primary recommendation:** Build two cfg-gated "core + thin adapter" crates, add the `client_credentials` grant (with an in-memory token cache and an optional host-side `user_agent` key) as one host task, resolve Tensions T1-T4 in the plan before writing code, and use the explicit host-triple `cargo test` command as the per-task verify.

## Project Constraints (from CLAUDE.md)

`.claude/CLAUDE.md` only imports `AGENTS.md` and `WORKFLOW.md` (the GSD Path pipeline rules). The actionable directives for this phase:
- Claims need checked sources; never report a command as passing unless it ran; report partial or failed results plainly (AGENTS.md "Evidence and honesty").
- Preserve uncertainty as `NEEDS-USER` / `RESEARCH` / confidence level; record user corrections and vetoes verbatim.
- No `.project/` directory exists in this repo, so the Path pipeline is not active for this work; the GSD `.planning/` flow governs.
- From STATE.md and the orchestrator: build on the uncommitted `fluvio-wise` work; never modify the `fluvio/` reference checkout; do not create, rename or switch git branches; do not fix `fluvio-wise` gaps outside this milestone.
- Host code (root crate) carries `[lints.clippy] all = "warn"` and `pedantic = "warn"` (Cargo.toml); new host code in `src/plugins/` should be clippy-pedantic clean. Plugin crates are separate workspaces and carry no lint table in the stub.

## Resolution of CONTEXT "Open for research" items

| # | Item | Verdict | Evidence |
|---|------|---------|----------|
| O1 | Host request validation (Phase 5) must permit the plugin to set `User-Agent` and `Authorization` | **CONFIRMED** | `src/plugins/host/fetch.rs:61-70` holds the full deny list, verbatim: `const FORBIDDEN_REQUEST_HEADERS: [&str; 8] = ["host", "content-length", "transfer-encoding", "connection", "proxy-connection", "upgrade", "te", "trailer"];`. Neither `user-agent` nor `authorization` is in it. `Egress::client` (fetch.rs:300-309) builds `reqwest::Client::builder().redirect(reqwest::redirect::Policy::none())` with no default `user_agent`, so the plugin's header is the only UA on the wire. Redirect hops strip `authorization`, `cookie`, `proxy-authorization` only when the origin changes (fetch.rs:393-395). Response headers are returned to the plugin lower-cased (`name.as_str()`, fetch.rs:405-415), so read `x-ratelimit-*` by lower-case name. A non-redirect non-2xx response (429, 403, 401) is returned as an ordinary response with status and headers (README "There is no per-source request rate limiter"). [VERIFIED: src/plugins/host/fetch.rs read this session] |
| O2 | Can the registry list two sources pointing at the same `.wasm`; do alias-keyed state/auth files stay distinct | **CONFIRMED** | Registry validation rejects only a source name that duplicates a fetcher name (`config.rs:834-845`, `ConfigError::DuplicateName`); there is no component-path uniqueness check. `load_components` (loader.rs:79-100) loops sources by name and compiles `source.component` per source, so two aliases on one file each get their own `InstancePre` (compiled twice; a start-up cost only). Cursor file is `dir.join(format!("{alias}.json"))` (cursor.rs:172, 261), token file is `<state-dir>/auth/<alias>.json` (store.rs `path_with`). Names accept `[a-z0-9_-]` (config.rs:805-811: `.bytes().all(\|b\| b.is_ascii_lowercase() \|\| b.is_ascii_digit() \|\| b == b'_' \|\| b == b'-')`), so `reddit-user` and `youtube-user` are valid. [VERIFIED: files read this session] |
| O3 | Reddit `client_credentials` for installed vs script app types; can app-only tokens read `/r/{sub}/new` on `oauth.reddit.com` | **CONFIRMED with a constraint** | Reddit's OAuth2 wiki: `client_credentials` is for "Confidential clients (web apps / scripts)"; installed apps "have no secret, and thus, are ineligible for `client_credentials` grant" and must use `https://oauth.reddit.com/grants/installed_client` with a `device_id`. Token endpoint `https://www.reddit.com/api/v1/access_token`, HTTP Basic (client_id : client_secret), body `grant_type=client_credentials`; app-only tokens "never receive a refresh_token"; "All bearer tokens expire after 1 hour"; API calls go to `https://oauth.reddit.com` with `Authorization: bearer TOKEN`. PRAW docs: web and script apps use client credentials for app-only/read-only access and installed apps cannot. Therefore register the Reddit app as type **web app** (it has a secret AND a redirect URI): one registration serves `client_credentials` for `reddit` and the authorization-code flow (Basic with the secret) for `reddit-user`. Installed-app (no secret) registrations are not supported by D-03 as written. Reading public subreddit listings with an app-only token is the PRAW "read-only" use case; PRAW's page does not state `/r/{sub}/new` explicitly, so first fixture capture must confirm (A3). [CITED: github.com/reddit-archive/reddit/wiki/OAuth2; praw.readthedocs.io/en/stable/getting_started/authentication.html] |
| O4 | YouTube `subscriptions.list` pagination/quota; how to derive new uploads from subscriptions cheaply | **RESOLVED (cost is high; see O4 detail)** | `subscriptions.list`: 1 unit per call, `maxResults` 0-50 default 5, `mine=true` requires OAuth, `pageToken` pagination, `order` = `alphabetical` / `relevance` / `unread`. `playlistItems.list`: 1 unit per call, `maxResults` 0-50 default 5. Default quota 10,000 units/day, resets midnight Pacific. There is no batch uploads call: one `playlistItems.list` per subscribed channel per poll. [CITED: developers.google.com/youtube/v3/determine_quota_cost, /docs/subscriptions/list, /docs/playlistItems/list] |
| O5 | (Added) Host-side token calls and Reddit's User-Agent rule | **GAP FOUND** | `TokenClient::build` hard-codes `.user_agent(concat!("fili/", env!("CARGO_PKG_VERSION")))` (client.rs:129). Reddit's API rules require `<platform>:<app ID>:<version string> (by /u/<reddit username>)` and prohibit spoofing; the token endpoint is Reddit's, so `fili/0.3.0` does not meet the rule. Phase 6 research already deferred this to Phase 7 (06-RESEARCH.md Open Question 3). See Tension T4. [VERIFIED: client.rs read; CITED: github.com/reddit-archive/reddit/wiki/API] |

### O4 detail: subscriptions quota arithmetic

Per poll cost in units = `ceil(S/50)` (subscriptions pages) + `C` (one `playlistItems.list` per distinct channel, `C = S` when only subscriptions). Daily cost = polls/day x per poll cost. Against the default 10,000 units/day: [ASSUMED arithmetic on cited costs]

| Channels polled (C) | Interval | Polls/day | Units/day | Verdict |
|---|---|---|---|---|
| 20 | 900 s (sample) | 96 | 1,920 + subs pages | fine |
| 50 | 900 s | 96 | 4,800 + 96 | fine |
| 100 | 900 s | 96 | 9,600 + 192 = 9,792 | exhausts quota |
| 100 | 1800 s | 48 | 4,896 | fine |

Cheaper derivations considered: `subscriptions` `contentDetails.newItemCount` and `activityType` appear in the resource schema (`contentDetails.newItemCount`: "The number of new items in the subscription since its content was last read"), but the docs page shows no deprecation notice and no behaviour guarantee, and the count is per-user "read" state rather than a stable change signal, so do not rely on it (A14). `channels.list` (1 unit, batch of up to 50 ids, `statistics.videoCount`) could detect which channels changed before spending a `playlistItems.list` each, but needs per-channel counts in the cursor, which collides with the 4096-byte cap (Pitfall 3) and with D-11's cursor layout; keep it as a deferred optimisation, not Phase 7 scope. Recommended Phase 7 mitigation within the locked decisions: add a `max_channels` / `subscriptions_limit` setting (default 50) so one poll's cost and cursor size are bounded, document the formula, and order subscriptions by `relevance` so the cap keeps the user's most relevant channels.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Fetch/refresh app-only Reddit token (`client_credentials`) | Host (`src/plugins/auth`) | - | D-03: plugin never sees client id/secret; host owns token endpoints and TLS |
| User login (PKCE code flow) for `reddit-user`/`youtube-user` | Host (`fili auth login`, Phase 6) | - | Phase 6 complete; Phase 7 only adds sample `[host.oauth]` blocks |
| Allowlist, timeout, TLS, redirect handling on API calls | Host fetch layer (Phase 5) | Fetcher component | Host enforces regardless of fetcher; plugin only builds requests |
| Build HTTP requests (URL, query, headers, UA, auth header) | Plugin (core module) | - | Plugin owns API shape; host forbids only framing headers |
| Parse API JSON, map to WIT `item`, build `raw` | Plugin (core module) | - | D-08/D-09; host passes items through unchanged |
| Cursor (per-feed high-water marks) | Plugin (opaque bytes) | Host stores verbatim, caps at 4096 B | D-11; host never interprets cursor |
| Rate-limit/quota error classification | Plugin | Host backoff | D-13/D-15: plugin returns typed `poll-error`; host owns the waiting |
| Backoff, polling interval, cursor commit after flush | Host scheduler (Phase 3) | - | Carried forward, unchanged |
| Settings parsing/validation (`subreddits`, `channels`, UA contact) | Plugin `init` | Host passes raw `[settings]` TOML | Phase 1 D-05; errors are `config(string)` naming the field |
| Fixture replay / offline tests | Plugin crate native tests | - | D-17; no network, no keys |
| Registry/sample TOML + `.env.example` + sample-registry tests | Repo `examples/plugins` + `src/plugins/mod.rs` tests | - | Existing tests assert exact secret sets and source names; must be updated |

## Standard Stack

### Core (plugin crates)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `wit-bindgen-rt` | `0.41` (features `["bitflags"]`) | Runtime for `cargo component`-generated bindings | Keep identical to `plugins/stub-source/Cargo.toml` and its `Cargo.lock` (`wit-bindgen-rt` 0.41.0); the registry's latest is 0.44.0 but cargo-component 0.21.1 generates against 0.41, so do not bump. [VERIFIED: plugins/stub-source/Cargo.toml and Cargo.lock read; `cargo search` shows 0.44.0] |
| `serde` | `1` (feature `derive`) | Deserialize API JSON and settings | Already used by stubs; resolved 1.0.229 in the local cache. [VERIFIED: cargo search] |
| `serde_json` | `1` | Parse listing/playlist JSON, build `raw` and cursor | Root crate already uses it; built for wasm32-unknown-unknown and native in the experiment. [VERIFIED: experiment build + cargo search 1.0.154] |
| `toml` | `1` | Parse the raw `[settings]` string in `init` | Same as stub-source (`toml = "1"`; lock has 1.1.8). [VERIFIED: stub Cargo.toml/Cargo.lock] |
| `percent-encoding` | `2` | Percent-encode query values (page tokens, playlist ids) | Tiny, no_std-friendly; compiled and tested on both targets in the experiment. [VERIFIED: experiment] |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `time` | `0.3`, `default-features = false`, `features = ["parsing"]` | Parse YouTube RFC 3339 `publishedAt` to epoch ms | YouTube plugin only. Compiled for wasm32-unknown-unknown and parsed `2024-01-01T00:00:00Z` and `...00.123Z` correctly in the experiment. Do not use `std::time::SystemTime` (no clock on this target). [VERIFIED: experiment] |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `time` | `chrono` (default-features off) | Works, heavier; the root crate already uses chrono but plugins are separate workspaces. `time` is smaller. |
| `percent-encoding` | `url` crate | `url` pulls `idna` tables into the wasm; only encoding is needed. |
| Hand-written `Http` trait | `http`/`ureq` crates | None run on `wasm32-unknown-unknown` without WASI; all HTTP must go through the `fetch` import. |
| `cargo component test` | explicit `cargo test --target <host>` | The latter was executed and works; do not depend on `cargo component test`. |

**Installation (per plugin crate, no network needed beyond crates.io once):**
```bash
# prerequisites already installed here: cargo-component 0.21.1, wasm-tools, rustc 1.96.0, wasm32-unknown-unknown
(cd plugins/reddit  && cargo component build --release)
(cd plugins/youtube && cargo component build --release)
```

**Version verification:** `cargo search` this session returned serde 1.0.229, serde_json 1.0.154, time 0.3.55, percent-encoding 2.3.2, toml 1.2.1, wit-bindgen-rt 0.44.0 (pinned to 0.41 deliberately). The offline experiment resolved time 0.3.46 from the local cache; `0.3` semver ranges cover both.

## Package Legitimacy Audit

| Package | Registry | Age | Downloads | Source Repo | Verdict | Disposition |
|---------|----------|-----|-----------|-------------|---------|-------------|
| serde | crates.io | 2014 | 32.8M/wk | github.com/serde-rs/serde | OK | Approved (already in stubs) |
| serde_json | crates.io | 2015 | 32.9M/wk | github.com/serde-rs/json | OK | Approved |
| toml | crates.io | long-lived | 24.1M/wk | github.com/toml-rs/toml | OK | Approved (already in stubs) |
| wit-bindgen-rt | crates.io | - | 1.07M/wk | github.com/bytecodealliance/wit-bindgen | OK | Approved (pinned 0.41, already in stubs) |
| time | crates.io | 2014 | 18.7M/wk | github.com/time-rs/time | OK | Approved |
| percent-encoding | crates.io | 2017 | 21.4M/wk | github.com/servo/rust-url | OK | Approved |

`gsd-tools query package-legitimacy check --ecosystem crates ...` returned `OK` for all six. These packages were selected from the existing stubs, the root `Cargo.toml`, and ecosystem knowledge; the legitimacy tool confirms registry reality, signals and repos. **Packages removed due to [SLOP] verdict:** none. **Packages flagged [SUS]:** none. No postinstall concept applies to crates.

## Architecture Patterns

### System Architecture Diagram

```
 fili run / fili plugins run-once <alias>
        |
        v
 +--------------------+   token?   +-------------------------------+
 | Scheduler          |----------->| TokenProvider (per alias)     |
 | (host, Phase 3)    |<-----------|  grant=authorization_code:    |
 |  cursor in/out     |  Option<   |    read <state>/auth/<a>.json |
 |  backoff           |  token>    |    refresh if <60 s left      |
 +--------+-----------+            |  grant=client_credentials     |
          | poll(cursor, token)    |    (NEW) POST token_url, Basic|
          v                        |    cache in memory, no file   |
 +--------------------+            +--------------+----------------+
 | .wasm component    |                           | host TLS client,
 |  (one per platform,|                           | outside plugin allowlist
 |   N instances)     |                           v
 |  adapter -> core:  |                   Reddit / Google token endpoints
 |   1 settings check |
 |   2 cursor decode  |
 |   3 per feed:      |   fetch(request)   +----------------------------+
 |     build request  |------------------->| Host fetch layer (Phase 5) |
 |     http.get()     |<-------------------| allowlist, timeout, <=5    |
 |     map status/    |  response (status, | redirects, 8 MiB cap       |
 |     headers/body   |  headers, body)    +-------------+--------------+
 |     parse + filter |                                  | fetcher component
 |   4 items + new    |                                  v
 |     cursor         |                    oauth.reddit.com / www.googleapis.com
 +--------+-----------+
          | Result<poll-response, poll-error>
          v
 Host: map items (<=1000, <=1 MB, key platform:id) -> Fluvio topic
       cursor <= 4096 B stored AFTER flush; errors -> backoff
```

### Recommended Project Structure
```
plugins/reddit/                     # same shape for plugins/youtube
├── Cargo.toml                      # cdylib + rlib; [workspace]; component target ../../wit world "source"
├── Cargo.lock                      # committed (stubs commit theirs)
├── rust-toolchain.toml             # channel 1.96.0, targets = ["wasm32-unknown-unknown"]
├── .cargo/config.toml              # [build] target = "wasm32-unknown-unknown"
├── .gitignore                      # /target/  /src/bindings.rs
├── src/
│   ├── lib.rs                      # pub mod core modules; cfg(target_arch="wasm32") bindings + adapter
│   ├── settings.rs                 # parse/validate [settings] -> config errors naming the field
│   ├── http.rs                     # Http trait, HttpRequest/HttpResponse/HttpError (own types, no bindings)
│   ├── api.rs                      # request building, status/header classification, JSON parsing
│   ├── cursor.rs                   # encode/decode per-feed marks, size guard
│   ├── poll.rs                     # orchestration: feeds -> items + new cursor; PollError type
│   └── component.rs                # cfg(wasm32): Guest impl + fetch adapter + PollError conversion
└── tests/
    ├── fixtures/                   # sanitised real responses + README.md (how recorded)
    └── replay.rs                   # integration tests through a mock Http (needs rlib)
examples/plugins/{filimon.toml,reddit.toml,reddit-user.toml,youtube.toml,youtube-user.toml,.env.example}
src/plugins/{config.rs,auth/*,mod.rs} src/commands/auth.rs plugins/README.md   # host grant work (D-03)
```
The crate directory names are fixed by D-16. Package names: recommend `reddit-source` / `youtube-source` (matching `stub-source`), producing `reddit_source.wasm` / `youtube_source.wasm`; set `[package.metadata.component] package = "filimon:reddit-source"`. [ASSUMED naming, discretion]

### Pattern 1: cfg-gated bindings so native tests work (VERIFIED by experiment)
**What:** `[lib] crate-type = ["cdylib", "rlib"]`; everything except the component glue is ordinary Rust; `mod bindings;` and the `Guest` impl live behind `#[cfg(target_arch = "wasm32")]`. `cargo component build --release` produces the `.wasm`; tests run with the explicit host triple.
**When to use:** both crates, always.
```rust
// src/lib.rs  (executed in /tmp/exp/p this session: component conformed to world `source`, native tests passed)
pub mod core;                       // for Phase 7: settings, http, api, cursor, poll
#[cfg(target_arch = "wasm32")]
#[allow(warnings)]
mod bindings;                       // generated by cargo-component; gitignored
#[cfg(target_arch = "wasm32")]
mod component;                      // Guest impl, fetch adapter
```
```bash
# Native unit + integration tests (a bare `cargo test` fails: exit status 126, it targets wasm32)
(cd plugins/reddit && cargo test --target "$(rustc -vV | sed -n 's/^host: //p')")
# Component build + world conformance
(cd plugins/reddit && cargo component build --release \
  && wasm-tools component targets ../../wit -w source target/wasm32-unknown-unknown/release/reddit_source.wasm)
```
Observed: `cargo test --target x86_64-apple-darwin --offline` ran the in-crate test, a `tests/` integration test using `include_str!("fixtures/a.json")`, and printed `test result: ok`; bare `cargo test` printed `process didn't exit successfully: .../exp_source-....wasm (exit status: 126)`. [VERIFIED: executed this session]

### Pattern 2: `Http` trait seam (D-17)
**What:** The core never mentions WIT types. It defines `HttpRequest { url, headers: Vec<(String,String)> }`, `HttpResponse { status: u16, headers: Vec<(String,String)>, body: Vec<u8> }`, `HttpError { Denied(String), Timeout, Transport(String), RateLimited(Option<u32>), NotConfigured, InvalidRequest(String) }` (mirrors `fetch-error` in wit/types.wit) and `trait Http { fn get(&self, req: &HttpRequest) -> Result<HttpResponse, HttpError>; }`. `component.rs` implements it with the imported `fetch`; tests implement it with a scripted mock that records requests (so tests can assert URL, query, `user-agent`, `authorization`/`x-goog-api-key`).

### Pattern 3: Per-feed independent state, one page per feed per poll
Each feed (Reddit `r/<sub>:<listing>` or `home`/`saved`; YouTube playlist id) has its own mark. A poll fetches one page per feed (limit 100 max for Reddit, 50 max for YouTube; D-10 first-poll sizes 25 and 10), filters to items strictly newer than the mark, and writes a new mark only for feeds whose fetch succeeded. Output items oldest-first within a poll (discretion; makes downstream chronological and lets an overflow lose the oldest, as D-11 documents).

### Pattern 4: Typed error mapping table (D-13, D-15, discretion rows marked)
| Condition | Plugin returns |
|---|---|
| `access-token` is `none` where one is required (`reddit-user`, or `reddit` if the host failed to supply one) | `auth-required` (D-04) |
| Reddit HTTP 429, or any response with `x-ratelimit-remaining` < 1 | `rate-limited(Some(x-ratelimit-reset))` (D-13); `Retry-After` is not documented for Reddit, use only `x-ratelimit-reset`, `None` when absent |
| Reddit/YouTube HTTP 401 | `auth-required` (discretion; host refreshes the token before the next poll) |
| YouTube 403 reason `quotaExceeded` (also `dailyLimitExceeded`) | `transient("YouTube quota exceeded (quotaExceeded): daily Data API quota exhausted; resets at midnight Pacific")` (D-15, verbatim text) |
| YouTube 403 `rateLimitExceeded` / HTTP 429 | `rate-limited(None)` (discretion) |
| YouTube 400 `keyInvalid`, 403 `accessNotConfigured`, 403 `forbidden` / `insufficientPermissions` | `config("...")` naming the secret or scope (discretion; retrying cannot fix these) |
| 404 `playlistNotFound` | `config` naming the `channels` entry (T3 mitigation) |
| 5xx, `Timeout`, `Transport` | `transient` |
| `RateLimited(n)` from `fetch` | `rate-limited(n)` |
| `Denied`, `NotConfigured` | `config` naming allowlist / fetcher |
| `InvalidRequest` | `fatal` (plugin bug or unusable setting) |
| JSON malformed / unexpected shape | `transient("malformed response ...")` with no body echoed (retry; could be an HTML error page) |

### Anti-Patterns to Avoid
- **Logging or echoing request URLs, headers, tokens, bodies.** The host already omits them; the plugin must too (API key and bearer token live in request headers; log feed names and status codes only).
- **Returning an empty successful poll on an error response.** That is exactly SC4's forbidden outcome; every non-2xx maps to an error.
- **Relying on page order for correctness.** Compute the new mark as the max over the page and filter by value, not position.
- **Using `std::time`, `std::env`, `std::fs`** in the plugin (README: no clock, filesystem or env on this target).
- **Putting `client_id`/`client_secret` into the plugin's `[host.secrets]`** for Reddit once D-03 lands; the secret moves to `[host.oauth] client_secret_env`, which the plugin never receives.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| RFC 3339 `publishedAt` to epoch ms | A string-slicing date parser | `time::OffsetDateTime::parse(s, &Rfc3339)` | Fractional seconds, offsets, leap-year math |
| URL query encoding | Manual `format!` of unescaped values | `percent-encoding` (`NON_ALPHANUMERIC` set) | Page tokens and ids; avoids query injection via settings |
| JSON parsing of API responses | Regex/manual scanning | `serde` + `serde_json` with `#[serde(default)]` on optional fields | Reddit/YouTube add and omit fields freely |
| Settings parsing | Ad hoc string splitting | `toml::from_str` into a `#[serde(deny_unknown_fields)]` struct (stub-source does this) | Misspelled keys become `config` errors naming the field |
| OAuth token exchange / refresh / Basic auth | Plugin-side token calls via `fetch` | Host `TokenClient`/`TokenProvider` (extended with `client_credentials`) | D-03; plugin must never see client credentials; host already sets Basic auth when a secret exists (client.rs:244-248) |
| Backoff, retry, interval | Sleep/loop in the plugin | Return typed `poll-error`; host scheduler waits | No clock in guest; Phase 3 D-01 |
| Rate-limit pacing | Token bucket in plugin | Reactive 429/remaining<1 only (D-13) | Stateless plugin; deferred by user |

**Key insight:** every hard problem here (time, retries, auth, TLS, allowlist) is owned by the host or a library; the plugin is a pure function from (settings, cursor, token, HTTP responses) to (items, cursor, typed error), which is exactly what makes fixture tests sufficient.

## Runtime State Inventory

Not a rename/refactor/migration phase, with one qualification: the sample registry currently names `reddit` with `[host.secrets]` `client_id`/`client_secret` and a singular `listing`. This is a greenfield implementation of the plugins, but D-03/D-05 change published sample shapes.

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | None - no deployed cursor or token files exist for `reddit`/`youtube` (the plugins have never run; samples point at non-existent `components/*.wasm`). Verified by `examples/plugins/filimon.toml` reading `component = "components/reddit.wasm"` and no `components/` dir present. | none |
| Live service config | None | none |
| OS-registered state | None | none |
| Secrets/env vars | `examples/plugins/.env.example` lists `REDDIT_CLIENT_ID`, `REDDIT_CLIENT_SECRET`, `YOUTUBE_API_KEY`; test `sample_env_example_lists_every_declared_secret` (src/plugins/mod.rs:149-180) asserts that set equals the `[host.secrets]` env-var names. After D-03, `REDDIT_CLIENT_ID` moves into the TOML (`[host.oauth] client_id`) and `REDDIT_CLIENT_SECRET` becomes `client_secret_env`, which the test does not collect. | code edit: update `.env.example`, extend the test to also collect `host.oauth.client_secret_env`, and update the expected set |
| Build artifacts | `plugins/*/target` are gitignored per crate; new crates need their own `.gitignore` (`/target/`, `/src/bindings.rs`) | add to both new crates |

## Common Pitfalls

### Pitfall 1: Bare `cargo test` in a plugin crate fails
**What goes wrong:** `.cargo/config.toml` sets `[build] target = "wasm32-unknown-unknown"`, so `cargo test` tries to execute a `.wasm` (exit status 126). **Why:** the crate is built for wasm by default. **Avoid:** always `cargo test --target "$(rustc -vV | sed -n 's/^host: //p')"`; put it in the crate README and every task `<verify>`. **Warning sign:** "process didn't exit successfully ... .wasm".

### Pitfall 2: 4096-byte cursor cap disables the plugin
**What goes wrong:** the host treats a `next-cursor` over `MAX_CURSOR_BYTES` as a plugin fault and disables the plugin with a `fatal` error (README "Host behavior"; code `src/plugins/host/cursor.rs:31`: `pub const MAX_CURSOR_BYTES: usize = 4096;`). **Numbers (computed):** compact YouTube entry `"UU<22>":[epoch_s,"<11-char id>"]` is about 53 bytes (about 77 feeds max); with D-11's literal RFC 3339 string plus object it is about 73 bytes (about 56 feeds max); a Reddit entry for a 21-character subreddit is about 62 bytes. **Avoid:** use compact encoding (integers, short keys), validate at `init` that `feeds x worst-case-entry + overhead <= 4096` and return a `config` error naming `channels`/`subscriptions_limit`/`subreddits` otherwise; clamp subscriptions to the same limit.

### Pitfall 3: 1000-item per-poll cap drops items silently
**What goes wrong:** the host keeps the first 1000 items per poll, drops the rest, and still advances the cursor (`src/plugins/host/map.rs:26`: `pub const MAX_ITEMS_PER_POLL: usize = 1000;`). With D-10's 10 items per playlist, more than 100 channels would overflow on the first poll. **Avoid:** the plugin caps its own output below 1000 (the feed cap above makes 77 x 10 = 770 reachable but a Reddit config with many subs x 100 limit is not), truncating oldest-first and not advancing the mark past emitted items.

### Pitfall 4: Reddit HTML-escaped fields
Without `raw_json=1`, Reddit escapes `&`, `<`, `>` in titles/URLs as HTML entities. Always send `raw_json=1`. [ASSUMED from training; confirm on first fixture capture, A3]

### Pitfall 5: Reddit listings contain non-posts and placeholder posts
`saved` returns both posts (`t3`) and comments (`t1`) [ASSUMED, A2]; a mistyped subreddit can redirect to a subreddit-search listing of `t5` children (the host follows redirects within the allowlist, otherwise returns `denied`; A4). Filter `kind == "t3"` and skip the rest. Deleted/removed posts keep `author` `[deleted]` and `selftext` `[removed]`/`[deleted]`; D-18 requires a fixture; decide to emit with those literal strings or skip (discretion; recommend emit, since the cursor mark must still advance past them).

### Pitfall 6: Mark equality must not re-emit
With the mark `{fullname, created_utc}`, use strict `created_utc > mark` for filtering. Using `>=` with a different fullname re-emits a same-second sibling on every poll forever. Accept the tiny loss for a same-second late arrival. [ASSUMED design reasoning]

### Pitfall 7: YouTube `playlistItems.snippet.publishedAt` is the add-to-playlist time
Doc: `snippet.publishedAt`: "The date and time that the item was added to the playlist"; `contentDetails.videoPublishedAt`: "The date and time that the video was published to YouTube". For an uploads playlist the two usually match, but a private/scheduled video made public later can differ [ASSUMED], so a mark based on `snippet.publishedAt` can miss it. Requesting `part=snippet,contentDetails` costs the same 1 unit (quota is per call, not per part). See Tension T3b.

### Pitfall 8: Private / deleted videos appear in uploads playlists
Placeholder entries with titles like "Private video" / "Deleted video" and no useful data [ASSUMED, A15]. Skip entries whose title matches those placeholders or lacking `resourceId.videoId`; still let them advance the mark.

### Pitfall 9: Host sample tests will fail until updated
Changing `examples/plugins/*.toml`, adding `reddit-user.toml`/`youtube-user.toml` and sources breaks: `loads_sample_registry` (expects sources `["reddit", "stub", "youtube"]` and exact secret keys), `sample_env_example_lists_every_declared_secret`, `SAMPLE_TOMLS: [&str; 4]` (src/plugins/mod.rs:29), and `tests/plugins_cli.rs:164` (`["reddit", "stub", "youtube", "http", "stub-fetcher"]`). Update all four in the same plan as the sample changes. The sample registry loads strictly, so `REDDIT_CLIENT_SECRET` (via `client_secret_env`) must be present in the test's `.env` or loading fails.

### Pitfall 10: `fili auth status/login` for a `client_credentials` plugin
`login_command` (src/commands/auth.rs:146-183) refuses a plugin without `[host.oauth]`; a `client_credentials` plugin has the table but no login. Add an explicit refusal ("uses client_credentials; there is nothing to log in to") and an `app-only` status row, otherwise `auth status` prints `not-logged-in` for `reddit`. Update README's documented state list.

## Code Examples

### Cursor map (compact, size-guarded) [pattern, project-specific]
```rust
// Source: derived from D-11 and MAX_CURSOR_BYTES = 4096 (cursor.rs:31); numbers computed this session
use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

/// YouTube: key = playlist id; value = [published epoch seconds, video id]
#[derive(Serialize, Deserialize, Default)]
pub struct YtCursor(pub BTreeMap<String, (i64, String)>);

pub const MAX_CURSOR_BYTES: usize = 4096; // mirror of the host cap; keep a unit test tying both

pub fn encode(c: &YtCursor) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(c).map_err(|e| e.to_string())?;
    if bytes.len() > MAX_CURSOR_BYTES {
        return Err(format!("cursor would be {} bytes (cap {MAX_CURSOR_BYTES})", bytes.len()));
    }
    Ok(bytes)
}
```
Decode failure policy (discretion, recommended): an undecodable cursor is treated as a first poll with a `log` warning, so a layout change self-heals; this does mean a one-page re-emit.

### Reddit request and rate-limit classification
```rust
// Source: Reddit API rules + prawcore parse (int(float(remaining)), int(reset)); URL shape from Reddit OAuth2 wiki
fn listing_request(base: &str, sub: &str, listing: &str, limit: u32, token: &str, ua: &str) -> HttpRequest {
    HttpRequest {
        url: format!("{base}/r/{sub}/{listing}?limit={limit}&raw_json=1"), // base = https://oauth.reddit.com
        headers: vec![
            ("authorization".into(), format!("bearer {token}")),
            ("user-agent".into(), ua.to_string()),
            ("accept".into(), "application/json".into()),
        ],
    }
}

fn header<'a>(h: &'a [(String, String)], name: &str) -> Option<&'a str> {
    h.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
}

/// Some(reset seconds) when the response says "stop": 429 or remaining < 1 (header may be "59.0").
fn rate_limit_hit(resp: &HttpResponse) -> Option<Option<u32>> {
    let remaining = header(&resp.headers, "x-ratelimit-remaining").and_then(|v| v.trim().parse::<f64>().ok());
    if resp.status == 429 || remaining.is_some_and(|r| r < 1.0) {
        let reset = header(&resp.headers, "x-ratelimit-reset").and_then(|v| v.trim().parse::<f64>().ok());
        return Some(reset.map(|s| s.max(0.0).ceil() as u32));
    }
    None
}
```

### User-Agent builder (D-14)
```rust
// Format source: github.com/reddit-archive/reddit/wiki/API  "<platform>:<app ID>:<version string> (by /u/<reddit username>)"
pub fn user_agent(contact: &str) -> Result<String, String> {
    let name = contact.trim().trim_start_matches('/').trim_start_matches("u/").trim_start_matches("U/");
    let ok = (3..=20).contains(&name.len()) && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if !ok { return Err("settings.user_agent_contact: expected a Reddit username such as \"u/yourname\"".into()); }
    Ok(format!("filimon:reddit:{} (by /u/{name})", env!("CARGO_PKG_VERSION")))
}
```
`<alias>` is not available inside the plugin (see T2); `reddit` is the fixed app id here.

### YouTube quota error classification (D-15)
```rust
// Source: developers.google.com/youtube/v3/docs/core_errors (reason codes and statuses)
#[derive(serde::Deserialize)] struct GErr { error: Option<GErrBody> }
#[derive(serde::Deserialize)] struct GErrBody { #[serde(default)] errors: Vec<GReason> }
#[derive(serde::Deserialize)] struct GReason { reason: Option<String> }

fn classify_youtube(status: u16, body: &[u8]) -> Option<PollError> {
    if (200..300).contains(&status) { return None; }
    let reason = serde_json::from_slice::<GErr>(body).ok()
        .and_then(|e| e.error).and_then(|e| e.errors.into_iter().next()).and_then(|r| r.reason);
    Some(match (status, reason.as_deref()) {
        (403, Some("quotaExceeded" | "dailyLimitExceeded")) => PollError::Transient(
            "YouTube quota exceeded (quotaExceeded): daily Data API quota exhausted; resets at midnight Pacific".into()),
        (429, _) | (403, Some("rateLimitExceeded" | "userRateLimitExceeded")) => PollError::RateLimited(None),
        (401, _) => PollError::AuthRequired,
        (400, Some("keyInvalid")) | (403, Some("accessNotConfigured" | "insufficientPermissions" | "forbidden")) =>
            PollError::Config(format!("YouTube rejected the credentials or project ({})", reason.unwrap_or_default())),
        (s, _) if s >= 500 => PollError::Transient(format!("YouTube HTTP {s}")),
        (s, r) => PollError::Transient(format!("YouTube HTTP {s}{}", r.map(|r| format!(" ({r})")).unwrap_or_default())),
    })
}
```

### Requests the YouTube plugin makes (1 unit each)
```
GET https://www.googleapis.com/youtube/v3/playlistItems?part=snippet,contentDetails&playlistId=UU<22>&maxResults=10
GET https://www.googleapis.com/youtube/v3/subscriptions?part=snippet&mine=true&maxResults=50&order=relevance[&pageToken=..]
headers: x-goog-api-key: <key>     (youtube instance, no token)
         authorization: Bearer <token>   (youtube-user instance)
```
Header form: Google documents `x-goog-api-key` as the primary way to send an API key, with `?key=` as the fallback ("To include an API key with a REST API call, use the `x-goog-api-key` HTTP header"; "If you can't use the HTTP header, you can use the `key` query parameter") [CITED: docs.cloud.google.com/docs/authentication/api-keys-use]. That page is generic to Google Cloud APIs; whether `www.googleapis.com/youtube/v3` honours it is unconfirmed (A11). Fall back to `key=` if the first capture shows rejection. Host logs omit query strings, so either form is not logged by the host.

### Host `client_credentials` token request (what the host must send)
```
POST https://www.reddit.com/api/v1/access_token
Authorization: Basic base64(client_id:client_secret)     # TokenClient::post_form already does this when client_secret is set (client.rs:244-246)
Content-Type: application/x-www-form-urlencoded
User-Agent: <must be Reddit-compliant; see T4>
grant_type=client_credentials
-> {"access_token": "...", "token_type": "bearer", "expires_in": 3600?, "scope": "*"}   # parse_token_set already tolerates these fields (client.rs:327-345)
```

### `[host.oauth]` additions (recommended shape; D-03)
```toml
# reddit.toml (app-only instance)
[host]
poll_interval_secs = 300
allowlist = ["oauth.reddit.com"]          # token host is outside the plugin allowlist (Phase 6 D-13)

[host.oauth]
grant = "client_credentials"
token_url = "https://www.reddit.com/api/v1/access_token"
client_id = "<your web-app client id>"    # public, lives in the TOML (Phase 6 D-12)
client_secret_env = "REDDIT_CLIENT_SECRET"
user_agent = "filimon:reddit:0.1.0 (by /u/yourname)"   # NEW optional key (T4); applies to token calls only

[settings]
subreddits = ["rust", "programming"]
listings = ["new"]
user_agent_contact = "u/yourname"
```
```toml
# reddit-user.toml (PKCE instance, same .wasm)
[host.oauth]
authorize_url = "https://www.reddit.com/api/v1/authorize"
token_url = "https://www.reddit.com/api/v1/access_token"
scopes = ["read", "history"]
client_id = "<same web-app client id>"
client_secret_env = "REDDIT_CLIENT_SECRET"
authorize_params = { duration = "permanent" }        # Phase 6 D-14; without it no refresh token
revocation_url = "https://www.reddit.com/api/v1/revoke_token"
[settings]
feed = "home"                                        # or "saved" (+ username, see T1)
user_agent_contact = "u/yourname"
```
```toml
# youtube-user.toml
[host.oauth]
authorize_url = "https://accounts.google.com/o/oauth2/v2/auth"
token_url = "https://oauth2.googleapis.com/token"
revocation_url = "https://oauth2.googleapis.com/revoke"
scopes = ["https://www.googleapis.com/auth/youtube.readonly"]
client_id = "<desktop client id>"
client_secret_env = "YOUTUBE_OAUTH_CLIENT_SECRET"    # Google's doc lists client_secret as optional; desktop clients usually need it (A12)
```
Endpoints and scope are quoted from Google's native-app and scopes pages ("https://accounts.google.com/o/oauth2/v2/auth", "https://oauth2.googleapis.com/token", "https://oauth2.googleapis.com/revoke", loopback `http://127.0.0.1:port`; scope "https://www.googleapis.com/auth/youtube.readonly" described as "View your YouTube account"). Reddit authorize/revoke URLs are from training knowledge consistent with the wiki's token and revoke URLs (A5).

## Tensions with locked decisions (flagged, not overridden)

| ID | Decision | Evidence | Impact | Recommendation for the plan |
|----|----------|----------|--------|-----------------------------|
| **T1** | D-12 `created_utc` high-water mark, applied to the Reddit **saved** (and arguably home `best`) feed | The saved listing is ordered by when the user saved the item, not by `created_utc` [ASSUMED: PRAW lists saved items most-recently-saved first]; a user saving an old post would never emit it. Saved also needs `/user/{username}/saved` and the `history` scope [CITED: PRAW scope text "Access my voting history and comments or submissions I've saved or hidden"; endpoint path ASSUMED]. | A saved feed that silently ignores new saves of older posts | Keep D-12 for subreddit listings and for home via `/new` (A1). For `saved`, store in that feed's cursor value the fullnames of the top N (e.g. 25) of the previous page and emit entries until the first already-seen fullname. This keeps the "JSON map of per-feed marks" shape of D-11 (value type is per-feed). Get one-line user confirmation in plan-phase or discuss. Also require `username` in `[settings]` for `saved` (config error naming the field) rather than a `/api/v1/me` call plus `identity` scope. |
| **T2** | D-14 `filimon:<alias>:<version>` | `init(settings, secrets)` (wit/worlds.wit:18) carries no alias; the plugin cannot know it is `reddit` vs `reddit-user`. | The literal format is unimplementable | Use the fixed app id `reddit` (both instances share one UA), optionally overridable by a `[settings] app_id` key. Mention in plan. |
| **T3a** | D-06 `UC` to `UU` with no lookup | Not documented: search found no official Google documentation of the swap; Stack Overflow and the Google issue tracker call it undocumented (one commenter reports a related trick stopped working); community guides treat it as standard. The documented route is `channels.list` `contentDetails.relatedPlaylists.uploads`. | Could break without notice | Keep D-06. On 404 `playlistNotFound` return a `config` error naming the exact `channels` entry. Optional (discretion): one-time `channels.list` fallback (1 unit) only on that 404. |
| **T3b** | D-09/D-11 use `snippet.publishedAt` | Doc distinguishes `snippet.publishedAt` (added to playlist) from `contentDetails.videoPublishedAt` (video published). | Possible missed uploads for private-then-public/scheduled videos | Request `part=snippet,contentDetails` (same 1 unit) and use `videoPublishedAt` for `published` and the mark when present, else `snippet.publishedAt`. `raw` can stay the `snippet` per D-09. Needs user nod because D-09 says `publishedAt`. |
| **T4** | D-03/D-14 UA | Host token calls send `fili/<version>` (client.rs:129), which is not Reddit's required format. Phase 6 research flagged the need for a per-plugin override for Phase 7. | Reddit may throttle or block the token request ("Many default User-Agents ... drastically limited" is training-memory wording; the rules page says spoofing is prohibited and a unique descriptive format is required) | Add an optional host-side `[host.oauth] user_agent` applied to token/revoke requests. It is additive and low-risk; include it in the D-03 host task. |
| **T5** | Roadmap SC2/SC3 text | ROADMAP.md §Phase 7 says "After `fili auth login reddit`" / "`fili auth login youtube`"; D-01/D-07 make the login targets `reddit-user` / `youtube-user`. | Verification wording mismatch | Plan verification against the instance names in CONTEXT; note the ROADMAP wording in the plan or update it. |
| **T6** | D-08 `url` for link posts | "`url` = permalink for self posts (external link kept in `raw`)" does not say what `url` is for link posts. | Ambiguity | Recommend `url` = `https://www.reddit.com` + `permalink` for every post (stable, always parseable, host drops empty/unparsable URLs); the external link stays in `raw.url`. |

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Reddit self-service API keys | Approval-gated access under the "Responsible Builder Policy" | Announced November 2025 | The user may not be able to obtain a Reddit client id/secret to record D-18 fixtures or run Phase 8 live tests (see Open Question 1). Existing apps keep access. |
| Reddit 60 requests/min (archived API wiki) | 100 queries/min per OAuth client id, averaged over a 10-minute window | July 2023 | The archived wiki page still says 60; use the header values, not a hard-coded limit (D-13 is header driven anyway) |
| YouTube `search.list` at 100 units/call | Current quota page describes `search.list` with its own default 100 calls/day allocation at cost 1 per call | Present docs (fetch summary, MEDIUM) | Irrelevant to implementation: Phase 7 never calls `search.list` (D-15); SC4 wording uses the older "100-unit" framing |
| Google refresh tokens for consent screens in "Testing" | Expire after 7 days | Ongoing Google policy | `youtube-user` logins made against an unpublished consent screen need re-login weekly; document in the sample TOML comments (MEDIUM, third-party sources) |

**Deprecated/outdated:** archived Reddit wiki limit of 60/min; `subscriptions.contentDetails.newItemCount` as a change signal (do not rely on it).

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | For a logged-in user, `GET https://oauth.reddit.com/new` returns the home-feed "new" sort (front page across subscriptions), so a `created_utc` mark works for `feed = "home"` | Tension T1 | Home feed would be wrong or empty; verify at first live capture; fallback: `/best` with the same fullname-set rule as `saved` |
| A2 | `saved` lives at `/user/{username}/saved`, needs scope `history`, and mixes `t3` posts and `t1` comments | T1, Pitfall 5 | Saved feed errors or comments leak as posts |
| A3 | Reddit listing child shape (`kind: "t3"`, `data.{name,title,selftext,permalink (relative),created_utc (float seconds),is_self,stickied,url,author,removed_by_category}`), `raw_json=1` behaviour, and that app-only tokens may read `/r/{sub}/new` | Pitfalls 4-5, O3 | Parser struct wrong; confirmed only partially by PRAW attribute docs |
| A4 | A nonexistent/banned subreddit redirects to a subreddit-search listing (or 404), and `403` with a JSON `reason` marks a private subreddit | Pitfall 5, error table | Wrong per-feed error behaviour |
| A5 | Reddit ignores the PKCE `code_challenge` params, accepts the code flow for a **web app** with HTTP Basic (id:secret), and its authorize/revoke URLs are `https://www.reddit.com/api/v1/authorize` and `.../revoke_token` | `reddit-user.toml` example | AUTH-01 for Reddit fails; Phase 6 research marked this unverified live |
| A6 | `X-Ratelimit-Remaining` may be a float string and `X-Ratelimit-Reset` an integer: prawcore's parse is `int(float(...))` and `int(...)` [CITED], the Reddit docs name the headers only | Code example | Low; the example parses both as `f64` |
| A7 | Reddit may throttle/block token requests with UA `fili/<version>` | T4 | If Reddit tolerates it, the optional `user_agent` key is unneeded but harmless |
| A8 | Reddit now requires approval for new API access (Responsible Builder Policy, November 2025); sources are a Reddit-announcement search result and third-party write-ups, nothing from 2026 | Open Question 1 | If access is still self-service, D-18 real capture is straightforward |
| A9 | Uploads playlist lists newest first (position 0 = newest) | Pattern 3 | Low; the design filters by value, not order |
| A10 | Private to public (or scheduled) videos can have `snippet.publishedAt` earlier than `videoPublishedAt` | T3b | If equal in practice, T3b is harmless |
| A11 | `x-goog-api-key` header is honoured by `www.googleapis.com/youtube/v3` | Code examples | Fall back to `key=` query |
| A12 | Google "Desktop app" credentials need `client_secret` at the token endpoint in practice despite the doc listing it as optional | `youtube-user.toml` | Login fails without `client_secret_env`; Phase 6 research A6 |
| A13 | YouTube error body is `{"error":{"code":403,"errors":[{"reason":"quotaExceeded", ...}]}}` (reason location is documented, exact JSON nesting from memory) | Code examples | Parser mismatch; fixture D-18 `quotaExceeded 403` pins it |
| A14 | `subscriptions.contentDetails.newItemCount` is not a reliable change signal | O4 detail | A cheaper subscription strategy was missed |
| A15 | Uploads playlists can contain "Private video"/"Deleted video" placeholder entries | Pitfall 8 | Placeholders emitted as items |
| A16 | Subreddit names are 3-21 characters of letters, digits, underscore (validate loosely as `[A-Za-z0-9_]{1,21}`) | settings validation | Rejecting valid names; keep the check permissive |
| A17 | Package naming `reddit-source` / `youtube-source` | Project structure | None (naming only) |

## Open Questions

1. **Can the user actually obtain Reddit API credentials?**
   - What we know: Reddit announced in November 2025 that self-service API access closed and new access needs approval; existing access continues.
   - What's unclear: whether this applies to a personal, non-commercial web app today (no 2026 confirmation found).
   - Recommendation: Phase 7 stays offline-provable. Make fixture capture a `checkpoint:human-action` with a documented fallback: schema-faithful hand-authored fixtures labelled `synthetic` in each `fixtures/README.md` (a deviation from D-18's "real" wording that needs the user's acceptance). Phase 8 live tests (TEST-03) must skip cleanly without credentials, which the roadmap already requires.

2. **Does the YouTube plugin need the `channels.list` fallback and per-channel change detection?**
   - What we know: the dominant cost is N playlist reads per poll; `channels.list` batches 50 ids for 1 unit.
   - What's unclear: whether users will hit the 10,000/day quota at the sample interval.
   - Recommendation: ship the `subscriptions_limit` guard and the quota formula in docs; defer `channels.list` change detection.

3. **Saved-feed identity.** `username` setting versus `/api/v1/me` plus `identity` scope (T1). Recommend the setting.

4. **Where should the `client_credentials` token live?**
   - Recommendation: in-memory only in `TokenProvider` (no token file, no `fili auth status` file semantics, one extra token request per process start and per `run-once`), refreshed when under 60 s remain (same `REFRESH_MARGIN_SECS` constant, provider.rs:47). Persisting would reuse the 0600 store but complicates `status`/`logout`. Planner decides; in-memory is simpler to test.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| rustc / cargo | Build and test | yes | 1.96.0 (matches the crates' toolchain pin) | none needed |
| `wasm32-unknown-unknown` target | Component build | yes (installed) | - | none needed |
| cargo-component | `cargo component build` | yes | 0.21.1 (pinned version in plugins/README.md) | `cargo build --target wasm32-wasip2` fallback per plugins/README.md needs user sign-off |
| wasm-tools | Conformance (`component targets`) | yes | on PATH (`~/.cargo/bin`) | none needed |
| crates.io network | First fetch of new crate deps | yes (experiment resolved `time`, `percent-encoding` from local cache offline) | - | `--offline` works for already cached versions |
| Reddit API credentials (web app) | D-18 real capture, Phase 8 live | not available here | - | synthetic fixtures (Open Question 1) |
| Google API key + OAuth desktop client | D-18 real capture, `youtube-user` live | not available here | - | synthetic fixtures |
| Fluvio cluster | `--ignored live_` host tests only | not needed in Phase 7 | - | Phase 8 |

**Missing dependencies with no fallback:** none for the offline deliverables.
**Missing dependencies with fallback:** API credentials (capture is a human step; synthetic fixtures labelled as such).

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test harness (`cargo test`); plugin crates natively on the host triple; host crate `cargo test -p filimon` |
| Config file | per-crate `Cargo.toml` + `.cargo/config.toml`; none needed beyond the explicit `--target` flag |
| Quick run command | `(cd plugins/reddit && cargo test --target "$(rustc -vV \| sed -n 's/^host: //p')")` and the same for `plugins/youtube` |
| Full suite command | both plugin native suites, then `cargo test -p filimon plugins::` and `cargo test -p filimon --test plugins_cli --test auth_cli`, then the component build + conformance commands below |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| RDDT-01 | Parse a normal `new`/`hot` page into items; self vs link post; stickied skip on `new`, kept on `hot`; deleted/removed post; empty page; `t5`/non-`t3` children skipped; request URL carries `raw_json=1`, `limit=25` first poll | unit (mock `Http`) | `cd plugins/reddit && cargo test --target <host> listing` | Wave 0 |
| RDDT-01 | Cursor: first poll `none` emits one page; second poll emits only `created_utc` > mark; deleted marked post does not cause re-emit; strict `>` tie rule | unit | `cargo test --target <host> cursor` | Wave 0 |
| RDDT-02 | `reddit-user` settings (`feed = "home"/"saved"`, `username` for saved) validated; `none` token returns `auth-required`; saved uses the fullname-set rule (T1) | unit | `cargo test --target <host> user_feed` | Wave 0 |
| RDDT-03 | 429 and `x-ratelimit-remaining` of `0.0`/`0` yield `rate-limited(Some(reset))`; header parse tolerates floats; UA is `filimon:reddit:<ver> (by /u/<name>)`; missing `user_agent_contact` is a `config` error naming the field; `authorization: bearer` and `user-agent` headers present on every request | unit | `cargo test --target <host> rate_limit user_agent` | Wave 0 |
| YTUB-01 | Playlist id derived `UC`->`UU`; `@handle` rejected with `config` naming `channels`; page parsed to items (title, description, watch URL, channel title, epoch-ms `published`, `raw`=snippet); empty page; private/deleted placeholders skipped | unit | `cd plugins/youtube && cargo test --target <host> playlist` | Wave 0 |
| YTUB-02 | `subscriptions.list` pagination (page token), subscriptions then per-channel playlists, `subscriptions_limit` clamp, `none` token returns `auth-required`, bearer header used instead of API key | unit | `cargo test --target <host> subscriptions` | Wave 0 |
| YTUB-03 | Only `playlistItems`/`subscriptions` URLs are ever requested (assert on recorded mock requests; no `search`, no `videos`); `quotaExceeded` 403 returns `transient` with the exact D-15 text; 401 -> `auth-required`; 429 -> `rate-limited`; recovery next poll after a success response | unit | `cargo test --target <host> quota` | Wave 0 |
| YTUB-01/02, RDDT-01/02 | Cursor encode stays <= 4096 bytes at the configured cap; oversize config rejected at `init` | unit | `cargo test --target <host> cursor_size` | Wave 0 |
| TEST-01 | Every fixture in D-18 exists and is exercised; suite passes with no network and no env vars | unit/integration | `cargo test --target <host>` in each crate | Wave 0 |
| D-03 host | `grant = "client_credentials"`: config validation (secret required, authorize_url/redirect_port/authorize_params rejected, naming plugin+field); token request is Basic + `grant_type=client_credentials` + optional UA; cached until <60 s left; refresh failure mapping; `login` refuses; `status` shows app-only | unit (reuses `TestHttpServer`, `form_pairs`, `oauth_config_for` helpers) | `cargo test -p filimon plugins::config plugins::auth` and `cargo test -p filimon --test auth_cli` | extend existing |
| Samples | New `reddit.toml`/`reddit-user.toml`/`youtube.toml`/`youtube-user.toml` load; `.env.example` matches declared secrets incl. `client_secret_env`; sample `list` shows five sources | unit/CLI | `cargo test -p filimon plugins::tests` and `cargo test -p filimon --test plugins_cli list_shows_the_sample_registry_plugins` | edit existing |
| Component conformance | Built `.wasm` matches world `source` and imports only `filimon:plugin/*` | build | `wasm-tools component targets ../../wit -w source target/wasm32-unknown-unknown/release/<crate>.wasm` (exit 0) in each crate | n/a |

### Sampling Rate
- **Per task commit:** the touched crate's native `cargo test --target <host>` (seconds), or `cargo test -p filimon plugins::auth` for host work.
- **Per wave merge:** all four commands in the full-suite row above plus both `cargo component build --release` + `wasm-tools component targets` checks.
- **Phase gate:** full suite green, and `fili plugins check reddit`/`youtube` (needs built `.wasm` plus a `.env` with stub values) succeeds before `/gsd-verify-work`. `cargo test -p filimon -- --ignored wasm_` is Phase 8 (TEST-02) scope.

### Wave 0 Gaps
- [ ] `plugins/reddit/` and `plugins/youtube/` crate skeletons (Cargo.toml, toolchain pin, `.cargo/config.toml`, `.gitignore`, cfg-gated `lib.rs`) copied from `plugins/stub-source` and adjusted per Pattern 1
- [ ] `tests/fixtures/` sets with a `README.md` recording provenance (real vs synthetic) and the curl recipes below
- [ ] Mock `Http` helper and fixture loader in each crate's tests (recording mock so tests assert on requested URLs and headers)
- [ ] Host test scaffolding for `client_credentials` extends existing helpers (`src/plugins/auth/testing.rs`, `host/testing.rs::TestHttpServer`); no new framework

Fixture capture recipes (for the human step; never commit tokens or keys):
```bash
# Reddit app-only token, then a listing with headers (-D -) for the 429/ratelimit fixtures
curl -s -u "$ID:$SECRET" -A "filimon:reddit:0.1.0 (by /u/yourname)" -d grant_type=client_credentials https://www.reddit.com/api/v1/access_token
curl -s -D - -A "filimon:reddit:0.1.0 (by /u/yourname)" -H "Authorization: bearer $TOKEN" "https://oauth.reddit.com/r/rust/new?limit=5&raw_json=1"
# YouTube playlist page
curl -s -H "x-goog-api-key: $KEY" "https://www.googleapis.com/youtube/v3/playlistItems?part=snippet,contentDetails&playlistId=UU_x5XG1OV2P6uZZ5FSM9Ttw&maxResults=5"
```
(`UC_x5XG1OV2P6uZZ5FSM9Ttw` is the channel id already in `examples/plugins/youtube.toml`.)

## Security Domain

`security_enforcement` is enabled (absent/true in `.planning/config.json`, ASVS level 1).

### Applicable ASVS Categories
| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | yes | Host-managed OAuth only; plugin receives an access token, never client id/secret/refresh token (Phase 6 D-12, this phase D-03); `client_secret_env` resolved by the host, redacted by existing `Secret` type |
| V3 Session Management | partial | Token lifetime/refresh in host (`REFRESH_MARGIN_SECS` = 60); `client_credentials` token cached in memory; no new file |
| V4 Access Control | yes | Host allowlist per source (`oauth.reddit.com` only for Reddit, `www.googleapis.com` only for YouTube); redirect hops re-checked by host |
| V5 Input Validation | yes | `serde` + `deny_unknown_fields` settings; validate subreddit names and channel ids by charset, reject `@handles`; percent-encode all query values; treat all API JSON as untrusted |
| V6 Cryptography | no (host TLS) | Never hand-roll; no crypto in the plugins |
| V7 Error handling / logging | yes | Never log or echo URLs, headers, tokens, response bodies; plugin errors name field/status only |

### Known Threat Patterns for this stack
| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Query/path injection through settings (`subreddits`, `channels`, `username`) | Tampering | Strict charset validation at `init`; percent-encode; never interpolate unchecked text into a path |
| User-Agent header injection via `user_agent_contact` (CR/LF) | Tampering | Validate username charset; host also rejects malformed header values (`to_header_map`) |
| Secret leakage in plugin errors or logs (API key in URL, bearer token) | Information disclosure | Send key as `x-goog-api-key` header; log only feed names and status codes; no request echo (host precedent: fetch.rs doc "No error text carries guest data") |
| Malicious/huge API response (deep JSON, huge selftext, HTML error page) | DoS | Host caps responses at 8 MiB (`pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;`, fetch.rs:36) and records at 1,000,000 bytes; `serde_json` default recursion limit; cap item count per poll below 1000; truncate/skip oversize `raw` rather than panic |
| Redirect to a non-allowlisted host | Spoofing/SSRF | Host denies redirect hops outside the allowlist (`denied`), mapped to a `config` error |
| Client secret exposure to the plugin | Information disclosure | D-03 keeps `client_secret_env` host-side; add a host test asserting the recording stub receives no client secret in `init` for a `client_credentials` plugin |
| Replay of a rotated/stale token | Spoofing | Host refresh/persistence rules unchanged; in-memory app token has a 60 s margin |
| Panic in guest (index/unwrap on untrusted JSON) | DoS | No `unwrap`/`expect`/slicing on API data; the host discards a trapping instance and backs off, but a repeatable trap is a permanent poll loop |

## Sources

### Primary (HIGH confidence)
- Repository, read this session: `wit/types.wit`, `wit/worlds.wit`, `src/plugins/config.rs`, `src/plugins/auth/{client,provider,store,testing}.rs`, `src/plugins/host/{fetch,loader,map,scheduler,cursor}.rs`, `src/commands/auth.rs`, `src/plugins/mod.rs`, `plugins/README.md`, `plugins/stub-source/*`, `examples/plugins/*`, `tests/plugins_cli.rs`, Phase 6 `06-CONTEXT.md`/`06-RESEARCH.md`
- Executed experiment in `/tmp/exp/p`: cfg-gated bindings crate built with `cargo component build --release --offline`, conformed with `wasm-tools component targets ... -w source`, native tests passed with `cargo test --target x86_64-apple-darwin`, bare `cargo test` failed (exit 126); `time` (parsing only) and `percent-encoding` verified on both targets
- `gsd-tools query package-legitimacy check --ecosystem crates` (all OK), `cargo search`

### Secondary (MEDIUM confidence; official docs fetched, summarised by the fetch tool)
- https://github.com/reddit-archive/reddit/wiki/OAuth2 (grant types, Basic auth, token endpoint, 1-hour tokens, no refresh token for app-only)
- https://github.com/reddit-archive/reddit/wiki/API (User-Agent format, headers `X-Ratelimit-Used/Remaining/Reset`, OAuth required; its 60 req/min figure is superseded)
- https://praw.readthedocs.io/en/stable/getting_started/authentication.html (web/script app client credentials; installed apps cannot)
- https://raw.githubusercontent.com/praw-dev/prawcore/main/prawcore/rate_limit.py (`int(float(remaining))`, `int(reset)`)
- https://developers.google.com/youtube/v3/determine_quota_cost, /docs/playlistItems, /docs/playlistItems/list, /docs/subscriptions, /docs/subscriptions/list, /docs/core_errors
- https://developers.google.com/identity/protocols/oauth2/native-app and /scopes (endpoints, loopback, `youtube.readonly`)
- https://docs.cloud.google.com/docs/authentication/api-keys-use (`x-goog-api-key`)

### Tertiary (LOW confidence; search results only, flagged in the Assumptions Log)
- Reddit "Responsible Builder Policy" announcement (r/redditdev, November 2025) and third-party summaries (A8)
- Stack Overflow / Google issue tracker threads on the undocumented `UC`->`UU` convention (T3a)
- Third-party write-ups on Reddit 100 QPM/10-minute window and Google refresh-token 7-day Testing expiry

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - crates mirror existing stubs; build and test pattern executed end to end.
- Architecture: HIGH for host integration (code read) and plugin seam (experiment); MEDIUM for the `client_credentials` provider design (a recommendation, not yet implemented).
- Pitfalls: MEDIUM - host-side pitfalls verified in code; API-behaviour pitfalls (Reddit shapes, YouTube placeholders) rest on documentation plus assumptions pinned by the required fixtures.

**Research date:** 2026-11-11
**Valid until:** 2026-11-25 (Reddit access policy and quota docs move quickly); host-code findings valid until Phase 7 plans land.
