---
phase: 08-integration-live-e2e-authoring
plan: 03
subsystem: testing
tags: [bash, jq, live-e2e, fluvio, evidence, redaction]

requires:
  - phase: 07-reddit-and-youtube-plugins
    provides: "reddit and youtube source components, sample plugin TOMLs and the `fili run` / `plugins list --json` / `consume --source` surface the runbook drives"
provides:
  - "scripts/live-e2e.sh with subcommands preflight, run, sanitise, check-evidence and selftest for the human-run TEST-04 live E2E"
  - "D-09 gate: exit 3 with a STOP (D-09) message when Reddit credentials are missing, explicit LIVE_E2E_WAIVE_REDDIT reason recorded in the transcript and evidence/WAIVER.md"
  - "sanitiser and secret scan that keep raw payloads, author names, PERSON entities, e-mail addresses and every .env value out of committed evidence"
  - ".gitignore entry for the .live-e2e/ work directory"
affects: [08-06 live E2E checkpoint]

actuals:
  tokens: 15000
  tasks: 2
  commits: 2

tech-stack:
  added: []
  patterns:
    - "single-script runbook with hermetic selftest (fake env file, fake fluvio first on PATH, synthetic records)"
    - "secrets reach grep only through a mode-600 pattern file; messages carry key names only"

key-files:
  created:
    - scripts/live-e2e.sh
  modified:
    - .gitignore

key-decisions:
  - "The .env is parsed with the same literal rules as src/plugins/secrets.rs (first key wins, quotes stripped, nothing expanded) and is linked into the work directory, never copied"
  - "fili run is started directly through env (not through a bash function) so $! is fili's own pid and SIGINT reaches it; SIGTERM then SIGKILL are the fallbacks"
  - "Sanitiser redacts e-mail addresses before truncating text, so a cut cannot leave a half address behind"
  - "run removes a stale WAIVER.md and sample at start, so evidence from an earlier waived run cannot excuse a later run"

patterns-established:
  - "Secret scan: every non-empty .env value of length 6+ plus the three credential variables, tested with grep -F -f <pattern file>, a hit names only the key"

requirements-completed: [TEST-04]

duration: 15min
completed: 2026-11-12
status: complete
plan_head_before: a2e32c66493b73235314e2a39580454f917644d1
plan_head_after: 8f27ca487d924238e0ca8912305c31d0af033efd
commits: 2

coverage:
  - id: D1
    description: "preflight names every unmet prerequisite and exits 3 with STOP (D-09) when Reddit credentials are missing (exit 1 for other failures, never printing a value)"
    requirement: TEST-04
    verification:
      - kind: integration
        ref: "scripts/live-e2e.sh preflight with an empty env file (rc 3, STOP (D-09), FAIL: REDDIT_CLIENT_ID) and with fake keys plus a missing tokenizer (rc 1, FAIL: GLiNER tokenizer.json, no value in output)"
        status: pass
    human_judgment: false
  - id: D2
    description: "sanitiser, evidence check and secret scan, proven hermetically by the selftest (planted secret fails and is not echoed, no social entities fails, missing reddit fails without WAIVER.md and passes with it)"
    requirement: TEST-04
    verification:
      - kind: integration
        ref: "scripts/live-e2e.sh selftest prints SELFTEST: PASS; a mutated copy with the secret scan disabled prints SELFTEST: FAIL"
        status: pass
    human_judgment: false
  - id: D3
    description: "the run itself (news crawl, fili run until both sources poll ok, graceful stop, enriched consume) against a real cluster, real keys and the GLiNER model"
    requirement: TEST-04
    verification: []
    human_judgment: true
    rationale: "Needs real keys, the GLiNER model files and a human; it is plan 08-06's checkpoint. Only a stubbed fili and fluvio exercised the flow here."
---

# Phase 8 Plan 03: Live E2E runbook Summary

**One re-runnable `scripts/live-e2e.sh` runbook for the human-run live E2E: named preflight failures, a Reddit stop-and-ask gate (exit 3), the run, and a sanitiser plus secret scan whose safety logic is proven hermetically by `selftest`.**

## Performance

- **Duration:** about 15 min
- **Completed:** 2026-11-12T07:59:19Z
- **Tasks:** 2
- **Files modified:** 2 (`scripts/live-e2e.sh` created, `.gitignore`)

## Accomplishments

