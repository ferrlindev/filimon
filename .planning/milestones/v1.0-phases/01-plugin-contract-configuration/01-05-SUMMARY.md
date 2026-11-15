---
phase: 01-plugin-contract-configuration
plan: 05
subsystem: config
tags: [rust, dotenv, secrets, miette, parser]
gap_closure: true
closed_gap: "01-VERIFICATION.md gaps[0] (CONF-02, truth row 46): .env values silently rewritten by $ substitution (CR-01)"
review_findings_addressed: [CR-01, WR-02, WR-03, IN-02]
review_findings_excluded:
  - "IN-03: plan 01-06 (src/cli.rs, CONF-01/D-13)"
  - "WR-01, WR-06: deferred to Phase 5 (FETCH-02) by 01-VERIFICATION.md"
  - "WR-04: left open; changing the env closure type touches every load_registry_with_env caller, and no success criterion depends on it"
  - "WR-05: left open; no criterion requires bounds; triage with Phase 3 backoff arithmetic"
  - "IN-01, IN-04, IN-05, IN-06, IN-07, IN-08: left open; info-level and outside the .env value path"
reversibility: reversible

requires:
  - phase: 01-plugin-contract-configuration (01-02, 01-03, 01-04)
    provides: read_dotenv seam, resolve_secrets, load_registry_with_env, ConfigError diagnostics, sample .env.example
provides:
  - "Strict literal .env parser (read_dotenv + parse_line) that never expands $NAME/${NAME} and rejects every ambiguous form with a line number and no value text"
  - "read_dotenv returns Vec<ConfigError> (one DotenvInvalid per bad line, line order)"
  - "resolve_secrets/finish_plugin take Option<&HashMap>; None (unusable .env) suppresses MissingSecret"
  - "EmptySecret with origin and dynamic #[help] fix"
  - "Operator docs: plugins/README.md '## Plugin secrets' and commented examples/plugins/.env.example"
affects: [phase-03-host-runtime, phase-05-fetch, plugin-secrets, config-diagnostics]

actuals:
  tokens: 9669
  tasks: 3
  commits: 6
plan_head_before: 4821ae52ebeb472325f278d383bc1520b0a76929
plan_head_after: b77918fb128d7464902ff9c7328e0010ef53b5a8

tech-stack:
  added: []
  removed: [dotenvy]
  patterns:
    - "Literal-or-reject parsing: accept only forms with one reading; reject the rest with fixed reason phrases"
    - "Optional fallback map (Option<&HashMap>) to signal an unusable source and suppress cascading diagnostics"
    - "Diagnostic help computed per instance via miette #[help] field"

key-files:
  created: []
  modified:
    - Cargo.toml
    - Cargo.lock
    - src/plugins/secrets.rs
    - src/plugins/config.rs
    - src/plugins/error.rs
    - examples/plugins/.env.example
    - plugins/README.md

key-decisions:
  - "Replaced dotenvy 0.15 (01-RESEARCH 'Don't Hand-Roll' recommendation) with a hand-written strict literal parser, because dotenvy always substitutes $ and reads the real process environment (CR-01), and it cannot be configured not to"
  - ".env format is literal-or-reject (F1-F7 accepted, R1-R8 rejected); reversible because no ambiguous form is accepted today"
  - "An empty process variable still overrides .env (D-12 unchanged); only the EmptySecret diagnostic changes to name the real source and fix"
  - "A key literally named `export` still parses: the export prefix is stripped only when whitespace plus a key-start character follows"

patterns-established:
  - "Parser takes only line text, never an environment: tested with an env closure that panics on PATH lookup"
  - "Rejection reasons are module constants; tests compare reasons by constant and assert value markers are absent from Display and Debug"

requirements-completed: [CONF-02, CONF-03]

