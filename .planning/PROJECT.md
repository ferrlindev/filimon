# filimon (`fili`)

## What This Is

filimon is a Rust, streaming-first semantic news crawler: pages flow through Fluvio topics, get processed by WISE-framework SmartModules (preprocess → extract → relevance filter), and are enriched with zero-shot NER (GLiNER) into JSON output. This milestone adds a **WASM/WIT plugin system** so social platforms (Reddit and YouTube first; X and others later) can stream posts into the same pipeline as additional sources — producing the same enriched output as news articles.

## Core Value

A user-supplied WASM component, written only against filimon's published WIT contract and listed in the registry, can stream items from a social platform into Fluvio and come out the other end as enriched records alongside news articles.

## Requirements

### Validated

<!-- Inferred from existing code (see .planning/codebase/). -->

- ✓ CLI (`fili`) with clap-based subcommands dispatching to `src/commands/` — existing
- ✓ HTTP crawler fetches pages, extracts and prioritises links, and produces `RawPage` records to Fluvio — existing
- ✓ Local HTML files can be produced to Fluvio (`fili file <paths>`) — existing
- ✓ Typed Fluvio producer/consumer layer over JSON records (`src/streaming.rs`) — existing
- ✓ WISE pipeline as chained SmartModules: `map-preprocess` → `map-extract` → `filter-relevance` (threshold 0.05) — existing
- ✓ Consumer reads `articles` topic, optionally enriches with GLiNER NER (batch/`--limit` mode), writes JSON — existing
- ✓ WIT package `filimon:plugin@0.1.0` defining `source` (exports init/poll; imports fetch, log) and `fetcher` worlds; stub crates build components from the WIT alone — Phase 1
- ✓ `filimon.toml` registry + per-plugin TOML load into typed config; `.env` secrets resolved per plugin alias only; errors name plugin and field/key — Phase 1
- ✓ Unified `SourceItem` envelope covers news pages and social posts; news stays on the WISE SmartModule chain, social items pass straight through to the consumer, and both land NER-enriched in the same JSON output — Phase 2

### Active

- [ ] Plugin host in `fili` built on the WASM component model, loading `.wasm` components listed in an explicit registry (`filimon.toml`)
- [ ] Host-provided HTTP fetch import with per-plugin domain allowlist, timeouts, and rate limiting
- [ ] Host-driven polling: host calls `poll(cursor)` on a schedule, persists the returned cursor, produces items to Fluvio
- [ ] Host passes each plugin's resolved settings and secrets into `init` (loading/validation done in Phase 1; delivery needs the Phase 3 host)
- [ ] Generic host-managed OAuth (PKCE + local callback): `fili auth login <plugin>`, token storage and refresh on the host; plugin TOML declares endpoints/scopes; plugin receives only access tokens via WIT
- [ ] Reference Reddit plugin (official Reddit API) as a separately built `.wasm` component
- [ ] Reference YouTube plugin (official YouTube Data API) as a separately built `.wasm` component
- [ ] Fetcher plugins: a second WIT world (`fetcher`) so the HTTP client is itself a supplied WASM component; source plugins' fetch and (optionally) the news crawler route through the configured fetcher; default HTTP fetcher shipped; host still enforces allowlist/timeout
- [ ] Plugin CLI: `fili plugins list | check | run-once`, plus a run command that polls all enabled sources
- [ ] Fixture-based tests per plugin (recorded API responses; runs in CI without network or keys)
- [ ] Integration tests running compiled Reddit/YouTube components inside the host against replayed fixtures into a local Fluvio topic; live mode when `.env` keys exist
- [ ] Live end-to-end run: `fili` loads Reddit + YouTube plugins, polls real APIs, records flow through Fluvio, enriched JSON is produced alongside news articles

### Out of Scope

- X (Twitter) plugin — deferred: X API read access is gated by the developer app's paid tier, and user login does not bypass it; revisit once access is sorted (user decision)
- Scraping / unofficial endpoints / session-cookie access — official APIs only (user decision; ToS risk)
- Fixing existing `fluvio-wise` gaps (watch mode not producing, silent record drops in streaming layer, NER only in batch mode, dead commented code in `main.rs`) — explicitly not this milestone (user decision)
- Auto-discovering plugins from a directory — explicit registry only (user decision)
- Native dynamic libraries / external-process plugins — WASM components with WIT only (user decision)
- Plugin-driven push loops — host polls; plugins are stateless (user decision)

## Context

