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
│  (crawler)     │  (subscriber)    │ (filesystem) │  (local)    │
└────────────────┴──────────────────┴──────────────┴─────────────┘
         │                │                 │             │
         ▼                ▼                 ▼             ▼
┌─────────────────────────────────────────────────────────────────┐
│              Streaming Layer (streaming.rs)                      │
│   PipelineCoordinator → TypedProducer/TypedConsumer             │
│          Fluvio Topics: raw-pages, articles                     │
└─────────────────────────────────────────────────────────────────┘
         │                                                │
         ▼                                                ▼
┌──────────────────────────────────────────────────────────────────┐
│          Fluvio Streaming Pipeline (SmartModules)               │
│                                                                  │
│  raw-pages ──[map-preprocess]──→ preprocessed ──[map-extract]  │
│                (tokenize)                      (extract & score)│
│                                                      │           │
│                                                      ▼           │
│                              extracted ──[filter-relevance]──→  │
│                                          (threshold: 0.05)       │
│                                                      │           │
│                                                      ▼           │
│                                               articles topic     │
└──────────────────────────────────────────────────────────────────┘
         │
         ▼
┌──────────────────────────────────────────────────────────────────┐
│         Consumer & Enrichment (commands/consume.rs)             │
│                                                                  │
│  TypedConsumer ──→ batch/stream processing ──→ NER (GLiNER)    │
│  (models.rs)       (commands/consume.rs)        (scorer.rs)     │
│                                                  │              │
│                                                  ▼              │
│                                            output.rs (JSON)     │
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
- Contains: RawPage, TokenizedContent, PreprocessedBundle, ExtractedArticle, NamedEntity, PipelineStats
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
4. **Producer sends** (`src/streaming.rs:111`) RawPage to `filimon-raw-pages` topic
5. **SmartModule 1** (map-preprocess) tokenizes → TokenizedContent
6. **SmartModule 2** (map-extract) extracts metadata, scores relevance → ExtractedArticle
7. **SmartModule 3** (filter-relevance) filters by threshold → `filimon-articles` topic
8. **Consumer handler** (`src/commands/consume.rs:20`) creates TypedConsumer
9. **Batch consume** (`src/streaming.rs:185`) fetches articles with timeout
10. **Optional NER** (`src/scorer.rs:83`) enriches entities if `--enrich_ner` flag set
11. **Output** (`src/output.rs:23`) prints formatted summaries to stdout

### File Mode Path

1. **CLI entry** (`src/main.rs:51`) parses Command::File { paths }
2. **run_file** (`src/main.rs:320`) creates PipelineCoordinator
3. **Iterate paths** — read each HTML file from disk
4. **Create RawPage** with `file://` URL — send to producer (same as produce path step 4)
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

**ExtractedArticle:**
- Purpose: Final structured output carrying title, body, metadata, relevance score, keywords
- Examples: `src/models.rs:59`
- Pattern: Serde-serialized JSON; consumed by final topic subscriber

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
- Location: `src/commands/consume.rs:20`
- Triggers: `fili consume [--from-beginning] [--limit N] [--enrich-ner]`
- Responsibilities: Subscribe to articles topic, optionally batch or stream, optionally enrich with NER

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
