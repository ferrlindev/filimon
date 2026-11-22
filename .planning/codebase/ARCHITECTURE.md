---
last_mapped_commit: ee408de9974ea0cee802f332ee2386ef13b0428b
last_mapped_at: 2026-11-22
---
<!-- refreshed: 2026-11-22 -->
# Architecture

**Analysis Date:** 2026-11-22

## System Overview

```text
┌─────────────────────────────────────────────────────────────┐
│                CLI (binary `fili`, clap)                     │
│  `src/main.rs`  `src/cli.rs`  `src/commands/*.rs`            │
├──────────────┬───────────────┬───────────────┬──────────────┤
│ produce/file │  watch        │ run / plugins │ consume      │
│ crawler.rs   │ commands/     │ commands/     │ commands/    │
│              │ watch.rs      │ run.rs,       │ consume.rs   │
│              │               │ plugins.rs    │ scorer.rs    │
└──────┬───────┴───────┬───────┴───────┬───────┴──────┬───────┘
       │               │               │              │
       ▼               ▼               ▼              ▲
┌──────────────────────────┐  ┌───────────────────────┴──────┐
│ Wasm plugin host         │  │ Streaming layer              │
│ `src/plugins/host/`      │  │ `src/streaming.rs`           │
│ (wasmtime, scheduler,    │──▶ topics, producers, consumers │
│ cursors, sink, fetch)    │  └───────────────┬──────────────┘
│ `src/plugins/config.rs`  │                  ▼
│ `src/plugins/auth/`      │  ┌──────────────────────────────┐
│ `src/plugins/secrets.rs` │  │ Fluvio cluster + SmartModules│
└────────────▲─────────────┘  │ `smartmodules/*`             │
             │ WIT            └──────────────────────────────┘
┌────────────┴─────────────┐
│ Guest components (wasm)  │
│ `plugins/*`, `wit/*.wit` │
└──────────────────────────┘
```

## Component Responsibilities

| Component | Responsibility | File |
|-----------|----------------|------|
| CLI dispatch | Parse args, init logging, route subcommands | `src/main.rs`, `src/cli.rs`, `src/args.rs` |
| Crawler | Fetch/parse URLs and local HTML into `RawPage` | `src/crawler.rs` |
| Streaming | Topic names, `PipelineCoordinator`, Fluvio produce/consume | `src/streaming.rs` |
| Models | `RawPage`, `ExtractedArticle`, `SourceItem` (news + social) | `src/models.rs` |
| NER scorer | GLiNER entity extraction on consumer side | `src/scorer.rs` |
| Output | JSON writer, terminal sanitising | `src/output.rs` |
| Plugin registry config | Parse `filimon.toml` + per-plugin TOML + `.env` | `src/plugins/config.rs`, `src/plugins/secrets.rs` |
| Plugin auth | OAuth (PKCE, callback, token store, providers) | `src/plugins/auth/` |
| Host engine | wasmtime engine, linker, guest imports | `src/plugins/host/engine.rs`, `instance.rs` |
| Scheduler | Per-plugin poll loop, backoff, cursor persist | `src/plugins/host/scheduler.rs`, `backoff.rs` |
| Cursor store | Durable per-plugin cursors in state dir | `src/plugins/host/cursor.rs` |
| Sink | `ItemSink` trait; `FluvioSink` and test `VecSink` | `src/plugins/host/sink.rs` |
| Item mapping | WIT `item` to `SourceItem::social`, validation | `src/plugins/host/map.rs` |
| Fetch policy | Host-side outbound HTTP for guests | `src/plugins/host/fetch.rs` |
| SmartModules | preprocess, extract, relevance filter chain | `smartmodules/map-preprocess`, `map-extract`, `filter-relevance` |
| Source plugins | RSS, Reddit, YouTube, stub, fault (wasm components) | `plugins/rss`, `plugins/reddit`, `plugins/youtube`, `plugins/stub-source`, `plugins/fault-source` |
| Fetcher plugins | Pluggable HTTP fetchers | `plugins/http-fetcher`, `plugins/replay-fetcher`, `plugins/stub-fetcher` |

## Pattern Overview

**Overall:** Streaming pipeline over Fluvio topics, with a WebAssembly component plugin host (WIT contract) feeding a social topic.

**Key Characteristics:**
- Single Rust binary `fili` (`Cargo.toml` `[[bin]]`), subcommand-per-mode in `src/commands/`.
- Plugins are separate crates compiled to wasm components, each its own crate with its own `Cargo.lock` and `rust-toolchain.toml`; they are not built by the main package.
- Plugin output is untrusted; the host is the validation gate (`src/plugins/host/map.rs`).
- Cursor is persisted only after the sink accepts a batch (at-least-once).
- Generic over traits (`ItemSink`, `InstanceFactory`) so tests use in-memory fakes (`src/plugins/host/testing.rs`).

## Layers

**CLI/commands:**
- Location: `src/commands/`, `src/cli.rs`
- Depends on: streaming, plugins, crawler, output
- Used by: `src/main.rs`

**Domain/model:**
- Location: `src/models.rs`, `src/scorer.rs`, `src/output.rs`
- Depends on: serde, gline-rs

**Streaming:** `src/streaming.rs` (Fluvio client, topics `filimon-raw-pages` and `filimon-social-items`).

**Plugin config + auth:** `src/plugins/config.rs`, `src/plugins/auth/`, `src/plugins/secrets.rs`; secrets are filtered per plugin before crossing into a guest `init`.

**Plugin host:** `src/plugins/host/` (see table above).

**Guest contract:** `wit/host.wit`, `wit/types.wit`, `wit/worlds.wit` (package `filimon:plugin@0.1.0`; worlds `source` with `init`/`poll`, and `fetcher` with `fetch`).

## Data Flow

### News path

1. `fili produce <URL>` / `fili file` / `fili watch` crawl or read HTML (`src/crawler.rs`, `src/commands/produce.rs`, `watch.rs`).
2. `RawPage` produced to `filimon-raw-pages` (`src/streaming.rs`).
3. Consumer attaches SmartModule chain map-preprocess, map-extract, filter-relevance (`smartmodules/*`, `transforms*.yaml`).
4. `fili consume` (`src/commands/consume.rs`) applies NER (`src/scorer.rs`) and writes JSON (`src/output.rs`).

### Social plugin path

1. `fili run` (`src/commands/run.rs`) loads registry (`src/plugins/config.rs`) and `Host::load` (`src/plugins/host/mod.rs`).
2. Per-plugin task in `scheduler.rs`: instantiate, `init`, `poll(cursor, access-token)`.
3. Items mapped (`map.rs`) to `SourceItem::social`, sent via `FluvioSink` to `filimon-social-items`, then cursor persisted.
4. `fili consume` merges social items with news items.

`fili plugins run-once|check|list` uses `src/plugins/host/once.rs` with no sink/cursor.

**State Management:** Cursors and tokens in a state directory next to the registry file; locked against concurrent processes (`src/commands/plugins.rs` `state_dir_for`).

## Key Abstractions

**`SourceItem`:** unified news/social record, `src/models.rs`.
**`ItemSink`:** output seam for the host, `src/plugins/host/sink.rs`.
**`InstanceFactory`:** wasm instance creation seam, `src/plugins/host/instance.rs`.
**`LoadedRegistry` / `PluginConfig`:** typed registry, `src/plugins/config.rs`.

## Entry Points

**`fili` binary:** `src/main.rs` (clap `Cli`/`Command` in `src/cli.rs`); subcommands: produce, file, consume, watch, plugins, run, auth.
**Guest components:** `plugins/*/src/lib.rs` (rss/reddit/youtube also split `component.rs`, `poll.rs`, `cursor.rs`, `http.rs`, `settings.rs`).

## Architectural Constraints

- **Threading:** tokio multi-thread runtime; one task per plugin; wasmtime epoch ticker thread (10 ms) in `engine.rs`; `poll` never concurrent on one instance.
- **Global state:** none module-level beyond constants (`TOPIC_*` in `src/streaming.rs`).
- **No WASI** linked into guests; only `filimon:plugin/*` interfaces.
- **Message key** `"{platform}:{id}"` assumes a single-partition social topic (`map.rs`).
- Lints: clippy `pedantic` warn, `unsafe_code` warn (`Cargo.toml`).

## Anti-Patterns

### Running project commands or secrets through plugin boundary

**What happens:** Secrets passed to a guest unfiltered.
**Why it's wrong:** Guests are untrusted.
**Do this instead:** Declare aliases in the plugin TOML; filtering happens in `src/plugins/config.rs`.

### Trusting plugin item fields

**What happens:** Skipping validation of guest ids/urls.
**Why it's wrong:** The consumer does no validation.
**Do this instead:** Validate in `src/plugins/host/map.rs`; drop bad items with a content-free warning.

## Error Handling

**Strategy:** `thiserror` typed errors per module (`src/plugins/error.rs`, `host/error.rs`, `auth/error.rs`), `miette` diagnostics at the CLI.

**Patterns:**
- A bad item is dropped, never fails the poll.
- Load failures per component are reported by plugin name and path, no secrets or content.

## Cross-Cutting Concerns

**Logging:** `tracing`; default directive `fili::plugins=info` (`src/main.rs`).
**Validation:** terminal output sanitised via `sanitize_for_terminal` (`src/output.rs`).
**Authentication:** OAuth PKCE in `src/plugins/auth/`.

---

*Architecture analysis: 2026-11-22*