coverage:
  - id: D1
    description: "A single-quoted .env value containing $ reaches the plugin byte for byte; unquoted/double-quoted $ is rejected at its line with no value text; parsing never consults any environment"
    requirement: CONF-02
    verification:
      - kind: unit
        ref: "src/plugins/config.rs#plugins::config::tests::dotenv_single_quoted_dollar_round_trips"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#plugins::config::tests::dotenv_unquoted_dollar_is_rejected"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#plugins::config::tests::dotenv_never_expands_process_env"
        status: pass
      - kind: unit
        ref: "src/plugins/secrets.rs#plugins::secrets::tests::dotenv_dollar_outside_single_quotes_is_rejected"
        status: pass
    human_judgment: false
  - id: D2
    description: "Every accepted form F1-F7 yields its documented value (incl. CRLF, BOM, unnormalised Unicode); every rejected form R1-R8 gives one DotenvInvalid at its own counted line, in line order"
    requirement: CONF-02
    verification:
      - kind: unit
        ref: "src/plugins/secrets.rs#plugins::secrets::tests::dotenv_accepts_documented_forms"
        status: pass
      - kind: unit
        ref: "src/plugins/secrets.rs#plugins::secrets::tests::dotenv_accepts_crlf_and_bom"
        status: pass
      - kind: unit
        ref: "src/plugins/secrets.rs#plugins::secrets::tests::dotenv_rejects_ambiguous_forms"
        status: pass
      - kind: unit
        ref: "src/plugins/secrets.rs#plugins::secrets::tests::dotenv_duplicate_key_is_rejected"
        status: pass
      - kind: unit
        ref: "src/plugins/secrets.rs#plugins::secrets::tests::dotenv_reports_every_bad_line_in_order"
        status: pass
      - kind: unit
        ref: "src/plugins/secrets.rs#plugins::secrets::tests::dotenv_line_number_is_counted"
        status: pass
    human_judgment: false
  - id: D3
    description: "An unusable .env reports only its DotenvInvalid problems (no false MissingSecret), with deterministic registry-then-.env-then-source ordering"
    requirement: CONF-03
    verification:
      - kind: unit
        ref: "src/plugins/config.rs#plugins::config::tests::unusable_dotenv_suppresses_missing_secret"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#plugins::config::tests::malformed_dotenv_reports_line_not_content"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#plugins::config::tests::dotenv_problems_keep_line_order"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#plugins::config::tests::collects_all_problems"
        status: pass
    human_judgment: false
  - id: D4
    description: "EmptySecret names its real source (process environment or .env path) with a fix that works; D-12 precedence unchanged"
    requirement: CONF-02
    verification:
      - kind: unit
        ref: "src/plugins/secrets.rs#plugins::secrets::tests::empty_process_var_names_process_env"
        status: pass
      - kind: unit
        ref: "src/plugins/secrets.rs#plugins::secrets::tests::empty_dotenv_value_names_dotenv_file"
        status: pass
      - kind: unit
        ref: "src/plugins/config.rs#plugins::config::tests::empty_secret_value_is_error"
        status: pass
    human_judgment: false
  - id: D5
    description: "Operator docs (plugins/README.md '## Plugin secrets', examples/plugins/.env.example) explain the literal format, rejected forms and the process-environment escape hatch clearly"
    verification:
      - kind: other
        ref: "grep phrase checks from Task 3 verify (never expands, pa$$word, single quotes, duplicate, process environment)"
        status: pass
      - kind: unit
        ref: "src/plugins/mod.rs#plugins::tests::sample_env_example_lists_every_declared_secret"
        status: pass
    human_judgment: true
    rationale: "Grep checks prove the required phrases exist; whether the prose is clear to an operator is a reading judgment"

duration: 18min
completed: 2026-11-03
status: complete
---

# Phase 1 Plan 05: Literal .env Parser Summary

**dotenvy is gone. `.env` values are now read literally by a strict parser in `secrets.rs`: `KEY='pa$$word'` delivers `pa$$word`, and any `$`, backslash or other ambiguous form outside single quotes is rejected with its line number and no value text. A broken `.env` no longer triggers false MissingSecret errors, and EmptySecret names the process environment or the `.env` path as the source.**

## Performance

- **Duration:** about 18 min
- **Started:** about 2026-11-03T11:48Z (the start-time capture command was blocked by the worktree sandbox, so this is approximate from the base commit and first RED commit times)
- **Completed:** 2026-11-03T12:06:23Z
- **Tasks:** 3 of 3
- **Files modified:** 7

## Accomplishments

