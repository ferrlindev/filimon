---
last_mapped_commit: 5122e6b9c4aa51ca49831917f1a1035fdc543870
last_mapped_at: 2026-11-03
---
# Codebase Structure

**Analysis Date:** 2026-11-03

## Directory Layout

```
filimon/
├── src/                          # Main application source
│   ├── main.rs                   # CLI entry point and command dispatch
│   ├── cli.rs                    # Clap CLI struct definitions
│   ├── args.rs                   # Additional argument types
│   ├── commands/                 # Command handlers (produce, consume, watch)
│   │   ├── mod.rs                # Re-exports for command module
│   │   ├── produce.rs            # Crawl and produce raw pages
│   │   ├── consume.rs            # Subscribe to articles and format output
│   │   └── watch.rs              # Watch directories for HTML files
│   ├── streaming.rs              # Fluvio abstraction layer
│   ├── models.rs                 # Data structures (RawPage, ExtractedArticle, etc.)
│   ├── crawler.rs                # Web crawling and link extraction
│   ├── scorer.rs                 # GLiNER-based NER
│   ├── output.rs                 # Formatting and JSON output
│   ├── config.rs                 # Load watch directories from JSON
│   └── check.rs                  # Validation helpers
│
├── smartmodules/                 # WASM-based Fluvio transformations
│   ├── map-preprocess/           # Stage 2: tokenization
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   ├── map-extract/              # Stage 3: extraction and relevance scoring
│   │   ├── Cargo.toml
│   │   └── src/lib.rs
│   └── filter-relevance/         # Stage 4: relevance threshold filter
│       ├── Cargo.toml
│       └── src/lib.rs
│
├── fluvio/                       # Fluvio ecosystem submodule (vendored)
│   ├── .github/
│   ├── crates/
│   └── ...
│
├── models/                       # GLiNER model files (local, not in repo)
│   └── gliner_small-v2.1/
│       ├── tokenizer.json
│       └── onnx/model.onnx
│
├── output/                       # Output directory for JSON results
│
├── Cargo.toml                    # Workspace manifest
├── Cargo.lock                    # Dependency lock file
│
├── docker-compose.yml            # Fluvio cluster setup
├── Dockerfile                    # Containerized build
│
├── README.md                     # Project documentation
│
├── transforms.yaml               # Fluvio pipeline config (all stages)
├── transforms-extract-only.yaml  # Alternative: skip preprocessing
├── transforms-filter-only.yaml   # Alternative: final filtering only
│
├── test_config_panic.json        # Test fixture for config loading
│
├── .gitignore                    # Git ignore patterns
├── config.json                   # Watch mode configuration (local)
│
└── .planning/codebase/           # Generated codebase analysis
    ├── ARCHITECTURE.md           # Architecture and data flow
    └── STRUCTURE.md              # This file
```

## Directory Purposes

**`src/`:**
- Purpose: Main Rust binary source code
- Contains: CLI logic, streaming abstraction, data models, transformations
- Key files: `main.rs` (entry), `streaming.rs` (Fluvio wrapper), `models.rs` (data shapes)

**`src/commands/`:**
- Purpose: Distinct operational modes
- Contains: produce.rs, consume.rs, watch.rs (and produce/consume/file/watch dispatchers)
- Key files: `produce.rs` (crawl + send), `consume.rs` (subscribe + enrich), `watch.rs` (stubbed)

**`smartmodules/`:**
- Purpose: WASM transformations deployed to Fluvio cluster
- Contains: Three independent Cargo projects for map-preprocess, map-extract, filter-relevance
- Key files: Each has `src/lib.rs` with `#[smartmodule(...)]` macro
- Notes: Compiled to WASM and registered with Fluvio before pipeline runs

**`fluvio/`:**
- Purpose: Fluvio community source (submodule)
- Contains: Full Fluvio SDK and CLI source
- Used for: Building from source; normally not needed if Fluvio binaries are pre-installed

**`models/`:**
- Purpose: Local cache of GLiNER NER model files
- Contains: `gliner_small-v2.1/` with tokenizer.json and ONNX model
- Notes: Downloaded separately; path configured via `--tokenizer` and `--model` CLI flags

**`output/`:**
- Purpose: Directory for JSON output files
- Contains: Results from consume operations (wise_output.json)
- Notes: User-writeable; created on first run

## Key File Locations

