---
last_mapped_commit: 5122e6b9c4aa51ca49831917f1a1035fdc543870
last_mapped_at: 2026-11-03
---
# Coding Conventions

**Analysis Date:** 2026-11-03

## Naming Patterns

**Files:**
- `snake_case.rs` for module files
- `mod.rs` for module root files that re-export submodules
- Examples: `src/models.rs`, `src/streaming.rs`, `src/commands/produce.rs`

**Functions:**
- `snake_case` for all function names (public and private)
- Async functions use `async fn` with no special suffix
- Examples: `run_produce`, `fetch_page`, `extract_links`

**Variables:**
- `snake_case` for local variables, function parameters, and struct fields
- `mut` when mutability is required
- Examples: `total_produced`, `targets`, `seed_url`

**Types:**
- `PascalCase` for struct names: `CrawlTarget`, `RawPage`, `ExtractedArticle`
- `PascalCase` for enum names: `Command`
- `UPPER_CASE` for constants: `LINKS_PER_SEED`, `TOPIC_RAW_PAGES`, `NER_LABELS`
- Single-letter generics for type parameters: `T`, `F`, `L`, `R`

**Traits:**
- `PascalCase` with `Ext` suffix for extension traits: `ValidateWithExt`, `PartitionMapExt`

## Code Style

**Formatting:**
- Rust 2024 edition (specified in `Cargo.toml`)
- No explicit `.rustfmt.toml` in project root; uses Rust defaults
- 4-space indentation (Rust standard)
- Line length follows Rust conventions (roughly 100 characters)

**Linting:**
- **clippy:** All warnings enabled via `[lints.clippy]`
  - `all = "warn"` enables all clippy lints
  - `pedantic = "warn"` enables pedantic lints
- **Rust linter:** `unsafe_code = "warn"` warns on unsafe blocks
- No unsafe code expected in main source

**Organization:**
- Module-level documentation at the top of each file with ASCII decorators
- Example: `// ─────────────────────────────────────────────────────────────────────────────`
- Section headers within modules use comment lines and dashes: `// ── Section Name ───...`

## Import Organization

**Order:**
1. Rust standard library imports (`std::...`)
2. External crate imports (third-party dependencies)
3. Internal module imports (`crate::...`)

**Example from `src/streaming.rs`:**
```rust
use fluvio::{Fluvio, Offset, RecordKey, TopicProducerConfigBuilder};
use fluvio::metadata::topic::TopicSpec;
use fluvio::spu::SpuSocketPool;
use futures_util::StreamExt;
use miette::Result;
use serde::{de::DeserializeOwned, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};
```

**Path Aliases:**
- No explicit path aliases in scope
- Root crate accessed via `crate::` prefix: `crate::models::RawPage`

## Error Handling

**Patterns:**
- Primary error type: `Result<T>` = `miette::Result<T>` (in async contexts)
- Fallback: `Result<T, String>` (in blocking code like crawlers)
- Fallback: `Result<T, Box<dyn std::error::Error>>` (for complex error handling)

**Error conversion:**
- Use `map_err()` with contextual messages
- Use `miette::miette!()` macro for formatted errors
- Example from `src/streaming.rs:60-61`:
```rust
.map_err(|e| miette::miette!("Failed to build producer config: {}", e))?
```

**Error propagation:**
- Use `?` operator for early return
- Use `match` expressions for conditional handling
- Log errors with `error!()` macro before continuing
- Example from `src/commands/produce.rs:37-43`:
```rust
let seed_page = match crawler::fetch_page(&seed) {
    Ok(p) => p,
    Err(e) => {
        eprintln!("  ✗ {e}");
        continue;
    }
};
```

**Panic handling:**
- Tests use `#[should_panic]` for expected panics (see `src/args.rs:122-141`)
- Avoid unwrap() in production code; use `?` or explicit error handling

## Logging

**Framework:** `tracing` crate

**Macros:**
- `info!()` - informational messages
- `warn!()` - warnings for non-critical issues
- `error!()` - error logging with context

