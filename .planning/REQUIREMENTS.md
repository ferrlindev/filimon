# Requirements: filimon (`fili`) — Social Source Plugin System

**Defined:** 2026-11-03
**Core Value:** A user-supplied WASM component, written only against filimon's published WIT contract and listed in the registry, can stream items from a social platform into Fluvio and come out the other end as enriched records alongside news articles.

## v1 Requirements

Requirements for this milestone. Each maps to roadmap phases.

### Plugin Contract (WIT)

- [ ] **WIT-01**: A versioned WIT package in the repo defines a `source` world — plugin exports `init(config)` and `poll(cursor) -> (items, next-cursor)`; host provides `fetch`, `log`, and settings/secret/token access imports
- [ ] **WIT-02**: The same WIT package defines a `fetcher` world — fetcher exports `fetch(request) -> response`; host provides a low-level outbound-HTTP capability import
- [ ] **WIT-03**: A plugin built outside the filimon repo, using only the published WIT, loads and runs in `fili` without rebuilding `fili`

### Plugin Host

- [ ] **HOST-01**: `fili` loads source and fetcher components listed in an explicit registry (`filimon.toml`); nothing unlisted is loaded
- [ ] **HOST-02**: `fili` polls each enabled source plugin on its configured interval, persists the returned cursor, and resumes from it after restart
- [ ] **HOST-03**: Per-plugin TOML sets poll interval and backoff on errors / HTTP 429
- [ ] **HOST-04**: Items returned by plugins are produced to Fluvio as `SourceItem` records
- [ ] **HOST-05**: A plugin that traps or returns an error is logged and backed off without crashing `fili` or stopping other plugins

### Fetchers

- [ ] **FETCH-01**: A source plugin's `fetch` calls are routed to the fetcher component configured for that source in `filimon.toml`
- [ ] **FETCH-02**: The host enforces each source's domain allowlist and request timeout regardless of which fetcher handles the request
- [ ] **FETCH-03**: filimon ships a default HTTP fetcher component with TLS certificate validation enabled
- [ ] **FETCH-04**: The news crawler can be configured to fetch pages through a fetcher component instead of its built-in client (hook for a future headless-browser fetcher)

### Config & Secrets

- [ ] **CONF-01**: Each plugin has its own TOML settings file (subreddits, channels, intervals, allowlist) referenced from `filimon.toml`
- [ ] **CONF-02**: API keys / client secrets load from `.env`; each plugin receives only the secrets its config names
- [ ] **CONF-03**: Invalid config (missing file, bad TOML, missing secret) produces a clear error naming the plugin and field

### OAuth

- [ ] **AUTH-01**: User can run `fili auth login <plugin>` to complete an OAuth 2.0 PKCE flow via local callback, using endpoints and scopes declared in the plugin's TOML
- [ ] **AUTH-02**: Host stores tokens locally and refreshes access tokens automatically before expiry
- [ ] **AUTH-03**: Plugins receive only a current access token via the WIT contract — never refresh tokens
- [ ] **AUTH-04**: User can run `fili auth status` and `fili auth logout <plugin>` to inspect and revoke stored logins

### Pipeline

- [ ] **PIPE-01**: A unified `SourceItem` envelope (source type, platform, id, url, title, text, author, published time, raw metadata) covers news pages and social posts
- [ ] **PIPE-02**: News items continue through the existing WISE SmartModule chain
- [ ] **PIPE-03**: Social items pass through to the consumer with no social-specific filtering in v1
- [ ] **PIPE-04**: The consumer runs NER on social item text and writes it to the same JSON output as news articles, labelled by source

### CLI

- [ ] **CLI-01**: User can run `fili plugins list` to see registered plugins (source + fetcher), enabled state, and last poll status
- [ ] **CLI-02**: User can run `fili plugins check <name>` to validate a component against its WIT world plus its config/secrets, without polling
- [ ] **CLI-03**: User can run `fili plugins run-once <name>` to perform a single poll and print returned items (no Fluvio produce unless requested)
- [ ] **CLI-04**: User can start a run command that polls all enabled source plugins and produces to Fluvio

### Reddit Plugin

- [ ] **RDDT-01**: Reddit plugin polls new/hot posts from subreddits configured in its TOML via the official Reddit API
- [ ] **RDDT-02**: With a logged-in user, Reddit plugin can poll that user's own home feed / saved posts
- [ ] **RDDT-03**: Reddit plugin honours Reddit rate-limit headers and sends a compliant User-Agent

### YouTube Plugin

- [ ] **YTUB-01**: YouTube plugin polls new uploads from channels configured in its TOML via the YouTube Data API (uploads playlists)
- [ ] **YTUB-02**: With a logged-in user, YouTube plugin can poll new uploads from that user's subscriptions
- [ ] **YTUB-03**: YouTube plugin stays within daily quota by using low-cost endpoints and surfaces quota errors clearly

