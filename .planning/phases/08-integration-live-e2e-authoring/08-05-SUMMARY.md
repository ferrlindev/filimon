---
phase: 08-integration-live-e2e-authoring
plan: 05
subsystem: plugin-authoring
tags: [wasm-component, cargo-component, wit, fetcher, docs, shell-proof]

requires:
  - phase: 08-integration-live-e2e-authoring
    provides: plan 08-01 replay fetcher and plugins README; plan 08-02 template crate and the source leg of the proof script
provides:
  - the fetcher leg of scripts/verify-authoring.sh (default legs source fetcher, LEGS selects)
  - docs/plugin-guide.md, the third-party authoring guide for source and fetcher plugins, pinned to tag wit-v0.1.0
affects: [08-07 user proof run and tag creation]

actuals:
  tokens: 7000
  tasks: 2
  commits: 2
plan_head_before: 6a774d25a831146bd70a77752e334c2fdca15057
plan_head_after: a430a2c6890d718869108f985f5f2018c3b1238e

tech-stack:
  added: []
  patterns:
    - "Guide commands are the commands the proof script runs; the fetcher's lib.rs in the guide is byte-identical to the script's heredoc"
    - "Legs as functions sharing one prepare_template helper, PASS lines printed after the global binary-hash and tree checks"

key-files:
  created:
    - docs/plugin-guide.md
  modified:
    - scripts/verify-authoring.sh
    - plugins/README.md
    - plugins/replay-fetcher/Cargo.toml

key-decisions:
  - "User-directed at Task 1: WIT tag is wit-v0.1.0 (recorded as the wit-tag line in the guide); the fetcher walkthrough is fresh-from-stub, not copy-replay-fetcher"
  - "The sparse checkout (script and guide) sets wit plugins/template plugins/stub-fetcher, because the walkthrough copies stub-fetcher"
  - "The fetcher leg replaces the whole src/lib.rs of the stub-fetcher copy (the stub's lib.rs carries test-only modes) with a fixed canned fetch; the 404 body is {\"canned\":\"no fixture\"}"

patterns-established:
  - "Throwaway-repo tag tests need tag.gpgsign false in that repo's own config"

requirements-completed: [DOCS-01, DOCS-02, WIT-03]

coverage:
  - id: D1
    description: "A fetcher built outside the repository from a copy of plugins/stub-fetcher, with its fetch replaced by a canned answer, serves the template source's request through the registry assignment: canned 200, unmatched 404, and no canned answer with the fetcher key removed; the fili binary and repository tree stay unchanged"
    requirement: "DOCS-02"
    verification:
      - kind: integration
        ref: "scripts/verify-authoring.sh (AUTHORING PROOF: PASS (legs: source fetcher)), local mode, fetcher-only mode and remote mode against a throwaway bare clone"
        status: pass
    human_judgment: false
  - id: D2
    description: "docs/plugin-guide.md covers prerequisites, getting the WIT and template by tag, build target, source anatomy, build and check, plugin TOML, registration, the fetcher walkthrough with the trust limit, the proof script and troubleshooting"
    requirement: "DOCS-01"
    verification:
      - kind: other
        ref: "plan Task 3 verify greps adapted to stub-fetcher (heading loop, wit-tag line, sparse-checkout line identical in guide and script, no dead relative links, no unfinished-work or TLS-weakening text, lib.rs block identical to the script heredoc)"
        status: pass
    human_judgment: true
    rationale: "Whether the guide reads clearly to an outside author is a judgment; the outside-author run from the real tag is plan 08-07"
  - id: D3
    description: "The guide pins the WIT distribution path (sparse git clone of a tag plus plain copies) that the script proves in remote mode"
    requirement: "WIT-03"
    verification:
      - kind: integration
        ref: "WIT_REF=authoring-selftest WIT_REPO=<throwaway bare clone> scripts/verify-authoring.sh"
        status: pass
    human_judgment: false

