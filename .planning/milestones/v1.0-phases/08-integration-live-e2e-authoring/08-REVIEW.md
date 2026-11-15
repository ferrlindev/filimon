---
phase: 08-integration-live-e2e-authoring
reviewed: 2026-11-15T00:00:00Z
depth: standard
files_reviewed: 20
files_reviewed_list:
  - .gitignore
  - docs/plugin-guide.md
  - plugins/README.md
  - plugins/replay-fetcher/.cargo/config.toml
  - plugins/replay-fetcher/.gitignore
  - plugins/replay-fetcher/Cargo.toml
  - plugins/replay-fetcher/rust-toolchain.toml
  - plugins/replay-fetcher/src/fixtures.rs
  - plugins/replay-fetcher/src/lib.rs
  - plugins/template/.cargo/config.toml
  - plugins/template/.gitignore
  - plugins/template/Cargo.toml
  - plugins/template/rust-toolchain.toml
  - plugins/template/src/lib.rs
  - plugins/template/template.toml
  - scripts/live-e2e.sh
  - scripts/verify-authoring.sh
  - src/plugins/host/integration_tests.rs
  - src/plugins/host/mod.rs
  - src/plugins/host/testing.rs
findings:
  critical: 2
  warning: 13
  info: 9
  total: 24
  necessity: 0
status: issues_found
---

# Phase 8: Code Review Report

**Reviewed:** 2026-11-15
**Depth:** standard
**Files Reviewed:** 20
**Status:** issues_found

## Summary

I read all 20 files in full. I did not run anything: no live E2E, no credentialed tests, no
network calls. I did not read `.env` or `.env.example`. The `necessity-review.md` rubric was read and
produced no necessity findings. `plugins/replay-fetcher/src/bindings.rs` is generated and gitignored,
so it was not reviewed.

The Rust code (`mod.rs`, the replay fetcher, the template and the in-crate integration tests) is
mostly sound. Scratch topics are deleted through a `catch_unwind` wrapper, builds are serialised
under a mutex, and credential-bearing failures use fixed text. The weak points are in
`scripts/live-e2e.sh`, which produces the committed evidence:

- A rerun can accept stale poll status as proof of a successful poll (CR-01).
- A failed or blocked run destroys previously committed evidence (WR-01).
- Relative arguments resolve against the wrong directory (WR-02).
- The sanitiser and selftest leave PII and provenance gaps (WR-04 to WR-06).

`verify-authoring.sh` has weak assertions and claims in its header and docs that the code does not
back (WR-08 to WR-10). The known registry-less `fili produce` panic is listed as CR-02.

## Critical Issues

### CR-01: `live-e2e.sh run` accepts a stale `last_poll` from a previous run as "every source polled"

**File:** `scripts/live-e2e.sh:443,562-591`
**Issue:** Step 2 waits until `plugins list --json` shows every enabled source with
`last_poll.state == "ok"`. The state directory `$WORK/state` is reused across runs: `mkdir -p`
keeps it, and the header says "Re-running `run` reuses the work directory". The status file
`<alias>.status.json` is written only after a poll finishes (`scheduler.rs:264`,
`cursor.rs:312`). Nothing resets it at start-up. On any rerun the first loop iteration therefore
reads the previous run's `ok`. The script prints "all enabled sources polled successfully",
SIGINTs `fili run` before it has polled, and moves on. The transcript then records a successful
social poll that never happened in this run. Persisted cursors also mean that, even when a poll does
happen, it fetches nothing new. The evidence exists to prove a live run, so this makes it
untrustworthy.
**Fix:** Start each run from empty state, or require a status newer than the launch:
```bash
rm -rf "$WORK/state"        # before write_live_registry's mkdir, in run_steps
mkdir -p "$WORK/state"
# or, alternatively, record `started=$(date -u +%Y-%m-%dT%H:%M:%SZ)` and require
# (.last_poll.at >= $started) in the jq filter.
```

