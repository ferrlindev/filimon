---
last_mapped_commit: ee408de9974ea0cee802f332ee2386ef13b0428b
last_mapped_at: 2026-11-22
---
# External Integrations

**Analysis Date:** 2026-11-22

## APIs & External Services

**Social/content sources (via Wasm plugins):**
- Reddit - OAuth API at `https://oauth.reddit.com` (app-only token flow)
  - Plugin: `plugins/reddit`
  - Auth: `REDDIT_CLIENT_ID`, `REDDIT_CLIENT_SECRET`, `REDDIT_USER_AGENT_CONTACT` (optional)
- YouTube Data API v3 - `plugins/youtube`
  - Auth: `YOUTUBE_API_KEY`; optional user OAuth (`YOUTUBE_OAUTH_CLIENT_SECRET`) through `src/plugins/auth/`
- RSS/Atom feeds - `plugins/rss` (arbitrary syndication URLs)
- Generic HTTP - `plugins/http-fetcher`; test doubles `plugins/replay-fetcher`, `stub-fetcher`, `stub-source`, `fault-source`
- Plugins fetch only through host-mediated `fetch` (`src/plugins/host/fetch.rs`, contract `wit/host.wit`)

**Web crawling:**
- Arbitrary websites fetched with reqwest + scraper (`src/crawler.rs`)

**Model hosting:**
- Hugging Face - GLiNER model download by `model-downloader` compose service (`docker-compose.yml`), stored in `models/`

## Data Storage

**Databases:**
- None. Streaming log is Fluvio.

**Streaming:**
- Fluvio cluster (SC + SPU), client in `src/streaming.rs`; topic `filimon-social-items` (created manually via `fluvio topic create`)
- SmartModules: `smartmodules/filter-relevance`, `map-preprocess`, `map-extract`
- Ports (compose host-offset): 19003, 19005, 19010, 19011

**File Storage:**
- Local filesystem: `output/` (`src/output.rs`), plugin cursors/state (`src/plugins/host/cursor.rs`), logins at `<state-dir>/auth/<alias>.json` (mode 0o600, `src/plugins/auth/store.rs`)

**Caching:**
- None

## Authentication & Identity

**Auth Provider:**
- Custom OAuth 2.0 with PKCE and local callback for plugins (`src/plugins/auth/pkce.rs`, `callback.rs`, `login.rs`, `provider.rs`, `client.rs`)
- Per-plugin secret scoping in `src/plugins/secrets.rs` (redacted Debug)

## Monitoring & Observability

**Error Tracking:**
- None

**Logs:**
- `tracing` + `tracing-subscriber` (env-filter); `RUST_LOG` in compose

## CI/CD & Deployment

**Hosting:**
- Docker Compose (`Dockerfile`, `docker-compose.yml`)

**CI Pipeline:**
- None detected (no `.github/workflows`)

## Environment Configuration

**Required env vars:**
- `REDDIT_CLIENT_ID`, `REDDIT_CLIENT_SECRET`, `YOUTUBE_API_KEY` (only for credentialed tests / those plugins); `REDDIT_USER_AGENT_CONTACT` optional
- `RUST_LOG`

**Secrets location:**
- `.env` at repo root (gitignored, present); template `.env.example`

## Webhooks & Callbacks

**Incoming:**
- Transient localhost OAuth redirect listener during login (`src/plugins/auth/callback.rs`)

**Outgoing:**
- None

---

*Integration audit: 2026-11-22*