- `preflight` prints `ok:` / `FAIL: <what>: <fix>` per prerequisite (jq, fili binary, Fluvio cluster, two topics, three SmartModules, the three credentials, three built components, GLiNER tokenizer and model with a pointer to README.md "GLiNER NER Setup"). Missing Reddit credentials print `STOP (D-09)` and exit 3, outranking exit 1; a non-empty `LIVE_E2E_WAIVE_REDDIT` prints `warn: Reddit waived by the user: <reason>` instead.
- `run` builds the live registry (only reddit, youtube and the http fetcher, absolute component paths, `.env` symlinked, only the public client_id written) in the gitignored `.live-e2e/`, then does the news crawl, `fili run` with polling of `plugins list --json` until every enabled source reports `ok`, graceful stop, the `--enrich-ner` consume, a news-and-social check (message to raise `LIMIT`), sanitise, and `check-evidence` after the transcript tee is closed.
- `sanitise` keeps at most `SAMPLE_PER_TYPE` records per (source_type, platform), nulls `raw`, replaces authors with `redacted`, redacts PERSON entity texts and e-mail addresses, truncates text to 280 characters.
- `check-evidence` requires news and social, youtube, reddit (unless `WAIVER.md`), social entities, null `raw`, redacted authors and PERSON entities, scans every evidence file for every `.env` value of 6+ characters and for `bearer` / `access_token`, and names only the key.
- `selftest` (no network, no keys, fake `fluvio` first on PATH) prints `SELFTEST: PASS`.

## Task Commits

1. **Task 1: preflight, D-09 gate, live registry and run** - `48cf318` (feat)
2. **Task 2: sanitiser, secret scan, evidence check, selftest** - `8f27ca4` (feat)

## Files Created/Modified

- `scripts/live-e2e.sh` - the runbook (executable, bash 3.2 compatible, no shell tracing)
- `.gitignore` - `.live-e2e/` in the plugin-host state block

## Decisions Made

See `key-decisions` above. The news seed default `https://blog.rust-lang.org/` is the planner's discretion, overridable with `NEWS_SEED_URL`.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] `$!` named a subshell, not fili**
- **Found during:** Task 2 ad-hoc check of `run` with a stub `fili` and `fluvio` (before the Task 1 commit)
- **Issue:** starting `fili_run ... &` forked the bash function into a subshell, so `$!` was the subshell pid and SIGINT never reached `fili run`; the stop always fell through to the 30 second SIGTERM path.
- **Fix:** the background launch calls `env -u FILI_REGISTRY -u FILI_STATE_DIR -u RUST_LOG "$FILI_BIN" ... &` directly (env execs fili, so `$!` is fili's pid).
- **Files modified:** scripts/live-e2e.sh
- **Verification:** the stubbed run stops gracefully with no SIGTERM warning.
- **Committed in:** 48cf318

**2. [Process] Task 1 commit holds a reduced script**
The script was written whole and then split: commit 48cf318 contains the Task 1 subset (no sanitise, check-evidence, selftest), commit 8f27ca4 the full script, so each commit passes its own task verify.

---

**Total deviations:** 1 auto-fixed (Rule 1), 1 process note
**Impact on plan:** none on scope.

## Issues Encountered

- The real `fili run` stop was not exercised (no keys, no built components, no GLiNER model in this checkout). The flow was checked with stub `fili` and `fluvio` binaries in `/tmp` (not committed): produce, poll, SIGINT stop, consume, sanitise and check-evidence all ran, and the waived and D-09 paths behaved. `fili` handles both SIGINT and SIGTERM as graceful stops (`src/commands/run.rs`), so the SIGTERM fallback is safe. The job-control choice (`set -m` so the background child keeps default SIGINT handling) is unconfirmed against the real binary; plan 08-06 is where it first runs for real.
- A real `.env` value that appears legitimately in the transcript (for example a 6+ character setting such as a hostname) makes `check-evidence` fail by design; the failure names the key so the operator can see why.

## Known Stubs

None.

## Threat Flags

None. The script adds no network endpoint or auth path beyond the plan's threat model (T-08-10 to T-08-13).

## User Setup Required

None for this plan. Running it for real (plan 08-06) needs `.env` credentials, a Fluvio cluster with the two topics and three SmartModules, the three built components and the GLiNER model files (absent in this checkout; see README.md "GLiNER NER Setup").

## Next Phase Readiness

Plan 08-06 can run `scripts/live-e2e.sh run` as the human checkpoint; if Reddit keys are absent it must stop at exit 3 and ask the user before setting `LIVE_E2E_WAIVE_REDDIT`.

## Self-Check: PASSED

- `scripts/live-e2e.sh` present and executable; `.gitignore` has `.live-e2e/` (`git check-ignore` confirms)
- commits `48cf318` and `8f27ca4` are ancestors of HEAD
- both tasks' verify commands and acceptance criteria pass; `selftest` prints `SELFTEST: PASS`; `examples/plugins/filimon.toml` unchanged

---
*Phase: 08-integration-live-e2e-authoring*
*Completed: 2026-11-12*
