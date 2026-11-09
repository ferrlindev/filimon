---
status: complete
phase: 03-component-host-runtime
source: [03-VERIFICATION.md]
started: 2026-11-09T12:08:20Z
updated: 2026-11-09T15:23:51.423Z
---

## Current Test

[testing complete]

## Tests

### 1. Confirm the HOST-01 edge: 'nothing unlisted is loaded' is satisfied by registry-only loading (no directory scan; a sibling .wasm not in filimon.toml and a listed-but-disabled component are never opened)
expected: User agrees this wording and its test proof (loader records every opened path; sibling and disabled files absent from it; no read_dir/glob anywhere under src/plugins/host) is the intended meaning, including that a listed component whose plugin TOML failed to load is also not opened
result: pass

### 2. Confirm the HOST-05 isolation edge, including the known spinning-guest gap
expected: User accepts that isolation means: a trap, guest error, host-side sink error, or task panic in one plugin never stops the process or other plugins; a guest that tries to grow memory past the fixed 64 MiB per-instance cap is refused and rebuilt like a trap; a spinning guest is NOT terminated while the host runs (it yields every epoch tick, never blocks siblings or shutdown, but stays stuck in its poll and is never polled again); there are NO configurable per-plugin CPU/memory limits (v2 HOST-06), no CPU limit at all, no log-rate limit, and no cross-poll dedupe (v2 HOST-07)
result: pass

### 3. Triage the 8 still-open review findings (03-REVIEW-DISPOSITION.md): WR-02 epoch-ticker spawn failure is swallowed, WR-04 item URL scheme unrestricted, WR-05 guest-chosen platform can impersonate another plugin / ambiguous key, WR-07 empty --state-dir resolves to cwd, and info IN-01 (Unicode line/bidi controls survive sanitising), IN-02 (invalid RUST_LOG dropped silently, drop warnings unbounded), IN-03 (state file without a cursor key reads as first poll), IN-04 (ignored wasm/live tests, no CI). WR-01, WR-03 and WR-06 are already fixed.
expected: Each open finding is marked fixed, deferred (with a target phase) or declined in 03-REVIEW-DISPOSITION.md; recommended before Phase 4 builds on the host: WR-02 and WR-07
result: pass

## Summary

total: 3
passed: 3
issues: 0
pending: 0
skipped: 0
blocked: 0

## Gaps
