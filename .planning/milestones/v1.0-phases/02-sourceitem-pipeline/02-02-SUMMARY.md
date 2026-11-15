---
phase: 02-sourceitem-pipeline
plan: 02
subsystem: streaming
tags: [rust, fluvio, futures-util, clap, ner, sourceitem, social]

# Dependency graph
requires:
  - phase: 02-sourceitem-pipeline
    provides: "02-01: SourceItem/OutputRecord, NEWS_CHAIN, topic_stream, preflight, Incoming/collect_records, news consume path"
provides:
  - "TOPIC_SOCIAL_ITEMS = filimon-social-items (not created by ensure_topics)"
  - "OutputRecord::from_social and is_rfc3339"
  - "decode_social (labels by topic of origin), merge_incoming (stream::select), required_inputs"
  - "EntityDetector trait (impl for NerEngine), NER_INPUT_CHARS, ner_input, enrich"
  - "fili consume --source both|news|social and --output <path>; merged news+social batch and streaming"
affects: [02-03, phase-03-plugin-host, phase-08-integration-tests]

# Actuals (#2632) - chars/4 over the realized diff (git diff 071d77b HEAD = 54073 chars)
actuals:
  tokens: 13518
  tasks: 3
  commits: 5
plan_head_before: 071d77b68cd7a5750ea13d48c2fa9428d6e1966c
plan_head_after: 240c35bacf731f2fd564abceee1af241a5036392

# Tech tracking
tech-stack:
  added: ["tokio test-util feature (dev-dependency only; no new crate, Cargo.lock unchanged)"]
  patterns:
    - "Records are labelled by topic of origin, never by the source_type they claim"
    - "Each source is one Stream<Item = Incoming> branch; branches merge with futures_util::stream::select, and an unselected source is stream::empty()"
    - "NER goes through the EntityDetector trait so tests inject fake detectors"
    - "Cluster tests use with_scratch_topics, which deletes its topics even when an assertion panics"

key-files:
  created: []
  modified:
    - src/streaming.rs
    - src/models.rs
    - src/commands/consume.rs
    - src/cli.rs
    - src/main.rs
    - Cargo.toml

key-decisions:
  - "Social NER input is title + newline + text, cut to the first 500 chars of the combined string; news NER input stays body_preview unchanged"
  - "Diagnostics for social records name only topic and offset (and, for NER errors, the sanitised record id), never the record value or the bad published string"
  - "The NER error diagnostic passes the record id through sanitize_for_terminal, because social ids are untrusted and go to stderr"
  - "consume_batch takes a small Batch struct (limit, source, output, ner) rather than seven parameters"
  - "Scratch topic names carry nanos, pid and a per-process counter, because macOS SystemTime has microsecond resolution and the cluster tests run in parallel"

patterns-established:
  - "Topic of origin wins: decode_social always yields SourceType::Social and adds a diagnostic when the record claims news"
  - "Preflight is scoped to the selected sources through required_inputs, before any stream opens"

requirements-completed: []  # PIPE-03 and PIPE-04 are also declared by 02-03; the shared-ID gate (requirements.ready-ids) blocked marking them

