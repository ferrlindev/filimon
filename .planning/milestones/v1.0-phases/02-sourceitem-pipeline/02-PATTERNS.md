# Phase 2: SourceItem Pipeline - Pattern Map

**Mapped:** 2026-11-04
**Files analyzed:** 12 (new or modified)
**Analogs found:** 11 / 12 (all analogs are git-tracked source; none are mirrors)

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match Quality |
|-------------------|------|-----------|----------------|---------------|
| `src/models.rs` (add SourceType, SourceItem, OutputRecord, ArticleMeta, date/platform helpers, tests) | model | transform | `src/models.rs` (RawPage / ExtractedArticle, lines 19-70) | exact |
| `src/streaming.rs` (add TOPIC_SOCIAL_ITEMS, remove TOPIC_ARTICLES, NEWS_CHAIN, news_chain(), client accessor / chain consumer, tests) | service | streaming | `src/streaming.rs` (FluvioStream::producer/consumer/ensure_topics, lines 26-100) | exact |
| `src/commands/consume.rs` (rewrite: preflight, two streams, select merge, enrichment, JSON output, tests) | controller (command) | streaming + batch | `src/commands/consume.rs` (lines 1-89) + `src/streaming.rs::consume_batch` (lines 184-209) | exact |
| `src/commands/produce.rs` (send SourceItem::news) | controller (command) | streaming (produce) | itself, lines 62-74 | exact |
| `src/main.rs` (`run_file` sends SourceItem::news; Consume arm passes source/output) | controller (dispatch) | request-response | itself, lines 58-81 and 321-358 | exact |
| `src/cli.rs` (Consume `--source`, `--output` flags + tests) | config | request-response | itself, Consume variant lines 69-82; tests lines 94-168 | exact |
| `src/output.rs` (generalise save_json / print_summary to OutputRecord) | utility | file-I/O | itself, lines 13-68 | exact |
| `Cargo.toml` (add chrono 0.4, no default features, std+clock) | config | — | existing `[dependencies]` | exact |
| `examples/fixtures/news-article.html` (new fixture) | test fixture | — | README.md:168 inline HTML sample | partial |
| `README.md` (topic steps, mermaid diagram) | docs | — | README.md:22-30, 120-126 | exact |
| `.planning/codebase/{ARCHITECTURE,INTEGRATIONS,STRUCTURE}.md`, `docker-compose.yml:131-132` comment | docs | — | n/a (text edits) | n/a |
| Cluster `#[ignore]` integration tests (in-crate) | test | streaming | none in repo | none |

## Pattern Assignments

### `src/models.rs` (model, transform)

**Analog:** `src/models.rs`

**Imports + struct convention** (lines 8-9, 23-27):
```rust
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Raw HTML fetched from a single URL.
/// ... Streamed to the `filimon-raw-pages` Fluvio topic.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawPage {
    pub url: String,
    pub html: String,
}
```
- Every record type: doc comment naming the topic it travels on, `#[derive(Debug, Clone, Serialize, Deserialize)]`, all-`pub` fields.
- Keep `RawPage` (still used by `crawler::fetch_page`, `extract_links`, `watch.rs`). Add `SourceItem` per RESEARCH Pattern 1 and `OutputRecord`/`ArticleMeta` per Pattern 5.
- `ExtractedArticle` (lines 55-70) is the input of `OutputRecord::from_article`; fix its doc comment at line 57 (mentions `filimon-articles`, D-03).
- `NamedEntity` (lines 48-53) is reused for `OutputRecord.entities`.
- `SourceItem::social` gets `#[cfg_attr(not(test), allow(dead_code, reason = "produced by the plugin host in Phase 3"))]`, copying the style from `src/plugins/mod.rs` lines 10-16:
```rust
#![cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "consumed by the plugin host and CLI in Phases 3-4"
    )
)]
```
- models.rs has no tests today. Add `#[cfg(test)] mod tests { use super::*; ... }` in the same style as `src/cli.rs` lines 94-100. The WIT-alignment test can read `wit/types.wit` with `include_str!("../wit/types.wit")` (no current `include_str!` usage in src; this is new).

---

### `src/streaming.rs` (service, streaming)

**Analog:** `src/streaming.rs`

**Imports** (lines 16-24):
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
Add: `fluvio::consumer::{ConsumerConfigExt, RetryMode}`, `fluvio::{SmartModuleContextData, SmartModuleExtraParams, SmartModuleInvocation, SmartModuleInvocationWasm, SmartModuleKind}`, `fluvio::metadata::smartmodule::SmartModuleSpec`, `std::collections::BTreeMap`.

**Topic constants** (lines 26-29): replace `TOPIC_ARTICLES` with `pub const TOPIC_SOCIAL_ITEMS: &str = "filimon-social-items";`. Header comment lines 6-14 must drop the `articles` line.

