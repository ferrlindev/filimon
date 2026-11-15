---
phase: 08-integration-live-e2e-authoring
verified: 2026-11-15T08:00:00Z
status: passed
score: 5/5 must-haves verified
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
  - .planning/phases/08-integration-live-e2e-authoring/evidence/README.md
  - .planning/phases/08-integration-live-e2e-authoring/evidence/WAIVER.md
  - .planning/phases/08-integration-live-e2e-authoring/evidence/credentialed-tests.txt
  - .planning/phases/08-integration-live-e2e-authoring/evidence/enriched-sample.json
  - .planning/phases/08-integration-live-e2e-authoring/evidence/live-e2e-transcript.txt
  - docs/plugin-guide.md
  - plugins/replay-fetcher/src/lib.rs
  - plugins/template/src/lib.rs
  - scripts/live-e2e.sh
  - scripts/verify-authoring.sh
  - src/plugins/host/integration_tests.rs
  - src/plugins/host/testing.rs
covered_digest: "v4:sha256:8418bd9aa0f49a55474ac8c478c944a8b9f6d518c56440e443c16a41b3cde38f"
behavior_unverified: 0
overrides_applied: 1
overrides:
  - must_have: "A live end-to-end run works: fili loads the Reddit and YouTube plugins, polls the real APIs (live E2E with real Reddit and YouTube data), the consumer writes NER-enriched social records alongside news articles"
    reason: "Reddit waived under D-09: takes time to complete. The user accepted the waiver; live Reddit proof is tracked as a deferred item in .planning/STATE.md (Deferred Items). The Reddit app-only path is covered by replay tests only."
    accepted_by: "user (session dialog, recorded in 08-UAT.md test 1)"
    accepted_at: "2026-11-15"
re_verification:
  previous_status: human_needed
  previous_score: 4/5
  gaps_closed:
    - "Human verification item 1 (Reddit waiver): ruled by the user, accepted, applied above as an override"
    - "Human verification item 2 (YouTube live-run provenance, CR-01): CR-01 fixed in 508db2a, live script rerun from an empty state directory, evidence regenerated in af7aeea"
    - "Documentation defects: evidence/authoring-README.md claim about plugins/replay-fetcher corrected (verified against the tag); README and SUMMARY 'poll of this run' claim now rests on a fresh poll (see residual note about per-record attribution)"
  gaps_remaining: []
  regressions: []
---

# Phase 8: Integration, Live E2E & Authoring Verification Report

**Phase Goal:** The whole system is proven end to end. Compiled plugins run inside the host against replayed and live APIs into Fluvio, enriched social JSON comes out alongside news, and a third party can build and register a plugin from the guide alone
**Verified:** 2026-11-15T08:00:00Z
**Status:** passed (one success criterion passes by an accepted user override)
**Re-verification:** Yes, after the user's rulings (Reddit waiver accepted, CR-01 fixed and the live script rerun)

## Goal Achievement

### Observable Truths (ROADMAP Success Criteria)

| #   | Truth | Status | Evidence |
| --- | ----- | ------ | -------- |
| 1 | Integration tests run the compiled Reddit and YouTube `.wasm` components inside the host against replayed fixtures and assert the expected `SourceItem`s land in a local Fluvio topic; no network and no keys | VERIFIED (re-checked) | Re-ran `cargo test -p filimon e2e_replay_` today: 5 passed, 0 failed (includes both scratch-topic tests against the local cluster, and the no-cluster in-memory host path). No regression since the first report. |
| 2 | The same tests run in live mode against the real APIs only when credentials are in `.env`, and skip cleanly when they are not | VERIFIED for the mechanism and YouTube; Reddit positive path unexercised (covered by the override) | `credentialed-tests.txt` (regenerated in af7aeea): `test result: ok. 10 passed; 0 failed`; line 106 `skipped: missing REDDIT_CLIENT_ID`; `e2e_credentialed_youtube_live_poll_has_the_expected_structure ... ok` (ran, not skipped). The Reddit live test shows `ok` only because it skipped. |
| 3 | A live end-to-end run works: `fili` loads the Reddit and YouTube plugins, polls the real APIs, records flow through Fluvio; the consumer writes NER-enriched social records alongside news articles in the same JSON output | PASSED (override) | YouTube and news half independently verified, see below. Reddit half not met by evidence and accepted by the user's override. See "Override assessment". |
| 4 | Using only the template and guide, a source plugin built with `cargo component` outside the repo, against only the published WIT, is registered in `filimon.toml` and runs in an unmodified `fili` binary | VERIFIED (re-checked) | Re-ran `scripts/verify-authoring.sh` in local mode today: exit 0, `PASS: source leg`, `PASS: fetcher leg`, `AUTHORING PROOF: PASS (legs: source fetcher)`, including step 22 (fili tree untouched). Remote-mode transcript `authoring-proof-remote.txt` unchanged (not re-run, needs network). |
| 5 | The guide covers writing a fetcher plugin and assigning it to a source, and a fetcher built by following it serves that source's requests | VERIFIED (re-checked) | The fetcher leg of the same re-run passed. `docs/plugin-guide.md` unchanged since the first report. |

