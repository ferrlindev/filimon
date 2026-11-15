---
phase: 08-integration-live-e2e-authoring
verified: 2026-11-15T07:10:00Z
status: human_needed
score: 4/5 must-haves verified
covered_files:
  - .planning/phases/08-integration-live-e2e-authoring/08-01-PLAN.md
  - .planning/phases/08-integration-live-e2e-authoring/08-01-SUMMARY.md
  - .planning/phases/08-integration-live-e2e-authoring/08-02-PLAN.md
  - .planning/phases/08-integration-live-e2e-authoring/08-02-SUMMARY.md
  - .planning/phases/08-integration-live-e2e-authoring/08-03-PLAN.md
  - .planning/phases/08-integration-live-e2e-authoring/08-03-SUMMARY.md
  - .planning/phases/08-integration-live-e2e-authoring/08-04-PLAN.md
  - .planning/phases/08-integration-live-e2e-authoring/08-04-SUMMARY.md
  - .planning/phases/08-integration-live-e2e-authoring/08-05-PLAN.md
  - .planning/phases/08-integration-live-e2e-authoring/08-05-SUMMARY.md
  - .planning/phases/08-integration-live-e2e-authoring/08-06-PLAN.md
  - .planning/phases/08-integration-live-e2e-authoring/08-06-SUMMARY.md
  - .planning/phases/08-integration-live-e2e-authoring/08-07-PLAN.md
  - .planning/phases/08-integration-live-e2e-authoring/08-07-SUMMARY.md
  - docs/plugin-guide.md
  - plugins/replay-fetcher/src/lib.rs
  - plugins/template/src/lib.rs
  - scripts/live-e2e.sh
  - scripts/verify-authoring.sh
  - src/plugins/host/integration_tests.rs
  - src/plugins/host/testing.rs
covered_digest: "v4:sha256:a0809f9a5a09eac1d73265290ec5c4ef4a0f33c7740bfcab7b9bb65aa7c23541"
behavior_unverified: 0
overrides_applied: 0
human_verification:
  - test: "Rule on the Reddit waiver (D-09) for SC3 / TEST-04 and the Reddit half of SC2 / TEST-03"
    expected: "Either accept the waiver (add an `overrides:` entry for SC3 and TEST-04 naming the user as accepter, reason 'takes time to complete', so the phase can pass), or reject it (set REDDIT_CLIENT_ID and REDDIT_CLIENT_SECRET in the root .env, rerun scripts/live-e2e.sh run and the credentialed tests, recommit evidence)"
    why_human: "The roadmap SC3 and REQUIREMENTS TEST-04 say 'real Reddit and YouTube data'. No Reddit record from a live poll exists anywhere in the evidence. Only the user can decide that a documented waiver satisfies a requirement; the verifier cannot."
  - test: "Decide whether the committed YouTube live-run evidence is acceptable given that its transcript did not perform a fresh poll"
    expected: "Either accept that the YouTube records in enriched-sample.json come from the earlier live poll recorded in .live-e2e/state/youtube.status.json (at 2026-11-15T06:00:49Z, 10 items, real video ids), or have the live run repeated from an empty state directory (rm -rf .live-e2e/state) so the transcript itself shows a fresh poll (SENT > 0 in run.log). Fixing CR-01 in scripts/live-e2e.sh first is recommended."
    why_human: "Evidence-quality judgement. The data is real live YouTube data that flowed through Fluvio and the NER consumer, but the committed transcript's 'all enabled sources polled successfully' line was satisfied by a stale status file from a previous run (see Truth 3)."
---

# Phase 8: Integration, Live E2E & Authoring Verification Report

**Phase Goal:** The whole system is proven end to end. Compiled plugins run inside the host against replayed and live APIs into Fluvio, enriched social JSON comes out alongside news, and a third party can build and register a plugin from the guide alone
**Verified:** 2026-11-15T07:10:00Z
**Status:** human_needed
**Re-verification:** No, initial verification

## Goal Achievement

