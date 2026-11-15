---
phase: 08-integration-live-e2e-authoring
plan: 06
subsystem: testing
tags: [live-e2e, fluvio, youtube, gliner, ner, evidence, waiver]

requires:
  - phase: 08-integration-live-e2e-authoring
    provides: "plan 08-03 live-e2e.sh (preflight, run, check-evidence); plan 08-04 credentialed tests"
provides:
  - "Sanitised, user-run live E2E evidence: 14 NER-enriched records (11 news, 3 YouTube social, 0 Reddit)"
  - "Run transcript and credentialed test run (10 passed) with real YouTube key"
  - "WAIVER.md and evidence README.md recording the user's Reddit waiver and an index for 08-VERIFICATION.md"
affects: [08-VERIFICATION, phase-08-verification]

actuals:
  tokens: 90000
  tasks: 3
  commits: 5
plan_head_before: 85ab56a00f3100b11b1bb8533812479b4d17a572
plan_head_after: e13466be1b874ace6d47d9bb75fd6a3782c33f63

tech-stack:
  added: []
  patterns: ["human-run live proof committed as sanitised evidence with check-evidence gate"]

key-files:
  created:
    - .planning/phases/08-integration-live-e2e-authoring/evidence/enriched-sample.json
    - .planning/phases/08-integration-live-e2e-authoring/evidence/live-e2e-transcript.txt
    - .planning/phases/08-integration-live-e2e-authoring/evidence/credentialed-tests.txt
    - .planning/phases/08-integration-live-e2e-authoring/evidence/README.md
    - .planning/phases/08-integration-live-e2e-authoring/evidence/WAIVER.md
  modified:
    - scripts/live-e2e.sh

key-decisions:
  - "D-09: Reddit waived by the user (reason: takes time to complete); Reddit half of SC3 / TEST-04 left unmet for the verifier to accept or reject"
  - "News crawl in the live run goes through a generated registry with [crawler] fetcher = http"
  - "Sample is newest-first, drops Reddit under a waiver, and check-evidence requires a seed-host news record"

requirements-completed: [TEST-04, TEST-03]

coverage:
  - id: D1
    description: "Live E2E run: fili polled the real YouTube API, records flowed through local Fluvio, fili consume --enrich-ner produced a sanitised enriched sample alongside news"
    requirement: TEST-04
    verification:
      - kind: e2e
        ref: "scripts/live-e2e.sh check-evidence .planning/phases/08-integration-live-e2e-authoring/evidence"
        status: pass
    human_judgment: true
    rationale: "Reddit half of SC3 / TEST-04 is waived by the user and unmet; the verifier must accept or reject the waiver"
  - id: D2
    description: "Credentialed tests with real keys: YouTube live poll passes, Reddit skips under the waiver"
    requirement: TEST-03
    verification:
      - kind: integration
        ref: "cargo test -p filimon e2e_credentialed_ (credentialed-tests.txt: 10 passed; 0 failed)"
        status: pass
    human_judgment: true
    rationale: "Reddit positive path was skipped under the waiver; only the YouTube positive path is shown"

duration: multi-session (human-run checkpoints)
completed: 2026-11-15
status: complete
---

# Phase 8 Plan 06: Live E2E and Credentialed Evidence Summary

**Reddit waived (D-09): the user waived Reddit with the reason "takes time to complete". The Reddit half of SC3 and TEST-04 is unmet and needs an accept or reject ruling at verification.**

User-run live end-to-end proof over YouTube and news with GLiNER NER enrichment, committed as a sanitised 14-record sample, transcript, credentialed test output (10 passed), WAIVER.md and an index README.

## Performance

- **Duration:** spread over several sessions (human-run checkpoints; continuation agent resumed at Task 3)
- **Tasks:** 3 of 3 (Task 1 resolved under a waiver, Task 2 answered `waive`, Task 3 verified and committed)
- **Files modified:** 5 evidence files created, `scripts/live-e2e.sh` changed by two fix commits

## Accomplishments

- The user ran `LIVE_E2E_WAIVE_REDDIT='takes time to complete' LIMIT=100 scripts/live-e2e.sh run`; `fili` polled the real YouTube API, records flowed through the local Fluvio cluster, and `fili consume --enrich-ner` produced the sample.
- `enriched-sample.json` holds 14 records: news blog.rust-lang.org 3, example.com 2, local 3, techcrunch.com 3; social youtube 3; Reddit 0. Every record has NER entities, `raw` is null, authors are null or `redacted`.
- `scripts/live-e2e.sh check-evidence` prints `ok: evidence is complete and clean` (re-run by the executor).
- `cargo test -p filimon e2e_credentialed_` shows `test result: ok. 10 passed; 0 failed`; the only skip is `skipped: missing REDDIT_CLIENT_ID`, allowed because WAIVER.md exists.
- The repository-root credential file is not tracked by git; no credential value is in any committed file.