**Score:** 5/5 truths (4 verified, 1 passed by override); 0 behavior-unverified.

### SC3 evidence, re-checked after the rerun

**(i) Fresh poll in this run: what is and is not committed evidence.**

- Committed, direct: the transcript starts `Sun Nov 15 06:42:48 UTC 2026` and shows `step 2 ... all enabled sources polled successfully; stopping fili run` and `source youtube: ok`. It does NOT show poll counts.
- Committed, indirect but decisive: `scripts/live-e2e.sh` (commit 508db2a) executes `rm -rf "$WORK/state"; mkdir -p "$WORK/state"` in `write_live_registry` before `fili run` starts (lines 451-454). The step 2 wait loop reads `plugins list --json` from that same state directory. After a wipe, `last_poll.state == "ok"` can only be written by a poll made in this run, so the transcript line is now a real proof of a fresh poll. `scripts/live-e2e.sh selftest` (case 9, stale status must not survive) printed `SELFTEST: PASS` when I ran it.
- Uncommitted (gitignored), corroborating: `.live-e2e/run.log` exists and ends with the table `youtube  1  10  0  0  stopped` (POLLS 1, SENT 10, DROPPED 0, FAILURES 0). `.live-e2e/state/youtube.status.json` reads `"at":"2026-11-15T06:42:53Z","ok":true,"items":10`, five seconds after the transcript start (06:42:48Z). So the executor's report is accurate, and the README says explicitly that these counts come from the uncommitted work log.
- Contrast with the first report: the status file then predated the run by six minutes and POLLS was 0. That defect (CR-01) is gone.

**Residual note on per-record attribution (does not change the verdict).** The uncommitted `.live-e2e/enriched.json` (51 records) holds 20 YouTube records, every one of the 10 video ids twice, because the production topic still held the 10 records of the earlier 06:00:49Z poll plus the 10 sent by this run (state wipe resets cursors, so the same latest 10 videos are re-sent). The committed sample's three YouTube records are `vLAegkGGWrM` twice and `rsOiZE19RVo` once, so one of the duplicated pair necessarily comes from the earlier run and the third record's origin cannot be told. `evidence/README.md` and `08-06-SUMMARY.md` say "The YouTube records come from the live poll of this run". That is true of the run as a whole (10 items were sent fresh, genuine live data) but overstated for the three sample records, and the README does not mention the duplicate pair or that the topic held the earlier run's copies. The same README discloses the older news groups. I rate this a minor documentation inaccuracy, not a gap: the goal is that a live run works end to end, which the fresh poll, the transcript and the enriched output now show.

**(ii) Sample content, checked with `jq`/`grep` on `evidence/enriched-sample.json` (14 records).**

- Types and platforms: news blog.rust-lang.org 3, example.com 2, local `file:` 3, techcrunch.com 3 (11 news); social youtube 3; Reddit 0. Matches the README table.
- Entities: every record has an `entities` array, minimum 2, maximum 9; the YouTube records have 2 each (PRODUCT/ORGANIZATION, PRODUCT/PERSON).
- `raw` is null on all 14; `author` is null or `redacted` on all 14; 0 e-mail-address matches; 0 `AIza` matches in any evidence file; `scripts/live-e2e.sh check-evidence` prints `ok: evidence is complete and clean`.
- YouTube ids `vLAegkGGWrM` and `rsOiZE19RVo` are real video ids (they appear in the live transcript and in the live consume output, not replay-fixture ids such as `vidNew00001`). No Reddit record in the sample. The transcript itself, however, prints three replay-fixture Reddit records (`reddit.com/r/rust/comments/fili02`, author `u1`, text `lol cat`) that were already in the production topic; the README says Reddit records in the topics are not from this run and were kept out of the sample. Accurate.
- Minor, not a failure: the transcript contains raw consume output, including YouTube channel names and PERSON entity text (for example public video credits). `evidence/README.md` describes the transcript as "key names and fixed texts only", which understates that. The data is public YouTube metadata and no credential or e-mail address is present.

