---
phase: 03-component-host-runtime
plan: 02
subsystem: plugins
tags: [wasm, component-model, test-fixture, fault-injection, cargo-component]
requires:
  - phase: 01-plugin-contract
    provides: "wit/ source world, stub-source scaffold and audited dependency set"
provides:
  - "plugins/fault-source wasm component with nine fault modes for host-level backoff and isolation tests"
affects: [03-06, 03-10]
tech-stack:
  added: []
  patterns: ["per-instance thread_local state in a stateless-by-contract guest", "own workspace + toolchain pin + wasm32-unknown-unknown target per plugin crate"]
key-files:
  created:
    - plugins/fault-source/Cargo.toml
    - plugins/fault-source/Cargo.lock
    - plugins/fault-source/rust-toolchain.toml
    - plugins/fault-source/.cargo/config.toml
    - plugins/fault-source/.gitignore
    - plugins/fault-source/src/lib.rs
  modified:
    - plugins/README.md
key-decisions:
  - "Modes are selected from [settings] mode; after_polls added so tests can say 'ok once, then trap' and observe the counter restart on a rebuilt instance"
requirements-completed: []
requirements-supplied-fixture-for: [HOST-03, HOST-05]
status: complete
commits: 2
plan_head_before: c09e8e12c1036eeb2a0aeaef91cde0f7d11fa596
plan_head_after: d69a45c
actuals:
  tokens: 4500
  tasks: 2
  commits: 2
duration: 12 min
completed: 2026-11-09
---

# Phase 3 Plan 02: fault-source fixture Summary

Fault-injection `source` world component (nine modes, per-instance poll counter, `after_polls`) built on its own pinned 1.96.0 toolchain, conforming to the world with no WASI imports and the same dependency set as stub-source.

## Accomplishments

- `plugins/fault-source` builds with `cargo component build --release` to `target/wasm32-unknown-unknown/release/fault_source.wasm` (gitignored, confirmed with `git check-ignore`).
- Modes: `ok`, `rate-limited`, `transient`, `auth-required`, `fatal`, `config`, `init-fatal`, `trap` (panic -> wasm trap), `spin` (black_box loop). Unknown settings keys and unknown modes return `PollError::Config`.
- `ok` mode returns item `f{n}` (platform `fault`) and echoes the incoming cursor (`[0]` when none). `after_polls` makes early polls behave as `ok`.
- `secrets` is bound as `_secrets` and never used; log lines contain only the mode name and counters.
- `wasm-tools component targets ../../wit -w source` exits 0; component imports are only `filimon:plugin/types` and `filimon:plugin/log` (no `wasi:`).
- `plugins/README.md` documents the fixture, labels it a test fixture, and adds the build line and output path. `examples/plugins/filimon.toml` is untouched.

## Task Commits

1. Task 1: fixture crate - `0b924f4`
2. Task 2: README documentation (conformance checks are verification-only) - `d69a45c`

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Lock-parity gate is order-sensitive**
- **Found during:** Task 2 verify
- **Issue:** The planned gate `diff <(sed 's/stub-source/ROOT/' ...) <(sed 's/fault-source/ROOT/' ...)` fails after the build, because cargo re-sorts `Cargo.lock` package entries alphabetically and `fault-source` sorts before `getrandom` while `stub-source` sorts after `serde`/`syn`. The dependency content is identical; only the root block moved.
- **Fix:** Kept the cargo-canonical lock (restoring stub order would be rewritten on the next build). Verified parity order-insensitively: 19 package blocks each, sorted and compared with root names normalised, equal. No dependency version differs from stub-source.
- **Files modified:** none beyond the lock cargo wrote
- **Commit:** n/a (verification method only)

**Total deviations:** 1 auto-fixed (1 blocking verification-gate defect). **Impact:** none on the delivered artifacts. Later plans that re-run the literal `diff` gate will see an ordering diff; use the order-insensitive comparison.

## Worktree notes

The cwd-drift sentinel and plan-head ledger files under the worktree git dir could not be written (the sandbox refuses git-dir access from a worktree-isolated agent). `commits:` is measured from the dispatch base `c09e8e1` supplied in the prompt: `git rev-list --count c09e8e1..HEAD` = 2 before this SUMMARY commit.

## Known Stubs

None.

## Threat Flags

None. T-3-02 accepted (fixture never registered), T-3-03 mitigated (secrets unused, no settings/secret/cursor logging), T-3-SC mitigated (no new dependency version).

## Flagged Assumptions

HOST-05 "traps or returns an error" coverage is judged in plans 03-06 and 03-10; this plan only supplies the fixture. Requirements are not marked complete here.

## Self-Check: PASSED

- FOUND: plugins/fault-source/{Cargo.toml,Cargo.lock,rust-toolchain.toml,.cargo/config.toml,.gitignore,src/lib.rs}
- FOUND commits: 0b924f4, d69a45c