coverage:
  - id: D1
    description: "A social SourceItem on the social topic reaches fili consume with no SmartModule attached, while news text 'lol cat' is dropped by the chain (SC3)"
    requirement: PIPE-03
    verification:
      - kind: integration
        ref: "src/commands/consume.rs#commands::consume::tests::social_passthrough (--ignored, local cluster, scratch topics)"
        status: pass
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::low_relevance_social_is_kept"
        status: pass
    human_judgment: false
  - id: D2
    description: "News chain stream and plain social stream merge into one stream; --limit counts both kinds; per-source order kept; same-id records stay separate; 5 s idle stop"
    requirement: PIPE-03
    verification:
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::merged_limit_counts_both_kinds"
        status: pass
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::per_source_order_preserved"
        status: pass
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::news_and_social_with_same_id_stay_separate"
        status: pass
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::idle_timeout_stops_collection"
        status: pass
      - kind: integration
        ref: "src/commands/consume.rs#commands::consume::tests::news_through_chain (--ignored, local cluster)"
        status: pass
    human_judgment: false
  - id: D3
    description: "Social decode: topic-of-origin label, non-RFC 3339 published becomes null with a diagnostic, undeserializable record gives a diagnostic without its content, empty text is delivered, html dropped"
    requirement: PIPE-03
    verification:
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::social_label_comes_from_topic"
        status: pass
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::invalid_social_published_becomes_null"
        status: pass
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::undeserializable_social_record_is_reported"
        status: pass
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::social_with_empty_text_is_delivered"
        status: pass
      - kind: unit
        ref: "src/models.rs#models::tests::from_social_drops_html_and_labels_social"
        status: pass
    human_judgment: false
  - id: D4
    description: "Missing topic fails preflight with an error naming it and `fluvio topic create` (D-05)"
    requirement: PIPE-03
    verification:
      - kind: integration
        ref: "src/commands/consume.rs#commands::consume::tests::missing_topic_is_named (--ignored, local cluster)"
        status: pass
    human_judgment: false
  - id: D5
    description: "NER runs on social title+text (500-char cap by chars) and news body_preview in one pass, through an injectable EntityDetector; output keeps full text and has no html (SC4 wiring)"
    requirement: PIPE-04
    verification:
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::ner_runs_on_social_text"
        status: pass
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::ner_input_keeps_news_text"
        status: pass
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::ner_input_truncates_by_chars"
        status: pass
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::empty_ner_input_skips_detector"
        status: pass
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::detector_error_keeps_record"
        status: pass
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::no_detector_leaves_entities_empty"
        status: pass
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::enriched_output_labels_and_omits_html"
        status: pass
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::social_text_is_not_truncated_in_output"
        status: pass
    human_judgment: false
  - id: D6
    description: "Live SC4 run with the GLiNER model: a social item with named entities gets non-empty entities in the merged JSON output"
    requirement: PIPE-04
    verification: []
    human_judgment: true
    rationale: "GLiNER model files are absent on this machine and --enrich-ner silently falls back to no NER; plan Task 2 human-check, harvested at end of phase (needs filimon-social-items, which Plan 02-03 creates)"
  - id: D7
    description: "fili consume --source both|news|social (default both) and --output <path> (default output/wise_output.json); preflight follows the selection"
    requirement: PIPE-04
    verification:
      - kind: unit
        ref: "src/cli.rs#cli::tests::consume_flags_default"
        status: pass
      - kind: unit
        ref: "src/cli.rs#cli::tests::consume_source_and_output_parse"
        status: pass
      - kind: unit
        ref: "src/cli.rs#cli::tests::consume_rejects_unknown_source"
        status: pass
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::required_inputs_follow_source_filter"
        status: pass
      - kind: other
        ref: "target/debug/fili consume --help lists --source <SOURCE> [both, news, social] and output/wise_output.json"
        status: pass
    human_judgment: false

# Metrics
duration: 15min
completed: 2026-11-03
status: complete
---

# Phase 2 Plan 02: Social SourceItems and unified enriched output Summary

**Social `SourceItem`s now travel on `filimon-social-items` with no SmartModules. `fili consume` merges that plain stream with the news chain stream through `futures_util::stream::select`, labels every record by its topic of origin, runs NER on social title+text (capped at 500 chars) as well as news, and writes both kinds into one JSON array. `--source` narrows the streams and preflight, and `--output` picks the file.**

## Performance

- **Duration:** about 15 min
- **Started:** 2026-11-03T20:15:45Z
- **Completed:** 2026-11-03T20:30:50Z
- **Tasks:** 3 (one tracer, two TDD)
- **Files modified:** 6

## Accomplishments
- D-01/PIPE-03: `TOPIC_SOCIAL_ITEMS` exists, and the social stream opens with `topic_stream(topics.social, offset, Vec::new())`, so the chain never sees social records. The live cluster test shows the contrast: the social item `lol cat` is delivered, while the same text produced as news is dropped at relevance 0.0.
- D-04: the news and social branches merge into one stream, and `--limit` caps the total across both kinds. Each source keeps its own order.
- D-05: `ensure_topics()` still creates only `filimon-raw-pages`. Preflight names a missing topic together with `fluvio topic create <name>` before any stream opens.
- D-07/D-08/D-09 (social side): `OutputRecord::from_social` drops html and leaves article null. A non-RFC 3339 `published` becomes null with a diagnostic, and a claimed `news` label is overridden to social with a diagnostic.
- PIPE-04/SC4 wiring: one `enrich` pass runs the detector on news body_preview and on social title + newline + text (500 Unicode scalars). Detector errors keep the record and give one diagnostic. The JSON keeps the full original text.
- The `--source both|news|social` and `--output` flags work, and preflight and the opened streams follow the selection.

