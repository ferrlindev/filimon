---
phase: 02-sourceitem-pipeline
plan: 03
subsystem: docs
tags: [readme, fluvio, sourceitem, codebase-map, docker-compose]

# Dependency graph
requires:
  - phase: 02-sourceitem-pipeline (02-01)
    provides: "SourceItem news envelope, consumer-side news chain via consumer_with_config, preflight, OutputRecord JSON output"
  - phase: 02-sourceitem-pipeline (02-02)
    provides: "filimon-social-items plain stream, merged news+social consume, --source and --output flags"
provides:
  - "README operator path: create both topics, send news and social SourceItems, run fili consume, read one JSON array"
  - "README fili consume flag table and unified output record example (news with article block, social with article null)"
  - "Codebase map (ARCHITECTURE, INTEGRATIONS, STRUCTURE) describing SourceItem, both topics, the consumer-side chain and JSON output"
  - "filimon-social-items topic created on the local dev cluster (D-05 operator step)"
  - "D-03 docs side: no doc names the retired articles topic"
affects: [phase-03-plugin-host, phase-08-integration-tests]

# Actuals (#2632) - chars/4 over the realized diff (git diff f7b50c5 1793f05 = 32880 chars)
actuals:
  tokens: 8220
  tasks: 2
  commits: 2
plan_head_before: f7b50c501208701d6d12fc535404cfcc8e850773
plan_head_after: 1793f05c5a8cd0943905a4f42245cd61c77632dc

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "README samples are executable: the live verify extracts README's own social line and runs it"

key-files:
  created:
    - .planning/phases/02-sourceitem-pipeline/deferred-items.md
  modified:
    - README.md
    - .planning/codebase/ARCHITECTURE.md
    - .planning/codebase/INTEGRATIONS.md
    - .planning/codebase/STRUCTURE.md
    - docker-compose.yml
    - .planning/REQUIREMENTS.md

key-decisions:
  - "README output examples use the real values from the live run (fixture relevance 0.489, word_count 70; README news sample word_count 17), not invented numbers"
  - "README drops the OUTPUT_PATH env row, because fili never reads that variable; --output is the only way to set the output path"
  - "README help listing is copied from the built fili --help, so it matches the real global flags (--registry, --config, --ls added; --output removed from globals)"

patterns-established:
  - "Docs claims are checked against a live run: every topic, flag and sample record in README was exercised on the local cluster"

requirements-completed: [PIPE-02, PIPE-03, PIPE-04]

coverage:
  - id: D1
    description: "README operator path works as written: topics created by hand, README social sample produced, fixture news sent with fili file, one fili consume run writes a JSON array with the news record (article set) and the social t3_fili02 record (article null), and no html key"
    requirement: PIPE-03
    verification:
      - kind: integration
        ref: "bash /tmp/fili-02-03-verify.sh (plan Task 1 second <automated> block, verbatim); jq printed true, EXIT=0, run twice"
        status: pass
    human_judgment: false
  - id: D2
    description: "D-05 at the CLI: with filimon-social-items missing, fili consume --limit 1 exits non-zero and names `fluvio topic create filimon-social-items`"
    requirement: PIPE-03
    verification:
      - kind: integration
        ref: "/tmp/fili-02-03-missing.log from the first verify run (topic was absent): 'Topic `filimon-social-items` not found. Create it with: fluvio topic create filimon-social-items'"
        status: pass
    human_judgment: false
  - id: D3
    description: "README documents the D-02 topology, --source both|news|social, --output (default output/wise_output.json) and the unified output record; the README news sample still parses through the chain (D-06)"
    requirement: PIPE-04
    verification:
      - kind: other
        ref: "Task 1 doc gate (plan's first <automated> block): DOC_GATE_PASS; grep -c filimon-articles README.md = 0"
        status: pass
      - kind: integration
        ref: "README news echo line executed, then `fluvio consume filimon-raw-pages -B -d --transforms-file transforms.yaml` printed the Tech Market Surges ExtractedArticle; `fili consume --from-beginning --limit 10` (README literal) wrote output/wise_output.json with social and news records; --source social and --source news each returned only their kind"
        status: pass
    human_judgment: false
  - id: D4
    description: "Codebase map and docker-compose comment describe SourceItem, both topics, the consumer-side chain via consumer_with_config and the JSON output; no retired-topic mention; compose service command unchanged"
    requirement: PIPE-02
    verification:
      - kind: other
        ref: "Task 2 <automated> gate: TASK2_GATE_PASS; grep counts filimon-articles = 0 (3 files), subscribe to articles = 0 (2 files)"
        status: pass
    human_judgment: false
  - id: D5
    description: "Mermaid diagram and ASCII architecture diagram read clearly to an operator"
    verification: []
    human_judgment: true
    rationale: "Rendering and readability of the mermaid flowchart on GitHub is a visual judgment; no test renders it"