### Observable Truths (ROADMAP Success Criteria)

| #   | Truth | Status | Evidence |
| --- | ----- | ------ | -------- |
| 1 | Integration tests run the compiled Reddit and YouTube `.wasm` components inside the host against replayed fixtures and assert the expected `SourceItem`s land in a local Fluvio topic; no network and no keys | VERIFIED | I ran `cargo test -p filimon e2e_replay_` myself: 5 passed, 0 failed. `e2e_replay_reddit_poll_lands_exact_items_in_a_scratch_topic` and `e2e_replay_reddit_and_youtube_land_exact_items_in_a_scratch_topic` ran against the local cluster (6.3 s, no `skipped:` line, so they were not silently skipped) and `e2e_replay_host_path_needs_no_cluster` ran the same body into an in-memory sink. `integration_tests.rs` assigns `fetcher = "replay"` (replay-fetcher component), uses the real compiled `reddit_source.wasm` / `youtube_source.wasm`, allowlists `oauth.reddit.com` / `www.googleapis.com`, uses a `filimon-test-replay-` scratch topic deleted through a `catch_unwind` wrapper, and `assert_replay_items` compares every field (id, url, title, author, published, text, raw) against values computed from the fixtures, oldest first, key `<platform>:<id>`. |
| 2 | The same tests run in live mode against the real APIs only when credentials are in `.env`, and skip cleanly when they are not | VERIFIED, with a Reddit caveat | `run_live_poll` (integration_tests.rs:808) calls `missing_key(...)` and prints the documented `skipped: missing <KEY>` line before building anything or touching a cluster; the gate helper tests exist and passed in `credentialed-tests.txt` (10 passed, 0 failed, run by the user). YouTube live poll ran with the real key and passed; the Reddit live test printed `skipped: missing REDDIT_CLIENT_ID` and shows as `ok`, so the Reddit positive path has never run against the real API. Covered by the waiver decision in Human Verification item 1. |
| 3 | A live end-to-end run works: `fili` loads the Reddit and YouTube plugins, polls the real APIs, records flow through Fluvio; the consumer writes NER-enriched social records alongside news articles in the same JSON output | NEEDS HUMAN DECISION | Reddit half: not met. The run used `LIVE_E2E_WAIVE_REDDIT='takes time to complete'`; the generated registry `.live-e2e/filimon.toml` loads only `youtube` and `http`; `fili run` printed `loaded sources: youtube`. The sample holds 0 Reddit records. YouTube/news half: real data is present and enriched (see below) but the committed transcript did not show a fresh poll (CR-01 confirmed, see below). Not marked VERIFIED. |
| 4 | Using only the template and guide, a source plugin built with `cargo component` outside the repo, against only the published WIT, is registered in `filimon.toml` and runs in an unmodified `fili` binary | VERIFIED | Annotated signed tag `wit-v0.1.0` exists locally and on origin (`git ls-remote` shows tag object `cd7e905`, peeled `31c1ded`); the tag contains `wit/worlds.wit`, `plugins/template`, `plugins/stub-fetcher`, `plugins/replay-fetcher`, `docs/plugin-guide.md`, `scripts/verify-authoring.sh`; no diff between the tag and HEAD for the guide, template, stub-fetcher, verify script and `wit/`. I re-ran `scripts/verify-authoring.sh` in local mode: it ended `AUTHORING PROOF: PASS (legs: source fetcher)`, exit 0, including step 21/22 (fili binary hash and tree unchanged). The user's own local and from-the-tag transcripts (`authoring-proof-local.txt`, `authoring-proof-remote.txt`) both end with the same PASS line. I did not re-run remote mode (network). |
| 5 | The guide covers writing a fetcher plugin and assigning it to a source, and a fetcher built by following it serves that source's requests | VERIFIED | `docs/plugin-guide.md` has "Write a fetcher plugin" (line 220), `fetcher = "<fetcher name>"` assignment, build, TOML, and a pinned `<!-- wit-tag: wit-v0.1.0 -->`. The proof's fetcher leg (steps 11 to 20, re-run by me) shows the canned 200 answer reaching the source, the 404 for an unmatched URL, and no canned answer when the `fetcher` key is absent. |

