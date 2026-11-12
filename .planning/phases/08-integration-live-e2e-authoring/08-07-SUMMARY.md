---
phase: 08-integration-live-e2e-authoring
plan: 07
subsystem: testing
tags: [wit, authoring, git-tag, evidence, verify-authoring]

requires:
  - phase: 08-integration-live-e2e-authoring
    provides: "plan 08-02 authoring proof script; plan 08-05 plugin guide, template and stub-fetcher fixture"
provides:
  - "Signed annotated tag wit-v0.1.0 on origin, created and pushed by the user, pointing at 31c1dde"
  - "User-run authoring proof transcripts (local and from the tag), both ending AUTHORING PROOF: PASS (legs: source fetcher)"
  - "evidence/authoring-README.md indexing tag, tagged commit, commands and transcripts for 08-VERIFICATION.md"
affects: [08-VERIFICATION, phase-08-verification]

actuals:
  tokens: 900
  tasks: 2
  commits: 1
plan_head_before: 31c1deddebc855e5bc5656a89aa16f650d5acbe0
plan_head_after: e6060b9479ff77b665d077a251c0cb330cb67954

tech-stack:
  added: []
  patterns: ["contract published as an immutable git tag; authors pin the tag name"]

key-files:
  created:
    - .planning/phases/08-integration-live-e2e-authoring/evidence/authoring-proof-local.txt
    - .planning/phases/08-integration-live-e2e-authoring/evidence/authoring-proof-remote.txt
    - .planning/phases/08-integration-live-e2e-authoring/evidence/authoring-README.md
  modified: []

key-decisions:
  - "Tag wit-v0.1.0 is final and is never moved, deleted or force-pushed; a fix is a new tag plus a guide edit"
  - "plugins/stub-fetcher is the fetcher fixture the tag carries, replacing the plan's plugins/replay-fetcher (plan 08-05 Task 1 answer)"

requirements-completed: [WIT-03]

coverage:
  - id: D1
    description: "Tag wit-v0.1.0 exists locally and on origin, carries wit/, plugins/template, plugins/stub-fetcher, docs/plugin-guide.md and scripts/verify-authoring.sh, and its wit/ equals the working tree's"
    requirement: WIT-03
    verification:
      - kind: other
        ref: "git ls-remote --tags origin wit-v0.1.0; git cat-file -e wit-v0.1.0:<path>; git diff --quiet wit-v0.1.0 -- wit"
        status: pass
    human_judgment: false
  - id: D2
    description: "User's own run of scripts/verify-authoring.sh, locally and from the tag, ends with AUTHORING PROOF: PASS (legs: source fetcher) (SC4)"
    requirement: WIT-03
    verification:
      - kind: other
        ref: ".planning/phases/08-integration-live-e2e-authoring/evidence/authoring-proof-local.txt and authoring-proof-remote.txt (grep of PASS and mode lines)"
        status: pass
    human_judgment: true
    rationale: "The SC4 proof is defined as the user's own witnessed run (D-12); the transcripts are checked mechanically here but the witnessing is the user's"

duration: 4min
completed: 2026-11-12
status: complete
---

# Phase 8 Plan 07: WIT contract tag and authoring proof evidence Summary

**Signed tag wit-v0.1.0 on origin carrying the WIT, template and stub fetcher, with user-run local and from-the-tag authoring proofs both ending AUTHORING PROOF: PASS (legs: source fetcher).**

## Performance

- **Duration:** 4 min (continuation after the user completed the human-action tasks)
- **Completed:** 2026-11-12T11:02:32Z
- **Tasks:** 2 of 2 (both human-action checkpoints resolved by the user)
- **Files modified:** 3 created

## Accomplishments

- Task 1 (tag): `wit-v0.1.0` is a signed annotated tag, tag object `cd7e905cd7fabc32e37d01b0ddb4f76db3f6bd05`, pointing at `31c1deddebc855e5bc5656a89aa16f650d5acbe0`. The user created and pushed it; this run re-confirmed it with `git ls-remote --tags origin wit-v0.1.0` and `git rev-list -n 1 wit-v0.1.0`. The tag contains `wit/worlds.wit`, `plugins/template/Cargo.toml`, `plugins/stub-fetcher/Cargo.toml`, `docs/plugin-guide.md` and `scripts/verify-authoring.sh`, `git diff --quiet wit-v0.1.0 -- wit` is clean, and the guide's `<!-- wit-tag: wit-v0.1.0 -->` line matches.
- Task 2 (proof): the user ran `scripts/verify-authoring.sh` in their own shell in local mode and with `WIT_REF=wit-v0.1.0`. This run re-ran the plan's Task 2 automated verify: both transcripts contain `AUTHORING PROOF: PASS (legs: source fetcher)`, the local one `mode: local (git archive HEAD wit)`, the remote one `mode: remote wit-v0.1.0`, and the README names `authoring-proof-remote.txt`. The transcripts contain no FAIL line and no secret-like strings.
- `evidence/authoring-README.md` written with tag, tagged commit, UTC date, both commands and both transcript names. The transcripts were not modified or regenerated.

