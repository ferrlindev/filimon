---
last_mapped_commit: 5122e6b9c4aa51ca49831917f1a1035fdc543870
last_mapped_at: 2026-11-03
---
<!-- refreshed: 2026-11-03 -->
# Architecture

**Analysis Date:** 2026-11-03

## System Overview

filimon is a Fluvio-based streaming pipeline for semantic news extraction. It implements the WISE framework (Web-Intelligent Semantic Extractor) with real-time record processing through SmartModules (WASM transformations) and zero-shot Named Entity Recognition using GLiNER.

```text
┌─────────────────────────────────────────────────────────────────┐
│                       CLI Entry Point (main.rs)                 │
│              Dispatch to command handlers via clap              │
├────────────────┬──────────────────┬──────────────┬─────────────┤
│   Produce      │    Consume       │    Watch     │    File     │
│  (crawler)     │ (merged streams) │ (filesystem) │  (local)    │
└────────────────┴──────────────────┴──────────────┴─────────────┘
         │                │                 │             │
         ▼                ▼                 ▼             ▼
┌─────────────────────────────────────────────────────────────────┐
│              Streaming Layer (streaming.rs)                     │
│   PipelineCoordinator → TypedProducer (SourceItem::news)        │
│   FluvioStream::preflight / topic_stream (consumer_with_config) │
│   Topics: filimon-raw-pages (news), filimon-social-items        │
└─────────────────────────────────────────────────────────────────┘
         │                                                │
         ▼                                                ▼
┌──────────────────────────────────────┐   ┌──────────────────────┐
│ filimon-raw-pages stream, with the   │   │ filimon-social-items │
│ news chain attached by fili consume  │   │ stream, no           │
│ (SmartModules run in the cluster):   │   │ SmartModules         │
│  map-preprocess → map-extract →      │   │ (social SourceItem)  │
│  filter-relevance (threshold 0.05)   │   │                      │
│  → ExtractedArticle                  │   │                      │
└──────────────────────────────────────┘   └──────────────────────┘
         │                                                │
         └──────────────────┬─────────────────────────────┘
                            ▼
┌──────────────────────────────────────────────────────────────────┐
│         Consumer & Enrichment (commands/consume.rs)              │
│                                                                  │
│  stream::select merge ──→ limit / idle stop ──→ NER (GLiNER)     │
│  (OutputRecord, models.rs)                      (scorer.rs)      │
│                                                  │               │
│                                                  ▼               │
│                          output::save_json → one JSON array      │
│                          (default output/wise_output.json)       │
└──────────────────────────────────────────────────────────────────┘
```

## Component Responsibilities

| Component | Responsibility | File |
|-----------|----------------|------|
| CLI Entry | Parse commands, load NER engine, dispatch handlers | `src/main.rs` |
| Commands | Implement produce/consume/watch/file operations | `src/commands/{produce,consume,watch}.rs` |
| Streaming | Wrap Fluvio client, typed produce/consume, topic management | `src/streaming.rs` |
| Models | Define serializable data structures for pipeline stages | `src/models.rs` |
| Crawler | Fetch pages, extract links, score priority | `src/crawler.rs` |
| Scorer | GLiNER-based zero-shot NER | `src/scorer.rs` |
| Output | Format and save results to JSON/stdout | `src/output.rs` |
| Config | Load watch directories from JSON | `src/config.rs` |

## Pattern Overview

**Overall:** Event-driven streaming pipeline with WASM-based transformations

**Key Characteristics:**
- Command pattern for CLI dispatch to distinct modes
- Producer-consumer pattern through Fluvio topics
- Typed generics for serialization/deserialization
- WASM-based transformations for CPU-intensive processing
- Optional consumer-side enrichment with large models (GLiNER)

## Layers

**CLI Layer:**
- Purpose: Parse arguments, resolve configuration, dispatch to command handlers
- Location: `src/main.rs`, `src/cli.rs`
- Contains: Command routing via clap, NER engine initialization
- Depends on: All command modules, config, scorer
- Used by: User entry point

**Command Layer:**
- Purpose: Implement distinct operational modes (produce/consume/watch/file)
- Location: `src/commands/{produce.rs, consume.rs, watch.rs}`
- Contains: Mode-specific logic, streaming coordination
- Depends on: Streaming, crawler, models, output, scorer
- Used by: CLI entry point

**Streaming Layer:**
- Purpose: Typed abstraction over Fluvio for JSON record flow
- Location: `src/streaming.rs`
- Contains: FluvioStream, TypedProducer, TypedConsumer, PipelineCoordinator
- Depends on: Fluvio SDK, serde
- Used by: All command handlers

**Data Model Layer:**
- Purpose: Define shapes for each pipeline stage
- Location: `src/models.rs`
- Contains: SourceItem, RawPage, TokenizedContent, PreprocessedBundle, ExtractedArticle, OutputRecord, ArticleMeta, NamedEntity, PipelineStats
- Depends on: serde
- Used by: All layers (ubiquitous)

