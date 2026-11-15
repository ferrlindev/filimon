# Phase 2: SourceItem Pipeline - Research

**Researched:** 2026-11-04
**Domain:** Rust / Fluvio 0.50 client (consumer-side SmartModule chains, multi-stream merge), serde record contracts, consumer-side NER output
**Confidence:** HIGH (core API and D-06 contract verified by compile + live probes against the local Fluvio 0.18.1 cluster)

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

#### Topic routing
- **D-01:** Social `SourceItem`s go to a **new dedicated topic, `filimon-social-items`**, and never pass through the SmartModule chain. The three SmartModules stay **untouched** (no source and no rebuild changes). Two things follow: the news path cannot regress (SC2), and social records cannot be filtered (SC3). — **Reversibility:** costly — topic names are wiring that Phase 3 (host produce) and Phase 8 (integration tests) will depend on.
- **D-02:** `fili consume` **applies the SmartModule chain itself** by reading `filimon-raw-pages` with the `map-preprocess → map-extract → filter-relevance` transforms attached through the Fluvio 0.50 consumer API. This is the same chain `transforms.yaml` declares. There is no intermediate articles topic and no extra process. Research must confirm the exact Fluvio 0.50 consumer API for attaching a chain of hub/locally loaded SmartModules (`filimon/map-preprocess@0.1.0` etc.).
- **D-03:** **Remove the `filimon-articles` topic.** That means deleting `TOPIC_ARTICLES`, dropping it from `ensure_topics()`, and updating README and `.planning/codebase/` references (ARCHITECTURE, INTEGRATIONS, STRUCTURE) so nothing documents a dead topic.
- **D-04:** By default, `fili consume` reads **both sources concurrently** (news through the chain on raw-pages, plus social-items) into one output stream. `--limit N` caps the **total** number of records across both. An optional narrowing flag (e.g. `--source news|social`, defaulting to both) may be added. SC4 must pass with a plain `fili consume --enrich-ner --limit N` run.
- **D-05:** **Operators create `filimon-social-items` by hand**, documented in the README next to the existing `fluvio topic create` steps. `ensure_topics()` does not create it. If the topic is missing, the consumer must fail with an error that names it, not hang silently.

#### News envelope shape
- **D-06:** The crawler (`fili produce`) and `fili file` produce a **`SourceItem` to `filimon-raw-pages`** that carries an extra **`html: Option<String>`** field for news. Its top-level `url` and `html` keys mean `map-preprocess` still deserializes the record as `RawPage { url, html }`, because serde ignores unknown fields. So raw-pages carries real `SourceItem`s with **zero SmartModule changes**. `text` may be empty at ingest for news. Planner must verify that no SmartModule struct uses `deny_unknown_fields` and that the field names match exactly. — **Reversibility:** costly — the raw-pages record shape becomes a contract that the SmartModules and any external producer rely on.
- **D-07:** The output record is **one schema for both kinds: `SourceItem` fields plus enrichment.** For news, the consumer maps `ExtractedArticle` onto SourceItem fields (`title`, `author`, `published`, `text` = `body_preview`, `url`). It moves article-only fields (`relevance_score`, `top_keywords`, `inferred_category`, `word_count`) into a news-specific block (e.g. `article: Option<ArticleMeta>`), and puts NER entities in a shared field. `html` is **never** in the output.
- **D-08:** `source_type` is an enum `news | social`. For **news**: `platform` = site host (e.g. `bbc.co.uk`, taken from the URL) and `id` = the canonical URL. For **social**: `platform` and `id` come straight from the plugin's WIT item.
- **D-09:** `published` in output is **RFC 3339 or null**. Social values convert from WIT epoch-ms (Phase 1 D-03). News `published_date` (a free-form string from map-extract) is **parsed on a best-effort basis**. When parsing fails, `published` is null and the original string is kept in the news article block.

#### Carried forward (locked in Phase 1)
- The WIT `item` record (`wit/types.wit`): id, url, title?, text, author?, published (u64 epoch ms)?, platform, raw (JSON string)?. `SourceItem` must line up with it field for field (SC1). It adds `source_type` and the news-only `html`.
- Consumer NER runs only in `--limit` batch mode (known gap, out of scope). PIPE-04 is verified in that mode.

### Claude's Discretion
- **JSON output target.** Choose between a file through `save_json` (e.g. `--output <path>`), JSON lines on stdout, or both. The requirement is that news and social land in the **same** JSON output, labelled by `source_type` and `platform`. The existing human summary printout may stay.
- **Social NER input.** Choose what NER runs on (text only, or title plus text) and how long posts and video descriptions are truncated. News currently uses `body_preview` (about 400–500 chars).
- Mapping social `raw` (JSON string) into the output: embed it as parsed JSON or keep it as a string.
- Where `SourceItem` lives (e.g. in `src/models.rs` or a new module), its Rust naming, and serde attributes.
- How the consumer merges the two streams concurrently (e.g. `tokio::select!` or merged streams) and the shape of the narrowing flag.
- How a social `SourceItem` gets produced in Phase 2 tests before the Phase 3 host exists (e.g. a test helper or a small produce path). It must not add a user-facing capability that belongs to Phase 3 or 4.

### Deferred Ideas (OUT OF SCOPE)
None. The discussion stayed within phase scope. (The existing gaps noted in the scout, such as silent deserialization drops in `consume_batch` and no NER in streaming mode, stay out of scope per the milestone decision.)
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| PIPE-01 | A unified `SourceItem` envelope (source type, platform, id, url, title, text, author, published time, raw metadata) covers news pages and social posts | Pattern 1 (`SourceItem` shape with serde attributes), WIT field map, `html` skip-if-none rule (Pitfall 1), RFC 3339 conversion via `chrono` (Code Examples) |
| PIPE-02 | News items continue through the existing WISE SmartModule chain | Pattern 2 (`ConsumerConfigExt` + `SmartModuleInvocation::Predefined` chain, compile- and live-verified), D-06 live probe (SourceItem-shaped record passed the unmodified chain), preflight checks (Pattern 4) |
| PIPE-03 | Social items pass through to the consumer with no social-specific filtering in v1 | Separate plain consumer on `filimon-social-items` (no `smartmodule` vec), Pattern 3 merge; SC3 contrast fixture (`"lol cat"` would score below 0.05) |
| PIPE-04 | The consumer runs NER on social item text and writes it to the same JSON output as news articles, labelled by source | Pattern 5 (`OutputRecord` with no `html` field), injectable entity detector so NER-on-social is unit-testable without the (currently missing) GLiNER model, `save_json` generalised |
</phase_requirements>

## Project Constraints (from CLAUDE.md)

`.claude/CLAUDE.md` imports `AGENTS.md` and `WORKFLOW.md`. Those define the separate "GSD Path" `.project/` pipeline, which is **not initialized** here (orchestrator note); its STATE/gate mechanics do not apply. Directives that still apply:

- Claims need checked sources; never report a command as passing unless it ran. Report partial or failed results plainly. Preserve uncertainty as a confidence level.
- Record user corrections and vetoes verbatim (CONTEXT.md decisions are copied verbatim above).
- From STATE.md: build on the uncommitted `fluvio-wise` work, **do not fix its known gaps**, and **never modify the `fluvio/` reference checkout**.
- From REQUIREMENTS.md Out of Scope: fixing existing `fluvio-wise` gaps (watch mode, silent drops, batch-only NER, dead code) is not this milestone.
- Repo conventions observed: `edition = "2024"`, `rust-version = "1.85"`, `[lints.clippy] all/pedantic = "warn"`, `unsafe_code = "warn"` [VERIFIED: Cargo.toml]; errors via `miette::Result` + `miette::miette!` strings; commands in `src/commands/*.rs` dispatched from `main.rs`; not-yet-used items carry `#![cfg_attr(not(test), allow(dead_code, reason = "..."))]` (see `src/plugins/mod.rs:10-16`).

## Summary

The Fluvio 0.50.1 client (the version in `Cargo.lock`) exposes the same chain mechanism the `fluvio consume --transforms-file` CLI uses: build a `ConsumerConfigExt` whose `smartmodule: Vec<SmartModuleInvocation>` holds one `SmartModuleInvocation { wasm: SmartModuleInvocationWasm::Predefined("filimon/map-preprocess@0.1.0"), kind: SmartModuleKind::Generic(SmartModuleContextData::None), .. }` per transform, in order, and pass it to `Fluvio::consumer_with_config`. I compiled exactly that against `fluvio 0.50.1` and ran it against the local 0.18.1 cluster. It returned `ExtractedArticle` JSON from `filimon-raw-pages`. The SPU resolves `Predefined(name)` by looking up the loaded SmartModule by that FQDN, so the `uses:` strings from `transforms.yaml` work unchanged.

D-06 holds. No SmartModule struct uses `deny_unknown_fields`. A SourceItem-shaped record (`source_type`, `platform`, `id`, `url`, `title: null`, `text: ""`, …, `html: "<html>…"`) passed through the **unmodified** chain and produced a correct `ExtractedArticle`. The probe also exposed a sharp edge. A record with `"html": null` is a **poison record**. map-preprocess fails with `invalid type: null, expected a string`, and the default `consumer_with_config` retry mode (`TryUntil(100)`) reconnects to the **same offset** over and over. That blocks every later news record. The CLI in `-d` mode also hides SmartModule errors, including `SmartModuleNotFound`, behind `WARN` logs, and `fili` installs no tracing subscriber. Plans must therefore (a) serialize `html` with `skip_serializing_if = "Option::is_none"` and never send a social item to raw-pages, (b) build the news consumer with `RetryMode::Disabled` and handle an `Err` item explicitly, and (c) preflight-check both topics and all three SmartModules through the admin API so failures name the missing thing.

Everything outside the Fluvio wiring is pure Rust and easy to unit-test: the `SourceItem` type, URL→platform, epoch-ms/free-form date→RFC 3339 (`chrono`, already in the lockfile through fluvio), `ExtractedArticle`→`OutputRecord` mapping, NER input selection, and the merged-stream `--limit`/idle-timeout loop (testable with `futures_util::stream::iter` fakes). One gap matters for verification. The GLiNER model files (`tokenizer.json`, `onnx/model.onnx`) are **not present** locally, and `load_ner_engine` silently falls back to "no NER". So NER-on-social must be proven through an injectable detector in unit tests, and the live SC4 run must be gated behind a model download.

**Primary recommendation:** Add `SourceItem`/`SourceType`/`OutputRecord`/`ArticleMeta` to `src/models.rs`, make the news producers emit `SourceItem::news(url, html)`, and rewrite `commands/consume.rs` around two `consumer_with_config` streams: raw-pages with the 3-module chain, and social-items plain, both with `RetryMode::Disabled`. Merge them with `futures_util::stream::select`, run admin-API preflight checks first, and write one JSON array through a generalised `save_json` to `--output` (default `output/wise_output.json`).

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| `SourceItem` envelope type + JSON contract | `fili` binary (shared models, `src/models.rs`) | Phase 3 host (future producer) | One Rust type that producers, consumer, and the Phase 3 host import as `crate::models::SourceItem` (bin-only crate) |
| News ingest (crawl/file → SourceItem) | `fili` producer commands (`commands/produce.rs`, `main.rs::run_file`) | Fluvio topic `filimon-raw-pages` | Producers own envelope construction; the topic stores it |
| News extraction + relevance filtering | Fluvio SPU (SmartModule chain, server-side WASM) | `fili consume` (attaches the chain per stream request) | D-02: the chain runs in the SPU, invoked by the consumer's stream request, so no extra process |
| Social pass-through | Fluvio topic `filimon-social-items` | `fili consume` (plain stream) | D-01: no SmartModules attached, so nothing is filtered |
| Topic/SmartModule existence checks | `fili consume` via Fluvio admin API (SC) | — | Fail fast with a named error (D-05) before opening streams |
| Merge, limit, NER enrichment, JSON output | `fili consume` (client process) | Local filesystem (`--output`) | NER is too heavy for WASM SmartModules; consumer owns output |
| Topic creation | Operator (`fluvio topic create`) for social-items; `ensure_topics()` for raw-pages | README | D-05 |

## Standard Stack

### Core (already in the project)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| `fluvio` | 0.50.1 (lockfile) | Producer/consumer/admin client, consumer-side SmartModule chain | Existing dependency. `consumer_with_config` + `ConsumerConfigExt` is the non-deprecated API [VERIFIED: ~/.cargo/registry/.../fluvio-0.50.1/src/fluvio.rs:351-358, consumer/config.rs:81-109] |
| `futures-util` | 0.3 | `StreamExt::map/next`, `stream::select` to merge two streams | Existing dependency. `ConsumerStream: Stream<…> + Unpin` [VERIFIED: fluvio-0.50.1/src/consumer/stream.rs:26], so `select` works without boxing (compiled in probe) |
| `tokio` | 1 (full) | `tokio::time::timeout` idle cutoff (existing 5 s pattern) | Existing |
| `serde` / `serde_json` | 1 | Envelope (de)serialization; `serde_json::Value` for `raw` | Existing |
| `url` | 2 | `Url::parse(..).host_str()` for news `platform` | Existing dependency. Probe confirmed `file:///…` gives `host_str() == None` |
| `miette` | 7 | Error reporting | Project convention |

### Supporting (new direct dependency)
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| `chrono` | 0.4 (0.4.45 already in `Cargo.lock` via fluvio) | `DateTime::<Utc>::from_timestamp_millis` → RFC 3339; best-effort parse of news `published_date` | Use `default-features = false, features = ["std", "clock"]` (compiled in probe). It adds no new crate to the lockfile [VERIFIED: `cargo tree -i chrono` → `chrono v0.4.45 └── fluvio v0.50.1`] |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| `consumer_with_config` (ConsumerConfigExt) | deprecated `partition_consumer(..).stream_with_config(offset, ConsumerConfig { smartmodule, .. })` | The existing `TypedConsumer` uses the deprecated path. It still works and has no retry loop, but it is deprecated since 0.21.8, and a missing topic surfaces only through partition lookup, which can wait up to `FLV_METADATA_TIMEOUT` (default 60 000 ms) [VERIFIED: fluvio-0.50.1/src/sync/mod.rs:29-34]. Prefer `consumer_with_config` + `RetryMode::Disabled` |
| `futures_util::stream::select` | `tokio::select!` loop over two `.next()` futures | Both work. `select` yields one merged stream, so the limit/timeout loop and its unit tests stay simple |
| `chrono` | `time` crate | `time` is not in the lockfile. `chrono` is already compiled |
| `--output` file | JSON Lines on stdout | `main.rs` prints a banner box to stdout first, and summaries print to stdout, so stdout JSONL would be polluted |