## Task Commits

1. **Task 1 (tracer): social topic, plain stream merged with news, labelled social record:** `25bef5f` (feat)
2. **Task 2 (TDD): NER on social text with an injectable detector**
   - RED: `fe39178` (test)
   - GREEN: `c353b73` (feat)
3. **Task 3 (TDD): `--source` and `--output`, preflight scoped to the selection**
   - RED: `a138762` (test)
   - GREEN: `240c35b` (feat)

**Plan metadata:** in the docs commit that adds this SUMMARY.

## Files Created/Modified
- `src/streaming.rs`: `TOPIC_SOCIAL_ITEMS` with doc comment; the header comment names the social topic; `ensure_topics` unchanged
- `src/models.rs`: `is_rfc3339`, `OutputRecord::from_social`, SourceItem doc names the social topic; tests `from_social_drops_html_and_labels_social`, `is_rfc3339_accepts_only_rfc3339`
- `src/commands/consume.rs`: `ConsumeTopics::social`, `decode_social`, `social_incoming`, `merge_incoming`, `required_inputs`, `open_incoming`, `EntityDetector` + `impl for NerEngine`, `NER_INPUT_CHARS`, `ner_input`, `enrich`; `run(from_beginning, limit, source, output, ner)`; 18 new unit tests and 2 new cluster tests; `DEFAULT_OUTPUT` moved out
- `src/cli.rs`: `SourceFilter` (`Both`, `News`, `Social`), `DEFAULT_OUTPUT`, Consume `--source` / `--output`, updated doc comments, 3 tests; `--threshold` untouched
- `src/main.rs`: Consume arm passes `source` and `output`; header comment; `load_ner_engine` unchanged
- `Cargo.toml`: `[dev-dependencies] tokio = { features = ["test-util"] }` for the paused-time test

## TDD Gate Compliance

### Task 2
- **RED:** `fe39178`. `EntityDetector`, its `NerEngine` impl and `NER_INPUT_CHARS` were added as interface. `ner_input` (returns "") and `enrich` (does nothing) were compile-only stubs. Five of the eight tests failed on assertions: ner_runs_on_social_text, ner_input_keeps_news_text, ner_input_truncates_by_chars, detector_error_keeps_record, and social_text_is_not_truncated_in_output (index 0 on an empty call list). The other three pass on the stub because they cover the "do nothing" cases (no detector, empty input, labels/no html).
  - Evidence: libtest text is not a supported format, so the target was rerun with JUnit (`RUSTC_BOOTSTRAP=1 target/debug/deps/fili-ca7390a9a34c3183 commands::consume::tests::ner_runs_on_social_text --exact -Z unstable-options --format junit`, exit 101). `gsd-tools check tdd-red-evidence` returned `RED_EVIDENCE_OK (target_test_failed)` for `commands::consume::tests#ner_runs_on_social_text`. The first classifier call used the `::` name and got `INVALID_RED no_target_test_failure`; it was re-run with the JUnit `class#method` name.
  - **Semantic assessment:** the target ran and failed on the planned assertion: the detector call list was `[]`, expected `["Markets rallied", "Ferris\nlol cat"]`. There was no compile, setup or collection fault.
- **GREEN:** `c353b73`. All 22 consume tests pass.
- **REFACTOR:** none.

### Task 3
- **RED:** `a138762`. `SourceFilter`, `DEFAULT_OUTPUT` and the Consume `source`/`output` fields were added without defaults, `required_inputs` returned `(vec![], &[])`, and main.rs took the new fields with `..` so the crate compiled. Three tests failed: consume_flags_default (assertion on the empty default list), consume_source_and_output_parse (a required `--output` made the parse fail), and required_inputs_follow_source_filter (assertion). consume_rejects_unknown_source passes on the stub, since the enum already rejects `video`.
  - Evidence: JUnit rerun of `commands::consume::tests::required_inputs_follow_source_filter` (exit 101). The classifier returned `RED_EVIDENCE_OK (target_test_failed)`.
  - **Semantic assessment:** the target failed on the planned assertion (left `[]`, right `["filimon-raw-pages", "filimon-social-items"]`).
