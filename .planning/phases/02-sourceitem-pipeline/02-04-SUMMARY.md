---
phase: 02-sourceitem-pipeline
plan: 04
subsystem: consumer
tags: [rust, serde_json, fluvio, smartmodule, diagnostics, terminal-sanitization, security]

# Dependency graph
requires:
  - phase: 02-sourceitem-pipeline (02-01)
    provides: decode_news, describe_stream_error, news chain consumer, cluster test helpers
  - phase: 02-sourceitem-pipeline (02-02)
    provides: decode_social, social topic consumer, output::sanitize_for_terminal
provides:
  - describe_json_error (serde_json classify/line/column only; never the Display text)
  - hint_summary and NO_DETAIL (keeps only the module-authored start of a SmartModule hint)
  - diagnostic_line and print_diagnostic (the single sanitized stderr choke point for consume diagnostics)
  - 7 new unit tests and 2 new #[ignore] cluster tests pinning the 02-VERIFICATION.md reproductions
affects: [phase-02-verification, 02-SECURITY T-02-02, 02-REVIEW CR-01, phase-03-host, phase-08-integration]

# Actuals (#2632)
actuals:
  tokens: 4882
  tasks: 2
  commits: 4
plan_head_before: eb989bb488c9597c6281572d42773d3767baeab3
plan_head_after: e406c7a0ec68607aeb5c04a0bcf347472e9936ea

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Diagnostics from structured accessors: build error text from serde_json classify()/line()/column(), never from Display"
    - "Prefix cut for third-party error text: keep only the text before the first double quote, backtick or control character"
    - "Single stderr choke point: every consume diagnostic goes through print_diagnostic -> diagnostic_line -> output::sanitize_for_terminal"

key-files:
  created: []
  modified:
    - src/commands/consume.rs

key-decisions:
  - "Content-free decode diagnostics come from serde_json's structured accessors; the double-quote toggle redactor was deleted, not improved"
  - "SmartModule hints are cut in the consumer (D-01 keeps the modules unchanged) at the first double quote, backtick or control character, max 200 chars, `no detail` when empty"
  - "The stderr choke point lives in consume.rs next to its three callers; src/output.rs is untouched"
  - "RED evidence for Rust tests is captured with libtest's JUnit reporter (RUSTC_BOOTSTRAP=1 ... -Z unstable-options --format junit --exact on the target), which the tdd-red-evidence classifier accepts"
  - "REQUIREMENTS.md PIPE-02/PIPE-03 left as Gaps Found: closing them belongs to the phase re-verification the plan names as the next step"

patterns-established:
  - "describe_json_error: the only way consume turns a serde_json::Error into diagnostic text"
  - "print_diagnostic: the only non-comment eprintln! in src/commands/consume.rs"

requirements-completed: [PIPE-02, PIPE-03]

coverage:
  - id: D1
    description: "Social decode diagnostics hold no record content: ESC-bearing and backtick-bearing source_type give one diagnostic with topic, offset and line/column only"
    requirement: PIPE-03
    verification:
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::decode_social_withholds_variant_value"
        status: pass
      - kind: integration
        ref: "src/commands/consume.rs#commands::consume::tests::poison_social_record_is_reported_without_value (cargo test -p filimon -- --ignored)"
        status: pass
    human_judgment: false
  - id: D2
    description: "News decode diagnostics hold no record content, including the 02-VERIFICATION.md escaped-quote reproduction"
    requirement: PIPE-02
    verification:
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::decode_news_withholds_escaped_quote_content"
        status: pass
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::empty_and_malformed_values_are_reported"
        status: pass
    human_judgment: false
  - id: D3
    description: "SmartModule hints and other stream errors keep only module-authored text, cut by characters"
    requirement: PIPE-02
    verification:
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::stream_error_hint_withholds_quoted_value"
        status: pass
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::other_stream_errors_withhold_record_value"
        status: pass
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::hint_detail_truncates_by_chars"
        status: pass
      - kind: integration
        ref: "src/commands/consume.rs#commands::consume::tests::poison_news_record_is_reported_without_value (real map-preprocess hint)"
        status: pass
    human_judgment: false
  - id: D4
    description: "Every consume diagnostic reaches stderr through one sanitizing choke point"
    verification:
      - kind: unit
        ref: "src/commands/consume.rs#commands::consume::tests::diagnostic_line_strips_control_chars"
        status: pass
      - kind: other
        ref: "awk region gate: only non-comment eprintln! in src/commands/consume.rs is inside print_diagnostic"
        status: pass
    human_judgment: false
  - id: D5
    description: "No regression: full unit suite, all 5 cluster tests, clippy clean in consume.rs, D-01 files unchanged"
    verification:
      - kind: unit
        ref: "cargo test -p filimon (109 passed, 5 ignored)"
        status: pass
      - kind: integration
        ref: "cargo test -p filimon -- --ignored (5 passed)"
        status: pass
      - kind: other
        ref: "git diff --quiet 0ed7d3f -- smartmodules/ transforms.yaml src/scorer.rs"
        status: pass
    human_judgment: false

