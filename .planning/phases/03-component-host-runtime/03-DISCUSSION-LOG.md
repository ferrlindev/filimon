# Phase 3: Component Host Runtime - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-11-09
**Phase:** 3-Component Host Runtime
**Areas discussed:** Scheduling, backoff & failure; Cursor persistence

---

## Scheduling, backoff & failure

| Question | Options | Selected |
|----------|---------|----------|
| Retry-after vs backoff | Max of both, capped / Retry-after wins / Ignore retry-after | Max of both, capped |
| Backoff curve | Exponential reset on success / Slow decay / Jitter | Exponential, reset on success |
| fatal / config / auth-required | Disable fatal+config, backoff auth / Backoff all / Disable all | Disable fatal+config, backoff auth-required |
| Trap handling | Re-instantiate and re-init / Keep instance / Disable after N traps | Re-instantiate and re-init |

## Cursor persistence

| Question | Options | Selected |
|----------|---------|----------|
| Store | JSON file per plugin / Single file / SQLite / Fluvio topic | JSON file per plugin |
| State dir | `.fili-state/` next to registry / XDG / CWD | `.fili-state/` next to registry |
| Commit timing | After produce+flush / Before / Two-phase | After produce+flush |
| Corrupt cursor | Disable plugin / Start from none / Abort | Disable plugin, name file |

## Claude's Discretion

- WASI policy, host entry point, interim fetch/token imports (areas not selected for discussion)

## Deferred Ideas

- Per-plugin limits (HOST-06), dedupe (HOST-07), backoff jitter/decay
