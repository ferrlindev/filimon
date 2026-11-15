# Phase 2: SourceItem Pipeline - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-11-04
**Phase:** 02-sourceitem-pipeline
**Areas discussed:** Topic routing, News envelope shape

---

## Topic routing

**Q: How do social items reach the consumer without being filtered?**

| Option | Description | Selected |
|--------|-------------|----------|
| Separate topic | New filimon-social-items topic, chain skipped, SmartModules untouched | ✓ |
| Shared topic, SM branch | All 3 SMs learn SourceItem and branch on source type | |
| Shared in, split out | One ingest topic, chain branches on type | |

**Q: What moves news records from raw-pages into articles?**

| Option | Description | Selected |
|--------|-------------|----------|
| Consumer applies chain | fili consume reads raw-pages with transforms attached | ✓ |
| Keep articles topic | A connector or stage writes filimon-articles | |
| You decide | Research picks | |

**Q: How does `fili consume` handle both sources?**

| Option | Description | Selected |
|--------|-------------|----------|
| Both by default | Concurrent, --limit caps total, optional --source | ✓ |
| Both, limit per source | --limit N from each | |
| Explicit flag required | --include-social | |

**Q: Who creates the social-items topic?**

| Option | Description | Selected |
|--------|-------------|----------|
| ensure_topics() | Auto-create | |
| Manual / README | fluvio topic create | ✓ |

**Q: What happens to filimon-articles?**

| Option | Description | Selected |
|--------|-------------|----------|
| Leave it, unused | Smallest diff | |
| Remove it | Delete constant, creation, doc refs | ✓ |
| You decide | | |

---

## News envelope shape

**Q: How does news become a SourceItem with the SmartModules untouched?**

| Option | Description | Selected |
|--------|-------------|----------|
| SourceItem + html field | Extra html field; serde ignores unknown fields so map-preprocess reads RawPage unchanged | ✓ |
| Html in `raw` | Breaks map-preprocess; overloads raw | |
| Convert at output only | raw-pages stays RawPage | |

**Q: What does a news record look like in the final output?**

| Option | Description | Selected |
|--------|-------------|----------|
| SourceItem + enrichment | One schema; article-only fields in a nested block | ✓ |
| ExtractedArticle + labels | Two schemas | |
| You decide | | |

**Q: What do source_type, platform, and id hold for news?**

| Option | Description | Selected |
|--------|-------------|----------|
| news / site domain / URL | | ✓ |
| news / "web" / URL | | |
| news / domain / URL hash | | |

**Q: How is published handled?**

| Option | Description | Selected |
|--------|-------------|----------|
| Best-effort parse | RFC 3339 or null; original string kept in article block | ✓ |
| Pass string through | | |

---

## Claude's Discretion

- JSON output target (file, stdout, or both). The area was offered but not selected.
- Social NER input and truncation. The area was offered but not selected.
- Social `raw` embedding, SourceItem module location, stream merge mechanics, Phase 2 test produce path for social items.

## Deferred Ideas

None.