### CR-02: Registry-less `fili produce` panics (known issue, `src/crawler.rs`)

**File:** `src/crawler.rs:64-75` (outside the review file list; the workaround is at `scripts/live-e2e.sh:504-508`)
**Issue:** `build_client()` builds a `reqwest::blocking::Client` inside the async `main`
runtime. Dropping a blocking client in an async context panics with "Cannot drop a runtime in a
context where blocking is not allowed". `fili produce <url>` therefore crashes without a
`[crawler]` table in the registry. `live-e2e.sh` only works around it by generating a registry with
`[crawler] fetcher = "http"`. Any user following the README `produce` path without a registry hits
the panic.
**Fix:** Use the async `reqwest::Client` in `fetch_page`, or wrap the blocking call in
`tokio::task::spawn_blocking`. Build the client once, not per page:
```rust
pub async fn fetch_page(client: &reqwest::Client, target: &CrawlTarget) -> Result<RawPage, String> { ... }
```

## Warnings

### WR-01: A failed or blocked `run` destroys the committed evidence

**File:** `scripts/live-e2e.sh:545,613-626`
**Issue:** `cmd_run` does `rm -f "$EVIDENCE_DIR/enriched-sample.json"` and truncates
`live-e2e-transcript.txt` through `tee` before any check. `run_steps` also removes `WAIVER.md` after
preflight. A rerun that stops at preflight (cluster down, exit 3 for D-09) or fails mid-way leaves
the committed evidence directory without its sample and with a transcript that only says "nothing
was run". Git can restore the files, but the script should not delete the last good evidence.
**Fix:** Write the transcript, sample and waiver into `$WORK/evidence.new/`. Run
`cmd_check_evidence` on that directory. Copy it over `$EVIDENCE_DIR` only after it passes.

### WR-02: Relative `sanitise` / `check-evidence` arguments resolve against the repo root, not the caller's directory

**File:** `scripts/live-e2e.sh:50,62,847-848`
**Issue:** The comment at line 50 says caller paths are made absolute before the script moves to
the repository root. That is done only for the environment variables. `cd "$REPO_ROOT"` (line 62)
runs first, and the positional arguments are passed through untouched. `cd /tmp && live-e2e.sh
check-evidence ./evidence` checks `<repo>/evidence`, and `sanitise out.json sample.json` reads and
writes inside the repo. A wrong directory can produce a false "evidence is complete and clean".
**Fix:** In `main`, absolutise before dispatch:
```bash
sanitise) cmd_sanitise "$(abs_path "${1:-}")" "$(abs_path "${2:-}")" ;;
check-evidence) cmd_check_evidence "$(abs_path "${1:-}")" ;;
```
Capture `PWD` before the `cd`, because `abs_path` uses the current `$PWD`.

### WR-03: Preflight demands the Reddit component even when Reddit is waived

**File:** `scripts/live-e2e.sh:221-229`
**Issue:** The component loop always includes `reddit/reddit_source.wasm`. Under
`LIVE_E2E_WAIVE_REDDIT` the registry never references that component (lines 490-496), so a missing
Reddit build fails the preflight for no reason.
**Fix:** Skip the `reddit/...` entry when `$waiver` is non-empty.

### WR-04: Selftest does not exercise e-mail redaction in `.text`