**Method pattern with miette mapping** (lines 48-63) — copy for a new `consumer_with_chain` / `client()` accessor:
```rust
pub async fn producer(&self, topic: &str) -> Result<TypedProducer> {
    let config = TopicProducerConfigBuilder::default()
        .batch_size(16384)
        .linger(std::time::Duration::from_millis(100))
        .build()
        .map_err(|e| miette::miette!("Failed to build producer config: {}", e))?;
    let producer = self
        .client
        .topic_producer_with_config(topic, config)
        .await
        .map_err(|e| miette::miette!("Failed to create producer: {}", e))?;
    Ok(TypedProducer { producer })
}
```
The new consumer builder follows the same shape with `ConsumerConfigExt::builder()...build()` then `self.client.consumer_with_config(config)` (RESEARCH Pattern 2).

**Admin usage** (lines 85-99) — `ensure_topics()` stays but with `let topics = [TOPIC_RAW_PAGES];` only (D-05: social not auto-created). Preflight in consume uses the same `self.client.admin().await` handle with `admin.list::<TopicSpec, String>(..)` / `list::<SmartModuleSpec, String>(..)`:
```rust
pub async fn ensure_topics(&self) -> Result<()> {
    let admin = self.client.admin().await;
    let topics = [TOPIC_RAW_PAGES, TOPIC_ARTICLES];
    for topic in topics { ... admin.create(topic.to_string(), false, topic_spec).await ... }
    Ok(())
}
```

**Idle-timeout batch loop** (lines 185-209) — reference for the merged loop in consume (5 s timeout, stop on limit):
```rust
while results.len() < max_count {
    match tokio::time::timeout(std::time::Duration::from_secs(5), stream.next()).await {
        Ok(Some(Ok(record))) => { ... serde_json::from_slice::<T>(value) ... }
        Ok(Some(Err(e))) => { error!("Stream error: {}", e); break; }
        Ok(None) => break,
        Err(_) => break,
    }
}
```
Do NOT copy the silent `if let Ok(..)` drop or `error!` (no subscriber installed; use `eprintln!`). `TypedConsumer` itself is left untouched (existing gap, out of scope).

**Test to add:** `news_chain_matches_transforms_yaml` — read `transforms.yaml` (`include_str!("../transforms.yaml")`), compare `uses:` lines to `NEWS_CHAIN` order.

---

### `src/commands/consume.rs` (command, streaming + batch)

**Analog:** current `src/commands/consume.rs`

**Header + imports** (lines 1-12):
```rust
// ─────────────────────────────────────────────────────────────────────────────
// commands/consume.rs - Consume mode
//
// Consumes processed articles from Fluvio with optional NER enrichment.
// ─────────────────────────────────────────────────────────────────────────────

use fluvio::Offset;

use crate::models;
use crate::output;
use crate::scorer::NerEngine;
use crate::streaming::{FluvioStream, TypedConsumer, TOPIC_ARTICLES};
```

**Entry signature + offset selection** (lines 20-37) — keep the shape, add `source` and `output` params:
```rust
pub async fn run(from_beginning: bool, limit: usize, ner: Option<&NerEngine>) -> miette::Result<()> {
    println!("[Consume] Connecting to Fluvio cluster...");
    let stream = FluvioStream::connect().await?;
    let offset = if from_beginning { Offset::beginning() } else { Offset::end() };
    ...
    if limit > 0 { consume_batch(..).await?; } else { consume_stream(..).await?; }
```

**NER enrichment pattern** (lines 57-65) — becomes the detector-injected enrich step (RESEARCH Pattern 6):
```rust
for mut article in articles {
    if let Some(engine) = ner {
        if let Ok(entities) = engine.detect(&article.body_preview) {
            article.named_entities = entities;
        }
    }
    output::print_summary(&article);
}
output::print_run_summary(limit, TOPIC_ARTICLES);
```
`NerEngine::detect(&self, text: &str) -> Result<Vec<NamedEntity>>` (`src/scorer.rs:83`) is unchanged.

**Streaming mode** (lines 72-89) keeps print-only, no NER (existing gap). With two streams it should iterate the merged stream rather than `TypedConsumer::process`.

**Tests:** in-file `#[cfg(test)] mod tests` with `#[tokio::test]` + `futures_util::stream::iter` fakes over `Incoming`; follow `src/cli.rs` lines 94-100 module layout.

---

### `src/commands/produce.rs` (command, produce)

**Analog:** itself, lines 62-74:
```rust
let page = match crawler::fetch_page(target) { Ok(p) => p, Err(e) => { println!("    ✗ {e}"); continue; } };
// Produce to Fluvio
let locked = producer.lock().await;
locked.send(&target.url, &page).await?;
println!("    → Produced to {}", TOPIC_RAW_PAGES);
```
Change only the send: `let item = SourceItem::news(page.url.clone(), page.html); locked.send(&item.url, &item).await?;`. Import `crate::models::{CrawlTarget, SourceItem}` (line 9 pattern).

---

### `src/main.rs` (dispatch)

**Dispatch arm** (lines 62-73):
```rust
Command::Consume { from_beginning, limit, enrich_ner } => {
    let ner = if enrich_ner { load_ner_engine(&cli.tokenizer, &cli.model) } else { None };
    run_consume(from_beginning, limit, ner.as_ref()).await?;
}
```
Add `source`, `ref output` destructuring and pass through.

