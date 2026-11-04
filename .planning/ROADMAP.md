# Roadmap: filimon

## Overview

This milestone adds a WASM/WIT source-plugin system to `fili`, so social platforms can stream posts into the same Fluvio pipeline as news and come out as the same NER-enriched JSON. Work is built in horizontal layers. Each layer rests on the one below it and is verified on its own, mostly through tests and test components. The system is assembled end to end only in the final phase. The layers, in order: the author-facing contract (WIT worlds plus registry, plugin TOML and `.env` formats), the `SourceItem` data plane, the component host runtime, the operator CLI, the fetcher layer, host-managed OAuth, the Reddit and YouTube reference plugins, and finally integration, the live end-to-end run, and the authoring guide. The existing `fluvio-wise` gaps (watch mode, silent drops in the streaming layer, NER only in batch mode, dead code) stay out of scope. The `fluvio/` upstream checkout is reference-only and is never modified.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [x] **Phase 1: Plugin Contract & Configuration** - Versioned WIT package (`source` + `fetcher` worlds) plus the `filimon.toml` registry, per-plugin TOML, and `.env` secret loading with clear errors (completed 2026-11-03)
- [ ] **Phase 2: SourceItem Pipeline** - Unified `SourceItem` envelope; news stays on the WISE chain, social items pass through, and the consumer NER-enriches both into one JSON output
- [ ] **Phase 3: Component Host Runtime** - `fili` loads registry-listed components, polls them on schedule, persists cursors, isolates failures, and produces `SourceItem`s to Fluvio
- [ ] **Phase 4: Plugin CLI** - `fili plugins list | check | run-once` and a run command that polls all enabled sources
- [ ] **Phase 5: Fetcher Layer** - Plugin HTTP routed through a configurable fetcher component, a host-enforced allowlist and timeout, a default TLS-validating fetcher, and an optional crawler hook
- [ ] **Phase 6: Host-Managed OAuth** - `fili auth login | status | logout` with PKCE, host-side token storage and refresh, and only access tokens passed to plugins
- [ ] **Phase 7: Reddit & YouTube Plugins** - Two separately built reference `.wasm` source components on the official APIs, each with offline fixture tests
- [ ] **Phase 8: Integration, Live E2E & Authoring** - Compiled plugins tested in the host against replayed and live APIs, a live end-to-end run, and a guide that lets third parties build plugins outside the repo

## Phase Details

### Phase 1: Plugin Contract & Configuration
**Goal**: Plugin authors and operators have a stable, versioned contract to write against: WIT worlds for source and fetcher plugins, plus the registry, per-plugin TOML, and `.env` secret formats that `fili` loads and validates
**Depends on**: Nothing (first phase)
**Requirements**: WIT-01, WIT-02, CONF-01, CONF-02, CONF-03
**Success Criteria** (what must be TRUE):
  1. A versioned WIT package in the repo defines a `source` world and a `fetcher` world, and both validate with `wasm-tools`. The `source` world exports `init(settings, secrets)` and `poll(cursor, access-token) -> (items, next-cursor)` and imports `fetch` and `log`; settings, the plugin's named secrets, and access tokens reach the plugin through the `init`/`poll` parameters. The `fetcher` world exports `fetch(request) -> response` and imports a low-level outbound-HTTP capability.
  2. A stub source crate and a stub fetcher crate each generate bindings from the WIT package alone (via `cargo component`) and compile to a valid component for their world.
  3. A sample `filimon.toml` registry that points to per-plugin TOML files (subreddits or channels, interval, allowlist) loads into typed config. Each plugin's resolved secrets contain only the `.env` keys its own TOML names.
  4. A missing plugin TOML file, malformed TOML, or missing `.env` secret produces an error that names both the plugin and the field or key at fault.
**Plans:** 6/6 plans complete

Plans:
**Wave 1**
- [x] 01-01-PLAN.md — WIT package `filimon:plugin@0.1.0` (source + fetcher worlds) and stub source/fetcher components built with `cargo component`, checked with `wasm-tools component targets` (WIT-01, WIT-02)
- [x] 01-02-PLAN.md — `src/plugins/` config loader: registry → plugin TOML → `.env` secrets into typed `LoadedRegistry`, with per-plugin secret isolation and redaction (CONF-01, CONF-02)

**Wave 2** *(blocked on Wave 1 completion)*
- [x] 01-03-PLAN.md — Collect-all miette diagnostics naming plugin + field/key for every config problem (CONF-03)
- [x] 01-04-PLAN.md — `examples/plugins/` sample registry and plugin TOMLs proven to load (SC3), plus the global `--registry` / `FILI_REGISTRY` flag (CONF-01, CONF-02)

**Wave 3** *(gap closure; blocked on Wave 2 completion)*
- [x] 01-05-PLAN.md — Strict literal `.env` parser that replaces dotenvy `$` substitution (CR-01): values delivered verbatim or rejected by line number, no false MissingSecret cascade (WR-02), EmptySecret names its real source (WR-03), quoting rule documented (CONF-02, CONF-03)
- [x] 01-06-PLAN.md — `--registry` default test asserted from `Cli::command()` metadata so FILI_REGISTRY cannot skip it (IN-03) (CONF-01)