**File:** `scripts/live-e2e.sh:666-700`
**Issue:** The fixture puts the e-mail in `text` of record `i == 0` ("Contact john.doe@example.com
now ..."). Every record has `published: null`, so `sort_by(.published // "") | reverse | .[0:3]`
keeps `i = 3, 2, 1` and drops `i == 0`. The `tojson | contains("@") | not` assertion only covers the
title e-mail of `i == 1`. A regression in the text `mail` redaction would pass the selftest.
**Fix:** Put the e-mail in a record that survives the cut (for example the last index), or give the
records distinct `published` values. Assert separately that the `text` of a kept record changed to
contain `[email]`.

### WR-05: Sanitiser is a denylist and `check-evidence` has no PII scan

**File:** `scripts/live-e2e.sh:271-286,363-370`
**Issue:** Only `title`, `text` and `entities[].text` are scrubbed of e-mail addresses, and only
`PERSON` entities and `author` are redacted. `url`, `id` (the URL for news), `platform`,
`article.top_keywords` and `article.published_raw` pass through. `text` and `title` keep names and
handles (for example `u/someone`). `check_evidence` verifies `raw`, `author` and `PERSON` only. It
never scans the committed files for `@`-patterns, so a leak outside those fields is not caught.
**Fix:** Build the output from an explicit allowlist of fields. Add an e-mail regex scan of every
evidence file to `check-evidence`, and fail on a hit.

### WR-06: Sample provenance cannot show the records are from this run

**File:** `scripts/live-e2e.sh:594-598,336-349`
**Issue:** `consume --from-beginning --limit "$LIMIT"` reads the oldest `LIMIT` records of topics
that retain 7 days of earlier runs, fixtures and replay data. The newest-first sort in `sanitise`
only reorders those oldest records. Only the news side has a provenance check (the seed host);
the host can still match an older crawl. The youtube/reddit/entity checks pass on stale or replayed
records. The script prints a reminder but nothing enforces it.
**Fix:** Consume from a recorded offset or a fresh scratch topic, or filter the sample to records
whose `published` or ingestion time is after the run start. Alternatively, document that the
evidence is not proof of freshness.

### WR-07: `errexit` is disabled throughout `run`, so setup failures are silent

**File:** `scripts/live-e2e.sh:443-486,613-621`
**Issue:** `cmd_run` runs `run_steps` under `set +e` inside a pipeline. `write_live_registry` is
called with `|| return 1`. Bash ignores `set -e` inside functions called in `||` contexts. A failing
`mkdir -p`, `ln -sf "$ENV_FILE" "$WORK/.env"`, the `cp .../youtube.toml`, or a `>"$WORK/*.toml"`
redirect does not stop anything. Fili then fails later with an obscure "no credentials" or "config
not found" error. With `ln -sf` failing, the secret reaches fili only through the process
environment, which `fili_run` does not strip, so it can pass while hiding the cause.
**Fix:** Check each setup command explicitly (`mkdir -p ... || return 1`, `ln -sf ... || return 1`,
`cp ... || return 1`), or run `write_live_registry` in a context where `set -e` is active.

### WR-08: `verify-authoring.sh` "unassigned" assertion passes on any failure

**File:** `scripts/verify-authoring.sh:449-462`
**Issue:** The test claims "without the `fetcher` key the canned answer does not appear". It
passes if `fnone.out` has no "canned", whatever the exit code. If `run-once` dies for an unrelated
reason (a plugin trap, a config error, DNS to `canned.example`), `fnone.out` is empty and the leg
passes. The guide (lines 320-322) says `run-once` exits 1 with a transient error, but the script
only prints the exit code and asserts nothing about it.
**Fix:** Require a non-zero exit and that `fnone.err` or the output names `not-configured`:
```bash
[ "$NONE_RC" -ne 0 ] || fail "run-once succeeded without a fetcher"
grep -q 'not-configured' "$WORK/fnone.err" "$WORK/fnone.out" || fail "failure was not not-configured"
```

### WR-09: "Repository tree unchanged" check is too weak to prove its claim

**File:** `scripts/verify-authoring.sh:169,478-480`
**Issue:** The check compares `git status --porcelain` before and after. That output lists only
paths and status codes. It misses further edits to a file that is already modified, and it misses
changes to ignored files. This checkout already carries modified and untracked files, so the check
covers least where it matters. The step message and the guide both claim the tree is unchanged.
**Fix:** Compare content. For example hash `git diff` plus `git ls-files -o --exclude-standard`,
or compare `git stash create`-style tree hashes, before and after.

### WR-10: Script and docs claim "no network" and "nothing is run", neither of which holds

**File:** `scripts/verify-authoring.sh:32-36`, `docs/plugin-guide.md:353-354,373`
**Issue:** The docs say the proof "needs no network" and "Nothing it fetches is run or evaluated".
Both are wrong. The first `cargo component build` needs the crates.io index and crate downloads, and
rustup downloads the pinned 1.96.0 toolchain and target. In remote mode, building the copied crate
runs its `build.rs` and procedural macros (including `wit-bindgen`), and `WIT_REPO` is
caller-controlled. The renamed crate also rewrites `Cargo.lock`. Running the script against an
untrusted `WIT_REPO` executes arbitrary code from that repository.
**Fix:** Reword both: "needs the network the first time (toolchain and crates)" and "copied crates
are built, which runs their build scripts, so use only a trusted `WIT_REPO`". Optionally refuse a
`WIT_REPO` other than `origin` unless `ALLOW_UNTRUSTED_REPO=1`.

### WR-11: Template comment says the host de-duplicates by `id`; the docs say it does not

**File:** `plugins/template/src/lib.rs:134`
**Issue:** The comment reads "Platform-stable id: the host uses it to drop duplicates."
`plugins/README.md:324,377-378` ("There is no dedupe") and `docs/plugin-guide.md:112-113` say the
opposite, and `map.rs:117` keeps equal keys. Authors copy the template, so an author trusting the
comment would emit repeats and get duplicate records downstream.
**Fix:** Change the comment to "Platform-stable id: forms the message key `<platform>:<id>`; the
host does not de-duplicate across polls."

### WR-12: Credentialed live tests hit real APIs under a plain `cargo test`; failures lose all diagnostics

**File:** `src/plugins/host/integration_tests.rs:123-129,815-832`; `plugins/README.md:446-449`
**Issue:**
- (a) `run_live_poll` runs whenever the keys are present in the process environment or the
  repository `.env`. They are not `#[ignore]`d and need no opt-in variable, so any full
  `cargo test -p filimon` on a developer machine with credentials sends real requests and uses
  Reddit and YouTube quota.
- (b) `.map_err(|_| miette!("the live {name} poll failed"))` discards every error, including ones that cannot carry
  secrets ("only 0 of 1 records within 30 s", "was disabled: rate limited", cluster errors, topic
  delete failures). A failing live test cannot be diagnosed without editing the test.

**Fix:**
- (a) Gate on an explicit opt-in such as `FILI_TEST_LIVE=1`, and print `skipped: FILI_TEST_LIVE not set` otherwise.
- (b) Keep the fixed text but add a coarse stage tag (`registry`, `host`, `read-back`, `cleanup`) and the
  non-credential reason strings.

### WR-13: Built-in crawler disables TLS certificate verification

**File:** `src/crawler.rs:62-69` (outside the review file list, noticed while confirming CR-02)
**Issue:** `build_client()` calls `danger_accept_invalid_certs(true)` for every page fetched by
`fili produce` without a `[crawler]` fetcher. The comment says "sandbox only" but the code is
unconditional, which allows a man-in-the-middle to inject content into `filimon-raw-pages`.
**Fix:** Remove it, or make it an explicit opt-in (`--insecure` or an env var) off by default.

## Info

### IN-01: Dead `Cargo.lock` line in `.gitignore`

**File:** `.gitignore:3`
**Issue:** `Cargo.lock      # remove this line ...` is not a comment in gitignore syntax (`#` is
a comment only at the start of a line), so the pattern is the whole string and matches nothing.
`Cargo.lock` is tracked (root and every plugin), which is the intent. The line is misleading dead
text.
**Fix:** Delete the line and keep the tracked lock files.

### IN-02: `is_stale` is mtime-only and ignores toolchain and config inputs

**File:** `src/plugins/host/testing.rs:188-203`
**Issue:** Inputs are `src/`, `Cargo.toml`, `Cargo.lock`, `wit/` and the extras. Edits to
`rust-toolchain.toml`, `.cargo/config.toml` or a `build.rs` do not trigger a rebuild. An older
source mtime restored by a cache or `cp -p` also keeps a wasm that no longer matches. The risk is a
stale component in a green test run.
**Fix:** Add those files to `inputs`. Consider hashing instead of comparing mtimes.

### IN-03: Selftest step 8 greps the script's own source text

**File:** `scripts/live-e2e.sh:833-834`
**Issue:** `grep -qE 'fili_run --registry ... produce ' "$SELF"` proves a string exists in the
file, not that step 1 behaves correctly. A comment or dead branch containing that text would
satisfy it.
**Fix:** Delete it, or stub `FILI_BIN` with a script that records its arguments and run step 1
against it.

### IN-04: Debug formatting is used to write TOML strings

**File:** `src/plugins/host/integration_tests.rs:287,293`; `src/plugins/host/testing.rs:72,120,125,142,154`
**Issue:** `{client_id:?}`, `{contact:?}` and `{fetcher:?}` produce Rust escapes (`\u{301}`, `\'`),
which are not always valid TOML. A credential or path with non-ASCII or unusual characters yields
an unparseable or different value. The shell side validates `client_id` and the contact, but the
Rust side does not.
**Fix:** Serialise with `toml::Value::String(...).to_string()`, or validate the values the way the
shell does.

### IN-05: Transcript drain relies on `sleep 1`

**File:** `scripts/verify-authoring.sh:68-70,104-107`
**Issue:** `exec > >(tee -a "$PROOF_TRANSCRIPT")` never waits for the `tee` process. The
`sleep 1` in `cleanup` hopes it drains. On a slow disk the last lines (the PASS or FAIL banner) can
be missing from the committed transcript.
**Fix:** Use a named pipe or `tee` in a coprocess and `wait` for it, or run the script's body in a
function piped into `tee` and use `PIPESTATUS`.

### IN-06: Secret pattern file survives interrupts

**File:** `scripts/live-e2e.sh:378-390`
**Issue:** `pf` holds each secret value in turn in a temp file (mode 600). It is removed only on the
normal path, so an interrupt or failure leaves the last secret in `$TMPDIR`.
**Fix:** `trap 'rm -f "$pf"' RETURN` right after `mktemp`, or write the pattern to a pipe via
process substitution.

### IN-07: A failed topic delete is dropped when the test body already failed

**File:** `src/plugins/host/integration_tests.rs:512-520`
**Issue:** On `Ok(outcome)` with `outcome` an `Err`, `outcome?` returns before `deleted?`, so a
failed `delete_topic` leaves a `filimon-test-*` topic with no report. A real cluster slowly
accumulates scratch topics when failures and delete errors coincide.
**Fix:** `eprintln!` the delete error (it carries only the topic name) before returning the body's
error.

### IN-08: Shell dotenv parser does not follow the repository parser as claimed

**File:** `scripts/live-e2e.sh:89,133-144`; `src/plugins/secrets.rs:93-103`
**Issue:** The header says the parser follows the literal rules of the repository's parser. The
Rust `read_dotenv` rejects the whole file on a duplicate key or a bad line (for example unquoted
`a$b`, per the test at `integration_tests.rs:1146`). The shell parser takes the first occurrence
and skips bad lines. The script can therefore preflight "key is set" for a file that `fili` later
rejects, and the secret scan covers different values than `fili` reads.
**Fix:** Have preflight call a repo command that loads the registry, or fail on duplicate and
malformed lines in the shell parser.

### IN-09: Token-marker scan can reject legitimate news content

**File:** `scripts/live-e2e.sh:392-397`
**Issue:** `grep -i -e 'bearer' -e 'access_token'` over every evidence file fails on any news or
social record whose text contains those words (for example a post about bearer tokens). The run
then fails at the last step with "remove it and rerun", but the sample comes from live data.
**Fix:** Scan only the transcript and `WAIVER.md` for the markers, or match the header form
(`authorization: bearer `) rather than the bare word.

---

_Reviewed: 2026-11-15_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