# Metrics
duration: 8min
completed: 2026-11-03
status: complete
---

# Phase 2 Plan 3: Operator Docs and Codebase Map for the SourceItem Pipeline Summary

**README now walks an operator through both topics, a news and a social `SourceItem`, and one `fili consume` JSON array. That path was run live on the local cluster from README's own sample lines. The codebase map describes the consumer-side chain and the social topic, and no doc names the retired articles topic.**

## Performance

- **Duration:** about 8 min (a cold `cargo build` in the worktree took most of it)
- **Started:** 2026-11-03T20:34:57Z
- **Completed:** 2026-11-03T20:42:16Z
- **Tasks:** 2
- **Files modified:** 5 doc/config files, plus REQUIREMENTS.md and a new deferred-items.md

## Accomplishments

- README mermaid diagram, Create Topics, Test the Pipeline, `fili consume` flag table, output record example and help listing now match the code that 02-01 and 02-02 built.
- Live operator path on the real topics:
  - The D-05 error appeared while `filimon-social-items` was missing.
  - The topic was then created.
  - README's social line was executed and the fixture was sent with `fili file`.
  - One `fili consume --from-beginning --limit 500` wrote 3 news records and 1 social record, with no `html` key.
- ARCHITECTURE.md, INTEGRATIONS.md and STRUCTURE.md describe `SourceItem`, both topics, `topic_stream` / `consumer_with_config` with `news_chain()`, the `stream::select` merge and `save_json` output. The docker-compose comment no longer names the retired topic.
- PIPE-02, PIPE-03 and PIPE-04 are marked complete. This is the last plan that declares them, and `requirements.ready-ids` returned all three as ready.

## Task Commits

1. **Task 1 (tracer): README operator path** - `1b0f7bc` (docs)
2. **Task 2: Codebase map and docker-compose comment** - `1793f05` (docs)

**Plan metadata:** the commit that adds this SUMMARY (docs: complete plan)

## Files Created/Modified

- `README.md`: the following sections were updated.
  - The mermaid topology now has the social topic and labels the chain as applied by `fili consume`.
  - Create Topics has both create lines and the preflight error text.
  - There are SourceItem news and social samples.
  - A `fili consume` flags table and an output example were added.
  - The help listing was copied from `fili --help`.
  - The env table no longer has `OUTPUT_PATH`.
  - The watch example syntax is fixed.
  - The project tree was updated.
- `.planning/codebase/ARCHITECTURE.md`: the following parts were updated.
  - The ASCII overview.
  - Request path steps 4-11.
  - The File Mode step.
  - SourceItem, ExtractedArticle and OutputRecord abstractions.
  - The Data Model contents.
  - The consume entry point.
- `.planning/codebase/INTEGRATIONS.md`: the following parts were updated.
  - The topics list.
  - The topic payloads.
  - The file output path and shape.
  - The `OUTPUT_PATH` note.
  - The docker topics line.
  - Chain step 5.
- `.planning/codebase/STRUCTURE.md`: the tree comments and key-file entries for produce.rs, consume.rs, streaming.rs and models.rs.
- `docker-compose.yml`: only the stream-processor comment changed. The `command:` line is unchanged.
- `.planning/phases/02-sourceitem-pipeline/deferred-items.md`: two out-of-scope compose gaps.
- `.planning/REQUIREMENTS.md`: PIPE-02, PIPE-03 and PIPE-04 are checked, and the traceability table says Complete.

## Verification

- **Task 1, doc gate:** `DOC_GATE_PASS`. `grep -c filimon-articles README.md` gives 0.
- **Task 1, live gate:** the plan's second `<automated>` block was run verbatim from `/tmp/fili-02-03-verify.sh` in the worktree root. jq printed `true` and `EXIT=0`. The first run took the missing-topic branch:
  - `fili consume --limit 1` exited non-zero with `Topic \`filimon-social-items\` not found. Create it with: fluvio topic create filimon-social-items`.
  - Then `topic "filimon-social-items" created` was printed.
- **Tracer feedback gate:** the run is interactive with `human_verify_mode: end-of-phase`, and the tracer's verify is automated-only. The live verify was re-run after the final README edits. It passed with `true` and `EXIT=0`, and expansion continued with no checkpoint.
- **Extra live checks.** The prohibition says every README sample must be exercised, so these also ran:
  - README's news echo line was produced.
  - `fluvio consume filimon-raw-pages -B -d --transforms-file transforms.yaml` printed the Tech Market Surges article, which shows the SourceItem shape parses (D-06).
  - README's literal `fili consume --from-beginning --limit 10` wrote `output/wise_output.json` with social and news records and no `html`.
  - `--source social` and `--source news` each returned only their kind and logged only their stream.
  - `fili --ls /tmp/articles watch --help` parses.