### Phase 2: SourceItem Pipeline
**Goal**: Every pipeline record travels as a unified `SourceItem`. News keeps flowing through the WISE SmartModule chain, while social items pass straight through to the consumer and land, NER-enriched, in the same JSON output
**Depends on**: Phase 1
**Requirements**: PIPE-01, PIPE-02, PIPE-03, PIPE-04
**Success Criteria** (what must be TRUE):
  1. A `SourceItem` envelope (source type, platform, id, url, title, text, author, published time, raw metadata) round-trips through JSON for both a news page and a social post, and its fields line up with the Phase 1 WIT item record.
  2. `fili produce <url>` and `fili file <paths>` still yield extracted articles at the consumer through `map-preprocess → map-extract → filter-relevance`. The news path does not regress.
  3. A social `SourceItem` produced to Fluvio reaches the consumer without any social-specific filtering, even when its text would fall below the news relevance threshold.
  4. In the consumer's existing NER mode (`--enrich-ner` with `--limit`), NER runs on social item text, and social and news records go to the same JSON output, each labelled with source type and platform.
**Plans:** 4/6 plans executed

Plans:
**Wave 1**
- [x] 02-01-PLAN.md — News `SourceItem` through the unmodified SmartModule chain attached by `fili consume` (consumer_with_config) into one JSON output; SourceItem contract (WIT alignment, round-trip, RFC 3339) and articles topic retired in code (PIPE-01, PIPE-02)

**Wave 2** *(blocked on Wave 1 completion)*
- [x] 02-02-PLAN.md — Social items on `filimon-social-items` with no SmartModules, merged with news under one total `--limit`, NER on social text, `--source`/`--output` flags, named missing-topic error (PIPE-03, PIPE-04)

**Wave 3** *(blocked on Wave 2 completion)*
- [x] 02-03-PLAN.md — README operator path proven live on both topics, codebase map and compose comment updated, no doc names the retired topic (PIPE-02, PIPE-03, PIPE-04)
- [x] 02-04-PLAN.md — Gap closure: content-free consume diagnostics (serde_json structured accessors, SmartModule hint prefix) and one sanitized stderr choke point; closes CR-01 / T-02-02 (PIPE-02, PIPE-03)

**Wave 4** *(gap closure; blocked on Wave 3 completion)*
- [ ] 02-05-PLAN.md — Gap 2 (WR-06 cap): past the 100-message cap, every skipped record is still named on stderr by topic and exact offset in per-topic offset ranges; budget, cap, content-freedom, choke point and streaming mode unchanged (PIPE-02, PIPE-03)

**Wave 5** *(gap closure; blocked on 02-05, same file)*
- [ ] 02-06-PLAN.md — Gap 3 (D-09 year limits, WR-09): one guarded RFC 3339 formatter returns null for UTC years outside 0000..=9999 on the social, news and epoch paths (PIPE-01, PIPE-03)

### Phase 3: Component Host Runtime
**Goal**: `fili` hosts WASM components. It loads only what the registry lists, polls each source on its schedule, persists cursors across restarts, isolates failing plugins, and produces returned items to Fluvio as `SourceItem` records
**Depends on**: Phase 1, Phase 2
**Requirements**: HOST-01, HOST-02, HOST-03, HOST-04, HOST-05
**Success Criteria** (what must be TRUE):
  1. Source and fetcher components listed and enabled in `filimon.toml` are loaded. A `.wasm` file that sits next to them but is not in the registry is never instantiated.
  2. A test source component is polled at the interval set in its TOML, and its items appear in a Fluvio topic as `SourceItem` records. After the host is stopped and restarted, polling resumes from the last persisted cursor instead of starting over.
  3. When a component returns an error or reports rate limiting (HTTP 429), the host backs it off using the backoff settings in that plugin's TOML and polls it again after the backoff window.
  4. A component that traps or errors is logged with its plugin name and backed off, while the other plugins keep polling and the `fili` process stays up.
**Plans**: TBD

### Phase 4: Plugin CLI
**Goal**: Users can inspect, validate, try, and run registered plugins from `fili` without writing code
**Depends on**: Phase 3
**Requirements**: CLI-01, CLI-02, CLI-03, CLI-04
**Success Criteria** (what must be TRUE):
  1. `fili plugins list` shows every registered source and fetcher plugin with its kind, enabled state, and last poll status (time and ok/error).
  2. `fili plugins check <name>` reports, without polling, whether the component satisfies its declared WIT world and whether its TOML and required `.env` secrets are valid. A broken component or config exits non-zero with a message naming the problem.
  3. `fili plugins run-once <name>` performs a single poll and prints the returned items. Nothing is produced to Fluvio unless the produce option is given.
  4. The run command polls every enabled source plugin on its interval and produces items to Fluvio until it is stopped.
**Plans**: TBD

