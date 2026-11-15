---
status: complete
phase: 05-fetcher-layer
source: [05-VERIFICATION.md]
started: 2026-11-11T05:17:16Z
updated: 2026-11-11T05:24:58.004Z
---

## Current Test

[testing complete]

## Tests

### 1. Live crawl through a fetcher component (roadmap criterion 4)
expected: With a Fluvio cluster up and `[crawler] fetcher = "http"`, `fili produce <url>` fetches via the component and the consumer receives the articles
result: pass

### 2. Ctrl-C during a fetcher-component crawl
expected: Only complete records in the topic
result: pass

### 3. WR-01 decision: explicit non-existent registry path (prohibition 05-07)
expected: Decide whether `--registry typo.toml` or a bad `FILI_REGISTRY` must be an error. Today it silently selects the built-in client, which skips certificate validation (review finding WR-01). Either accept the literal plan wording or rule that an explicit path must error.
result: pass

### 4. Confirm prohibition verdicts for 05-01 and 05-04
expected: No silent fetcher substitution; no headers, bodies, url paths or queries in logs or error text. Verifier's non-authoritative verdict: both hold.
result: pass

### 5. Optional: net_ tests with internet access
expected: `cargo test -- --ignored net_` — the public HTTPS site succeeds with validation on; badssl invalid-certificate hosts are rejected
result: pass

## Summary

total: 5
passed: 5
issues: 0
pending: 0
skipped: 0
blocked: 0

## Gaps