duration: 40min
completed: 2026-11-12
status: complete
---

# Phase 8 Plan 05: Fetcher leg and plugin authoring guide Summary

**`scripts/verify-authoring.sh` gains a fetcher leg (a fetcher built outside the repo from a copy of `plugins/stub-fetcher` serves the template source's requests through the registry assignment), and `docs/plugin-guide.md` documents the whole source and fetcher authoring path, pinned to tag `wit-v0.1.0`.**

## Performance

- **Duration:** about 40 min (continuation from Task 2; Task 1 was the human decision)
- **Tasks:** 3 (Task 1 checkpoint answered by the user; Tasks 2 and 3 executed)
- **Files:** 1 created, 3 modified

## Accomplishments

- Fetcher leg: copy `plugins/stub-fetcher` (tar excludes `target/` and `src/bindings.rs`), copy the WIT in, rewrite `Cargo.toml` (path, crate name `canned-fetcher`, package `authors:canned-fetcher`, fail on any `../..`), write a fixed canned `src/lib.rs`, build, check against `world fetcher` with no `wasi:` import, register `[fetchers.canned]` and `fetcher = "canned"`, then `plugins check canned` and `my-source` (both `component: ok`), `run-once` shows `status 200: {"canned":true}`, a second registry with `fetch_url` at `/other` shows `status 404: {"canned":"no fixture"}`, a third registry with no `fetcher` key shows no canned answer.
- Legs: `LEGS` (default `source fetcher`), unknown leg exits 2, the fetcher-only run builds the template first through the same helper. PASS lines and `AUTHORING PROOF: PASS (legs: <legs>)` print after the binary-hash and tree-untouched checks.
- Guide with all eleven required headings, the `<!-- wit-tag: wit-v0.1.0 -->` line, the trust statement (`sees every header ... authorization and cookie included`), the exact `Never register the replay fetcher for a real source: ...` sentence, and a lib.rs code block byte-identical to the script's heredoc.

## Task Commits

1. Task 1: checkpoint:decision, answered by the user (no commit).
2. Task 2: fetcher leg - `5ca3484`
3. Task 3: plugin guide (plus a two-line wording fix elsewhere, see Deviations) - `a430a2c`

## Verification Results

- Task 2 verify 1, adapted only for the binary location (`FILI_BIN` pointing at the main checkout's `target/debug/fili`, because the worktree has none): exit 0, `PASS: source leg`, `PASS: fetcher leg`, `AUTHORING PROOF: PASS (legs: source fetcher)`.
- Task 2 verify 2 (remote mode against a throwaway bare clone in a temp directory, tagged `authoring-selftest` with `tag.gpgsign false` set in that clone): exit 0, `mode: remote authoring-selftest`, both PASS lines. No tag was created in the repository.
- Also: `LEGS=fetcher` alone passes; `LEGS=bogus` exits 2.
- Observed no-`fetcher`-key behaviour (plan item i): `run-once` exits 1 with `poll transient: fetch failed: FetchError::NotConfigured` and prints no canned answer. The script asserts "no `canned` in stdout" and prints the exit code it observed.
- Task 3 verify commands and acceptance greps, adapted to stub-fetcher (see Deviations): heading loop, `wit-tag` line, sparse-checkout line present in exactly two files, `cargo component build --release`, `sees every header`, `fixtures.rs`, `fetcher = `, `[fetchers.`, relative links all resolve, no TODO/TBD/placeholder/FIXME, no pipe-to-shell, no TLS weakening, no `eval `: all pass.
- Script acceptance: no `include_bytes`, no `cargo build` on an executable line, `PASS: fetcher leg`, `canned.example` and `canned_fetcher.wasm` present: pass.

## Deviations from Plan

### User-directed change (Task 1 answer, not an executor deviation)

The human user answered the Task 1 decision: tag name `wit-v0.1.0` (recommended), walkthrough shape `walkthrough=fresh-from-stub` (not the recommended copy-replay-fetcher shape). Consequences, applied exactly as the plan's Task 2 text describes for that choice:

- D-13 ("the guide's walkthrough reuses the replay fetcher as the worked example") no longer holds as written. The guide's worked fetcher is built from `plugins/stub-fetcher` with a fixed canned `fetch`; the documented steps are exercised by the script, but they are not the steps the plan 08-01 integration tests run. The replay fetcher stays in the repository and in the guide only as a one-paragraph table-driven reference (it names `src/fixtures.rs`) and in the never-register sentence.
- The sparse checkout line in both files is `sparse-checkout set wit plugins/template plugins/stub-fetcher` (the plan's `plugins/replay-fetcher` form no longer fits, since the walkthrough needs `stub-fetcher`). The plan's literal acceptance greps that name `plugins/replay-fetcher` in that line were therefore adapted, with the same intent (the line is identical in guide and script).
- Plan 08-07 reads the tag from the `<!-- wit-tag: wit-v0.1.0 -->` line. The tag must contain `wit/`, `plugins/template`, `plugins/stub-fetcher` and `docs/plugin-guide.md`, so it must be created after this phase's work is committed. No tag was created or pushed here.

### Auto-fixed Issues

**1. [Rule 1 - Bug] Stale "worked example" wording in two files**
- **Found during:** Task 3
- **Issue:** `plugins/README.md` (replay-fetcher entry) and the `description` in `plugins/replay-fetcher/Cargo.toml` called the replay fetcher the authoring guide's worked example, which the flipped walkthrough made false.
- **Fix:** reworded both to say the guide builds its worked fetcher from `stub-fetcher` and names the replay fetcher only as a table-driven reference. No code or behaviour change.
- **Files modified:** plugins/README.md, plugins/replay-fetcher/Cargo.toml
- **Commit:** a430a2c

**Total deviations:** 1 auto-fixed (Rule 1) plus the user-directed shape change above. **Impact:** none beyond the stated D-13 consequence.

## Issues Encountered (environment, not code)

- The worktree has no `target/debug/fili`; verification used `FILI_BIN` pointing at the main checkout's existing binary (read only; the script's hash check passed).
- The sandbox refuses git commands in compound shell lines inside this worktree, so the throwaway-clone check was run as separate plain commands. The user's global `tag.gpgsign=true` was countered with a repo-local `tag.gpgsign false` in the throwaway clone only.
- `wasm-tools`-related plan note: the proof prints `no fetcher key: run-once exited 1 ...`; plan 08-07 should expect exit 1 there.

## Known Stubs

None. (`plugins/stub-fetcher` is a deliberate test fixture; the guide says so and tells authors to replace its `lib.rs`.)

## Threat Flags

None. T-08-18 (fetcher header visibility) is stated in the guide's fetcher section; T-08-19 (no pipe-to-shell, no evaluation) holds in the script and guide; T-08-21 (never register the replay fetcher) is stated verbatim; T-08-20 stays with plan 08-07, which creates the tag and runs `WIT_REF=wit-v0.1.0 scripts/verify-authoring.sh` against the remote. Secrets stay in `.env`, named by alias.

## Next Phase Readiness

Plan 08-07 can run the SC4 and SC5 proofs, create and push `wit-v0.1.0` after everything is committed, and run `WIT_REF=wit-v0.1.0 scripts/verify-authoring.sh` against the remote. Note that the tag-time check that the `wit-tag` line names an existing tag is not possible until the tag exists, and the guide's tag-based commands cannot be followed by outsiders before then.

STATE.md, ROADMAP.md and REQUIREMENTS.md were not touched (orchestrator-owned in this wave).

## Self-Check: PASSED

- docs/plugin-guide.md, scripts/verify-authoring.sh exist; commits 5ca3484 and a430a2c are on this branch.