- **CR-01 gap closed.** `read_dotenv` now parses each line with a private `parse_line`. The parser takes only the line text, so it cannot read any environment. It accepts forms F1-F7 verbatim and rejects R1-R8, each with a fixed reason phrase. The CR-01 reproductions (`PW=pa$$word`, `Q="x$HOME/y"`, `S=abc$NOPE_UNSET_XYZ`) are each rejected at line 1. `R='pa$$word'` reads as `pa$$word`.
- **dotenvy removed.** It is out of Cargo.toml and Cargo.lock (the lock diff deleted 7 lines and added none), and no file under `src/` names it.
- **Every bad line is reported.** `read_dotenv` returns `Vec<ConfigError>`, one DotenvInvalid per bad line in line order (D-14). Line numbers are counted rather than searched for, which fixes IN-02. A duplicate key is rejected at its second line; the reason names the key and the first line, never a value.
- **WR-02 fixed.** `resolve_secrets` and `finish_plugin` take `Option<&HashMap>`. `None` means the `.env` is unusable, and then no MissingSecret is reported.
- **WR-03 fixed.** `EmptySecret` has an `origin` field and a `#[help] fix` field. An empty process variable still wins (D-12); its diagnostic names the process environment and says to unset the variable there. An empty `.env` value names the `.env` path, and its help never says "unset".
- **Docs.** `plugins/README.md` has a new `## Plugin secrets` section, and `examples/plugins/.env.example` has four new comment lines explaining the format.
- **Tests.** `cargo test --bin fili` passes 59 of 59: the 45 earlier tests plus 14 new ones.

## Task Commits

1. **Task 1 (tracer): literal `.env` values end to end**
   - RED: `4a521b9` test(01-05): add failing tests for literal .env values
   - GREEN: `fbb8b4f` feat(01-05): read .env values literally and reject unquoted $
2. **Task 2: per-line rejection, R8 and no MissingSecret cascade**
   - RED: `a599d3f` test(01-05): add failing tests for duplicate keys and the MissingSecret cascade
   - GREEN: `e1d5e3a` feat(01-05): reject duplicate .env keys and stop the MissingSecret cascade
3. **Task 3: EmptySecret source and docs**
   - RED: `760fb81` test(01-05): add failing tests for the EmptySecret source and fix
   - GREEN: `b77918f` feat(01-05): name the empty secret's real source and document the .env format

No refactor commits were needed.

## TDD Evidence (RED and GREEN)

The project uses Rust and `cargo test`. The `tdd-red-evidence` classifier has no adapter for libtest console output (tdd.md "Known gap — Rust"), so RED was checked by reading the output. The assessments are below.

- **Task 1 RED:** 2 of the 5 new tests failed on their planned assertions, and the build compiled.
  - `dotenv_unquoted_dollar_is_rejected` failed because the load succeeded.
  - `dotenv_dollar_outside_single_quotes_is_rejected` failed because `PW=pa$$word` came back as `{"PW": "pa"}`, which reproduces CR-01.
  - The other three passed already, because dotenvy keeps single-quoted values literal. They were written as regression pins.
  - Assessment: valid RED.
- **Task 2 RED:** 5 tests failed on their planned assertions.
  - `dotenv_duplicate_key_is_rejected` failed because the duplicate was silently accepted and overwritten.
  - `unusable_dotenv_suppresses_missing_secret`, `malformed_dotenv_reports_line_not_content`, `dotenv_unquoted_dollar_is_rejected` and `dotenv_problems_keep_line_order` each failed because extra MissingSecret problems appeared (WR-02).
  - The tests for accepted forms and R1-R7 passed already, because Task 1 had implemented them.
  - Assessment: valid RED.
- **Task 3 RED:** both new tests failed on their Display assertions. The message `is set but empty` named no source. The `origin` field assertion was added in GREEN, so that the RED build would compile.
  - Assessment: valid RED.
- **GREEN:** each task's named tests pass.

## Files Created/Modified

- `Cargo.toml`, `Cargo.lock`: dotenvy dependency removed.
- `src/plugins/secrets.rs`: the strict parser (`parse_line`, `strip_export`, `split_key`, `split_value`, `split_double_quoted`, `split_unquoted`, `check_trailer`) and the reason constants. Also `read_dotenv` returning a Vec, R8, `resolve_secrets` taking an optional map, `empty_secret_source`, and 11 new or renamed tests.
- `src/plugins/config.rs`: pass 2 now builds an `Option` map, and `finish_plugin` takes `Option<&HashMap>`. 5 new tests, plus 2 existing tests tightened to assert exactly one problem.
- `src/plugins/error.rs`: `EmptySecret` gains `origin` and a `#[help] fix`; the old static help is gone.
- `examples/plugins/.env.example`: 4 comment lines added. The three `KEY=` lines are unchanged.
- `plugins/README.md`: new `## Plugin secrets` section, placed before `## Contract version`.

## Decisions Made