# Metrics
duration: 9min
completed: 2026-11-03
status: complete
---

# Phase 2 Plan 04: Content-free consume diagnostics Summary

**Consume diagnostics are now built only from fixed text, topic, offset and serde_json's error class and position. SmartModule hints are cut to the module-authored prefix, and every diagnostic reaches stderr through one `sanitize_for_terminal` choke point. This closes the 02-01/02-02 "no diagnostic carries the record value" gap and CR-01.**

## Performance

- **Duration:** 9 min (about 1 min of that was the cold test build)
- **Started:** 2026-11-03T21:44:35Z
- **Completed:** 2026-11-03T21:53:46Z
- **Tasks:** 2
- **Files modified:** 1

## Accomplishments
- `describe_json_error` maps `classify()` to `invalid JSON` / `truncated JSON` / `wrong JSON shape` / `read error` plus `line()` and `column()`, with no wildcard arm. `decode_news` and `decode_social` both use it. The `redact_quoted` toggle redactor is deleted.
- `hint_summary` keeps the hint text before the first double quote, backtick or control character, trims trailing whitespace, cuts to 200 characters, and falls back to `no detail`. `describe_stream_error` applies it to the SmartModule hint and to every other `ErrorCode`'s text, and never reads the stored record key or value.
- `print_diagnostic` → `diagnostic_line` → `output::sanitize_for_terminal(message)` is the only stderr path. All three former `eprintln!` sites (batch diagnostics, NER diagnostics, streaming diagnostics) now call it.
- Both verifier reproductions (escaped-quote news value, ESC in social `source_type`) and backtick-wrapped values are pinned by unit tests and by cluster tests on scratch topics through the real chain.

## Task Commits

1. **Task 1 (tracer): social poison record → content-free diagnostic → one sanitized stderr line**
   - RED `6b86f3f` (test): `decode_social_withholds_variant_value`, `diagnostic_line_strips_control_chars`, cluster `poison_social_record_is_reported_without_value`
   - GREEN `1e895e0` (feat): `describe_json_error`, `decode_social` switch, `diagnostic_line`, `print_diagnostic`, 3 print sites
2. **Task 2: news decode and SmartModule-hint diagnostics, edge set, live chain proof**
   - RED `b0da388` (test): 5 unit tests + cluster `poison_news_record_is_reported_without_value`
   - GREEN `e406c7a` (feat): `decode_news` switch, `redact_quoted` deleted, `NO_DETAIL`, `hint_summary`, `describe_stream_error` rework

No REFACTOR commits: there was nothing to clean up after GREEN.

## TDD Gate Compliance

Both tasks have a `test(02-04)` RED commit followed by a `feat(02-04)` GREEN commit that descends from it.

