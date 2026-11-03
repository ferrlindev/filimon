# Phase 2: SourceItem Pipeline - Context

**Gathered:** 2026-11-04
**Status:** Ready for planning

<domain>
## Phase Boundary

Every pipeline record uses a single `SourceItem` envelope. It covers news pages and social posts and lines up with the Phase 1 WIT `item` record. News is ingested as a `SourceItem` and still flows through `map-preprocess → map-extract → filter-relevance` with no regression. Social items skip the chain completely and reach the consumer with no filtering. The consumer's existing NER mode (`--enrich-ner --limit`) enriches both kinds into one output, and each record is labelled with source type and platform.

Not in this phase: the plugin host (Phase 3), the plugin CLI (Phase 4), social SmartModule filtering (v2 PIPE-05), and fixes to existing `fluvio-wise` gaps (watch mode, silent drops, streaming-mode NER).

**Scope note (from codebase scout):** nothing in `fili` writes `filimon-articles` today. The chain only runs through a manual `fluvio consume filimon-raw-pages --transforms-file transforms.yaml`. The consumer prints summaries and never writes JSON (`output::save_json` is unused). Phase 2 has to wire the chain into `fili consume` for SC2 to be testable (see D-02). That is required by the success criteria and is not an existing-gap fix.

</domain>

<decisions>
## Implementation Decisions

### Topic routing
- **D-01:** Social `SourceItem`s go to a **new dedicated topic, `filimon-social-items`**, and never pass through the SmartModule chain. The three SmartModules stay **untouched** (no source and no rebuild changes). Two things follow: the news path cannot regress (SC2), and social records cannot be filtered (SC3). — **Reversibility:** costly — topic names are wiring that Phase 3 (host produce) and Phase 8 (integration tests) will depend on.
- **D-02:** `fili consume` **applies the SmartModule chain itself** by reading `filimon-raw-pages` with the `map-preprocess → map-extract → filter-relevance` transforms attached through the Fluvio 0.50 consumer API. This is the same chain `transforms.yaml` declares. There is no intermediate articles topic and no extra process. Research must confirm the exact Fluvio 0.50 consumer API for attaching a chain of hub/locally loaded SmartModules (`filimon/map-preprocess@0.1.0` etc.).
- **D-03:** **Remove the `filimon-articles` topic.** That means deleting `TOPIC_ARTICLES`, dropping it from `ensure_topics()`, and updating README and `.planning/codebase/` references (ARCHITECTURE, INTEGRATIONS, STRUCTURE) so nothing documents a dead topic.
- **D-04:** By default, `fili consume` reads **both sources concurrently** (news through the chain on raw-pages, plus social-items) into one output stream. `--limit N` caps the **total** number of records across both. An optional narrowing flag (e.g. `--source news|social`, defaulting to both) may be added. SC4 must pass with a plain `fili consume --enrich-ner --limit N` run.
- **D-05:** **Operators create `filimon-social-items` by hand**, documented in the README next to the existing `fluvio topic create` steps. `ensure_topics()` does not create it. If the topic is missing, the consumer must fail with an error that names it, not hang silently.

### News envelope shape
- **D-06:** The crawler (`fili produce`) and `fili file` produce a **`SourceItem` to `filimon-raw-pages`** that carries an extra **`html: Option<String>`** field for news. Its top-level `url` and `html` keys mean `map-preprocess` still deserializes the record as `RawPage { url, html }`, because serde ignores unknown fields. So raw-pages carries real `SourceItem`s with **zero SmartModule changes**. `text` may be empty at ingest for news. Planner must verify that no SmartModule struct uses `deny_unknown_fields` and that the field names match exactly. — **Reversibility:** costly — the raw-pages record shape becomes a contract that the SmartModules and any external producer rely on.
- **D-07:** The output record is **one schema for both kinds: `SourceItem` fields plus enrichment.** For news, the consumer maps `ExtractedArticle` onto SourceItem fields (`title`, `author`, `published`, `text` = `body_preview`, `url`). It moves article-only fields (`relevance_score`, `top_keywords`, `inferred_category`, `word_count`) into a news-specific block (e.g. `article: Option<ArticleMeta>`), and puts NER entities in a shared field. `html` is **never** in the output.
- **D-08:** `source_type` is an enum `news | social`. For **news**: `platform` = site host (e.g. `bbc.co.uk`, taken from the URL) and `id` = the canonical URL. For **social**: `platform` and `id` come straight from the plugin's WIT item.
- **D-09:** `published` in output is **RFC 3339 or null**. Social values convert from WIT epoch-ms (Phase 1 D-03). News `published_date` (a free-form string from map-extract) is **parsed on a best-effort basis**. When parsing fails, `published` is null and the original string is kept in the news article block.