### Phase 5: Fetcher Layer
**Goal**: All outbound HTTP from source plugins, and optionally from the news crawler, goes through a swappable fetcher component. The host enforces allowlists and timeouts whichever fetcher handles the request
**Depends on**: Phase 4
**Requirements**: FETCH-01, FETCH-02, FETCH-03, FETCH-04
**Success Criteria** (what must be TRUE):
  1. A source plugin's `fetch` calls are served by the fetcher component assigned to that source in `filimon.toml`. Changing the assignment to another fetcher changes which component handles the requests, without rebuilding either plugin.
  2. The host rejects, with a clear error, any request to a domain outside the source's allowlist and any request that exceeds its timeout. This holds for both the default fetcher and a custom fetcher.
  3. The bundled default HTTP fetcher component makes real HTTPS requests with TLS certificate validation on: a request to a host with an invalid certificate fails. The crawler's disabled certificate validation is not carried over.
  4. With the crawler configured to use a fetcher component, `fili produce <url>` fetches pages through that component and articles still reach the consumer.
**Plans**: TBD

### Phase 6: Host-Managed OAuth
**Goal**: Users can log in to a plugin's platform once through OAuth 2.0 PKCE. The host keeps tokens fresh and passes plugins only current access tokens
**Depends on**: Phase 4
**Requirements**: AUTH-01, AUTH-02, AUTH-03, AUTH-04
**Success Criteria** (what must be TRUE):
  1. `fili auth login <plugin>` sends the user to the provider's authorize URL (endpoints and scopes from that plugin's TOML), receives the code on a local callback, completes PKCE, and stores the tokens locally.
  2. The host refreshes an access token automatically before it expires, so the user does not have to log in again during normal polling.
  3. A logged-in plugin gets a current access token through the WIT token import and never sees the refresh token. A test component that records everything it receives can confirm this.
  4. `fili auth status` lists stored logins and their expiry state per plugin. `fili auth logout <plugin>` deletes that plugin's tokens, and later polls run without authentication.
**Plans**: TBD

### Phase 7: Reddit & YouTube Plugins
**Goal**: Two reference `.wasm` source components, built separately from `fili`, stream real Reddit and YouTube content through the host using only official APIs, and each is proven by offline fixture tests
**Depends on**: Phase 5, Phase 6
**Requirements**: RDDT-01, RDDT-02, RDDT-03, YTUB-01, YTUB-02, YTUB-03, TEST-01
**Success Criteria** (what must be TRUE):
  1. `fili plugins run-once reddit` returns new/hot posts from the subreddits in the Reddit plugin's TOML through the official Reddit API. It sends a compliant User-Agent and backs off according to Reddit's rate-limit headers.
  2. After `fili auth login reddit`, the Reddit plugin can also poll the logged-in user's home feed or saved posts.
  3. `fili plugins run-once youtube` returns new uploads from the channels in its TOML, read from their uploads playlists. After `fili auth login youtube`, it also returns new uploads from the user's subscriptions.
  4. YouTube polls use only low-cost Data API endpoints (no 100-unit search calls). A quota-exceeded response shows up as a clear, named error, not a silently empty poll.
  5. Each plugin's unit tests pass against recorded API fixtures with no network access and no API keys.
**Plans**: TBD

### Phase 8: Integration, Live E2E & Authoring
**Goal**: The whole system is proven end to end. Compiled plugins run inside the host against replayed and live APIs into Fluvio, enriched social JSON comes out alongside news, and a third party can build and register a plugin from the guide alone
**Depends on**: Phase 7
**Requirements**: TEST-02, TEST-03, TEST-04, WIT-03, DOCS-01, DOCS-02
**Success Criteria** (what must be TRUE):
  1. Integration tests run the compiled Reddit and YouTube `.wasm` components inside the host against replayed fixtures and assert that the expected `SourceItem`s land in a local Fluvio topic. They need no network and no keys, so they are safe for CI.
  2. The same integration tests run in live mode against the real APIs only when the required credentials are in `.env`, and skip cleanly when they are not.
  3. A live end-to-end run works: `fili` loads the Reddit and YouTube plugins, polls the real APIs, and records flow through Fluvio. The consumer writes NER-enriched social records alongside news articles in the same JSON output.
  4. Using only the plugin template and guide, a source plugin built with `cargo component` in a directory outside the filimon repo, against only the published WIT, is registered in `filimon.toml` and runs in an unmodified `fili` binary.
  5. The guide covers writing a fetcher plugin and assigning it to a source, and a fetcher built by following it serves that source's requests.
**Plans**: TBD

## Progress

**Execution Order:**
Phases execute in numeric order: 1 → 2 → 3 → 4 → 5 → 6 → 7 → 8. Phases 5 and 6 depend only on Phase 4, not on each other.

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Plugin Contract & Configuration | 6/6 | Complete    | 2026-11-03 |
| 2. SourceItem Pipeline | 4/4 | In Progress | - |
| 3. Component Host Runtime | 0/TBD | Not started | - |
| 4. Plugin CLI | 0/TBD | Not started | - |
| 5. Fetcher Layer | 0/TBD | Not started | - |
| 6. Host-Managed OAuth | 0/TBD | Not started | - |
| 7. Reddit & YouTube Plugins | 0/TBD | Not started | - |
| 8. Integration, Live E2E & Authoring | 0/TBD | Not started | - |