## Task Commits

1. **Task 1: tag creation and push** - no repository commit by this plan; the tag was created and pushed by the user (tag object `cd7e905`, commit `31c1ded`)
2. **Task 2: authoring proof evidence** - `e6060b9` (docs): three evidence files, committed by explicit path

**Plan metadata:** the SUMMARY commit follows this one (not counted in `actuals.commits`, which measures `31c1ded..e6060b9`).

## Files Created/Modified

- `.planning/phases/08-integration-live-e2e-authoring/evidence/authoring-proof-local.txt` - user's local-mode transcript (PASS)
- `.planning/phases/08-integration-live-e2e-authoring/evidence/authoring-proof-remote.txt` - user's transcript from tag wit-v0.1.0 (PASS)
- `.planning/phases/08-integration-live-e2e-authoring/evidence/authoring-README.md` - index for 08-VERIFICATION.md

## Decisions Made

- The tag name `wit-v0.1.0` stays fixed; the prohibition against moving, deleting or force-pushing it stands. Outside authors pin the name.
- The fetcher leg is built from `plugins/stub-fetcher`; `plugins/replay-fetcher` named in the plan does not exist in the tagged commit.

## Deviations from Plan

**1. [Orchestrator decision] Sequential execution on the main working tree**
- **Found during:** continuation of Task 2
- **Issue:** An earlier executor ran this plan in a throwaway worktree and wrote a halted SUMMARY there; it was never merged and does not exist here. The user's Task 2 transcripts landed in the main checkout (untracked evidence directory), so the orchestrator chose to finish the plan sequentially on the main tree.
- **Fix:** This run wrote the one real SUMMARY and committed the evidence by explicit path. The two unrelated tracked modifications (`.planning/config.json`, `.planning/phases/06-host-managed-oauth/06-VERIFICATION.md`) were left unstaged.

**2. [Plan 08-05 effect] plugins/stub-fetcher instead of plugins/replay-fetcher**
- **Found during:** Task 1 / Task 2 context
- **Issue:** Plan 08-05 Task 1 was answered "walkthrough fresh-from-stub", so the fetcher fixture is `plugins/stub-fetcher`. This plan's truths, the tag message text and the must-have file list name `plugins/replay-fetcher`. The tag message actually reads `filimon:plugin@0.1.0 authoring contract: wit/, plugins/template, plugins/stub-fetcher, docs/plugin-guide.md`, and the tag contents were checked against `plugins/stub-fetcher`.
- **Impact:** None on the proof; the script's fetcher leg passed from the tag with this fixture. Literal reading of the plan truth "contains plugins/replay-fetcher" is not met, by design of the 08-05 decision.

**3. [Evidence provenance] The remote proof was run more than once**
- The previous executor ran the remote proof once, and the user ran it once. The SC4 evidence is the user's own run (the two committed transcripts); this run did not re-execute `scripts/verify-authoring.sh` (the transcripts are the user's and were not regenerated). The plan's Task 1 automated verify (remote run from the tag) was therefore satisfied by the user's and the earlier executor's runs, not re-run here.

**Total deviations:** 3 documented (0 code fixes). **Impact on plan:** none on WIT-03 evidence; Task 1 and Task 2 were human-action checkpoints and were completed by the user.

## Issues Encountered

None in this run. Plan 08-06 (live E2E) is not part of this plan and is still pending.

## User Setup Required

None beyond what the user already did (push access to origin for the tag).

## Next Phase Readiness

- WIT-03 evidence is complete and indexed in `evidence/authoring-README.md` for 08-VERIFICATION.md.
- Phase 08 is not complete until plan 08-06 (live E2E) runs. STATE.md and ROADMAP.md were not touched by this run; the orchestrator owns those writes.

## Known Stubs

None.

## Threat Flags

None. No new network endpoints, auth paths or trust-boundary schema changes. T-08-26 and T-08-28 mitigations held: tag contents checked, tag not moved. No tag, push or git-config operation was performed by this run.

## Self-Check: PASSED

- FOUND: evidence/authoring-proof-local.txt, authoring-proof-remote.txt, authoring-README.md
- FOUND: commit e6060b9 is an ancestor of HEAD
- FOUND: tag wit-v0.1.0 on origin at cd7e905 -> 31c1ded

---
*Phase: 08-integration-live-e2e-authoring*
*Completed: 2026-11-12*