**Score:** 4/5 truths verified (SC3 awaits a human decision; 0 behavior-unverified).

### Evidence checks for SC3 (what the committed sample does and does not show)

Checked in `evidence/enriched-sample.json` (14 records) with `jq`, not from the SUMMARY:

- Platforms and types: news blog.rust-lang.org 3, example.com 2, local `file:` 3, techcrunch.com 3; social youtube 3; Reddit 0. The counts in `README.md` are accurate.
- NER: every record has an `entities` array (entity field names are `text` and `kind`: ORGANIZATION 17, PERSON 14, PRODUCT 10, DATE 14, LOCATION 4, EVENT 3, LAW_OR_REGULATION 3). The three YouTube records have 2, 2 and 5 entities.
- Sanitising: `raw` is null on all 14, `author` is null or `redacted` on all 14, PERSON entity text is `[redacted]`, no e-mail address anywhere in the file, no `AIza...` key pattern in the evidence, scripts, docs or `src`. `scripts/live-e2e.sh check-evidence` printed `ok: evidence is complete and clean` when I ran it. The root `.env` is not tracked (not read by me).
- The YouTube ids are real video ids, not the `vidNew00001`-style replay fixture ids. The data is genuine live YouTube data.
- Weakness in the sample: 8 of the 11 news records are old (example.com, local `file:` fixtures, an earlier techcrunch.com crawl), not from this run. That is disclosed in `README.md`.

**CR-01 confirmed on the committed run (adversarial finding).** The committed transcript starts at 06:06:38Z. `.live-e2e/state/youtube.status.json` still reads `"at":"2026-11-15T06:00:49Z"`, `items":10`, six minutes earlier. `.live-e2e/run.log` of the committed run ends with the table `youtube  POLLS 0  SENT 0  DROPPED 0  FAILURES 0  STATE stopped`. So `fili run` in the committed run was stopped before its first poll finished, and step 2's "all enabled sources polled successfully; stopping fili run" was satisfied by the previous run's status file. `README.md` and `08-06-SUMMARY.md` state "The YouTube records come from the live poll of this run". That statement is not supported: the YouTube records came from the poll at 06:00:49Z in an earlier run of the same script, for which no transcript is committed. They are still live data that reached Fluvio and went through `consume --enrich-ner`, which is why this is raised as a decision and not as a failure.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
| ----------- | ----------- | ----------- | ------ | -------- |
| TEST-02 | 08-01 | Compiled Reddit and YouTube `.wasm` run in the host against replayed fixtures, items reach a local Fluvio topic | SATISFIED | Truth 1; tests run by me and passed |
| TEST-03 | 08-04, 08-06 | Live mode that runs against real APIs only when credentials are in `.env` | SATISFIED for the mechanism and YouTube; Reddit live path unexercised (waiver) | Truth 2 |
| TEST-04 | 08-03, 08-06 | A live E2E run with real Reddit and YouTube data produces enriched JSON alongside news | NEEDS HUMAN: Reddit half unmet (waiver D-09), YouTube half real but provenance of the committed transcript is weak | Truth 3 |
| WIT-03 | 08-02, 08-05, 08-07 | A plugin built outside the repo, using only the published WIT, loads and runs in `fili` without rebuilding `fili` | SATISFIED | Truth 4 |
| DOCS-01 | 08-02, 08-05 | Template plus guide for a source plugin: `cargo component`, TOML, registration in `filimon.toml` | SATISFIED | Guide sections "Build and check", "Write the plugin TOML", "Register it"; proof source leg passed |
| DOCS-02 | 08-05 | Guide covers writing a fetcher plugin and assigning it to a source | SATISFIED | Truth 5 |

All six phase requirement IDs are claimed by at least one PLAN's `requirements` frontmatter and all appear in REQUIREMENTS.md. No orphaned requirements.