**Installation:**
```bash
cargo add chrono@0.4 --no-default-features --features std,clock
```

**Version verification:** `cargo info chrono` → `version: 0.4.45`, `rust-version: 1.62.0` (MSRV under the project's 1.85). `cargo search chrono` → `chrono = "0.4.45"` [VERIFIED: crates.io via cargo, this session].

## Package Legitimacy Audit

| Package | Registry | Age | Downloads | Source Repo | Verdict | Disposition |
|---------|----------|-----|-----------|-------------|---------|-------------|
| chrono | crates.io | ~12 yrs (first published 2014-11-20) | ~17.5M/wk | github.com/chronotope/chrono | [OK] | Approved (already a transitive dependency; it only becomes direct) |

**Packages removed due to [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none
(`gsd-tools query package-legitimacy check --ecosystem crates chrono` → `verdict: OK`, `postinstall: null`. Cargo crates have no npm-style postinstall. `build.rs` risk is n/a because the crate is already compiled in this workspace.)

## Architecture Patterns

### System Architecture Diagram

```
 fili produce <url> ──crawl──┐
                             ├─► SourceItem::news(url, html) ──produce(key=url)──► [topic filimon-raw-pages]
 fili file <paths> ──read────┘                                                          │
                                                                                        │ stream request carries
 (Phase 3 host; Phase 2: `fluvio produce`                                               │ smartmodule = [preprocess,
  or #[ignore] test helper)                                                             │  extract, filter-relevance]
   SourceItem::social(..) ──produce(key=platform:id)──► [topic filimon-social-items]    ▼
                                                              │              SPU runs chain (WASM)
                                                              │              drops relevance < 0.05
 fili consume [--from-beginning] [--limit N] [--enrich-ner] [--source both|news|social] [--output P]
   1. preflight: admin.list TopicSpec(raw-pages, social-items) + SmartModuleSpec(x3) ── missing? ─► named error, exit
   2. news stream  = consumer_with_config(raw-pages, chain, RetryMode::Disabled) ─► ExtractedArticle ─► OutputRecord::from_article
      social stream = consumer_with_config(social-items, no chain, RetryMode::Disabled) ─► SourceItem ─► OutputRecord::from_social
   3. stream::select(news, social) ─► loop until total == N or 5 s idle (Err item ─► stderr, stop that branch)
   4. --enrich-ner? ─► detector.detect(ner_input(record)) ─► record.entities
   5. print_summary(record) to stdout  +  save_json(&records, --output) ─► output/wise_output.json
```

### Recommended Project Structure
```
src/
├── models.rs            # + SourceType, SourceItem, OutputRecord, ArticleMeta, conversions, date helpers
├── streaming.rs         # TOPIC_SOCIAL_ITEMS added, TOPIC_ARTICLES removed, news_chain() + chain consumer helper
├── commands/consume.rs  # preflight, two streams, merge loop, enrichment, JSON output
├── commands/produce.rs  # send SourceItem::news instead of RawPage
├── main.rs              # run_file sends SourceItem::news; Consume args (--source, --output)
├── cli.rs               # Consume { source, output } flags
└── output.rs            # save_json/print_summary generalised to OutputRecord
```
Keep `RawPage` in `models.rs`. `crawler::fetch_page`/`extract_links` and `watch.rs` still use it internally. Convert at the produce boundary only.

### Pattern 1: `SourceItem` envelope (wire contract)
**What:** One struct for both kinds, field-named to match the WIT `item` record plus `source_type` and `html`.
**WIT source of truth** [VERIFIED: wit/types.wit:6-20]:
```
    record item {
        id: string,
        url: string,
        title: option<string>,
        text: string,
        author: option<string>,
        /// Unix epoch milliseconds (D-03).
        published: option<u64>,
        platform: string,
        raw: option<string>,
    }
```
**Recommended Rust shape** (field names verbatim from the WIT record; `published` is rendered RFC 3339 at construction, per Phase 1 D-03 "Host renders RFC 3339 when building SourceItem JSON"):
```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SourceType { News, Social }

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SourceItem {
    pub source_type: SourceType,
    pub platform: String,
    pub id: String,
    pub url: String,
    #[serde(default)] pub title: Option<String>,
    #[serde(default)] pub text: String,
    #[serde(default)] pub author: Option<String>,
    /// RFC 3339 (UTC). Social: converted from WIT epoch-ms.
    #[serde(default)] pub published: Option<String>,
    /// Platform metadata. Parsed from the WIT JSON string; kept as Value::String if not valid JSON.
    #[serde(default)] pub raw: Option<serde_json::Value>,
    /// News only. MUST be skipped when None: `"html": null` poisons map-preprocess (Pitfall 1).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
}
```
Constructors: `SourceItem::news(url: String, html: String)` sets `platform = platform_from_url(&url)`, `id = url.clone()`, `text = String::new()`, `html = Some(html)`. `SourceItem::social(id, url, title, text, author, published_ms: Option<u64>, platform, raw: Option<String>)` mirrors the WIT parameters 1:1, so Phase 3 maps `item` → `SourceItem` without reshaping. Mark `social` with `#[cfg_attr(not(test), allow(dead_code, reason = "produced by the plugin host in Phase 3"))]`.

**Verified chain-side contract** [VERIFIED: smartmodules/map-preprocess/src/lib.rs:19-23, smartmodules/map-extract/src/lib.rs:24-27]:
```rust
#[derive(Debug, Serialize, Deserialize)]
pub struct RawPage {
    pub url: String,
    pub html: String,
}
```
No `deny_unknown_fields` anywhere under `smartmodules/` (grep returned nothing, and the struct above was read). Field names `url`/`html` match exactly. Live probe: a SourceItem-shaped record through `fluvio consume <scratch> -B -d --transforms-file transforms.yaml` printed `{"url":"https://example.com/a","title":"Election policy report",…,"relevance_score":0.7260731816974316,…,"inferred_category":"Politics"}`.

Note: map-preprocess re-serializes `raw_page` as `RawPage { url, html }`, so the extra SourceItem fields are **dropped inside the chain**. The consumer must rebuild `source_type`/`platform`/`id` from `ExtractedArticle.url`, which D-07/D-08 already prescribe.

### Pattern 2: Attaching the SmartModule chain (D-02), compile- and live-verified
**Source:** fluvio CLI `create_smartmodule_list` [VERIFIED: fluvio/crates/fluvio-cli/src/client/smartmodule_invocation.rs:57-82] (reference checkout, read only) + client types [VERIFIED: fluvio-spu-schema-0.50.1/src/server/smartmodule.rs:25-33, 77-85, 123-150].
```rust
use std::collections::BTreeMap;
use fluvio::consumer::{ConsumerConfigExt, RetryMode};
use fluvio::{Offset, SmartModuleContextData, SmartModuleExtraParams,
             SmartModuleInvocation, SmartModuleInvocationWasm, SmartModuleKind};

pub const NEWS_CHAIN: [&str; 3] = [
    "filimon/map-preprocess@0.1.0",
    "filimon/map-extract@0.1.0",
    "filimon/filter-relevance@0.1.0",
];

pub fn news_chain() -> Vec<SmartModuleInvocation> {
    let name = NEWS_CHAIN.join(",");
    NEWS_CHAIN.iter().map(|uses| SmartModuleInvocation {
        wasm: SmartModuleInvocationWasm::Predefined((*uses).to_string()),
        kind: SmartModuleKind::Generic(SmartModuleContextData::default()), // = None
        params: SmartModuleExtraParams::new(BTreeMap::new(), None),
        name: Some(name.clone()),
    }).collect()
}

let config = ConsumerConfigExt::builder()
    .topic(TOPIC_RAW_PAGES)
    .partition(0)
    .offset_start(offset)            // Offset::beginning() or Offset::end()
    .smartmodule(news_chain())
    .retry_mode(RetryMode::Disabled) // see Pitfall 2
    .build()
    .map_err(|e| miette::miette!("Failed to build news consumer config: {e}"))?;
let news = client.consumer_with_config(config).await
    .map_err(|e| miette::miette!("Failed to open {TOPIC_RAW_PAGES} stream: {e}"))?;
```
This exact code compiled against `fluvio 0.50.1` and ran against the local cluster, returning `{"url":"https://example.com/news","title":"Tech Market Surges",…}` from `filimon-raw-pages`. `FluvioStream` needs a `client()` accessor, or new methods, to reach the `Fluvio` handle. The `NEWS_CHAIN` strings match `transforms.yaml` and `fluvio sm list` (`filimon/map-extract@0.1.0`, `filimon/map-preprocess@0.1.0`, `filimon/filter-relevance@0.1.0`) [VERIFIED: transforms.yaml:10-12, live `fluvio sm list`].

Builder field defaults [VERIFIED: fluvio-0.50.1/src/consumer/config.rs:15, 81-109]: `const DEFAULT_RETRY_MODE: RetryMode = RetryMode::TryUntil(100);`, `pub disable_continuous: bool` (default false), `pub smartmodule: Vec<SmartModuleInvocation>` (default empty), `pub offset_start: Offset` (required). Leave `disable_continuous` false: the existing 5 s idle timeout ends batch mode, and `disable_continuous(true)` with `Offset::end()` would return nothing.

### Pattern 3: Merging the two streams (D-04)
```rust
enum Incoming { News(ExtractedArticle), Social(SourceItem), Failed(&'static str, String) }

let news = news_stream.map(|r| match r {
    Ok(rec) => serde_json::from_slice::<ExtractedArticle>(rec.value())
        .map(Incoming::News)
        .unwrap_or_else(|e| Incoming::Failed("news", format!("offset {}: {e}", rec.offset()))),
    Err(code) => Incoming::Failed("news", describe(&code)),
});
let social = social_stream.map(|r| /* same, SourceItem → Incoming::Social */);
let mut merged = futures_util::stream::select(news, social);
```
- `stream::select` polls both fairly and ends only when both end. Compiled and ran in the probe with the two `consumer_with_config` streams.
- For `--source news|social`, open only one stream (use `futures_util::stream::empty()` or an `Either` for the other branch).
- Make the loop generic over `S: Stream<Item = Incoming> + Unpin` so unit tests can feed `futures_util::stream::iter(vec![…])`. Test `limit` counting across both kinds and the idle cutoff without Fluvio.
- On `Incoming::Failed`, write to **stderr** with `eprintln!`, not `tracing::error!`. No subscriber is installed (grep for `tracing_subscriber`/`init()` in `src/` found nothing), so `error!` output is invisible.
- Keep the existing semantics: stop when `records.len() == limit` or no item arrives within 5 s [VERIFIED: src/streaming.rs:192 `tokio::time::timeout(std::time::Duration::from_secs(5), stream.next())`].

### Pattern 4: Preflight (D-05 and missing SmartModules)
```rust
use fluvio::metadata::{topic::TopicSpec, smartmodule::SmartModuleSpec};
let admin = client.admin().await;
for topic in needed_topics {               // raw-pages and/or social-items per --source
    if admin.list::<TopicSpec, String>(vec![topic.to_string()]).await
        .map_err(|e| miette::miette!("Failed to list topics: {e}"))?.is_empty() {
        return Err(miette::miette!("Topic `{topic}` not found. Create it with: fluvio topic create {topic}"));
    }
}
for sm in NEWS_CHAIN {                      // only when news is selected
    if admin.list::<SmartModuleSpec, String>(vec![sm.to_string()]).await
        .map_err(|e| miette::miette!("Failed to list SmartModules: {e}"))?.is_empty() {
        return Err(miette::miette!("SmartModule `{sm}` is not loaded. Build and load it with smdk (see README)"));
    }
}
```
Verified live: `list::<TopicSpec>(["filimon-social-items"])` → empty (the topic does not exist yet on this cluster). `list::<SmartModuleSpec>(["filimon/does-not-exist@0.1.0"])` → 0. `list::<SmartModuleSpec>(["filimon/map-extract@0.1.0"])` → 1 (metadata name `map-extract-filimon-0.1.0`). Filtering by the FQDN works. `consumer_with_config` itself also returns `TopicNotFound` fast [VERIFIED: fluvio-0.50.1/src/fluvio.rs `.ok_or_else(|| FluvioError::TopicNotFound(topic.to_string()))`; CLI probe `Topic not found: filimon-social-items`]. The explicit preflight still gives the operator-actionable message and covers SmartModules, which `consumer_with_config` does **not** check up front.

### Pattern 5: Unified output record (D-07/D-08/D-09)
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArticleMeta {
    pub relevance_score: f64,
    pub top_keywords: Vec<String>,
    pub inferred_category: String,
    pub word_count: usize,
    pub published_raw: Option<String>,   // original map-extract string (D-09)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutputRecord {               // deliberately has NO html field
    pub source_type: SourceType,
    pub platform: String,
    pub id: String,
    pub url: String,
    pub title: Option<String>,
    pub text: String,
    pub author: Option<String>,
    pub published: Option<String>,      // RFC 3339 or null
    pub raw: Option<serde_json::Value>,
    pub entities: Vec<NamedEntity>,
    pub article: Option<ArticleMeta>,   // Some for news, None for social
}
```
- `from_article(a: ExtractedArticle)`: `platform = platform_from_url(&a.url)`, `id = a.url`, `title = Some(a.title)`, `text = a.body_preview`, `published = parse_news_published(a.published_date.as_deref())`, `article.published_raw = a.published_date`.
- `from_social(s: SourceItem)`: copy fields and drop `html`. The struct has no `html` field, so html cannot leak by construction. Add a test asserting `!json.contains("\"html\"")`.
- Existing `ExtractedArticle` fields [VERIFIED: src/models.rs:58-69]: `url: String, title: String, author: Option<String>, published_date: Option<String>, body_preview: String, word_count: usize, relevance_score: f64, top_keywords: Vec<String>, named_entities: Vec<NamedEntity>, inferred_category: String`.

### Pattern 6: Injectable NER for testability
`NerEngine::detect(&self, text: &str) -> Result<Vec<NamedEntity>>` [VERIFIED: src/scorer.rs:83]. Let the enrichment step take `Option<&dyn Fn(&str) -> miette::Result<Vec<NamedEntity>>>`, or a small `trait EntityDetector`, so unit tests pass a fake detector that records the text it received. That proves "NER runs on social item text" (SC4) without the model files, which are missing locally (see Environment Availability). Recommended NER input: news → `body_preview` (unchanged, no regression); social → `title` + `"\n"` + `text`, truncated to the first 500 `char`s, matching map-extract's `body.chars().take(500)` [VERIFIED: smartmodules/map-extract/src/lib.rs:393].

### Anti-Patterns to Avoid
- **Routing social items to raw-pages "with html: None":** serializes `"html": null` and halts the news chain (Pitfall 1). D-01 forbids it anyway.
- **A `#[serde(flatten)] SourceItem` inside the output record:** risks leaking `html`. Use the explicit `OutputRecord`.
- **Relying on `tracing::error!` for new error paths:** invisible. Use `eprintln!`.
- **Printing `ErrorCode`'s full `Display` for SmartModule runtime errors:** it embeds the full record value (the whole HTML page). Print `hint` + `offset` only (Pitfall 3).
- **Editing the SmartModules or `fluvio/`:** forbidden (D-01, STATE.md).

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Running the chain in-process | A Rust re-implementation of preprocess/extract/filter, or loading WASM locally | Server-side chain via `SmartModuleInvocationWasm::Predefined` | D-02. Identical behavior to `transforms.yaml` |
| Epoch-ms → RFC 3339 / date parsing | Manual calendar math or regex dates | `chrono::DateTime::<Utc>::from_timestamp_millis`, `DateTime::parse_from_rfc3339`, `NaiveDate::parse_from_str` | Leap years, offsets, out-of-range values (`from_timestamp_millis` returns `None`) |
| Host extraction | String splitting on `/` | `url::Url::parse(..).host_str()` | IDNs, ports, userinfo, `file://` |
| Stream merging | Manual polling / spawn + channel | `futures_util::stream::select` | Fair polling, termination semantics |
| Topic/SM existence | Waiting for stream errors / timeouts | `FluvioAdmin::list::<TopicSpec/SmartModuleSpec, String>` | Deterministic and names the missing item |

**Key insight:** every Fluvio-facing piece already exists in the client crate. The phase's real work is the serde contract and consumer orchestration, and both can be pure, unit-testable functions.

## Runtime State Inventory

D-03 removes a topic, so this partly counts as a removal phase.

| Category | Items Found | Action Required |
|----------|-------------|------------------|
| Stored data | Local cluster has topic `filimon-articles` (live `fluvio topic list`: `filimon-articles computed 1 1 7days … resolution::provisioned`). Existing `filimon-raw-pages` records are old `RawPage {url, html}` JSON. They are still chain-compatible, but `--from-beginning` replays them as news | Code edit only: stop creating the topic. README does not document a delete step (D-03; resolved as Open Question 3). No data migration needed (old raw-pages records parse) |
| Live service config | SmartModules `filimon/map-preprocess@0.1.0`, `filimon/map-extract@0.1.0`, `filimon/filter-relevance@0.1.0` are loaded on the local cluster (live `fluvio sm list`). `docker-compose.yml:131-132` comments describe a stream-processor that "outputs to filimon-articles" | Update the docker-compose comment (doc only). SmartModules stay untouched |
| OS-registered state | None. No launchd/systemd/cron entries reference filimon (no such files in repo; the stream-processor exists only as a docker-compose profile) | None |
| Secrets/env vars | None reference the articles topic (`TOKENIZER_PATH`, `MODEL_PATH`, `FILI_REGISTRY`, `FLUVIO_CLUSTER_ADDR` unaffected) | None |
| Build artifacts | None carry the topic name. `src/main.rs:177-215` contains commented-out code that mentions `TOPIC_ARTICLES` | Dead code is out of scope. A grep gate like "no `filimon-articles`/`TOPIC_ARTICLES` left" must exclude comment lines in `main.rs`, or the planner accepts those comment hits |

Doc references to update (D-03) [VERIFIED: grep this session]: `README.md:27` (mermaid `T2[filimon-articles]`), `README.md:123` (`fluvio topic create filimon-articles`), `.planning/codebase/ARCHITECTURE.md:135`, `.planning/codebase/INTEGRATIONS.md:36,43,149,170`, `docker-compose.yml:132`, `src/models.rs:57` (doc comment), `src/streaming.rs:6-14` header comment. `.planning/codebase/STRUCTURE.md` has **no** `filimon-articles` match. Check it for the pipeline description anyway, but no edit is required unless it describes the articles flow.

## Common Pitfalls

### Pitfall 1: `"html": null` poisons the news chain
**What goes wrong:** map-preprocess fails `Failed to parse RawPage: invalid type: null, expected a string`, and no later raw-pages record is delivered.
**Why it happens:** `RawPage.html` is `String`, not `Option` (verified above). serde accepts a *missing* unknown field but rejects `null` for `String`.
**How to avoid:** `#[serde(skip_serializing_if = "Option::is_none")]` on `SourceItem.html`. `SourceItem::news` always sets `Some(html)`. Social items never go to raw-pages. Unit test: `serde_json::to_value(&social_item)` has no `html` key.
**Warning signs:** the news branch stops yielding. stderr shows `SmartModule transform error … Offset: N`.
**Evidence:** live probe on scratch topic `filimon-research-probe-02` (created then deleted): records at offsets 0 (SourceItem news), 1 (`"html":null`), 2 (plain RawPage). `--start 2` returned the offset-2 article. `-B -d` returned only offset 0. `--start 1` (continuous) logged `WARN fluvio::retry: Error consuming record: SmartModule transform error Failed to parse RawPage: invalid type: null, expected a string at line 1 column 113 … Offset: 1` repeatedly, roughly every 1.3 s, until killed (exit 124).

### Pitfall 2: Default retry mode loops on a failing record
**What goes wrong:** with the default `RetryMode::TryUntil(100)`, a stream error is logged at WARN and the consumer reconnects from the same offset, up to 100 times with exponential backoff, so it effectively hangs.
**Why it happens:** `handle_consumer_records` only returns the error when `RetryMode::Disabled` [VERIFIED: fluvio-0.50.1/src/consumer/retry.rs:320-325 `Err(e) => { warn!(…); if let RetryMode::Disabled = inner.consumer_config.retry_mode { return Some((stream.clone(), Some(Err(e)))); } }`].
**How to avoid:** `.retry_mode(RetryMode::Disabled)` on both consumers. On the first news `Err`, report it and stop polling the news branch; social keeps flowing. Whether the stream can continue past a runtime error is MEDIUM confidence (the `-d` probe suggests it stops), so do not rely on it.
**Warning signs:** consume hangs with no output.

### Pitfall 3: SmartModule errors are silent or huge
**What goes wrong:** `SmartModuleNotFound` shows only as a WARN log, so the CLI exits 0 with no records (probe: `fluvio consume … --transforms-file bad.yaml -d` → exit 0, log `SmartModule filimon/does-not-exist@0.1.0 was not found`). Runtime errors' `Display` prints the full record value.
**How to avoid:** Pattern 4 preflight. For `ErrorCode::SmartModuleRuntimeError(e)`, print `e.offset` and `e.hint` only [VERIFIED: fluvio-protocol-0.50.1/src/link/smartmodule.rs:12-23 fields `pub hint: String`, `pub offset: Offset`, `pub kind: SmartModuleKind`, `pub record_key: Option<RecordData>`, `pub record_value: RecordData`; error_code.rs:152-165 variants `SmartModuleNotFound { name: String }`, `SmartModuleRuntimeError(Box<super::smartmodule::SmartModuleTransformRuntimeError>)`]. Import the type through `fluvio::dataplane::link::ErrorCode` (compiled in probe).

### Pitfall 4: `--enrich-ner` silently degrades to no NER
**What goes wrong:** `load_ner_engine` returns `None` and prints `NER: ✗ unavailable` when the model is missing, and consume continues. On this machine the model files are absent, so a "plain SC4 run" would produce empty `entities` and could look like a pass.
**How to avoid:** prove NER-on-social with the injected detector in unit tests. For the live SC4 check, require the model (download step) and assert non-empty `entities` on at least one social record. Do not change `load_ner_engine`'s fallback without a user decision (it is existing behavior).

### Pitfall 5: `file://` URLs have no host
**What goes wrong:** `platform_from_url("file:///tmp/a.html")` → `host_str() == None` (probe confirmed).
**How to avoid:** fall back to a fixed label for news without a host (recommend `"local"`; see Assumptions A2). Strip a leading `www.` so `https://www.bbc.co.uk/…` → `bbc.co.uk`, matching D-08's example (A1).

### Pitfall 6: Bin-only crate, so no `tests/` integration tests can import types
**What goes wrong:** `Cargo.toml` has only `[[bin]] name = "fili"`. Files in `tests/*.rs` cannot `use filimon::models::SourceItem`.
**How to avoid:** put Phase 2 tests in `#[cfg(test)] mod tests` inside `src/`. Cluster-dependent tests go there too, marked `#[ignore]`, and take topic names as parameters so they can use unique scratch topics. Flag for Phase 8, which may need a lib target.

### Pitfall 7: `cargo test --workspace` is already red
`map-preprocess` `tests::test_lemmatize` fails (`left: "cars"`, `right: "car"`), a pre-existing failure observed this session. SmartModules must not be touched (D-01), so use `cargo test -p filimon` as the phase gate and record the pre-existing failure.

## Code Examples

### Epoch-ms and news-date conversion (probe-verified calls)
```rust
use chrono::{DateTime, NaiveDate, NaiveDateTime, SecondsFormat, Utc};

pub fn epoch_ms_to_rfc3339(ms: u64) -> Option<String> {
    let ms = i64::try_from(ms).ok()?;
    DateTime::<Utc>::from_timestamp_millis(ms)
        .map(|d| d.to_rfc3339_opts(SecondsFormat::Millis, true))
}
// probe: 1_730_000_000_123 -> "2024-10-27T03:33:20.123Z"

pub fn parse_news_published(raw: Option<&str>) -> Option<String> {
    let s = raw?.trim();
    if let Ok(d) = DateTime::parse_from_rfc3339(s) {
        return Some(d.with_timezone(&Utc).to_rfc3339_opts(SecondsFormat::Secs, true));
    }
    if let Ok(d) = DateTime::parse_from_rfc2822(s) {
        return Some(d.with_timezone(&Utc).to_rfc3339_opts(SecondsFormat::Secs, true));
    }
    if let Ok(n) = NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S") {   // no offset: assume UTC (A3)
        return Some(n.and_utc().to_rfc3339_opts(SecondsFormat::Secs, true));
    }
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {                // date only: midnight UTC (A3)
        return Some(d.and_hms_opt(0, 0, 0)?.and_utc().to_rfc3339_opts(SecondsFormat::Secs, true));
    }
    None
}
// probe: parse_from_rfc3339("2024-05-01T10:00:00+01:00") -> UTC "2024-05-01T09:00:00+00:00"
```
map-extract's date sources [VERIFIED: smartmodules/map-extract/src/lib.rs:130-147]: `article:published_time` meta, then `<time datetime>`. Both are usually ISO 8601, so the RFC 3339 branch covers the common case.

### Producing a news SourceItem (producers)
```rust
// commands/produce.rs and main.rs::run_file: replace `locked.send(&url, &page)` (page: RawPage)
let item = SourceItem::news(page.url.clone(), page.html);
locked.send(&item.url, &item).await?;   // key stays the URL
```

### Producing a social SourceItem in Phase 2 without a new user-facing command
Operator/manual smoke (no `fili` change):
```bash
fluvio topic create filimon-social-items
echo '{"source_type":"social","platform":"reddit","id":"t3_abc","url":"https://www.reddit.com/r/rust/comments/abc","title":"Ferris","text":"lol cat","author":"u1","published":"2024-10-27T03:33:20.123Z","raw":{"score":3}}' \
  | fluvio produce filimon-social-items
```
Automated: a `#[cfg(test)]` helper that uses `TypedProducer::send` against a scratch topic in an `#[ignore]` test. `"lol cat"` contains no `TOPIC_TERMS`, so it would score below the 0.05 threshold [VERIFIED: smartmodules/filter-relevance/src/lib.rs:14 `const RELEVANCE_THRESHOLD: f64 = 0.05;`, :46 `Ok(article.relevance_score >= RELEVANCE_THRESHOLD)`]. That makes it the SC3 contrast case.

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `Fluvio::partition_consumer(topic, 0)` + `PartitionConsumer::stream(offset)` | `Fluvio::consumer_with_config(ConsumerConfigExt)` | Deprecated since 0.21.8 (`#[deprecated(since = "0.21.8", note = "use consumer_with_config() instead")]`) [VERIFIED: fluvio-0.50.1/src/fluvio.rs:236] | New consume code should use `consumer_with_config`. The existing `TypedConsumer` may stay for untouched paths |
| `SmartModuleKind::Filter/Map/…` per-kind invocation | `SmartModuleKind::Generic(SmartModuleContextData)` (the SPU detects the kind from WASM exports) | Generic SmartModule API | Use `Generic(None)` for all three; no need to declare map vs filter |
| Separate `filimon-articles` topic fed by an external chain run | Chain applied on the consumer stream request (D-02) | This phase | One fewer topic and process |

**Deprecated/outdated:**
- `TOPIC_ARTICLES` / `filimon-articles`: removed by D-03.
- `transforms-extract-only.yaml` / `transforms-filter-only.yaml` reference intermediate topics (`filimon-extracted`, `filimon-preprocessed`) that no code creates. They are not in D-03's list. Leave them (resolved as Open Question 5).

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | News `platform` strips a leading `www.` (so `www.bbc.co.uk` → `bbc.co.uk`) | Pitfall 5 / Pattern 5 | Cosmetic. Grouping by site splits `www.` vs bare host |
| A2 | News items with no URL host (`file://`) get `platform = "local"` | Pitfall 5 | Cosmetic, but it becomes part of the output contract |
| A3 | Offset-less / date-only news dates are interpreted as UTC (midnight for date-only) | Code Examples | Published time may be off by the site's timezone offset |
| A4 | Social `raw` is embedded as parsed JSON (`serde_json::Value`) and falls back to `Value::String` when the WIT string is not valid JSON | Pattern 1 | Downstream consumers see mixed types for invalid raw |
| A5 | Batch mode always writes a JSON array to `--output`, default `output/wise_output.json` (parent dir created; `output/` is gitignored). Streaming mode (no `--limit`) keeps print-only | Summary / Pattern 5 | If the user wants JSONL or opt-in output, SC4's "plain run" wording would need re-reading |
| A6 | Social NER input = `title + "\n" + text`, first 500 chars. 500 chars stays within GLiNER-small's context | Pattern 6 | Long titles reduce body coverage. Token-limit truncation inside gline-rs is unverified |
| A7 | After a `SmartModuleRuntimeError` with `RetryMode::Disabled`, the news stream should be treated as terminated (not resumable past the bad offset) | Pitfall 2 | If the stream would continue, stopping early loses later news in that run. Low risk because fili-produced records cannot be poison after Pitfall 1's fix |
| A8 | The README's model download (`urchade/gliner_small-v2.1` snapshot) yields `onnx/model.onnx` | Environment | If the repo lacks ONNX weights, the live SC4 check needs a different source (e.g. an ONNX-exported repo) |

## Open Questions (RESOLVED)

1. **Should `--enrich-ner` fail hard when the model cannot load?** — RESOLVED: No. `load_ner_engine` keeps its silent fallback unchanged (existing behavior, Pitfall 4). SC4 is proven with the injected detector in unit tests, and the live SC4 run with the GLiNER model is a human-check after the model download (Plan 02-02, Task 2; Plan 02-01 lists the fallback as untouched).
   - What we know: today it prints `NER: ✗ unavailable` and continues. The model files are missing locally.
   - What's unclear: whether changing that counts as fixing an existing gap (out of scope).
   - Recommendation: keep the behavior, prove SC4 with the injected detector, and make the live SC4 check a human-verify step after the model download.
2. **`--source` flag shape** — RESOLVED: `--source <both|news|social>` as a clap `ValueEnum` (`SourceFilter { Both, News, Social }` in `src/cli.rs`), default `both`. Preflight checks only the topics and SmartModules of the selected sources (Plan 02-02, Task 3).
   - Recommendation: `--source <both|news|social>` as a clap `ValueEnum`, default `both`. Preflight only checks topics/SMs for the selected sources.
3. **Delete the stale `filimon-articles` topic on the local cluster?** — RESOLVED: No, and README does not document it either. Per D-03 nothing may document the retired topic, so README carries no cleanup step for it, not even an optional delete line. No plan deletes the topic on the local cluster; it is left alone (Plan 02-03, "Planner decisions recorded for this plan" and Task 1). This supersedes the recommendation below.
   - Recommendation (superseded by D-03, see resolution above): document `fluvio topic delete filimon-articles` as optional operator cleanup in README. Plans must not run it automatically.
4. **Unused `--threshold` CLI flag** (`src/cli.rs:29-31`, default 0.05) is not wired to the chain. — RESOLVED: Leave it untouched (existing gap). It is not passed as a SmartModule param, because filter-relevance ignores params and uses a constant (Plan 02-01 "Untouched on purpose"; Plan 02-02, Task 3 keeps `--threshold` unchanged).
5. **Intermediate-topic transforms files** (`transforms-extract-only.yaml`, `transforms-filter-only.yaml`, see State of the Art) — RESOLVED: Leave them untouched. They are not in D-03's list, and no Phase 2 plan modifies them.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust toolchain | build/tests | ✓ | rustc 1.85.0, cargo 1.85.0 | — |
| Fluvio CLI + local cluster | SC2/SC3/SC4 live checks, preflight | ✓ | CLI 0.18.1, Platform 0.18.1 (local) | — |
| `fluvio` client crate | consume/produce | ✓ | 0.50.1 (lockfile); connects to the 0.18.1 platform (probe) | — |
| SmartModules loaded | news chain | ✓ | `filimon/map-preprocess@0.1.0`, `filimon/map-extract@0.1.0`, `filimon/filter-relevance@0.1.0` | — |
| `filimon-raw-pages` topic | news | ✓ | exists | `ensure_topics()` creates it on produce |
| `filimon-social-items` topic | social | ✗ | — | Operator runs `fluvio topic create filimon-social-items` (D-05). Part of the plan's verification steps |
| `smdk` | (not needed; SMs untouched) | ✓ | present at ~/.fluvio/bin | — |
| GLiNER model (`models/gliner_small-v2.1/tokenizer.json`, `onnx/model.onnx`) | live NER (SC4 live run) | ✗ | only `gliner_config.json`, `README.md`, `.gitattributes`, `.cache/` present | Unit tests with an injected detector. Live SC4 is gated on the README download step (docker `model-downloader` profile or `huggingface_hub` snapshot) |

**Missing dependencies with no fallback:** none block implementation.
**Missing dependencies with fallback:** `filimon-social-items` (manual create), GLiNER model (fake detector for automated tests; human-verify for the live run).

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in test harness (`#[test]`, `#[tokio::test]` available via tokio `full`) |
| Config file | none (unit tests inside `src/`; bin-only crate) |
| Quick run command | `cargo test -p filimon` (59 tests pass today in about 6 s, measured this session) |
| Full suite command | `cargo test -p filimon && cargo clippy -p filimon` (do **not** use `--workspace`: the pre-existing `map-preprocess::tests::test_lemmatize` failure) |
| Cluster tests | `cargo test -p filimon -- --ignored` (needs the local Fluvio cluster; uses scratch topics) |

### Phase Requirements → Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| PIPE-01 | news + social `SourceItem` JSON round-trip (`from_str(to_string(x)) == x`) | unit | `cargo test -p filimon models::tests::source_item_round_trip` | ❌ Wave 0 |
| PIPE-01 | every WIT `item` field (parsed from `wit/types.wit` text, kebab→snake) is a `SourceItem` JSON key, plus `source_type` | unit | `cargo test -p filimon models::tests::source_item_matches_wit_item` | ❌ Wave 0 |
| PIPE-01/02 | news `SourceItem` JSON deserializes as a `RawPage { url, html }` mirror (copy of the SM struct in the test); social JSON has no `html` key | unit | `cargo test -p filimon models::tests::news_item_is_rawpage_compatible` | ❌ Wave 0 |
| PIPE-01 | `epoch_ms_to_rfc3339`, `parse_news_published`, `platform_from_url` (incl. `file://`, `www.`) | unit | `cargo test -p filimon models::tests::` | ❌ Wave 0 |
| PIPE-02 | `news_chain()` yields 3 `Predefined` invocations in `transforms.yaml` order (test reads `transforms.yaml` and compares `uses:` lines) | unit | `cargo test -p filimon streaming::tests::news_chain_matches_transforms_yaml` | ❌ Wave 0 |
| PIPE-02 | `fili file <fixture.html>` → `fili consume --from-beginning --limit N` outputs a news record with `article.relevance_score >= 0.05` | integration (cluster) | `cargo test -p filimon -- --ignored news_through_chain` or manual smoke | ❌ Wave 0 |
| PIPE-03 | merge loop: social item with low-relevance text is emitted; `--limit` counts across both kinds; 5 s idle stops | unit (fake streams, `tokio::time::pause`) | `cargo test -p filimon commands::consume::tests::` | ❌ Wave 0 |
| PIPE-03 | social item produced to a scratch social topic reaches the consumer output | integration (cluster) | `cargo test -p filimon -- --ignored social_passthrough` | ❌ Wave 0 |
| PIPE-04 | enrichment calls the detector with social `title+text` (≤500 chars) and news `body_preview`; output has `source_type`/`platform`; no `html` key | unit (fake detector) | `cargo test -p filimon commands::consume::tests::ner_runs_on_social_text` | ❌ Wave 0 |
| PIPE-04 | live: `fili consume --enrich-ner --limit N` writes one JSON array with both kinds and non-empty entities | manual (needs model) | human-verify after model download | manual-only (model absent) |
| D-05 | missing social topic → error naming `filimon-social-items` | integration (cluster) / manual | run `fili consume --limit 1` before creating the topic | ❌ Wave 0 |

### Sampling Rate
- **Per task commit:** `cargo test -p filimon`
- **Per wave merge:** `cargo test -p filimon && cargo clippy -p filimon` (+ `-- --ignored` when the cluster is up)
- **Phase gate:** full suite green, plus manual SC2/SC3/SC4 smoke on the local cluster before `/gsd-verify-work`

### Wave 0 Gaps
- [ ] An HTML fixture with news terms, so relevance is ≥ 0.05 (e.g. `examples/fixtures/news-article.html`). Today no sample HTML exists in the repo (`examples/` holds only `plugins/`).
- [ ] Topic names passed as parameters into the consume core (a struct such as `ConsumeTopics { raw_pages, social }`) so `#[ignore]` tests can use scratch topics. Create and delete them in the test via `admin.create`/`admin.delete::<TopicSpec>`.
- [ ] A generic merge/enrich loop over `Stream<Item = Incoming>` plus a detector abstraction, so unit tests run without Fluvio or the model.

## Security Domain

`security_enforcement: true`, ASVS level 1.

### Applicable ASVS Categories
| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | — (local Fluvio profile; OAuth is Phase 6) |
| V3 Session Management | no | — |
| V4 Access Control | no | — |
| V5 Input Validation | yes | serde typed deserialization of topic records (social items come from third-party plugins or any producer). Reject or skip bad records with a visible stderr line. `serde_json` recursion limit (default 128) bounds `raw` nesting |
| V6 Cryptography | no | — |
| V7 Error Handling & Logging | yes | Do not dump full record values (HTML, raw) into error output (Pitfall 3). No secrets in records |
| V12 Files | yes (light) | `--output` path is operator-supplied CLI input. Create the parent dir, overwrite a regular file only. No path from record content is ever used |

### Known Threat Patterns for this stack
| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Terminal escape injection: social `title`/`text` printed by `print_summary` can contain ANSI/control sequences | Tampering / Spoofing (operator terminal) | Strip `char::is_control` (except `\n` → space) in the human summary only; the JSON output keeps the original text (serde escapes control chars) |
| Poison record halts news ingestion (`"html": null` or malformed JSON to raw-pages by any producer) | Denial of Service | `skip_serializing_if` on `html`, `RetryMode::Disabled`, visible error with offset (Pitfalls 1–2) |
| Oversized social text driving NER cost | Denial of Service | Truncate NER input to 500 chars (Pattern 6) |
| `html` leaking into output files | Information disclosure | `OutputRecord` has no `html` field, plus a unit test asserting no `"html"` key |
| Record spoofing (`source_type: "news"` written to social-items) | Spoofing | The consumer labels by **topic of origin**, not by the record's claimed `source_type`. Force `SourceType::Social` for records from the social topic, and warn on mismatch |

## Sources

### Primary (HIGH confidence)
- `~/.cargo/registry/src/index.crates.io-…/fluvio-0.50.1/src/{fluvio.rs, consumer/config.rs, consumer/retry.rs, consumer/stream.rs, consumer/mod.rs, admin.rs, sync/mod.rs, lib.rs}`: consumer API, retry semantics, admin list, metadata timeout
- `fluvio-spu-schema-0.50.1/src/server/smartmodule.rs`: `SmartModuleInvocation`, `SmartModuleInvocationWasm`, `SmartModuleKind`, `SmartModuleContextData`
- `fluvio-protocol-0.50.1/src/link/{error_code.rs, smartmodule.rs}`: error variants and runtime-error fields
- `fluvio/crates/fluvio-cli/src/client/{smartmodule_invocation.rs, consume/mod.rs}` and `fluvio/crates/fluvio-spu/src/{smartengine/context.rs, services/public/stream_fetch.rs}` (read-only reference checkout, 0.50.2): how the CLI builds the chain, and how the SPU resolves `Predefined` names
- Compile probe (scratch crate against fluvio 0.50.1, then removed) and live probes against the local Fluvio 0.18.1 cluster (scratch topic created and deleted; no project topic written)
- Repo files read: `wit/types.wit`, `src/{models,streaming,output,main,cli,scorer}.rs`, `src/commands/{consume,produce}.rs`, `smartmodules/*/src/lib.rs`, `smartmodules/*/SmartModule.toml`, `transforms.yaml`, `README.md`, `docker-compose.yml`

### Secondary (MEDIUM confidence)
- `cargo info chrono` / `cargo search chrono` / gsd package-legitimacy seam: version and legitimacy

### Tertiary (LOW confidence)
- Stream continuation behavior after a SmartModule runtime error with `RetryMode::Disabled` (A7)
- GLiNER model download source contents (A8)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH. Every crate is already in the lockfile; the API was compiled and run.
- Architecture: HIGH. The chain consumer, merge, and preflight were exercised live; the D-06 contract was probed live.
- Pitfalls: HIGH for 1–3, 5–7 (reproduced). MEDIUM for 4 (code read; the model is absent, so the NER path was not run).

**Research date:** 2026-11-04
**Valid until:** 2026-12-04 (stable; pinned fluvio 0.50.1)
