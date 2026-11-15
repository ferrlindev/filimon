---
phase: 03-component-host-runtime
plan: 07
subsystem: plugin-host
tags: [wasmtime, component-model, loader, wasi-policy, fetcher-world]
requires:
  - phase: 03-component-host-runtime
    provides: "03-04: load_components, LoadReport, HostEngine, source linker"
provides:
  - "Registry-only component opening: only enabled sources and fetchers are opened, every opened path recorded in LoadReport.opened"
  - "Import preflight: any import outside filimon:plugin/ (WASI included) rejected with HostError::ForeignImports naming the plugin, file and imports"
  - "fetcher world bindings (fetcher_world), HostEngine::fetcher_linker, outbound-http::send answering not-configured"
  - "Enabled fetchers loaded as FetcherPre into LoadReport.fetchers / fetcher_names; wrong-world components rejected with HostError::WrongWorld"
affects: [03-09, 03-10, phase-05-fetcher]
tech-stack:
  added: []
  patterns:
    - "second bindgen! with `with:` pointing types and log at source_world so HostState impls are written once"
    - "preflight via component.component_type().imports(engine) before instantiate_pre"
key-files:
  created: []
  modified:
    - src/plugins/host/loader.rs
    - src/plugins/host/engine.rs
    - src/plugins/host/error.rs
key-decisions:
  - "WASI is rejected, not linked (RESEARCH Discretion 1): smallest attack surface; allowing a locked-down WASI later is additive"
  - "A failed fetcher is reported in LoadReport.failures and does not stop sources this phase (RESEARCH A7, a judgement not a user decision)"
  - "wasmtime reason text in ComponentLoad and WrongWorld goes through clean_guest_text (512 chars); foreign import names 128 chars, list capped at 16 plus an '...and N more' marker"
requirements-completed: [HOST-01]
status: complete
commits: 2
plan_head_before: 0eff1cb38e8613d7ba6488e657314c6e84cb8afb
plan_head_after: f6f4418
actuals:
  tokens: 12000
  tasks: 2
  commits: 2
duration: 25 min
completed: 2026-11-09
---

# Phase 3 Plan 07: Registry-only loading, WASI rejection and fetcher loading Summary

Loading is registry-only and world-checked: unlisted and disabled files are never opened, a component importing WASI (or anything outside `filimon:plugin/`) is rejected with a named error before instantiation, enabled fetchers load against the `fetcher` world, and wrong-world components are rejected with `WrongWorld`.

## Accomplishments

- `error.rs`: `ForeignImports` (code `fili::host::foreign_import`, help points at wasm32-unknown-unknown and plugins/README.md) and `WrongWorld` (code `fili::host::wrong_world`); neither carries a source-code block.
- `engine.rs`: `fetcher_world` bindgen (types and log shared with `source_world`), `outbound_http::Host for HostState` returning `Err(not-configured)`, `fetcher_linker` on `HostEngine`. Still no WASI interface linked.
- `loader.rs`: `foreign_imports`, `describe_foreign`, `preflight_imports`, enabled-fetcher loading, `LoadReport.fetchers`, populated `fetcher_names`, failures collected per component. No `read_dir` or glob in the file.
- Tests: all 6 non-ignored tests and all 5 ignored wasm tests pass. The WASI component is a 46-byte binary built with `wasm-tools parse` from `(component (import "wasi:cli/environment@0.2.0" (instance)))`, embedded as a const.

## Task Commits

1. Task 1 (registry-only opening, foreign-import preflight, error variants; the loader also contains Task 2's fetcher loading and tests since both live in `loader.rs`): `50ae13f`
2. Task 2 (fetcher world bindings, linker, outbound-http impl in `engine.rs`): `f6f4418`

Note: the first commit holds the whole `loader.rs`, including the fetcher loading that depends on the `engine.rs` change in the second commit, so `50ae13f` alone does not compile. HEAD does.

## Deviations from Plan

**1. [Rule 2 - Hardening] ComponentLoad reason now sanitized**
- `ComponentLoad.reason` (from plan 03-04) previously used raw `format!("{e:#}")`; wasmtime text can echo guest names, so it now passes through `clean_guest_text(.., 512)` per threat T-3-23. Display still contains plugin name and path.

**2. [Style] `describe_foreign` takes `&[String]`**
- The plan listed it as taking `Vec<String>`; clippy flagged needless pass-by-value, so it borrows.

No other deviations.

## Verification

- `cargo test -p filimon plugins::host::loader`: 6 passed, 5 ignored.
- `cargo test -p filimon plugins::host::loader -- --ignored` (stub-source and stub-fetcher built in this worktree with `cargo component build --release`): 5 passed.
- `cargo test -p filimon plugins::host`: 16 passed, 7 ignored; ignored set: 7 passed.
- `mod.rs` untouched; `read_dir` and `glob` absent from `loader.rs`.
- `cargo clippy -p filimon --all-targets`: no warnings from lines added by this plan. Remaining `src/plugins/host/` warnings are pre-existing (`mod.rs for_registry`, `engine.rs plugin` unused method, `engine.rs:241` redundant closure in an existing test, `sink.rs`, `testing.rs`), so the plan's literal "no line starting with src/plugins/host/" criterion is not met; deferred as out of scope.

## Known Stubs

- `outbound-http::send` returns `not-configured` by design (real HTTP is Phase 5).

## Flagged Items

- HOST-01 edge (probe `unclassified`) stays unresolved and flagged for human review per the plan: registry-only loading is proven by the `opened` assertions and the absence of `read_dir`.
- RESEARCH A7 (failed fetcher does not stop sources) is a judgement; Phase 5 decides whether dependants are disabled.

## Threat Flags

None. No new network surface; mitigations T-3-20 to T-3-23 implemented as planned.

## Self-Check: PASSED

- src/plugins/host/loader.rs, engine.rs, error.rs: FOUND (modified)
- Commits 50ae13f and f6f4418: FOUND in HEAD history