## Task Commits

1. **Task 1: Live prerequisites (D-07, D-09)** - no repo change (human action; resolved under a waiver)
2. **Task 2: Wait or waive Reddit (D-09)** - no repo change (user answered `waive: takes time to complete`)
3. **Task 3: Run and commit sanitised evidence** - `e13466b` (docs)

Related fix commits made during this plan to the plan 08-03 script: `0342c17`, `9391302` (see Deviations).

**Plan metadata:** committed after this file (docs: complete plan)

## Files Created/Modified

- `.planning/phases/08-integration-live-e2e-authoring/evidence/enriched-sample.json` - sanitised enriched sample
- `.planning/phases/08-integration-live-e2e-authoring/evidence/live-e2e-transcript.txt` - run transcript
- `.planning/phases/08-integration-live-e2e-authoring/evidence/credentialed-tests.txt` - credentialed test run
- `.planning/phases/08-integration-live-e2e-authoring/evidence/README.md` - index with date, commands, counts, waiver and provenance
- `.planning/phases/08-integration-live-e2e-authoring/evidence/WAIVER.md` - Reddit waiver record
- `scripts/live-e2e.sh` - crawl routing and sampling fixes (commits 0342c17, 9391302)

## Decisions Made

- Reddit waived under D-09 with the user's reason verbatim; no replay or synthetic data substitutes for Reddit.
- Evidence is accepted only through `check-evidence`, which now also requires seed-host news and no Reddit in a waived sample.

## Deviations from Plan

1. **Sequential execution on the main working tree.** By orchestrator decision; the artifacts live in the main checkout (see 08-07-SUMMARY deviation 1).
2. **Task 1 verify could not pass literally under the waiver.** It requires `ok: REDDIT_CLIENT_ID is set`. Under the waiver, `LIVE_E2E_WAIVE_REDDIT='takes time to complete' scripts/live-e2e.sh preflight` passes and prints `warn: Reddit waived by the user: takes time to complete`. Recorded as a deviation, not a failure.
3. **GLiNER model source differs from the README recipe.** The model was fetched from `onnx-community/gliner_small-v2.1` (sha256 of `onnx/model.onnx`: `874bc905cf3537a3bca91a55c3d99c78dbc571ebc2d8e209eb0aadd63e9d3948`). The README recipe's repo `urchade/gliner_small-v2.1` holds no ONNX files, so the recipe cannot produce the files preflight checks. The README is not corrected by this plan.
4. **[Rule 1 - Bug] Commit 0342c17 changed `scripts/live-e2e.sh` (plan 08-03 artifact).** The news crawl now goes through a generated registry with `[crawler] fetcher = "http"`. `fili produce` with the built-in crawler panicked ("Cannot drop a runtime in a context where blocking is not allowed"): `src/crawler.rs` builds a `reqwest::blocking` client inside the async main. The brownfield crawler was NOT changed.
5. **[Rule 1 - Bug] Commit 9391302 changed the same script's sampling.** The sample takes the newest records, drops Reddit under a waiver, and `check-evidence` requires a news record from the seed host and no Reddit in a waived sample, because the first full run produced a sample made of older and replay data.

**Impact:** Evidence is genuine and sanitised; the Reddit half of the requirement remains unmet by user choice.

## Issues Encountered

- A registry-less `fili produce` still panics (`src/crawler.rs` blocking client inside the async runtime). Not fixed here; out of this plan's scope.
- The sample contains older news groups (example.com, local `file:` URLs, an earlier techcrunch.com crawl) because the consumer reads oldest-first and the topics retain records for 7 days. The YouTube records come from this run's live poll.

## Known Stubs

None.

## User Setup Required

Reddit app credentials remain unprovided (waived). If the waiver is rejected at verification, create a Reddit web app at https://www.reddit.com/prefs/apps, set `REDDIT_CLIENT_ID` and `REDDIT_CLIENT_SECRET` in the root `.env`, and rerun `scripts/live-e2e.sh run` and the credentialed tests.

## Next Phase Readiness

All plans of phase 8 are executed once this summary lands. 08-VERIFICATION.md must reference the files listed in `evidence/README.md` and rule on the Reddit waiver.

## Threat Flags

None.

## Self-Check: PASSED

- Evidence files exist and are committed in `e13466b`; `check-evidence` prints `ok: evidence is complete and clean`; the credential file is untracked.

---
*Phase: 08-integration-live-e2e-authoring*
*Completed: 2026-11-15*
