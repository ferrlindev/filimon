---
phase: 05-fetcher-layer
plan: 02
subsystem: plugins
tags: [wasm-component, cargo-component, fetcher, outbound-http]
status: complete

requires:
  - phase: 03-component-host
    provides: fetcher world, preflight check, `fili plugins check`
provides:
  - plugins/http-fetcher default HTTP fetcher component (pure pass-through to outbound-http.send)
  - sample registry entry [fetchers.http] at the built component path
affects: [05-03, 05-04, 05-08]

actuals:
  tokens: 4500
  tasks: 2
  commits: 2
plan_head_before: e6cc3f0b33f0ed374e892c502bbd5e65b0ae324f
plan_head_after: 08c1b6b

tech-stack:
  added: []
  patterns: ["thin guest, host enforces all network policy"]

key-files:
  created:
    - plugins/http-fetcher/Cargo.toml
    - plugins/http-fetcher/Cargo.lock
    - plugins/http-fetcher/src/lib.rs
    - plugins/http-fetcher/rust-toolchain.toml
    - plugins/http-fetcher/.cargo/config.toml
    - plugins/http-fetcher/.gitignore
  modified:
    - examples/plugins/filimon.toml
    - plugins/README.md

key-decisions:
  - "WD-07: default fetcher is a registry component, not embedded in the fili binary"
  - "WD-08: guest is a pure pass-through; redirects, TLS, allowlist and size caps are the host's"

requirements-completed: [FETCH-03]

duration: ~15min
completed: 2026-11-11
---

# Phase 5 Plan 02: Default HTTP Fetcher Component Summary

**`plugins/http-fetcher`: a `fetcher`-world component whose `fetch` logs one fixed line and returns `send(&request)` unchanged, built from `../../wit` alone and registered as `[fetchers.http]` in the sample registry.**

## Accomplishments

- Task 1 (tracer): new crate mirrors stub-fetcher layout. `cargo component build --release` succeeds; `wasm-tools component targets ../../wit -w fetcher` accepts it; the component imports only `filimon:plugin/{log,types,outbound-http}` and exports `filimon:plugin/fetch` (no `wasi:`). Lockfile is the stub-fetcher lockfile with the root package renamed: same three packages and identical checksums. Root `Cargo.toml`/`Cargo.lock` untouched.
- Task 2: `[fetchers.http]` now points at `../../plugins/http-fetcher/target/wasm32-unknown-unknown/release/http_fetcher.wasm`; README documents the crate, build line, landing path and conformance check (Host behavior section untouched).
- Tracer verified end-to-end before expansion.

## Verification

- `cargo component build --release` (in plugins/http-fetcher): ok.
- `wasm-tools component targets ../../wit -w fetcher ...http_fetcher.wasm`: exit 0.
- `cargo test -p filimon plugins::tests`: ok, `loads_sample_registry` and `sample_env_example_lists_every_declared_secret` pass.
- `fili --registry examples/plugins/filimon.toml plugins check http`: `component: ok`, `check passed`, exit 0.
- All task acceptance greps (send(&request), no request.url, lockfile checksums, git-ignored target and bindings.rs) pass.

## Deviations from Plan

None - plan executed exactly as written.

## Known Stubs

None.

## Threat Flags

None. T-5-07 and T-5-SC mitigations applied as specified.

## Self-Check: PASSED

- plugins/http-fetcher/src/lib.rs, Cargo.toml, Cargo.lock: FOUND
- Commits 4993c3e and 08c1b6b: in HEAD history