- **Departure from 01-RESEARCH "Don't Hand-Roll".** The research recommended dotenvy. Its 0.15 parser always substitutes `$` and reads the real process environment, and there is no setting to turn that off. That behavior is CR-01. A small hand-written parser that delivers values verbatim or rejects them was the only closure the verifier allowed.
- **The format decision is reversible.** Every accepted line has exactly one reading, so a later release can give R1-R8 a meaning without changing any value fili accepts today.
- **A key named `export` still works.** `export` is treated as a prefix only when whitespace and a key-start character follow it, so `export=1` and `export = 1` both read as the key `export`.
- **Rule precedence for unclosed double quotes.** In an unterminated double-quoted value, a `$` or backslash is reported before the missing close quote. This follows the plan's rule to report the first problem met scanning left to right.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Task 3 RED written so it compiles**
- **Found during:** Task 3, RED step
- **Issue:** Asserting on the new `origin` field before the field existed would have made the RED run a build failure, which tdd.md classes as INVALID_RED.
- **Fix:** RED asserted only Display and `help()`. The `origin` field assertions were added in the GREEN commit.
- **Files modified:** src/plugins/secrets.rs
- **Committed in:** 760fb81 (RED), b77918f (GREEN)

**2. [Rule 1 - Bug] README phrase missed the required casing**
- **Found during:** Task 3 verify
- **Issue:** The first draft said "Single quotes:", so the case-sensitive check for `single quotes` failed.
- **Fix:** Reworded the bullets to "In single quotes" and "In double quotes", and added a sentence saying when to use single quotes.
- **Files modified:** plugins/README.md
- **Committed in:** b77918f

**3. [Rule 2 - Missing check] Unquoted-dollar test tightened to exactly one problem**
- **Found during:** Task 2
- **Issue:** The must_haves truth says `ALPHA_TOKEN=pa$$word` produces exactly one problem, but Task 1's test only checked that a DotenvInvalid was present among the problems.
- **Fix:** Added `assert_eq!(errors.problems.len(), 1)` once WR-02 made that true.
- **Committed in:** a599d3f

---

**Total deviations:** 3 auto-fixed (1 blocking, 1 bug, 1 missing check)
**Impact on plan:** None of these changed scope. All three bring the code in line with the plan's stated truths or its TDD rules.

## Issues Encountered

- **No pre-plan commit ledger file.** The worktree sandbox blocked the compound shell command that writes it. `plan_head_before` is the base SHA verified by the startup `worktree_branch_check` (4821ae5). The commit count of 6 was measured with `git rev-list --count 4821ae5..HEAD`.
- **Estimate miss.** The plan estimated about 90k tokens. The actual figure was about 9.7k (diff characters divided by 4). The estimate had `confidence: low`.
- **Pre-existing lints.** `cargo clippy --bin fili --tests` still reports two lints in code this plan did not touch: `load_registry` is never used (config.rs:194), and a doc comment is missing backticks (mod.rs:41). The plan's gate is `cargo clippy --bin fili`, and it reports nothing under `src/plugins/`.

## Verification Results

1. `cargo test --bin fili`: 59 passed, 0 failed.
2. `cargo clippy --bin fili --message-format=short`: no line starts with `src/plugins/`.
3. `! git grep -q dotenvy -- Cargo.toml src/ && ! grep -q '^name = "dotenvy"' Cargo.lock`: passes.
4. `! grep -rnE 'set_var|remove_var' src/plugins/`: passes, so the plugin code never mutates the process environment.
5. Goal-backward check against 01-VERIFICATION `gaps[0].missing`:
   - a parser that delivers values verbatim or rejects them (Tasks 1-2);
   - the three regression tests (Task 1);
   - the operator note (Task 3).

## Threat Flags

None. No new network, auth or file-access surface was added. All threats T-01-17 to T-01-21 and T-01-SC are mitigated as planned.

## User Setup Required

None. No external service configuration is needed.

## Next Phase Readiness

- The CONF-02 gap is closed and ready for phase re-verification.
- The post-execution code review can mark CR-01, WR-02, WR-03 and IN-02 as addressed in 01-REVIEW-DISPOSITION.md. This plan did not edit that file.

## Self-Check: PASSED

- All 7 modified files exist.
- Commits 4a521b9, fbb8b4f, a599d3f, e1d5e3a, 760fb81 and b77918f are all ancestors of HEAD.
- Every task's acceptance criteria and the plan-level verification were re-run and pass.

---
*Phase: 01-plugin-contract-configuration*
*Completed: 2026-11-03*
