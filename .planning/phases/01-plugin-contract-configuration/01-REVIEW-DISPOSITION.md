---
phase: 01
review: 01-REVIEW.md
titles: json
findings:
  - id: WR-07
    severity: warning
    disposition: open
    title: "One bad line for an unrelated key makes `.env` unusable and fails the load, even when every secret comes from the process environment"
  - id: WR-08
    severity: warning
    disposition: open
    title: "`resolve_secrets` returns `Ok` with missing secrets whenever `dotenv` is `None`, and the loader logs \"resolved plugin secrets\" for them"
  - id: WR-09
    severity: warning
    disposition: open
    title: "`MissingSecret` can prescribe a `.env` line the new parser rejects (escalates IN-08)"
  - id: WR-01
    severity: warning
    disposition: open
    title: "Allowlist entries are checked with `url::Host::parse` but stored un-normalized; wildcards pass (carried, still open)"
  - id: WR-04
    severity: warning
    disposition: open
    title: "A non-UTF-8 process variable is silently treated as absent, and `.env` is used instead (carried, still open)"
  - id: WR-05
    severity: warning
    disposition: open
    title: "No upper bounds on durations or the backoff multiplier (carried, still open)"
  - id: IN-09
    severity: info
    disposition: open
    title: "Unicode whitespace delimits unquoted values and starts comments, so `KEY=abc<NBSP>#x` silently becomes `abc`"
  - id: IN-10
    severity: info
    disposition: open
    title: "The duplicate-key reason echoes line text, contradicting the module's own invariant; bare base64 lines parse as keys"
  - id: IN-11
    severity: info
    disposition: open
    title: "The guidance for values containing `'` is circular"
  - id: IN-12
    severity: info
    disposition: open
    title: "`read_dotenv` returns every `.env` value, including unrelated keys, as a plain `HashMap<String, String>` whose Debug prints them"
  - id: IN-07
    severity: info
    disposition: open
    title: "With the default `--registry filimon.toml`, every resolved path is relative to the current directory (carried, still open)"
  - id: CR-01
    severity: critical
    disposition: open
    title: "`.env` secrets are silently mangled by dotenvy `$` substitution, which also reads the real process env"
  - id: WR-02
    severity: warning
    disposition: open
    title: "A malformed `.env` also produces one false MissingSecret per alias"
  - id: WR-03
    severity: warning
    disposition: open
    title: "A set-but-empty process variable overrides a valid `.env` value, and the help text suggests a fix that cannot work"
  - id: WR-06
    severity: warning
    disposition: open
    title: "The WIT contract does not apply the source allowlist to the fetcher's `outbound-http.send`"
  - id: IN-01
    severity: info
    disposition: open
    title: "The WIT doc calls `settings` \"raw\" TOML, but the host sends re-serialised text"
  - id: IN-02
    severity: info
    disposition: open
    title: "`line_containing` can report the wrong `.env` line"
  - id: IN-03
    severity: info
    disposition: open
    title: "The CLI default-registry test passes without checking anything when `FILI_REGISTRY` is set"
  - id: IN-04
    severity: info
    disposition: open
    title: "The stub cursor wraps at 255 and repeats item ids"
  - id: IN-05
    severity: info
    disposition: open
    title: "The sample registry enables sources whose components do not exist and requires their secrets"
  - id: IN-06
    severity: info
    disposition: open
    title: "A blanket `allow(dead_code)` on the whole plugins module hides real dead code"
  - id: IN-08
    severity: info
    disposition: open
    title: "`[host.secrets]` alias names and env var names are not validated"
open: 22
total: 22
recorded: 2026-11-03T12:20:04.084Z
---

# Phase 01: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| WR-07 | warning | open | - |
| WR-08 | warning | open | - |
| WR-09 | warning | open | - |
| WR-01 | warning | open | - |
| WR-04 | warning | open | - |
| WR-05 | warning | open | - |
| IN-09 | info | open | - |
| IN-10 | info | open | - |
| IN-11 | info | open | - |
| IN-12 | info | open | - |
| IN-07 | info | open | - |
| CR-01 | critical | open | - (not in the current review) |
| WR-02 | warning | open | - (not in the current review) |
| WR-03 | warning | open | - (not in the current review) |
| WR-06 | warning | open | - (not in the current review) |
| IN-01 | info | open | - (not in the current review) |
| IN-02 | info | open | - (not in the current review) |
| IN-03 | info | open | - (not in the current review) |
| IN-04 | info | open | - (not in the current review) |
| IN-05 | info | open | - (not in the current review) |
| IN-06 | info | open | - (not in the current review) |
| IN-08 | info | open | - (not in the current review) |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
Set `deferred` by hand and put the reason in the Source cell; both are preserved. A `|` in the reason is kept as prose and escaped on the next run.
Re-running the gate keeps every row it can. A row the current review no longer reports is kept and its Source cell flagged, so a finding does not leave this record silently. ONE exception: when a finding id is REUSED by a different finding, the earlier decision cannot keep a row — the id is taken — and it is dropped. A RECORDED decision (anything but `open`) is named on the console when that happens; a row still at `open` is replaced silently, because `open` records no decision to lose.
