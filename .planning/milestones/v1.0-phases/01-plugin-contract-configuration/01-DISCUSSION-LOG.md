# Phase 1: Plugin Contract & Configuration - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-11-03
**Phase:** 01-plugin-contract-configuration
**Areas discussed:** WIT data shapes, Config delivery to plugin, Registry & TOML layout, Versioning & crate layout

---

## WIT data shapes

| Question | Options | Selected |
|----------|---------|----------|
| Cursor type | option<string> (rec) / option<list<u8>> / typed record | option<list<u8>> |
| Item record | typed + JSON extras (rec) / mostly JSON blob / fully typed | typed + JSON extras |
| Timestamp | option<u64> unix ms (rec) / RFC 3339 string / wasi:clocks datetime | option<u64> unix ms |
| Errors | typed variant (rec) / plain string / code + message | typed variant |

## Config delivery to plugin

| Question | Options | Selected |
|----------|---------|----------|
| Settings | init(JSON) (rec) / init(raw TOML) / get-setting import | init gets raw TOML string |
| Secrets | get-secret import (rec) / pushed in init | pushed in init |
| Token | get-access-token import (rec) / passed into poll | passed into poll |
| Fetcher config | init(settings) (rec) / no init | no init for fetchers |
| WIT-01 wording gap (follow-up) | reword WIT-01 (rec) / keep imports too / revisit | reword WIT-01 to match |

**Notes:** Choices remove settings/secret/token imports from the `source` world; user approved rewording WIT-01 and Phase 1 SC1.

## Registry & TOML layout

| Question | Options | Selected |
|----------|---------|----------|
| Registry shape | named tables (rec) / arrays of tables / single [[plugin]] list | named tables |
| Key split | registry = wiring (rec) / host policy in registry | registry = wiring |
| Path resolution | relative to filimon.toml + --registry (rec) / CWD / XDG | relative to filimon.toml + --registry |
| [settings] validation | opaque to host (rec) / host schema | opaque to host |
| Config error UX | collect all, miette (rec) / fail fast / skip + warn | collect all, miette |

## Versioning & crate layout

| Question | Options | Selected |
|----------|---------|----------|
| WIT version | filimon:plugin@0.1.0 semver (rec) / @1.0.0 / unversioned | filimon:plugin@0.1.0 |
| Layout | wit/ root + plugins/ outside workspace (rec) / all in workspace / separate repo | wit/ root + plugins/ outside workspace |
| Stubs | minimal but exercising (rec) / compile-only | minimal but exercising |
| Secret mapping | alias map, .env next to registry (rec) / plain list / alias map, CWD .env | alias map, .env next to registry |

## Claude's Discretion

WIT interface split and naming, log import shape, fetch request/response + fetch-error cases, [host] key names/defaults, Rust module layout and crates, toolchain installation/pinning for cargo-component and wasm-tools.

## Deferred Ideas

- Fetcher init/config (for v2 headless fetcher)
- Host-side schema validation of [settings]
- Skip-bad-plugins run-mode flag
