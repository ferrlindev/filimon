---
phase: 03-component-host-runtime
plan: 03
subsystem: infra
tags: [rust-toolchain, wasmtime, smartmodule, smdk, cargo]
requires:
  - phase: 03-component-host-runtime
    provides: "03-01: machine-local rustup override removed so the root pin takes effect"
provides:
  - "Root toolchain pin 1.96.0 and rust-version 1.95"
  - "wasmtime 48.0.8 (six explicit features), base64 0.22, tokio-util 0.7 in Cargo.toml and Cargo.lock"
  - "Nested 1.85.0 pins for the three SmartModule crates, proven by smdk build"
  - ".fili-state/ ignore rule"
affects: [03-04, 03-05, 03-06, 03-07, 03-08, 03-09, 03-10]
tech-stack:
  added: [wasmtime 48.0.8, base64 0.22, tokio-util 0.7]
  patterns: ["per-directory rust-toolchain.toml pins: root 1.96.0, smartmodules 1.85.0"]
key-files:
  created:
    - rust-toolchain.toml
    - smartmodules/filter-relevance/rust-toolchain.toml
    - smartmodules/map-preprocess/rust-toolchain.toml
    - smartmodules/map-extract/rust-toolchain.toml
  modified:
    - Cargo.toml
    - Cargo.lock
    - .gitignore
    - README.md
key-decisions:
  - "wasmtime resolves to 48.0.8; cargo notes v50.0.1 exists but needs Rust 1.97, so 48 LTS is kept as planned"
requirements-completed: []
requirements-enabled: [HOST-01, HOST-02, HOST-03, HOST-04, HOST-05]
status: complete
commits: 2
plan_head_before: d39338a347b15c15f86833bb04f57566553b8b99
plan_head_after: e9dd1a9
actuals:
  tokens: 6000
  tasks: 2
  commits: 2
duration: 15 min
completed: 2026-11-09
---

# Phase 3 Plan 03: Toolchain split and wasmtime 48 dependencies Summary

The host crate now builds on a repo-pinned Rust 1.96.0 with wasmtime 48.0.8 locked, while each SmartModule directory builds on its own 1.85.0 pin (proven by a real `smdk build`).

## Accomplishments

- Root `rust-toolchain.toml` (1.96.0, clippy, rustfmt); `rustc --version` at root prints 1.96.0.
- `Cargo.toml`: `rust-version = "1.95"`; `wasmtime` 48 with `default-features = false` and exactly the six features; `base64 = "0.22"`; `tokio-util = "0.7"`. No WASI crate, no `wat`/`cache`/`pooling-allocator`.
- `Cargo.lock`: single `wasmtime` entry at 48.0.8, no 30.x-47.x entry.
- `cargo check -p filimon` exits 0 (11 pre-existing warnings); `cargo test -p filimon`: 134 passed, 0 failed, 8 ignored.
- Nested `rust-toolchain.toml` (1.85.0, wasm32-unknown-unknown and wasm32-wasip1) in filter-relevance, map-preprocess, map-extract; each prints `rustc 1.85.0` from inside.
- `(cd smartmodules/filter-relevance && smdk build)` exits 0 with no `undefined symbol` output.
- README: two-bullet Prerequisites, retitled section 2 (no `rustup override set`), rewritten troubleshooting entry.
- `.gitignore` ignores `.fili-state/`. Dockerfile and all SmartModule sources/manifests untouched.

## Task Commits

1. Task 1: root pin, rust-version, dependencies, ignore rule - `5f1629b`
2. Task 2: nested SmartModule pins and README - `e9dd1a9`

## Deviations from Plan

None - plan executed exactly as written. Notes: `smdk build` did run offline-capable here (all crates already cached or fetched without issue); only `map-preprocess` and `map-extract` were verified by `rustc --version` rather than a full `smdk build`, as the plan specifies.

## Known Stubs

None.

## Threat Flags

None. T-3-04 mitigated (lock gate), T-3-05 mitigated (nested pins + smdk build), T-3-06 mitigated (explicit features, grep gates), T-3-SC mitigated (lock resolves wasmtime 48.0.8 from the registry).

## Worktree notes

Git-dir sentinel/ledger files could not be used in this sandbox; `commits:` measured from the dispatch base `d39338a`: `git rev-list --count d39338a..HEAD` = 2 before this SUMMARY commit. STATE.md and ROADMAP.md were not touched (orchestrator owns them). No rustup setting was modified.

## Self-Check: PASSED

- FOUND: rust-toolchain.toml and the three smartmodules/*/rust-toolchain.toml
- FOUND commits: 5f1629b, e9dd1a9