**(iii) Corrected statements.**

- `evidence/authoring-README.md` line 10 now says `plugins/replay-fetcher` does exist in the tagged commit. Confirmed: `git cat-file -e wit-v0.1.0:plugins/replay-fetcher/Cargo.toml` succeeds. Accurate.
- `evidence/README.md` and `08-06-SUMMARY.md` now state the Reddit waiver is accepted and deferred, that this is the second run after CR-01 (commit 508db2a), that POLLS 1 / SENT 10 come from the gitignored log and are not in the committed transcript, and that the first run's evidence was stale. Accurate, with the per-record caveat above.

### Override assessment (SC3 / TEST-04)

The override is applied in the frontmatter, naming the user as accepter, with their reason verbatim (`takes time to complete`) and the date of the ruling (2026-11-15, recorded in `08-UAT.md` test 1 and in `.planning/STATE.md` Deferred Items). It is a legitimate use of the mechanism: the user, who owns the scope, made an explicit, bounded and documented ruling, and live Reddit proof is tracked as a deferred item.

Stated plainly, so it is not hidden: the roadmap wording for SC3 and the wording of TEST-04 ask for real Reddit and YouTube data, and the committed evidence contains no Reddit record from a live poll. The generated registry loaded only `youtube` and `http` (`[Run] loaded sources: youtube`). The Reddit app-only path (RDDT-01) is therefore proven only by replay tests (SC1: the real compiled `reddit_source.wasm` inside the host, exact items in a scratch topic) and by its unit tests, never against the real Reddit API, and the Reddit live credentialed test passes only by skipping. The phase passes on SC3 because the user accepted that reduction, not because the evidence meets the literal wording. The deferred item should be closed later by creating a Reddit web app, setting `REDDIT_CLIENT_ID` / `REDDIT_CLIENT_SECRET` in the root `.env`, and rerunning `scripts/live-e2e.sh run` plus the credentialed tests.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
| ----------- | ----------- | ----------- | ------ | -------- |
| TEST-02 | 08-01 | Compiled Reddit and YouTube `.wasm` run in the host against replayed fixtures, items reach a local Fluvio topic | SATISFIED | Truth 1, re-run today |
| TEST-03 | 08-04, 08-06 | Live mode that runs against real APIs only when credentials are in `.env` | SATISFIED (mechanism and YouTube positive path; Reddit positive path not exercised, covered by the override) | Truth 2 |
| TEST-04 | 08-03, 08-06 | A live end-to-end run with real Reddit and YouTube data produces enriched JSON alongside news | SATISFIED WITH ACCEPTED WAIVER (YouTube plus news proven by a fresh-poll run; Reddit half waived by the user and deferred) | Truth 3 |
| WIT-03 | 08-02, 08-05, 08-07 | A plugin built outside the repo, using only the published WIT, loads and runs in `fili` without rebuilding `fili` | SATISFIED | Truth 4, re-run today |
| DOCS-01 | 08-02, 08-05 | Template plus guide for a source plugin | SATISFIED | Truth 4 |
| DOCS-02 | 08-05 | Guide covers writing a fetcher plugin and assigning it to a source | SATISFIED | Truth 5 |

No orphaned requirements.

**Tracking state to fix (orchestrator, not edited by me).** `.planning/REQUIREMENTS.md` currently reads: TEST-02 `[ ]`/Pending, WIT-03 `[ ]`/Pending, DOCS-01 `[ ]`/Pending, DOCS-02 `[ ]`/Pending, TEST-03 `[x]`/Complete, TEST-04 `[ ]`/Pending. Given the accepted override, all six should read `[x]` / Complete. For TEST-04 I recommend appending a note such as "Complete with accepted Reddit waiver, live Reddit proof deferred (STATE.md Deferred Items)", so the checkbox does not claim more than the evidence shows. `.planning/STATE.md` line 32 still says "Reddit waiver awaits a ruling" and line 83 says the Reddit half is "to be accepted or rejected at verification"; both are stale now (the Deferred Items table row at line 106 is correct). Reported, not edited.