### Carried forward (locked in Phase 1)
- The WIT `item` record (`wit/types.wit`): id, url, title?, text, author?, published (u64 epoch ms)?, platform, raw (JSON string)?. `SourceItem` must line up with it field for field (SC1). It adds `source_type` and the news-only `html`.
- Consumer NER runs only in `--limit` batch mode (known gap, out of scope). PIPE-04 is verified in that mode.

### Claude's Discretion
- **JSON output target.** Choose between a file through `save_json` (e.g. `--output <path>`), JSON lines on stdout, or both. The requirement is that news and social land in the **same** JSON output, labelled by `source_type` and `platform`. The existing human summary printout may stay.
- **Social NER input.** Choose what NER runs on (text only, or title plus text) and how long posts and video descriptions are truncated. News currently uses `body_preview` (about 400–500 chars).
- Mapping social `raw` (JSON string) into the output: embed it as parsed JSON or keep it as a string.
- Where `SourceItem` lives (e.g. in `src/models.rs` or a new module), its Rust naming, and serde attributes.
- How the consumer merges the two streams concurrently (e.g. `tokio::select!` or merged streams) and the shape of the narrowing flag.
- How a social `SourceItem` gets produced in Phase 2 tests before the Phase 3 host exists (e.g. a test helper or a small produce path). It must not add a user-facing capability that belongs to Phase 3 or 4.

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Contract
- `wit/types.wit` — the WIT `item` record that `SourceItem` must line up with (SC1)
- `.planning/phases/01-plugin-contract-configuration/01-CONTEXT.md` — D-01..D-04 (cursor, item shape, epoch-ms `published`, error variants)
- `.planning/REQUIREMENTS.md` — PIPE-01..PIPE-04
- `.planning/ROADMAP.md` §Phase 2 — success criteria SC1–SC4

### Pipeline code
- `src/models.rs` — `RawPage`, `ExtractedArticle`, `NamedEntity` (current record types)
- `src/streaming.rs` — topic constants, `ensure_topics()`, `TypedConsumer` (`consume_batch` silently drops undeserializable records, a known gap)
- `src/commands/consume.rs` — consumer and the NER batch path
- `src/commands/produce.rs`, `src/main.rs` (`run_file`) — news producers that must emit `SourceItem`
- `src/output.rs` — `print_summary`, unused `save_json`
- `transforms.yaml` — the chain the consumer must apply (D-02)
- `smartmodules/map-preprocess/src/lib.rs` — `RawPage` input struct (D-06 relies on serde ignoring extra fields)
- `smartmodules/filter-relevance/src/lib.rs` — threshold 0.05 (SC3 contrast case)

### Docs to update
- `README.md` — topic creation steps (add social-items, drop articles) and the pipeline diagram
- `.planning/codebase/ARCHITECTURE.md`, `.planning/codebase/INTEGRATIONS.md`, `.planning/codebase/STRUCTURE.md` — articles-topic references

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `FluvioStream` / `TypedProducer` / `TypedConsumer<T>` (`src/streaming.rs`): typed JSON produce/consume. They can be reused for social-items, and the consumer needs a variant that attaches SmartModule transforms.
- `NerEngine::detect(&str)` (`src/scorer.rs`): the NER call can be reused unchanged on social text.
- `output::save_json`: already exists for writing a JSON array to a file.

### Established Patterns
- Records are JSON via serde over Fluvio, and the SmartModules duplicate the record structs locally (no shared crate). D-06 depends on this loose coupling.
- Errors use `miette::Result` with `miette::miette!` strings.
- Commands live in `src/commands/*.rs` and are dispatched from `main.rs`. Note: `main.rs` contains large commented-out duplicates of the command bodies (dead code, out of scope).
- SmartModules are built with Rust 1.85 and smdk. D-01 and D-06 avoid touching them.

### Integration Points
- News producers: `commands/produce.rs` (crawler) and `run_file` in `main.rs`. Both switch from `RawPage` to `SourceItem`.
- Consumer: `commands/consume.rs` switches from reading `TOPIC_ARTICLES` as `ExtractedArticle` to reading raw-pages with the chain plus social-items, then mapping both into the unified output record.
- Phase 3 host will produce social `SourceItem`s to `filimon-social-items` (D-01), so the topic name and `SourceItem` type must be importable by host code.

</code_context>

<specifics>
## Specific Ideas

- Use serde's default unknown-field tolerance so a `SourceItem` with top-level `url` and `html` passes through `map-preprocess` unchanged (D-06).
- Show news `platform` as the site host so output can be grouped by site.

</specifics>

<deferred>
## Deferred Ideas

None. The discussion stayed within phase scope. (The existing gaps noted in the scout, such as silent deserialization drops in `consume_batch` and no NER in streaming mode, stay out of scope per the milestone decision.)

</deferred>

---

*Phase: 02-sourceitem-pipeline*
*Context gathered: 2026-11-04*