- **GREEN:** `240c35b`. The suite gives 102 passed.
- **REFACTOR:** none.

Gate sequence: `git log` shows test(02-02) before feat(02-02) for both TDD tasks (fe39178 then c353b73, a138762 then 240c35b). The evidence records are in `target/` (gitignored): `fili-02-02-red-evidence.json` and `fili-02-02-t3-red-evidence.json`.

## Verification Results

- `cargo test -p filimon`: `test result: ok. 102 passed; 0 failed; 3 ignored` (79 baseline + 23 new: 18 consume, 2 models, 3 cli).
- `cargo test -p filimon -- --ignored`: `test result: ok. 3 passed` (news_through_chain, social_passthrough, missing_topic_is_named). It ran 5 times in total and passed every time except once, before the scratch-name fix (see Deviations). Afterwards `fluvio topic list` shows only `filimon-raw-pages` and `filimon-articles`.
- Task 1 gates: all 4 PASS. The const and `let topics = [TOPIC_RAW_PAGES];` are present, the social stream uses `Vec::new()`, `stream::select` is present, and `git diff --quiet 0ed7d3f -- smartmodules/ transforms.yaml` exits 0. Clippy: consume.rs has 0 warnings, models.rs 4 (baseline 4), and streaming.rs 2 non-dead-code warnings (baseline 2).
- Task 2 gates: both PASS. The trait, the impl and the const are present, `git diff --quiet 0ed7d3f -- src/scorer.rs` exits 0, and consume.rs has 0 clippy warnings.
- Task 3 gates: all 3 PASS. `fili consume --help` lists `--source <SOURCE>` with both/news/social and the default `output/wise_output.json`. Clippy: cli.rs 3, main.rs 3 (baselines 3). `--threshold` is unchanged.
- The Task 1 and Task 2 gates were re-run on the final tree, and all passed.
- Plan-level: `git diff --quiet 0ed7d3f -- smartmodules/ transforms.yaml src/scorer.rs` exits 0. `cargo test --workspace` was not run, as the plan instructs.
- Tracer feedback gate (Task 1): the run is interactive with `human_verify_mode: end-of-phase`, and the tracer's verify is automated-only. The gate was re-run, it passed, and expansion continued with no checkpoint.