**`run_file`** (lines 340-349): replace `RawPage { url: url.clone(), html }` with `SourceItem::news(url.clone(), html)`; update `use models::RawPage;` (line 32) to `SourceItem`. `file://` URL ⇒ platform `"local"` (Pitfall 5). Leave the commented-out blocks (lines ~104-300) alone; any `TOPIC_ARTICLES` grep gate must exclude them. Header comment lines 12-13 describe the articles topic — update.

---

### `src/cli.rs` (config)

**Analog:** Consume variant (lines 69-82):
```rust
Consume {
    /// Consume from the beginning of the topic.
    #[arg(long)]
    from_beginning: bool,
    /// Maximum number of articles to consume ( 0 = unlimited).
    #[arg(long, default_value_t = 0)]
    limit: usize,
    /// Apply NER enrichment to consumed articles
    #[arg(long)]
    enrich_ner: bool,
},
```
Add `#[arg(long, value_enum, default_value_t = SourceFilter::Both)] source: SourceFilter` (derive `clap::ValueEnum`) and `#[arg(long, default_value = "output/wise_output.json")] output: PathBuf`. Test pattern (lines 126-131):
```rust
let cli = Cli::try_parse_from(["fili", "watch", "--registry", "conf/filimon.toml"]).expect("parses");
assert_eq!(cli.registry, PathBuf::from("conf/filimon.toml"));
```
Leave `--threshold` (lines 29-31) untouched.

---

### `src/output.rs` (utility, file-I/O)

**Analog:** itself, lines 9-20 and 23-58:
```rust
use crate::models::ExtractedArticle;
use std::fs::File;
use std::io::Write;

pub fn save_json(articles: &[ExtractedArticle], path: &str) -> std::io::Result<()> {
    let json = serde_json::to_string_pretty(articles)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    let mut f = File::create(path)?;
    f.write_all(json.as_bytes())?;
    Ok(())
}
```
Generalise to `&[OutputRecord]` and `&Path`, add `std::fs::create_dir_all(parent)`. `print_summary` keeps its `bar` / labelled-line layout; take `&OutputRecord`, print `source_type`/`platform`, show article block only when `article.is_some()`, and strip control chars from title/text (security note). `print_run_summary(accepted, output_path)` (lines 61-68) already fits.

---

### `Cargo.toml`
Add `chrono = { version = "0.4", default-features = false, features = ["std", "clock"] }` to `[dependencies]`; respect existing `[lints.clippy] all/pedantic = "warn"`.

### `README.md`
Lines 22-30 mermaid: replace `SM3 -->|filtered| T2[filimon-articles]` with flow to `fili consume`, add social-items node. Lines 120-126: replace `fluvio topic create filimon-articles` with `fluvio topic create filimon-social-items`; optional `fluvio topic delete filimon-articles` cleanup. Lines 168-171 raw-pages sample: may switch to a SourceItem-shaped sample.

## Shared Patterns

### Error handling
**Source:** `src/streaming.rs` lines 42-43, 54, 60
```rust
.map_err(|e| miette::miette!("Failed to connect to Fluvio: {}", e))?;
```
Apply to all new Fluvio/admin calls. Error text must name the topic/SmartModule (D-05).

### User-visible diagnostics
**Source:** `src/commands/produce.rs` line 40 (`eprintln!("  ✗ {e}");`), `src/main.rs` line 97.
Use `eprintln!` for stream/deserialize failures; never `tracing::error!` (no subscriber). Never print full record values (Pitfall 3).

### Dead-code allowance for Phase-3-consumed items
**Source:** `src/plugins/mod.rs` lines 10-16 (excerpt above).

### Unit tests in-crate
**Source:** `src/cli.rs` lines 94-168, `src/plugins/config.rs` line 620+ (`mod tests { use super::*; use tempfile::TempDir; ... }`). Bin-only crate: no `tests/` dir. `tempfile` already a dev-dep (used in cli.rs tests) — use it for `save_json` path tests.

## No Analog Found

| File | Role | Data Flow | Reason |
|------|------|-----------|--------|
| Cluster `#[ignore]` tests (in consume.rs/streaming.rs) | test | streaming | No cluster tests exist; use RESEARCH Wave 0 guidance (scratch topics via `admin.create`/`admin.delete::<TopicSpec>`, topics passed as params) |
| SmartModule chain consumer (`ConsumerConfigExt` + `SmartModuleInvocation`) | service | streaming | No existing use of `consumer_with_config`; copy RESEARCH Pattern 2 (compile-verified) |
| `futures_util::stream::select` merge | — | streaming | Not used in repo; RESEARCH Pattern 3 |
| chrono date helpers | utility | transform | chrono not used directly; RESEARCH Code Examples |

## Metadata

**Analog search scope:** `src/`, `src/commands/`, `src/plugins/`, `README.md`, `Cargo.toml`
**Files scanned:** 12
**Pattern extraction date:** 2026-11-04
