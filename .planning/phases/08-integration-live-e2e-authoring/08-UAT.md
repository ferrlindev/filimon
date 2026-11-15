---
status: testing
phase: 08-integration-live-e2e-authoring
source: [08-VERIFICATION.md]
started: 2026-11-15T06:25:53.368Z
updated: 2026-11-15T06:41:44.454Z
---

## Current Test

number: 2
name: Decide the YouTube live-run evidence (CR-01)
expected: |
  Either accept the waiver (an overrides entry for SC3 and TEST-04 naming the user as accepter, reason "takes time to complete"), or reject it: set the Reddit keys in the repo-root credentials file, rerun scripts/live-e2e.sh run and the credentialed tests, and recommit the evidence.
awaiting: user response

## Tests

### 1. Rule on the Reddit waiver (D-09) for SC3 / TEST-04 and the Reddit half of SC2 / TEST-03
expected: Accept the waiver with an override entry, or supply Reddit credentials and regenerate the evidence. No Reddit record from a live poll exists in the committed evidence.
result: ACCEPTED by the user on 2026-11-15 (waiver reason, verbatim: takes time to complete). Reddit live proof is tracked as a deferred item. The overrides entry is applied when the phase is re-verified.

### 2. Decide whether the committed YouTube live-run evidence is acceptable (CR-01)
expected: Either accept that the YouTube records come from the earlier live poll (status file dated 2026-11-15T06:00:49Z, 10 items, real video ids), because the committed run's transcript did not perform a fresh poll (run.log shows youtube POLLS 0), with the README and SUMMARY claim "from this run" corrected; or fix CR-01 (stale state reuse in scripts/live-e2e.sh) and repeat the live run from an empty state.
result: DECIDED by the user on 2026-11-15: fix CR-01 and rerun once. CR-01 fixed in 508db2a (every run starts from an empty state directory). Pending: the user's rerun of the live script and the credentialed tests, then recommit the evidence with corrected claims and re-verify.

## Summary

total: 2
passed: 1
issues: 0
pending: 1
skipped: 0
blocked: 0

## Gaps