**Patterns:**
- Log at connection/initialization: `info!("Connecting to Fluvio cluster...")`
- Log successful operations: `info!("Connected to Fluvio cluster")`
- Log non-fatal errors: `warn!("Topic creation note for {}: {}", topic, e)`
- Log handler errors: `error!("Handler error: {}", e)`
- Example from `src/streaming.rs:41-44`:
```rust
info!("Connecting to Fluvio cluster...");
let client = Fluvio::connect().await
    .map_err(|e| miette::miette!("Failed to connect to Fluvio: {}", e))?;
info!("Connected to Fluvio cluster");
```

**stderr output:**
- Use `eprintln!()` for errors to stderr
- Use `println!()` for user-facing output (progress, summaries)
- Examples: `eprintln!("  ✗ {e}")`, `println!("[Produce] Connecting to Fluvio cluster...")`

## Comments

**When to Comment:**
- Module-level block comments at file top explaining purpose
- Function/struct documentation for public APIs
- Inline comments for non-obvious logic or algorithm explanations
- Section dividers for logical grouping

**JSDoc/TSDoc (Rust doc comments):**
- Use `///` for public items (functions, structs, modules)
- Include `# Arguments` section for function parameters
- Include `# Errors` section for functions that return `Result<T>`
- Include `# Example` section for complex functions (optional)
- Example from `src/scorer.rs:54-77`:
```rust
/// Load a GLiNER span-mode model from ONNX files on disk.
///
/// # Arguments
/// * `tokenizer_path` - path to `tokenizer.json`
/// * `model_path` - path to `onnx/model.onnx`
///
/// # Errors
/// Returns an error if either file is missing or if the ONNX runtime
/// fails to load the model graph.
pub fn new(
    tokenizer_path: impl AsRef<Path>,
    model_path: impl AsRef<Path>,
) -> Result<Self, Box<dyn std::error::Error>> { ... }
```

**ASCII Decorators:**
- Section dividers: `// ─────────────────────────────────────────────────────────────────────────────`
- Subsection headers: `// ── Subsection Name ──────────────────────────────────────────────────────`
- Top-level boxes: Use `╔═══╗`, `║`, `╚═══╝` for important headers

## Function Design

**Size:**
- Prefer small, focused functions (< 50 lines for most functions)
- Async functions may be longer when coordinating multiple operations
- Example: `src/commands/produce.rs:run()` is 70 lines and handles a complete workflow

**Parameters:**
- Use semantic, descriptive names
- Avoid boolean blindness: use enums or struct fields instead of multiple bools when possible
- Use `impl Trait` for generic parameters when appropriate
- Example from `src/scorer.rs:64-67`:
```rust
pub fn new(
    tokenizer_path: impl AsRef<Path>,
    model_path: impl AsRef<Path>,
) -> Result<Self, Box<dyn std::error::Error>>
```

**Return Values:**
- Async functions return `miette::Result<T>` or `Result<T, ErrorType>`
- Blocking functions may return `Result<T, String>` for simplicity
- Functions with no meaningful return use `-> Result<()>`
- Example: `src/output.rs:14` returns `std::io::Result<()>`

## Module Design

**Exports:**
- Use `pub use` to re-export functions from submodules
- Example from `src/commands/mod.rs:5-7`:
```rust
pub use consume::run as run_consume;
pub use produce::run as run_produce;
pub use watch::run as run_watch;
```

**Barrel Files:**
- `mod.rs` files declare submodules and re-export key items
- Keeps public API clean at module level
- Example: `src/commands/mod.rs` exports run functions with semantic names

**Struct Derives:**
- Standard derives: `#[derive(Debug, Clone)]`
- Serialization: `#[derive(Serialize, Deserialize)]` for data flowing through Fluvio
- Example from `src/models.rs:23-27`:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawPage {
    pub url: String,
    pub html: String,
}
```

**Implementation Blocks:**
- impl blocks follow struct definition immediately
- Methods grouped logically (constructors, getters, business logic)
- No trait impl blocks mixed with inherent impl

---

*Convention analysis: 2026-11-03*
