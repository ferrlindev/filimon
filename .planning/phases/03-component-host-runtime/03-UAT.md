---
status: testing
phase: 03-component-host-runtime
source: [03-VERIFICATION.md]
started: 2026-11-09T12:08:20Z
updated: 2026-11-09T12:08:20Z
---

## Current Test

number: 1
name: Confirm the HOST-01 edge: 'nothing unlisted is loaded' is satisfied by registry-only loading (no directory scan; a sibling .wasm not in filimon.toml and a listed-but-disabled component are never opened)
expected: |
  User agrees this wording and its test proof (loader records every opened path; sibling and disabled files absent from it; no read_dir/glob anywhere under src/plugins/host) is the intended meaning, including that a listed component whose plugin TOML failed to load is also not opened
awaiting: user response

## Tests

### 1. Confirm the HOST-01 edge: 'nothing unlisted is loaded' is satisfied by registry-only loading (no directory scan; a sibling .wasm not in filimon.toml and a listed-but-disabled component are never opened)
expected: User agrees this wording and its test proof (loader records every opened path; sibling and disabled files absent from it; no read_dir/glob anywhere under src/plugins/host) is the intended meaning, including that a listed component whose plugin TOML failed to load is also not opened
result: [pending]

### 2. Confirm the HOST-05 isolation edge, including the known spinning-guest gap
expected: User accepts that isolation means: a trap, guest error, host-side sink error, or task panic in one plugin never stops the process or other plugins; a spinning guest is NOT terminated while the host runs (it yields every epoch tick, never blocks siblings or shutdown, but stays stuck in its poll and is never polled again); no CPU/memory limits (HOST-06) and no cross-poll dedupe (HOST-07)
result: [pending]

### 3. Triage the 7 open review warnings (03-REVIEW-DISPOSITION.md, all 'open') and decide which must be fixed before Phase 4 builds on the host, in particular WR-03 (oversized record key wedge), WR-02 (silent epoch-ticker failure), WR-07 (empty --state-dir) and the non-deferred half of WR-01 (item/cursor/log caps)
expected: Each finding is marked fixed, deferred (with a target phase) or declined in 03-REVIEW-DISPOSITION.md
result: [pending]

## Summary

total: 3
passed: 0
issues: 0
pending: 3
skipped: 0
blocked: 0

## Gaps