RED evidence came from libtest's JUnit reporter. The test binary was rerun once per target with `RUSTC_BOOTSTRAP=1 target/debug/deps/fili-<hash> <target> --exact -Z unstable-options --format junit`. The first attempt with plain libtest text got `INVALID_RED (invalid_record: Unsupported report format)`. A JUnit run over the whole module got `Incomplete JUnit test suite`, because libtest does not emit `<testcase>` elements for ignored tests. Single-target `--exact` runs fixed both. Each record went through `gsd-tools check tdd-red-evidence`:

| Target | Verdict | Semantic assessment |
|---|---|---|
| decode_social_withholds_variant_value | RED_EVIDENCE_OK | Failed on the no-`PAGE-BODY` assertion: the diagnostic carried serde's ``unknown variant `\u{1b}[2JPAGE-BODY` `` text |
| diagnostic_line_strips_control_chars | RED_EVIDENCE_OK | Failed on the no-control-char assertion: the RED stub (today's `"  ✗ {message}"` format) kept ESC, CR and LF |
| decode_news_withholds_escaped_quote_content | RED_EVIDENCE_OK | Failed on `line 1 column`: the desynchronised quote toggle ate the position and leaked `PAGE-BODY-SECRET` |
| empty_and_malformed_values_are_reported | RED_EVIDENCE_OK | Failed on `truncated JSON at line 1 column 0`: the message was serde's `EOF while parsing ...` Display text |
| stream_error_hint_withholds_quoted_value | RED_EVIDENCE_OK | Failed on no-`Location`: the whole hint passed through, quoted value included |
| other_stream_errors_withhold_record_value | RED_EVIDENCE_OK | Failed on no-`PAGE-BODY`: the look_back text with `Value: PAGE-BODY` passed through |

Both cluster tests also failed in RED on their no-`PAGE-BODY` assertions, through a real scratch topic. `hint_detail_truncates_by_chars` passed in RED, because today's `truncate_detail` already counts characters. It is an edge pin, and the plan does not claim it fails at HEAD.

## Files Created/Modified
- `src/commands/consume.rs`: the new helpers `describe_json_error`, `NO_DETAIL`, `hint_summary`, `diagnostic_line` and `print_diagnostic`. `redact_quoted` is removed, and the decode, stream-error and print paths are switched. Also 7 unit tests, 2 cluster tests, and the test helpers `poison_social`, `ESCAPED_QUOTE_VALUE`, `assert_content_free` and `runtime_error`.

## Decisions Made
- In Task 1's RED commit, `diagnostic_line` is a stub with today's unsanitized format. Without it the tests would fail to compile, and a compile failure is INVALID_RED. GREEN replaced it the next commit.
- Assumption A-3 was confirmed live. A JSON-string raw-pages record that map-preprocess rejects arrives as `ErrorCode::SmartModuleRuntimeError` at offset 0. The real hint is `Failed to parse RawPage: invalid type: string "a\"PAGE-BODY-SECRET", expected struct RawPage at line 1 column 21`, with no eyre `Location` block on the wire. The diagnostic now reads `<topic> offset 0: SmartModule error: Failed to parse RawPage: invalid type: string`.
- REQUIREMENTS.md is not updated. PIPE-02/PIPE-03 show "Gaps Found", and an earlier premature "Complete" was reverted in 30364d3. The plan's success criteria name a re-run of phase verification as the step that closes them. The frontmatter `requirements-completed` copies the plan's `requirements` field as the template requires.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] clippy `doc_markdown` warnings on the new `hint_summary` doc comment**
- **Found during:** Task 2 (clippy gate)
- **Issue:** `SmartModule`, `serde_json` and `look_back` without backticks raised 4 clippy warnings in consume.rs, which failed the gate.
- **Fix:** Put those identifiers in backticks and reflowed the comment. The wording is otherwise unchanged.
- **Files modified:** src/commands/consume.rs
- **Verification:** the clippy gate passes, with zero consume.rs warnings.
- **Committed in:** e406c7a (Task 2 GREEN)