### Required Artifacts

| Artifact | Expected | Status | Details |
| -------- | -------- | ------ | ------- |
| `plugins/replay-fetcher/`, `plugins/template/`, `plugins/stub-fetcher/` | Fetcher and template components | VERIFIED | Built and run by the tests and the proof I re-ran |
| `src/plugins/host/integration_tests.rs`, `testing.rs` | Replay and credentialed tests | VERIFIED | Tests re-run, pass |
| `scripts/verify-authoring.sh` | Authoring proof | VERIFIED | Re-run, PASS |
| `docs/plugin-guide.md` | Guide pinned to `wit-v0.1.0` | VERIFIED | Unchanged |
| `scripts/live-e2e.sh` | Live runbook, sanitiser, check-evidence | VERIFIED | CR-01 fixed (lines 451-454), selftest PASS, check-evidence ok |
| `evidence/` (8 files, tracked) | Live and authoring evidence | VERIFIED | Regenerated in af7aeea, corrected in 2f25e89; claims checked above |

### Key Link Verification

| From | To | Via | Status | Details |
| ---- | -- | --- | ------ | ------- |
| replay tests | compiled reddit / youtube wasm | `ensure_built`, `fetcher = "replay"` | WIRED | 5 tests pass |
| host run | Fluvio scratch topic | `drive_fluvio` + `scratch_topic` | WIRED | passed against the local cluster |
| `docs/plugin-guide.md` | tag `wit-v0.1.0` | `git clone --branch` | WIRED | tag on origin (first report); local tag still contains the replay fetcher |
| `scripts/live-e2e.sh` | consumer NER output | `fili consume --enrich-ner` | WIRED | all 14 sample records have entities |
| step 2 wait loop | a fresh poll of the current run | `plugins list --json` over a state dir wiped at run start | WIRED (was NOT_WIRED, CR-01) | wipe at lines 451-454; status file timestamp 06:42:53Z, five seconds after run start |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
| -------- | ------- | ------ | ------ |
| Replay into Fluvio and in-memory | `cargo test -p filimon e2e_replay_` | 5 passed, 0 failed | PASS |
| Authoring proof, local mode | `scripts/verify-authoring.sh` | `AUTHORING PROOF: PASS (legs: source fetcher)`, exit 0 | PASS |
| Live script safety logic incl. stale-state case | `scripts/live-e2e.sh selftest` | `SELFTEST: PASS` | PASS |
| Evidence complete and clean | `scripts/live-e2e.sh check-evidence <dir>` | `ok: evidence is complete and clean` | PASS |
| Credentialed tests, live E2E `run`, remote proof | not run by me (credentials and network, per constraints) | user's committed outputs only | SKIP |

### Probe Execution

None declared beyond `scripts/verify-authoring.sh` and the live script selftest, both run above.

### Anti-Patterns Found

None blocking. No TBD/FIXME/XXX in files this phase changed; no uncommitted changes under `src`, `scripts`, `docs`, `plugins` (git status shows only unrelated working-tree changes).

Open review items (advisory, unchanged from the first report): CR-02 (`fili produce` without a registry panics, brownfield defect worked around by the script), WR-08, WR-12. CR-01 and WR-06 are now addressed for the live evidence (fresh poll proven; old news groups disclosed).

### Human Verification Required

None. Both items from the first report are resolved by the user's rulings (see `re_verification`). The only residual risk is the accepted Reddit waiver, recorded as an override and a deferred item.

### Gaps Summary

No gaps. SC1, SC2 (mechanism and YouTube), SC4 and SC5 were re-verified by commands I ran. SC3's YouTube plus news half is now backed by a run whose fresh poll is guaranteed by committed script behavior (state wipe plus selftest) and corroborated by the gitignored work log and status file timestamp; its Reddit half passes only through the user's accepted waiver, which does not meet the roadmap's literal "real Reddit data" and is deferred. Minor documentation notes to optionally tidy: the README/SUMMARY sentence "YouTube records come from the live poll of this run" should be softened to say the topic also held the earlier run's copies (the sample contains one such duplicate pair), and the README's "key names and fixed texts only" description of the transcript understates that it includes raw consume output.

---

_Verified: 2026-11-15T08:00:00Z_
_Verifier: Claude (gsd-verifier)_
