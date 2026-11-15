# Phase 8: Integration, Live E2E & Authoring - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-11-12
**Phase:** 8-Integration, Live E2E & Authoring
**Areas discussed:** Replay & Fluvio harness, Live mode gating, Live E2E run & evidence, Authoring: template & WIT distribution

---

## Replay & Fluvio harness

| Question | Options | Selected |
|----------|---------|----------|
| Fixture delivery | Replay fetcher plugin / Local mock HTTP server / Host test hook | Replay fetcher plugin |
| Fluvio topic | Require local cluster, skip if absent / Start cluster in harness / Fail if absent | Require local cluster, skip if absent |
| .wasm build | cargo component in tests, cached / Prebuilt step / Commit built .wasm | cargo component in tests, cached |

## Live mode gating

| Question | Options | Selected |
|----------|---------|----------|
| Run/skip | Runtime skip on missing keys / Keys + FILI_LIVE=1 / #[ignore] | Runtime skip on missing keys |
| Credentials | App-only instances / Include PKCE if tokens stored | App-only instances |
| Test shape | One body, two backends / Separate tests | One body, two backends |

## Live E2E run & evidence

| Question | Options | Selected |
|----------|---------|----------|
| Execution | Human-run scripted checkpoint / Automated ignored test | Human-run scripted checkpoint |
| Evidence | Sanitised JSON + transcript / Transcript only / Nothing | Sanitised JSON + transcript |
| Reddit blocked | Require both, escalate / YouTube-only acceptable | Require both, escalate |

## Authoring: template & WIT distribution

| Question | Options | Selected |
|----------|---------|----------|
| Template | plugins/template crate / cargo-generate / Snippets only | plugins/template crate |
| WIT source | Copy from tag or git dep / WIT registry / Vendored in template | Copy from tag or git dep |
| Guide + proof | docs/plugin-guide.md + script / plugins README / Manual only | docs/plugin-guide.md + script |
| Fetcher proof | Reuse replay fetcher / Separate example | Reuse replay fetcher |

User chose the recommended option on every question.

## Claude's Discretion

Test layout and helper names, runbook format, replay URL-to-fixture mapping, guide structure.

## Deferred Ideas

WIT registry publishing; automated live E2E with news; PKCE live tests; CI workflows.