**REQUIREMENTS.md tracking is wrong and needs editing by the orchestrator:** TEST-02, WIT-03, DOCS-01 and DOCS-02 are still `[ ]` / `Pending` although their evidence is complete (the SUMMARYs for 08-01, 08-02, 08-05 and 08-07 did not mark them). TEST-03 and TEST-04 are `[x]` / `Complete`, which was a mechanical mark; TEST-04 must not stay `Complete` unless the user accepts the waiver.

### Required Artifacts

| Artifact | Expected | Status | Details |
| -------- | -------- | ------ | ------- |
| `plugins/replay-fetcher/` (Cargo.toml, src/lib.rs, src/fixtures.rs, rust-toolchain.toml) | Fetcher component serving frozen fixtures | VERIFIED | Built on demand by the tests (`bindings.rs` is generated and gitignored); wired through `fetcher = "replay"` |
| `src/plugins/host/integration_tests.rs`, `testing.rs`, `mod.rs` | Replay and credentialed tests, build-on-demand helper | VERIFIED | Tests ran and passed; no TBD/FIXME/XXX markers |
| `plugins/template/` | Source plugin template | VERIFIED | Built and run by the proof (re-run by me) |
| `plugins/stub-fetcher/` | Fetcher fixture for the guide | VERIFIED | Used by the proof's fetcher leg |
| `scripts/verify-authoring.sh` | Authoring proof, local and remote | VERIFIED | Local mode re-run by me: PASS |
| `docs/plugin-guide.md` | Source and fetcher guide pinned to a WIT tag | VERIFIED | 392 lines, tag line matches the existing tag |
| `scripts/live-e2e.sh` | Live E2E runbook, sanitiser, check-evidence | VERIFIED as a tool; has CR-01 | `check-evidence` re-run: ok |
| `evidence/` (8 files, all tracked) | Committed live and authoring evidence | PARTIAL | See Truth 3; `authoring-README.md` also wrongly says `plugins/replay-fetcher` "does not exist in the tagged commit" (it does; `git cat-file -e wit-v0.1.0:plugins/replay-fetcher/Cargo.toml` succeeds) |

### Key Link Verification

| From | To | Via | Status | Details |
| ---- | -- | --- | ------ | ------- |
| replay tests | compiled reddit / youtube wasm | `ensure_built` + registry with `fetcher = "replay"` | WIRED | Tests landed exact items |
| host run | Fluvio scratch topic | `drive_fluvio` + `scratch_topic` / `delete_topic` | WIRED | Replay tests passed against the local cluster |
| `docs/plugin-guide.md` | tag `wit-v0.1.0` | `git clone --branch wit-v0.1.0` | WIRED | Tag on origin; origin URL matches the guide URL |
| `scripts/live-e2e.sh` | consumer NER output | `fili consume --enrich-ner` | WIRED | Sample has entities on all records |
| `live-e2e.sh` step 2 wait loop | a fresh poll of the current run | `plugins list --json` over a reused `$WORK/state` | NOT_WIRED for reruns | CR-01, confirmed on the committed run |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
| -------- | ------- | ------ | ------ |
| Replay into a real Fluvio topic and in-memory | `cargo test -p filimon e2e_replay_` | 5 passed, 0 failed | PASS |
| Authoring proof, local mode | `PROOF_TRANSCRIPT=/tmp/verifier-authoring-local.txt scripts/verify-authoring.sh` | `AUTHORING PROOF: PASS (legs: source fetcher)`, exit 0 | PASS |
| Evidence is complete and clean | `scripts/live-e2e.sh check-evidence <evidence dir>` | `ok: evidence is complete and clean` | PASS |
| Credentialed tests | not run by me (constraint) | user's run: 10 passed, 0 failed, Reddit skipped | SKIP (taken from the user's committed transcript) |
| Live E2E `run` | not run by me (constraint) | n/a | SKIP |
| Remote authoring proof from the tag | not re-run (network) | user's committed transcript PASS | SKIP |

### Anti-Patterns Found