### Testing

- [ ] **TEST-01**: Each plugin has unit tests against recorded API fixtures — no network or keys required
- [ ] **TEST-02**: Integration tests run the compiled Reddit and YouTube `.wasm` plugins inside the host against replayed fixtures and assert the expected `SourceItem`s reach a local Fluvio topic
- [ ] **TEST-03**: Integration tests have a live mode that runs against real APIs only when credentials are present in `.env`
- [ ] **TEST-04**: A live end-to-end run with real Reddit and YouTube data produces enriched JSON alongside news articles

### Authoring

- [ ] **DOCS-01**: Plugin template plus guide covering building a source plugin with `cargo component`, writing its TOML, and registering it in `filimon.toml`
- [ ] **DOCS-02**: Guide covers writing a fetcher plugin and assigning it to a source

## v2 Requirements

Deferred to a future release. Tracked but not in current roadmap.

### Platforms

- **XPLT-01**: X (Twitter) plugin reading the logged-in user's home timeline (requires paid X API tier)
- **RDDT-04**: Reddit search-query feed
- **RDDT-05**: Reddit post comments
- **YTUB-04**: YouTube search-query feed
- **YTUB-05**: YouTube video comments

### Fetchers

- **FETCH-05**: Headless-browser fetcher component (via remote CDP/Browserless endpoint) usable by crawler and sources

### Host Hardening

- **HOST-06**: Per-plugin CPU (fuel/epoch) and memory limits
- **HOST-07**: Cross-poll item dedupe per plugin

### Pipeline

- **PIPE-05**: Social SmartModule chain (text normalization, relevance filter, min-length filter)

### Authoring

- **DOCS-03**: Rust guest SDK crate wrapping wit-bindgen output

## Out of Scope

Explicitly excluded. Documented to prevent scope creep.

| Feature | Reason |
|---------|--------|
| Scraping / unofficial endpoints / session cookies | Official APIs only (user decision; ToS risk) |
| Auto-discovery of plugins from a directory | Explicit registry only (user decision) |
| Native dylib or external-process plugins | WASM components with WIT only (user decision) |
| Plugin-driven push loops | Host polls; plugins stateless (user decision) |
| Fixing existing `fluvio-wise` gaps (watch mode, silent drops, batch-only NER, dead code) | Not this milestone (user decision) |

## Traceability

Which phases cover which requirements. Updated during roadmap creation.

| Requirement | Phase | Status |
|-------------|-------|--------|
| WIT-01 | Phase 1 | Pending |
| WIT-02 | Phase 1 | Pending |
| WIT-03 | Phase 8 | Pending |
| HOST-01 | Phase 3 | Pending |
| HOST-02 | Phase 3 | Pending |
| HOST-03 | Phase 3 | Pending |
| HOST-04 | Phase 3 | Pending |
| HOST-05 | Phase 3 | Pending |
| FETCH-01 | Phase 5 | Pending |
| FETCH-02 | Phase 5 | Pending |
| FETCH-03 | Phase 5 | Pending |
| FETCH-04 | Phase 5 | Pending |
| CONF-01 | Phase 1 | Pending |
| CONF-02 | Phase 1 | Pending |
| CONF-03 | Phase 1 | Pending |
| AUTH-01 | Phase 6 | Pending |
| AUTH-02 | Phase 6 | Pending |
| AUTH-03 | Phase 6 | Pending |
| AUTH-04 | Phase 6 | Pending |
| PIPE-01 | Phase 2 | Pending |
| PIPE-02 | Phase 2 | Pending |
| PIPE-03 | Phase 2 | Pending |
| PIPE-04 | Phase 2 | Pending |
| CLI-01 | Phase 4 | Pending |
| CLI-02 | Phase 4 | Pending |
| CLI-03 | Phase 4 | Pending |
| CLI-04 | Phase 4 | Pending |
| RDDT-01 | Phase 7 | Pending |
| RDDT-02 | Phase 7 | Pending |
| RDDT-03 | Phase 7 | Pending |
| YTUB-01 | Phase 7 | Pending |
| YTUB-02 | Phase 7 | Pending |
| YTUB-03 | Phase 7 | Pending |
| TEST-01 | Phase 7 | Pending |
| TEST-02 | Phase 8 | Pending |
| TEST-03 | Phase 8 | Pending |
| TEST-04 | Phase 8 | Pending |
| DOCS-01 | Phase 8 | Pending |
| DOCS-02 | Phase 8 | Pending |

**Coverage:**
- v1 requirements: 39 total
- Mapped to phases: 39
- Unmapped: 0 ✓

---
*Requirements defined: 2026-11-03*
*Last updated: 2026-11-03 after roadmap creation (traceability mapped)*
