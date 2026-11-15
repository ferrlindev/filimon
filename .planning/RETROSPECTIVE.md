# Project Retrospective

*A living document updated after each milestone. Lessons feed forward into future planning.*

## Milestone: v1.0 — WASM Source Plugins

**Shipped:** 2026-11-15
**Phases:** 8 | **Plans:** 65 | **Tasks:** 101

### What Was Built
- Versioned WIT contract (`source` + `fetcher` worlds), `filimon.toml` registry, per-plugin TOML and strict `.env` secrets.
- Unified `SourceItem` pipeline: news on the WISE chain, social items passed through, one NER-enriched JSON output.
- Component host runtime with scheduler, persisted cursors, failure isolation, and the `fili plugins` / `fili run` CLI.
- Fetcher layer with host-enforced allowlist, timeouts and TLS validation; host-managed PKCE OAuth with host-side tokens.
- Reddit and YouTube reference plugins, a plugin template, and an authoring guide proven from outside the repo.

### What Worked
- Horizontal layers verified in isolation made the final integration phase mostly assembly.
- Review/disposition loops caught real security issues early (`$` expansion in `.env`, diagnostics leaking record content).
- A fresh milestone audit surfaced the tech-debt list before archiving.

### What Was Inefficient
- Phases 3 and 5 closed with many review warnings still open (13 of 16 in phase 3).
- SUMMARY frontmatter bookkeeping gaps (HOST-05) and draft VALIDATION.md files on phases 1, 7, 8.
- Fixtures stayed synthetic and the live Reddit proof was waived, so live coverage is YouTube-only.
- Uncommitted edits to verified artifacts left phases reading as `stale` at close.

### Patterns Established
- Record values are untrusted: diagnostics carry no record content and go through one sanitising choke point.
- Plugins target `wasm32-unknown-unknown` with every capability an explicit WIT import.
- Waivers are recorded as deferred items with a reason rather than silently dropped.

### Key Lessons
1. Commit verification artifacts before running milestone close, or readiness reports them stale.
2. Close or explicitly defer review warnings per phase; do not let the ledger grow into the milestone gate.
3. Decide history hygiene (local commits e13466b/af7aeea with author names/PERSON entities) before any push.

### Cost Observations
- Model mix: not recorded
- Sessions: not recorded

---

## Cross-Milestone Trends

| Milestone | Phases | Plans | Closeout |
|-----------|--------|-------|----------|
| v1.0 | 8 | 65 | override_closeout (tech_debt) |