**Additions beyond the plan's assertions (no behaviour change):**
- The "no detail" hint cases in `stream_error_hint_withholds_quoted_value` also assert there is no `PAGE-BODY`.
- The tests use the small helpers `poison_social`, `ESCAPED_QUOTE_VALUE`, `assert_content_free` and `runtime_error` in place of repeated literals.

---

**Total deviations:** 1 auto-fixed (Rule 3)
**Impact on plan:** Cosmetic only. No scope change.

## Issues Encountered
- The sandbox rejects shell commands that compute a command name or `sed` argument at runtime. Verify gates and RED-evidence runs ran from scripts under `/tmp/`. The plan-commit ledger write into `.git/worktrees/<id>/` succeeded this time.
- RED evidence artifacts (JUnit reports and JSON records) are in the gitignored `target/gsd-red/`.

## Verification Results
- `cargo test -p filimon`: exit 0, `test result: ok. 109 passed; 0 failed; 5 ignored`, no `FAILED` line. All 13 named consume tests plus `output::tests::save_json_keeps_original_text` print `... ok`.
- `cargo test -p filimon -- --ignored`: exit 0, `test result: ok. 5 passed`. `news_through_chain`, `social_passthrough`, `missing_topic_is_named`, `poison_social_record_is_reported_without_value` and `poison_news_record_is_reported_without_value` all print `... ok`.
- The structural gates pass:
  - `redact_quoted` is gone.
  - Both decode functions call `describe_json_error(&e)`, and neither formats an error with Display.
  - `describe_stream_error` has 2 `hint_summary(` calls, including `hint_summary(&e.hint)`.
  - The only non-comment `eprintln!` is inside `print_diagnostic`, and there are 4 `print_diagnostic(` lines.
  - `output::sanitize_for_terminal(message)` and `const NO_DETAIL: &str = "no detail";` are both present.
- D-01: `git diff --quiet 0ed7d3f -- smartmodules/ transforms.yaml src/scorer.rs` exits 0, and there is no working-tree change in those paths.
- `cargo clippy -p filimon --message-format=short`: `Finished`, zero warnings in src/commands/consume.rs. The 29 warnings in other files predate this plan and are out of scope.
- `cargo test --workspace` was not run, per the plan.

## Threat Mitigations
- T-02-15 (parse diagnostics) is mitigated by `describe_json_error`. Tests: decode_news_withholds_escaped_quote_content, decode_social_withholds_variant_value, empty_and_malformed_values_are_reported.
- T-02-16 (SmartModule hint and other `ErrorCode` text) is mitigated by `hint_summary`. Tests: stream_error_hint_withholds_quoted_value, other_stream_errors_withhold_record_value, hint_detail_truncates_by_chars, and cluster test poison_news_record_is_reported_without_value.
- T-02-17 (terminal escape injection) is mitigated by the `print_diagnostic` choke point. Tests: diagnostic_line_strips_control_chars, and cluster test poison_social_record_is_reported_without_value.
- T-02-18 (record accounting): zero-byte, invalid-UTF-8 and empty-variant records each still give exactly one diagnostic naming topic and offset.
- T-02-19 (Unicode format characters, IN-05) stays accepted, as the plan states.

## User Setup Required

None. No external service configuration is required.

## Next Phase Readiness
- Re-run phase 2 verification (both prohibitions), then `/gsd-secure-phase 2` so T-02-02 can close. The code-review gate updates CR-01 in 02-REVIEW-DISPOSITION.md.
- Still out of scope and open: WR-01..WR-04 and IN-01..IN-08. Assumption A-1 (NER engine errors do not embed input text) is unchanged. The NER line now only passes through the choke point.

---
*Phase: 02-sourceitem-pipeline*
*Completed: 2026-11-03*

## Self-Check: PASSED
- FOUND: src/commands/consume.rs
- FOUND: 6b86f3f, 1e895e0, b0da388, e406c7a (all ancestors of HEAD; 4 commits since eb989bb)
