---
phase: 07-reddit-youtube-plugins
plan: 12
subsystem: testing
tags: [fixtures, provenance, offline-gate, reddit, youtube]
requires:
  - phase: 07-reddit-youtube-plugins
    provides: "plugins and sample registry from plans 07-01 to 07-11"
provides:
  - "final fixture provenance: every Reddit and YouTube fixture labelled synthetic (D-21 fallback)"
  - "green offline Phase 7 gate: plugin suites, component builds, plugins check x4, host suite"
affects: [phase-08-live-verification]
tech-stack:
  added: []
  patterns: []
key-files:
  created: []
  modified:
    - plugins/reddit/tests/fixtures/README.md
    - plugins/youtube/tests/fixtures/README.md
key-decisions:
  - "User replied `synthetic` at the Task 1 checkpoint: no real capture, all fixtures stay synthetic (accepted D-21 fallback)"
duration: 25min
completed: 2026-11-11
status: complete
actuals:
  tokens: 2000
  tasks: 2
  commits: 1
plan_head_before: b6347e3369bd2ed79fa1ca449d842934949a5657
commits: 1
---

# Phase 7 Plan 12: Fixture provenance and offline phase gate Summary

**All Reddit and YouTube fixtures stay `synthetic` by the user's D-21 ruling; the offline gate (native suites, both components, `plugins check` for four instances, host suite) ran and is green, with two acceptance-criterion regex discrepancies reported below.**

## Tasks

1. **Task 1 (human-action checkpoint):** the user replied `synthetic`. No capture, no commit.
2. **Task 2:** both fixture READMEs now state that real capture was offered and not made, every file keeps the `synthetic` label, and the assumptions below remain unconfirmed. No code or test change was needed (no captured fixtures to reconcile). Commit `278c657`.

## Assumptions status (research Assumptions Log)

Confirmed by a real capture: none.

UNCONFIRMED by a real capture (accepted D-21 fallback): A1 (home feed endpoint and shape), A2 (saved path, `history` scope, mixed `t3`/`t1` kinds), A3 (listing shape and app-only access to `/r/{sub}/new`), A4 (error shapes), A11 (`x-goog-api-key` header form), A13 (Google error JSON nesting). Live confirmation is Phase 8 (TEST-03).

## Offline gate results (actually run)

| Check | Result |
|-------|--------|
| Reddit native suite (`cargo test --target x86_64-apple-darwin`) | pass: 34 + 2 + 59 tests, 0 failed |
| YouTube native suite | pass: 22 + 1 + 52 tests, 0 failed |
| `cargo component build --release` both plugins | pass |
| `wasm-tools component targets ../../wit -w source` both | pass |
| No `wasi:` import in either component WIT | pass (0 matches each) |
| `fili plugins check` reddit, reddit-user, youtube, youtube-user (stub secrets in env only) | all four "check passed" |
| `cargo test -p filimon` | pass: 669 + 12 + 12 passed, 0 failed |
| README labelling loops (Reddit and YouTube acceptance criteria) | pass |
| `git diff --quiet 75b078e -- wit Cargo.toml Cargo.lock` | pass (untouched) |
| Sanitisation negative search | see discrepancy 1: no secret present, but the literal command exits non-zero |
| Clippy `Finished` and no diagnostics in host files | see discrepancy 2: `Finished`, but 3 pre-existing warnings match the path filter |

## Deviations from Plan

None in code. Two acceptance criteria do not pass literally; neither indicates a defect introduced by this plan, and both need an orchestrator or user decision rather than a guess.

**1. Sanitisation regex false positive.** The case-insensitive pattern `access_token` matches `plugins/youtube/tests/replay.rs:635`, the test function name `an_access_token_is_preferred_over_the_api_key_and_sent_as_bearer`. It is an identifier, not a credential; no `AIza` key, bearer value, token field, client secret, cookie or account name exists in any fixture, README or test (all other alternatives return no match). Renaming the test would satisfy the regex but would break plan 07-08's recorded verify, which names this test explicitly, so it was not renamed. Decision for the user: rename the test (and update the 07-08 verify reference) or relax the pattern to `"access_token"` with the quote character.

**2. Clippy path filter hits pre-existing warnings.** `src/plugins/config.rs:1503,1505,1506` (`Duration::from_secs(120)`, `from_secs(60)`, `from_secs(3600)`, a `duration_suboptimal_units` style lint from the installed clippy). Those lines were authored in commit `92607d7` (phase 6, unchanged by this phase), so they are out of scope under the scope boundary and were not fixed. The plan's stated baseline of 0 does not hold for the installed toolchain. Deferred.

## Known Stubs

None added. All fixtures are intentionally synthetic per D-21 (documented in both READMEs).

## Threat Flags

None. No new network endpoint, auth path or trust-boundary change; no secret was written to the repository (stub secret values were passed only through the process environment of the `plugins check` commands, no `.env` created).

## Self-Check: PASSED

- plugins/reddit/tests/fixtures/README.md and plugins/youtube/tests/fixtures/README.md modified and committed in 278c657.
- Commit 278c657 exists on the worktree branch.