- **Task 2 gate:** `TASK2_GATE_PASS`.
  - `filimon-articles` count is 0 in ARCHITECTURE.md, INTEGRATIONS.md and docker-compose.yml.
  - `subscribe to articles` count is 0 in ARCHITECTURE.md and STRUCTURE.md.
- **Plan-level checks:**
  - `cargo test -p filimon` gives `test result: ok. 102 passed; 0 failed; 3 ignored`.
  - `cargo test -p filimon -- --ignored` gives `test result: ok. 3 passed`.
  - `git diff --quiet 0ed7d3f -- smartmodules/ transforms.yaml src/scorer.rs` exits 0.
  - `cargo test --workspace` was not run, as instructed.
- **Cluster state afterwards:** `fluvio topic list` shows `filimon-raw-pages`, `filimon-social-items` and the untouched `filimon-articles`. The cluster tests' scratch topics were cleaned up.

## Decisions Made

- The README output examples carry the values the live run actually produced, instead of illustrative numbers. The old "Expected output" line for the manual chain check was already stale: it said word_count 16 and score 0.43, and the live run gives 17 and 0.448.
- `fili produce`, `fili file` and `fili watch` are all documented as auto-creating raw-pages. All three build a `PipelineCoordinator`, which calls `ensure_topics()`.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 2 - Missing Critical] README documented things fili does not have**
- **Found during:** Task 1
- **Issue:** The plan's prohibition is "MUST NOT document a topic, command or flag that `fili` does not have". README broke it in three places:
  - The help listing showed a global `--output [env: OUTPUT_PATH]`.
  - The env table listed `OUTPUT_PATH`, which no code reads.
  - The watch example `fili watch /tmp/articles` did not parse, because `watch` takes no positional argument.
- **Fix:**
  - The help listing is now copied from `fili --help`.
  - The `OUTPUT_PATH` row was replaced by a pointer to `fili consume --output`.
  - The example is now `fili --ls /tmp/articles watch`.
  - INTEGRATIONS.md now marks `OUTPUT_PATH` as unread.
- **Files modified:** README.md, .planning/codebase/INTEGRATIONS.md
- **Verification:** `fili --help` and `fili consume --help` output matches. `fili --ls /tmp/articles watch --help` parses.
- **Committed in:** 1b0f7bc, 1793f05

**2. [Rule 1 - Bug] Stale numbers in README's manual-chain expected output**
- **Found during:** Task 1 (extra live check)
- **Issue:** The expected output showed word_count 16, relevance 0.43 and 4 keywords. The live chain gives 17, 0.4477 and 12 keywords.
- **Fix:** Replaced it with the real output line.
- **Files modified:** README.md
- **Committed in:** 1b0f7bc

**3. [Scope note] Extra stale references beyond the listed lines**
- ARCHITECTURE.md's ASCII overview still showed `Topics: raw-pages, articles` and an `articles topic` box, and its Data Model and File Mode text still said RawPage.
- STRUCTURE.md's produce.rs and streaming.rs descriptions were also stale.
- All of these were updated so the map matches the code (D-03, "so nothing documents a dead topic"). This stays inside the plan's files.

---

**Total deviations:** 2 auto-fixed (1 missing critical, 1 bug), plus 1 in-scope widening of the doc update.
**Impact on plan:** Documentation accuracy only. No code changed.

## Issues Encountered

- **Sandbox command limits.** The sandbox refused compound shell commands that mix git with other tools, so git commands ran as plain, separate calls. The plan commit ledger (`.git/worktrees/.../gsd-plan-head-before-*`) was not written, because writes into `.git/worktrees/` had been rejected for earlier executors. The base is the known dispatch base f7b50c5, and `commits: 2` was measured with `git rev-list --count f7b50c5..HEAD` before the SUMMARY commit.
- **Live run left records in the real topics.** It added duplicate records to `filimon-raw-pages` (the fixture and the README news sample) and to `filimon-social-items` (t3_fili02). That is harmless, because dedupe is v2 (HOST-07).

## Deferred Items

These are logged in `deferred-items.md`:
- docker-compose sets `OUTPUT_PATH`, but `fili` never reads it.
- The `filimon-stream-processor` service runs a `stream` subcommand that does not exist. This predates the phase and is out of scope by plan.

## User Setup Required

None. `filimon-social-items` was created on the local dev cluster as the documented operator step.

## Next Phase Readiness

- Phase 2's docs and operator path are complete. Phase 3 (plugin host) can produce social `SourceItem`s to `filimon-social-items`, and they reach the same JSON output.
- No blockers.

---
*Phase: 02-sourceitem-pipeline*
*Completed: 2026-11-03*

## Self-Check: PASSED

- FOUND: README.md, .planning/codebase/ARCHITECTURE.md, .planning/codebase/INTEGRATIONS.md, .planning/codebase/STRUCTURE.md, docker-compose.yml
- FOUND: 1b0f7bc, 1793f05 (both ancestors of HEAD)
