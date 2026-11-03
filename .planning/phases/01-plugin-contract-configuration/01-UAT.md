---
status: testing
phase: 01-plugin-contract-configuration
source: [01-VERIFICATION.md]
started: 2026-11-03T12:25:00Z
updated: 2026-11-03T12:25:00Z
---

## Current Test

number: 1
name: "Prohibition (test-tier, 01-01): no WIT parameter, record field or import carries an OAuth refresh token; the only per-poll credential is `access-token: option<string>`"
expected: |
  Reviewer reads wit/*.wit and confirms. Verifier re-ran `grep -niE 'refresh[a-z-]*\s*:' wit/*.wit` (no match); wit/ unchanged since prior verification
awaiting: user response

## Tests

### 1. Prohibition (test-tier, 01-01): no WIT parameter, record field or import carries an OAuth refresh token; the only per-poll credential is `access-token: option<string>`
expected: Reviewer reads wit/*.wit and confirms. Verifier re-ran `grep -niE 'refresh[a-z-]*\s*:' wit/*.wit` (no match); wit/ unchanged since prior verification
result: [pending]

### 2. Prohibition (test-tier, 01-01): no fetch-contract field or option disables or weakens TLS certificate validation
expected: Reviewer confirms `record request` in wit/types.wit is exactly method/url/headers/body (verifier saw lines 55-60). `grep -niE 'insecure|skip[-_]?verify|danger|verify|cert' wit/*.wit` has no match
result: [pending]

### 3. Backstop truth (01-02 Edge CONF-01/concurrency, reaffirmed by 01-05 F1): each config file is read exactly once and both the parsed value and its NamedSource come from that one String
expected: Exactly three read_to_string calls: config.rs:211 (registry), config.rs:500 (plugin file), secrets.rs:69 (.env); each parse and diagnostic source is built from that text
result: [pending]

## Summary

total: 3
passed: 0
issues: 0
pending: 3
skipped: 0
blocked: 0

## Gaps
