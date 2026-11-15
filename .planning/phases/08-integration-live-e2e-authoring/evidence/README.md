# Plan 08-06 evidence: live end-to-end run (TEST-04) and credentialed tests (TEST-03)

**Reddit waived (D-09).** The user waived Reddit for this run with the reason, verbatim: `takes time to complete`. The Reddit half of SC3 and TEST-04 is therefore **unmet** and must be explicitly accepted or rejected at verification (08-VERIFICATION.md). The waiver is also recorded in `WAIVER.md` and in the transcript (`warn: Reddit waived by the user: takes time to complete`).

## Run

- Date (UTC): 2026-11-15 (transcript starts 06:06:38 UTC; credentialed tests finished about 06:08 UTC)
- Run by: the user, in their own shell (D-07), against the local Fluvio cluster and the real YouTube Data API
- Commands:
  - `LIVE_E2E_WAIVE_REDDIT='takes time to complete' LIMIT=100 scripts/live-e2e.sh run`
  - `cargo test -p filimon e2e_credentialed_ -- --nocapture 2>&1 | tee .planning/phases/08-integration-live-e2e-authoring/evidence/credentialed-tests.txt`
- Check: `scripts/live-e2e.sh check-evidence .planning/phases/08-integration-live-e2e-authoring/evidence` prints `ok: evidence is complete and clean` (re-run by the executor after the user's run).
- Credentialed tests: `test result: ok. 10 passed; 0 failed`; the only skip is `skipped: missing REDDIT_CLIENT_ID`, allowed because `WAIVER.md` exists. The YouTube live poll ran without skipping.
- `.env` is not tracked by git (`git ls-files | grep -qx '[.]env'` finds nothing). No credential value appears in any file here.

## Record counts in `enriched-sample.json` (14 records, jq over the sample)

| source_type | platform / host | records | with NER entities |
|-------------|-----------------|---------|-------------------|
| news | blog.rust-lang.org | 3 | 3 |
| news | example.com | 2 | 2 |
| news | local (file: URLs) | 3 | 3 |
| news | techcrunch.com | 3 | 3 |
| social | youtube | 3 | 3 |
| social | reddit | 0 | 0 (waived) |

Totals: 11 news records, 3 social records, 0 Reddit records. All 14 records have `raw` null, author null or `redacted`, and at least two entities.

## Provenance note

The consumer reads the topics oldest-first and the production topics keep earlier records for 7 days. The sample therefore holds older news groups (example.com, local `file:` URLs, an earlier techcrunch.com crawl) next to the blog.rust-lang.org crawl made by this run. The YouTube records come from the live poll of this run. Reddit records that exist in the topics are not from this run and were kept out of the sample. No replay or synthetic data stands in for the live sample.

## Files for 08-VERIFICATION.md to reference

- `enriched-sample.json` - sanitised, trimmed enriched sample (raw removed, authors redacted, PERSON entities and e-mail addresses redacted, text cut to 280 characters)
- `live-e2e-transcript.txt` - the run transcript (key names and fixed texts only)
- `credentialed-tests.txt` - the credentialed test run with real keys
- `WAIVER.md` - the Reddit waiver and its reason
- `README.md` - this index