**Entry Points:**
- `src/main.rs`: Async main entry point; parses CLI, loads NER engine, dispatches to commands
- `src/cli.rs`: Clap struct definitions (Cli, Command enum with variants)

**Configuration:**
- `Cargo.toml`: Workspace manifest; defines dependencies and SmartModule members
- `docker-compose.yml`: Fluvio cluster and dependencies (Zookeeper, SPU)
- `transforms.yaml`: Fluvio pipeline; maps topics to SmartModules

**Core Logic:**
- `src/streaming.rs`: Fluvio client wrapper; producers/consumers; topic management
- `src/models.rs`: RawPage → TokenizedContent → PreprocessedBundle → ExtractedArticle → NamedEntity
- `src/crawler.rs`: URL crawling, link extraction, priority scoring
- `src/scorer.rs`: GLiNER NER engine initialization and inference

**Commands:**
- `src/commands/produce.rs`: Crawl, discover links, send RawPage to topic
- `src/commands/consume.rs`: Subscribe to articles topic, batch/stream processing, optional NER enrichment
- `src/commands/watch.rs`: Stubbed; should watch directories for HTML changes

**Testing & Configuration:**
- `test_config_panic.json`: Test fixture for JSON deserialization errors
- `config.json`: Watch mode config (JSON list of directories)

## Naming Conventions

**Files:**
- Rust source: `snake_case.rs` (e.g., `map_extract.rs`, `run_consume.rs`)
- WASM modules: `kebab-case` directories (e.g., `map-extract/`, `filter-relevance/`)
- Config: JSON files in project root (e.g., `config.json`, `transforms.yaml`)

**Directories:**
- Command modules: `commands/` with `mod.rs` centralizing re-exports
- Fluvio artifacts: `smartmodules/` with each module as a sub-directory
- Output: `output/` (user-writeable, typically git-ignored)

**Types:**
- Structs: `PascalCase` (e.g., RawPage, ExtractedArticle, PipelineCoordinator)
- Enums: `PascalCase` variants (e.g., Command::Produce, Command::Consume)
- Functions: `snake_case` (e.g., `run_produce`, `fetch_page`, `score_link_priority`)
- Constants: `SCREAMING_SNAKE_CASE` (e.g., TOPIC_RAW_PAGES, ARTICLE_SIGNALS)

**Modules:**
- Library modules: `mod.rs` in directory; re-exports in parent
- Private submodules: prefix with `_` if needed; use `pub use` to expose publicly

## Where to Add New Code

**New Feature (e.g., database output):**
- Primary code: `src/output.rs` (add new save function for database)
- Models: `src/models.rs` (add DatabaseConfig struct if needed)
- Commands: `src/commands/consume.rs` (add new flag to command struct in cli.rs)
- Tests: Co-located in same files with `#[cfg(test)]` modules

**New SmartModule (e.g., sentiment analysis):**
- Create: `smartmodules/sentiment-analysis/` directory
- Structure: Copy `smartmodules/map-extract/` layout (Cargo.toml + src/lib.rs)
- Registration: Update `transforms.yaml` to add topic and SmartModule binding
- Input/output: Define new serde structs in module's `lib.rs`; use in main app via `models.rs`

**New CLI Command (e.g., export):**
- File: Create `src/commands/export.rs`
- Struct: Define variant in `Command` enum in `src/cli.rs`
- Handler: Implement `pub async fn run(...)` in export.rs
- Dispatch: Add `Command::Export { ... }` pattern match in `src/main.rs:58`
- Module: Add `pub mod export;` and `pub use export::run as run_export;` to `src/commands/mod.rs`

**Utilities & Helpers:**
- Shared helpers: `src/check.rs` or new `src/utils.rs`
- Validation: Add to `src/check.rs` (ItemError struct already defined)
- Extensions: Trait impls in the module where they're used; avoid `src/ext.rs` unless many

## Special Directories

**`target/`:**
- Purpose: Rust build artifacts (compiled binaries, dependencies)
- Generated: Yes
- Committed: No (in .gitignore)

**`.gsd-path/`:**
- Purpose: GSD Path pipeline state (not product code)
- Generated: Yes (by orchestrator)
- Committed: No

**`node_modules/` (if added):**
- Purpose: JavaScript dependencies (not present; Rust-only project)
- Generated: Yes (if npm used)
- Committed: No

**`smartmodules/target/`:**
- Purpose: WASM build artifacts
- Generated: Yes
- Committed: No

---

*Structure analysis: 2026-11-03*