| File | Line | Pattern | Severity | Impact |
| ---- | ---- | ------- | -------- | ------ |
| none | | No TBD/FIXME/XXX markers in the files this phase changed; no uncommitted changes under `src`, `scripts`, `docs`, `plugins` | none | |

Code-review items that bear on the phase goal (advisory, all `open` in `08-REVIEW-DISPOSITION.md`):

- CR-01: confirmed on the committed run, see Truth 3. This is the item that weakens the live evidence.
- WR-06: the sample cannot show freshness. Confirmed in effect (8 of 11 news records are old; YouTube ids are real but the run did not poll).
- CR-02: `fili produce` without a registry panics (`src/crawler.rs`, blocking reqwest client inside the async runtime). Not fixed; it is a brownfield defect, but it means a user following the README `produce` path without `[crawler]` hits a panic. Does not break a phase must-have, because the live script works around it.
- WR-08 (the "unassigned" assertion passes on any failure) does not invalidate Truth 5, because the canned and unmatched assertions are positive checks.
- WR-12: credentialed tests make real API calls under a plain `cargo test` when keys are in `.env`; the `-- --skip e2e_credentialed_` gate the orchestrator uses avoids this.

### Deferred Items

None. No later milestone phase covers Reddit live proof.

### Human Verification Required

#### 1. Reddit waiver (D-09) for SC3, TEST-04 and the Reddit half of SC2

**Question to the user:** "Do you accept that SC3 / TEST-04 ('a live end-to-end run with real Reddit and YouTube data') is satisfied by a YouTube-plus-news live run with Reddit waived (reason, verbatim: 'takes time to complete'), and that the Reddit live poll path stays unexercised against the real API?"

- If yes: add to this file's frontmatter
  ```yaml
  overrides:
    - must_have: "A live end-to-end run works: fili loads the Reddit and YouTube plugins, polls the real APIs (live E2E with real Reddit and YouTube data)"
      reason: "Reddit waived under D-09: takes time to complete"
      accepted_by: "<user>"
      accepted_at: "<ISO timestamp>"
  ```
  and re-verify; the phase then passes on this item.
- If no: create a Reddit web app, put `REDDIT_CLIENT_ID` and `REDDIT_CLIENT_SECRET` in the root `.env`, rerun `scripts/live-e2e.sh run` (without `LIVE_E2E_WAIVE_REDDIT`) and the credentialed tests, recommit the evidence.

#### 2. YouTube live-run provenance

**Question to the user:** "Accept the committed YouTube evidence as is (real live records from the 06:00:49Z poll, not from the 06:06 run whose transcript is committed), or repeat the run from an empty state directory so the transcript shows the poll?"

Recommended: fix CR-01 (`rm -rf "$WORK/state"` before the run, or require a status newer than the launch time), rerun once, and in the same run supply Reddit if the waiver is rejected. This makes one clean rerun serve both questions.

### Gaps Summary

No code gap blocks the phase goal. SC1, SC2 (mechanism and YouTube), SC4 and SC5 are verified by tests and a proof that I ran myself. SC3 is not marked verified: the Reddit half has no live evidence by the user's own waiver, which only the user can accept, and the committed YouTube live run did not itself perform a poll (stale status accepted by the wait loop, confirmed from `run.log` and the status file timestamp). Minor documentation defects to correct regardless of the decision: `evidence/README.md` and `08-06-SUMMARY.md` say the YouTube records are from "this run" (inaccurate); `evidence/authoring-README.md` says `plugins/replay-fetcher` is absent from the tagged commit (it is present); REQUIREMENTS.md checkboxes for TEST-02, WIT-03, DOCS-01 and DOCS-02 are stale, and TEST-04 is marked complete without an accepted waiver. The post-wave gate (`cargo build`, `cargo test -- --skip e2e_credentialed_`) was reported passing by the orchestrator and I did not re-run the full suite.

---

_Verified: 2026-11-15T07:10:00Z_
_Verifier: Claude (gsd-verifier)_