- Brownfield Rust 2024 crate (v0.3.0, rust-version 1.85) with a Cargo workspace including three SmartModule crates under `smartmodules/`. Codebase map in `.planning/codebase/` (ARCHITECTURE, STACK, CONCERNS, etc.).
- Fluvio 0.50 SDK; SmartModules are WASM themselves (compiled with SMDK, Rust 1.85 pinned for SmartModule builds). Plugin components are a *different* WASM use: host-side components loaded by `fili`, not SmartModules running in the Fluvio cluster.
- The Fluvio integration work lives on branch `fluvio-wise` and is currently uncommitted (`src/commands/`, `src/streaming.rs`, `src/config.rs`, `smartmodules/`). Plugin work builds on top of it.
- Current input record is `RawPage { url, html }`; social posts are short, already-structured items, which is why a unified `SourceItem` envelope is needed rather than forcing posts into HTML.
- GLiNER NER runs consumer-side (ONNX model too large for WASM).
- `fluvio/` in the workspace root is a separate git checkout of the upstream Fluvio repo (reference only).
- Known concerns relevant to this work (from CONCERNS.md): streaming layer silently drops deserialization errors; HTTP client in crawler is rebuilt per request; crawler disables TLS cert validation (sandbox-only) — the new host fetch must NOT inherit that.
- Motivation: social posts are additional sources feeding the same output — not a separate analytics product.

## Constraints

- **Plugin technology**: WASM component model + WIT — user decision; plugins must be buildable outside the filimon repo from the published WIT alone.
- **Network access**: plugins have no direct network; all HTTP goes through the host fetch import — enables allowlists, rate limits, timeouts.
- **Secrets**: API keys live in `.env`; per-plugin settings in a per-plugin TOML file; OAuth refresh tokens never cross into the plugin.
- **Data sources**: official platform APIs only.
- **Tech stack**: Rust (existing crate), Fluvio 0.50, existing SmartModule toolchain (Rust 1.85 for SmartModules).
- **Verification**: every plugin must be testable offline via recorded fixtures; milestone completion requires one live end-to-end run with real Reddit and YouTube APIs.

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| WASM components with WIT for plugins | Sandboxed, language-agnostic, third parties can supply plugins without rebuilding `fili` | — Pending |
| Host-provided fetch import (not WASI HTTP) | Host enforces domain allowlist, rate limits, timeouts; plugins stay simple | — Pending |
| Host polls plugins; host owns cursor | Plugins stay stateless; scheduling, persistence and backpressure in one place | — Pending |
| Explicit registry (`filimon.toml`) | Nothing runs unless listed; predictable and auditable | — Pending |
| Keys in `.env`, settings in per-plugin TOML | Separates secrets from shareable config | — Pending |
| Generic host-managed OAuth (PKCE) | Written once, reused by any plugin; refresh tokens stay on host | — Pending |
| Unified `SourceItem` envelope, per-type SmartModule chains | Social posts aren't HTML; still converge on same enriched output | ✓ Good — Phase 2: news keeps the WISE chain, social items skip it (D-01 leaves the SmartModules unchanged), both reach one NER-enriched output |
| Ship Reddit + YouTube; defer X | X read access requires paid app tier regardless of user login | — Pending |
| Leave existing `fluvio-wise` gaps alone | Keep milestone focused on plugin system | — Pending |
| Fetcher is itself a WASM plugin (`fetcher` world) | Lets a future headless-browser fetcher (remote CDP) be supplied for crawling without host changes | — Pending |
| Consumer diagnostics never carry record content, and all stderr writes go through one choke point | Record values are untrusted (page content, social text) and reach a terminal; control bytes and content must not leak | ✓ Good — Phase 2 (02-02, 02-04) |
| An out-of-range `published` becomes null with a located note; the record is still written, and a record and its notes stay one item | A bad timestamp must not lose a record, and a note must never name a record that was not written | ✓ Good — Phase 2 (02-06, 02-07) |
| Social items pass through (no social SmartModule chain in v1) | Keep v1 minimal; filtering deferred to v2 | — Pending |
| Reddit: subreddits + user feed; YouTube: channel uploads + user subscriptions | User-selected v1 feeds; search/comments deferred | — Pending |
| Skip project-level research | User decision at init; per-phase research remains enabled | — Pending |
| Plugin components target `wasm32-unknown-unknown` (no WASI imports) | Every capability is an explicit WIT import; smallest attack surface | ✓ Phase 1 — host acceptance of wasip1 components open for Phase 3 |
| Hand-written literal-or-reject `.env` parser (dotenvy removed) | dotenvy 0.15 always expands `$` from the process env (CR-01); verbatim-or-reject closes it and stays reversible | ✓ Phase 1 (01-05) |
| Secrets wrapped in redacting `Secret` newtype; diagnostics carry alias/env names only | Secret values never reach Debug, tracing or rendered reports | ✓ Phase 1 |

## Evolution

This document evolves at phase transitions and milestone boundaries.

**After each phase transition** (via `/gsd-transition`):
1. Requirements invalidated? → Move to Out of Scope with reason
2. Requirements validated? → Move to Validated with phase reference
3. New requirements emerged? → Add to Active
4. Decisions to log? → Add to Key Decisions
5. "What This Is" still accurate? → Update if drifted

**After each milestone** (via `/gsd-complete-milestone`):
1. Full review of all sections
2. Core Value check — still the right priority?
3. Audit Out of Scope — reasons still valid?
4. Update Context with current state

---
*Last updated: 2026-11-09 after Phase 2*
