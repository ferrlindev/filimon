---
status: testing
phase: 04-plugin-cli
source: [04-VERIFICATION.md]
started: 2026-11-10T07:36:40Z
updated: 2026-11-10T07:36:40Z
---

## Current Test

number: 1
name: Manual UAT of fili run against the production topic
expected: |
  With the cluster up and a registry listing the stub plugin: run two or more poll cycles and see items on filimon-social-items. One Ctrl+C stops gracefully with a summary and exit 0. A restart resumes from the saved cursor. Two Ctrl+C presses abort at once.
awaiting: user response

## Tests

### 1. Manual UAT of fili run against the production topic
expected: graceful stop on one Ctrl+C (summary, exit 0), cursor resume on restart, immediate abort on a second Ctrl+C. Automation never runs this (plan prohibition T-4-24).
result: [pending]

### 2. Reproduce review finding CR-01 (piped fili run)
expected: run fili run 2>&1 | tee run.log and press Ctrl+C. The process should stop. If it ignores a second Ctrl+C or SIGTERM, CR-01 is confirmed (println! before shutdown.cancel() panics on a broken pipe) and a gap plan is needed.
result: [pending]

## Summary

total: 2
passed: 0
issues: 0
pending: 2
skipped: 0
blocked: 0

## Gaps