**Business Logic Layer:**
- Purpose: Implement pipeline-specific behavior
- Location: `src/crawler.rs`, `src/scorer.rs`, `src/output.rs`, `src/config.rs`
- Contains: URL crawling/ranking, NER inference, formatting/saving, config loading
- Depends on: Models, external libraries (scraper, gliner, orp)
- Used by: Commands

**SmartModule Layer:**
- Purpose: WASM transformations in Fluvio cluster (not in main binary)
- Location: `smartmodules/{map-preprocess, map-extract, filter-relevance}`
- Contains: Stage 2 (tokenization), Stage 3 (extraction/scoring), Stage 4 (filtering)
- Depends on: Fluvio SmartModule SDK, serde
- Used by: Fluvio cluster (invoked by streaming layer)

## Data Flow

### Primary Request Path (Produce → Pipeline → Consume)

1. **CLI entry** (`src/main.rs:51`) parses Command::Produce
2. **Produce handler** (`src/commands/produce.rs:20`) creates PipelineCoordinator
3. **Crawler** (`src/crawler.rs:70`) fetches page → RawPage
4. **Producer sends** (`src/commands/produce.rs:71`, `src/streaming.rs:216`) a news `SourceItem` (`src/models.rs:117`, built by `SourceItem::news`) to the `filimon-raw-pages` topic. Social `SourceItem`s go to `filimon-social-items`, which operators create by hand; source plugins (Phase 3) will produce them
5. **Consumer handler** (`src/commands/consume.rs` `run`) connects and, through `collect_from_topics` / `open_incoming`, runs `FluvioStream::preflight` (`src/streaming.rs:155`) on the selected topics and SmartModules. A missing one stops the run with an error naming it (for example `fluvio topic create filimon-social-items`)
6. **News stream** — `FluvioStream::topic_stream` (`src/streaming.rs:129`) opens a `consumer_with_config` stream on `filimon-raw-pages` with the `news_chain()` invocations attached (`NEWS_CHAIN`: map-preprocess → map-extract → filter-relevance, `RetryMode::Disabled`). The SmartModules run in the cluster on that stream; filter-relevance drops articles below 0.05, and each kept `ExtractedArticle` becomes an `OutputRecord` (`OutputRecord::from_article`)
7. **Social stream** — `topic_stream` also opens a plain stream on `filimon-social-items` with no SmartModules; `decode_social` turns each `SourceItem` into an `OutputRecord` with `article: null`
8. **Merge** — `merge_incoming` (`futures_util::stream::select`) interleaves the two streams; `--source news|social` replaces the unselected branch with an empty stream
9. **Limit** — `collect_records` stops at `--limit` records across both kinds, or after 5 s with no new record (`IDLE_TIMEOUT`)
10. **Optional NER** (`enrich` in `src/commands/consume.rs`, `src/scorer.rs`) fills `entities` for news and social records when `--enrich-ner` is set (batch mode)
11. **Output** — `output::save_json` (`src/output.rs:15`) writes one JSON array to `--output` (default `output/wise_output.json`); `OutputRecord` has no html field. With `--limit 0` the merged stream is printed instead (`output::print_summary`)

### File Mode Path

1. **CLI entry** (`src/main.rs:51`) parses Command::File { paths }
2. **run_file** (`src/main.rs:320`) creates PipelineCoordinator
3. **Iterate paths** — read each HTML file from disk
4. **Create a news SourceItem** with a `file://` URL — send to producer (same as produce path step 4)
5. **Rest follows produce path** (steps 5-11)

### Watch Mode Path

1. **CLI entry** (`src/main.rs:51`) parses Command::Watch
2. **run_watch** (`src/main.rs:233`) — *currently stubbed; watchexec setup incomplete*
3. **File system events** detected by watchexec for `*.html` changes
4. **Would dispatch** to producer (follows produce path from step 4 onward)

**State Management:**
- **Topics:** Immutable event log. No state retained; each record independent.
- **Producer:** Mutex-wrapped to allow concurrent send/flush operations.
- **Consumer:** Offset-based (beginning/end); no state persisted between runs.
- **NER Engine:** Singleton loaded once at CLI startup; reused across all consume operations.

## Key Abstractions

**RawPage:**
- Purpose: Represents crawled or local HTML with its source URL
- Examples: `src/models.rs:24`, `smartmodules/map-preprocess/src/lib.rs:24`
- Pattern: Simple struct carrying URL + HTML; passed through producer

**SourceItem:**
- Purpose: Unified input envelope for news pages and social posts (`source_type`, `platform`, `id`, `url`, `title`, `text`, `author`, `published`, `raw`, news-only `html`); mirrors the WIT `item` record
- Examples: `src/models.rs:117`
- Pattern: News items on `filimon-raw-pages` (the chain reads only `url` and `html`); social items on `filimon-social-items`

**ExtractedArticle:**
- Purpose: Output of the news chain carrying title, body, metadata, relevance score, keywords
- Examples: `src/models.rs:62`
- Pattern: Serde-serialized JSON; delivered to `fili consume` on its `filimon-raw-pages` stream and mapped to `OutputRecord`