## Decisions Made
- The social NER cap applies to the combined `title\ntext` string, not to each part.
- No diagnostic carries record values: the bad `published` string, raw JSON and text are never echoed. The NER-error diagnostic names the record id, passed through `sanitize_for_terminal`.
- `open_incoming` is shared by batch and streaming mode, so both paths preflight before opening any stream.
- `consume_batch` takes a `Batch` settings struct, which keeps it under clippy's argument limit and readable.
- The Fluvio consumer stream is `Send`, so `StreamExt::boxed` is used (not `boxed_local`).

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] tokio `test-util` feature for the paused-time test**
- **Found during:** Task 1 (idle_timeout_stops_collection)
- **Issue:** `#[tokio::test(start_paused = true)]` needs tokio's `test-util` feature, which tokio's `full` feature does not include.
- **Fix:** added `[dev-dependencies] tokio = { version = "1", features = ["test-util"] }` to Cargo.toml. No new crate was installed, and Cargo.lock is unchanged.
- **Files modified:** Cargo.toml (not in the plan's files_modified)
- **Verification:** `idle_timeout_stops_collection ... ok` uses the real 5 s `IDLE_TIMEOUT` under paused time.
- **Committed in:** 25bef5f

**2. [Rule 1 - Bug] Parallel cluster tests could share a scratch topic**
- **Found during:** Task 3 (cluster gate)
- **Issue:** One run failed: `news_through_chain` read a Social record. macOS `SystemTime` has microsecond resolution (the leaked topic name ended in `000`), and the two cluster tests run in parallel, so names built only from `nanos()` collided.
- **Fix:** scratch names are now `filimon-test-<kind>-<nanos>-<pid>-<counter>`, using a process-wide `AtomicU32`.
- **Files modified:** src/commands/consume.rs
- **Verification:** the cluster suite passed 3 more times after the fix.
- **Committed in:** 240c35b

**3. [Rule 2 - Missing Critical] Scratch topics leaked when a cluster test panicked**
- **Found during:** Task 3 (same failed run)
- **Issue:** `with_scratch_topics` deleted its topics only on `Err`. A failing `assert!` unwinds past the delete and leaves a topic on the operator's cluster. One topic leaked, `filimon-test-raw-1793737675983552000`.
- **Fix:** the body runs under `catch_unwind`. Both topics are deleted, then the panic resumes. The leaked scratch topic was deleted with `fluvio topic delete` (no real topic was touched).
- **Files modified:** src/commands/consume.rs
- **Verification:** the cluster suite passes, and `fluvio topic list` shows only `filimon-raw-pages` and `filimon-articles`.
- **Committed in:** 240c35b

**4. [Rule 3 - Blocking] Clippy doc_markdown on new doc comments**
- **Found during:** Task 1 and Task 3 clippy gates
- **Issue:** The bare word `SmartModules`/`SmartModule` in new doc comments added a warning in consume.rs (which must have 0) and took cli.rs to 4 (baseline 3).
- **Fix:** reworded to "chain modules" / "news chain modules". The Consume doc text differs slightly from the plan's wording ("through the SmartModule chain").
- **Files modified:** src/commands/consume.rs, src/cli.rs
- **Committed in:** 25bef5f, 240c35b

**5. [Rule 2 - Missing Critical] NER-error diagnostic sanitises the record id**
- **Found during:** Task 2
- **Issue:** The diagnostic `<source_type> <id>: NER failed (...)` prints an untrusted social id to stderr, where control or ANSI sequences could reach the terminal (T-02-10).
- **Fix:** the id goes through `output::sanitize_for_terminal`, and the error text is truncated to 200 characters.
- **Files modified:** src/commands/consume.rs
- **Committed in:** c353b73

### Plan wording note
- In `ner_input_truncates_by_chars`, the plan says "a title of 499 chars plus a text gives 500 chars, ending inside the text". But 499 title chars plus the newline already make 500, so none of the text survives. The test asserts both cases: a 499-char title gives `title + "\n"` (500 chars), and a 498-char title gives `title + "\n" + "b"` (500 chars, ending inside the text). This pins the intent, that the cap applies to the combined string.

---

**Total deviations:** 5 auto-fixed (2 blocking, 1 bug, 2 missing critical)
**Impact on plan:** All five are test-infrastructure, lint or hardening fixes. There is no scope change, and nothing in smartmodules/, transforms.yaml or scorer.rs changed.

## Issues Encountered
- As in wave 1, the sandbox rejected compound shell commands and large heredocs in the worktree. The verify gates ran as scripts under `/tmp/fili-02-02-gates/` with the plan's exact checks, and file edits used the Write/Edit tools. The commit guard checks (branch, toplevel) ran as plain separate git commands. No plan-commit ledger file was written to `.git/worktrees/`, so `plan_head_before` is the orchestrator-verified spawn base 071d77b. `commits: 5` was measured with `git log --oneline 071d77b..HEAD`.

## User Setup Required

None for this plan. The live SC4 run needs the GLiNER model download and the `filimon-social-items` topic, which Plan 02-03 creates (end-of-phase human check).

## Next Phase Readiness
- Plan 02-03 can create `filimon-social-items` and run the live SC3/SC4 checks against `fili consume`.
- Phase 3's host can produce `SourceItem::social` to `TOPIC_SOCIAL_ITEMS`.
- PIPE-03 and PIPE-04 are not yet marked complete in REQUIREMENTS.md: 02-03 also declares them (shared-ID gate).
- Human check still open (end of phase): the live `--enrich-ner` run with the GLiNER model (Task 2 human-check).

## Self-Check: PASSED

- All 6 modified files exist on disk.
- Commits 25bef5f, fe39178, c353b73, a138762 and 240c35b are ancestors of HEAD.

---
*Phase: 02-sourceitem-pipeline*
*Completed: 2026-11-03*