**OutputRecord:**
- Purpose: One record of `fili consume`'s JSON output, for news and social alike; `article` (ArticleMeta) is set for news and null for social; no html field
- Examples: `src/models.rs:263`
- Pattern: Written as one JSON array by `output::save_json`

**PipelineCoordinator:**
- Purpose: Factory/singleton for producers and consumers; ensures topics exist
- Examples: `src/streaming.rs:215`, `src/commands/produce.rs:22`
- Pattern: Arc-wrapped to allow sharing across async tasks

**TypedProducer/TypedConsumer:**
- Purpose: Wrapper around raw Fluvio types; serializes/deserializes JSON automatically
- Examples: `src/streaming.rs:105`, `src/streaming.rs:143`
- Pattern: Generic over data type; PhantomData for type safety

**NerEngine:**
- Purpose: Lazy-loaded GLiNER ONNX model for zero-shot entity extraction
- Examples: `src/scorer.rs:50`
- Pattern: Singleton; heavy initialization; reused per consume command

## Entry Points

**Binary Entry Point:**
- Location: `src/main.rs:51` (async main)
- Triggers: User invokes `fili <command>`
- Responsibilities: Parse CLI, load NER engine (if needed), dispatch to command handler

**Command::Produce:**
- Location: `src/commands/produce.rs:20`
- Triggers: `fili produce <urls> [--depth N]`
- Responsibilities: Crawl seed URLs, discover links, rank by priority, send raw pages to Fluvio

**Command::Consume:**
- Location: `src/commands/consume.rs` (`run`)
- Triggers: `fili consume [--from-beginning] [--limit N] [--source both|news|social] [--output PATH] [--enrich-ner]`
- Responsibilities: Preflight the selected topics and SmartModules, read `filimon-raw-pages` through the news chain (attached via `consumer_with_config`) and `filimon-social-items` with no SmartModules, merge both streams, optionally enrich with NER, and write one JSON array (batch mode) or print records (streaming mode)

**Command::Watch:**
- Location: `src/main.rs:74` (dispatches to stubbed `run_watch`)
- Triggers: `fili watch`
- Responsibilities: Monitor directories for `*.html` changes, send to producer

**Command::File:**
- Location: `src/main.rs:77` (dispatches to `run_file`)
- Triggers: `fili file <paths>...`
- Responsibilities: Read local HTML files, send each to producer

## Architectural Constraints

- **Threading:** Async/await with Tokio; all I/O operations are non-blocking. NER engine (scorer.rs) is sync and CPU-bound; blocks the consumer thread during batch mode.
- **Global state:** NerEngine is initialized once per process and reused; loaded from CLI flags or environment variables. Producer is Mutex-wrapped to allow concurrent sends within a command.
- **Circular imports:** None detected. Modules import models.rs ubiquitously (central data model), but models.rs has no internal imports of other modules.
- **WASM limitations:** SmartModules run in WASM sandbox with 128MB memory limit; NER engine (too large) runs on consumer machine instead.
- **Topic partition:** All topics use single partition (partition 0) for ordering guarantees.

## Anti-Patterns

### State in Records

**What happens:** Records are treated as immutable events passing through the pipeline; no state is mutated at pipeline stages.

**Why it's wrong:** This is actually correct for event-driven streaming.

**Pattern:** Each record is independent; pipeline stages compute outputs without side effects.

### NER in SmartModule

**What happens:** NER was moved from SmartModule to consumer because the GLiNER ONNX model is ~200-500MB.

**Why it's wrong:** SmartModules are WASM sandboxes with strict memory limits; large models cannot be deployed there.

**Do this instead:** Perform optional enrichment post-consume (`src/scorer.rs:83`) using `--enrich-ner` flag.

### Watch Mode Blocking

**What happens:** `watchexec` closure is sync; cannot await producer sends directly.

**Why it's wrong:** Mixing sync and async code without proper bridging causes compilation errors.

**Do this instead:** Use a channel to send filesystem events to an async task that calls the producer (`src/main.rs:233-317` is partially stubbed).

## Error Handling

**Strategy:** Propagate errors up with `miette::Result` for user-friendly diagnostics. Log warnings at streaming layer (deprecation notices).

**Patterns:**
- `Err(format!("..."))` in synchronous functions (crawler.rs)
- `?` operator to propagate through `miette::Result<T>` chains
- `map_err(|e| miette::miette!(...))` for Fluvio SDK errors
- Continue-on-error in loops (produce handler skips failed pages, consumer skips deserialization failures)

## Cross-Cutting Concerns

**Logging:** tracing/tracing-subscriber configured by env (RUST_LOG). Used in streaming layer for connection/topic creation; consumer errors logged but don't stop stream.

**Validation:** Link extraction checks URL host (same-domain only), deduplicates, and filters by signals (article/noise keywords in `src/crawler.rs:12-54`). NER labels hardcoded in `src/scorer.rs:26-35`.

**Configuration:** CLI flags override environment variables (clap feature). Watch mode config loaded from JSON file (`src/config.rs`). Defaults: tokenizer_path, model_path, threshold (0.05), config file path.

---

*Architecture analysis: 2026-11-03*
