---
phase: 01
review: 01-REVIEW.md
titles: json
findings:
  - id: CR-01
    severity: critical
    disposition: open
    title: "`.env` secrets are silently mangled by dotenvy `$` substitution, which also reads the real process env"
  - id: WR-01
    severity: warning
    disposition: open
    title: "Allowlist entries are checked with `url::Host::parse` but stored un-normalized; wildcards pass"
  - id: WR-02
    severity: warning
    disposition: open
    title: "A malformed `.env` also produces one false MissingSecret per alias"
  - id: WR-03
    severity: warning
    disposition: open
    title: "A set-but-empty process variable overrides a valid `.env` value, and the help text suggests a fix that cannot work"
  - id: WR-04
    severity: warning
    disposition: open
    title: "A non-UTF-8 process variable is silently treated as absent and the `.env` value is used instead"
  - id: WR-05
    severity: warning
    disposition: open
    title: "No upper bounds on durations or the backoff multiplier in a config that claims to be \"validated\""
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
  - id: IN-07
    severity: info
    disposition: open
    title: "With the default `--registry filimon.toml`, all resolved paths are relative to the current directory"
  - id: IN-08
    severity: info
    disposition: open
    title: "`[host.secrets]` alias names and env var names are not validated"
open: 15
total: 15
recorded: 2026-11-03T10:34:14.545Z
---

# Phase 01: Code Review Disposition

| Finding | Severity | Disposition | Source |
|---------|----------|-------------|--------|
| CR-01 | critical | open | - |
| WR-01 | warning | open | - |
| WR-02 | warning | open | - |
| WR-03 | warning | open | - |
| WR-04 | warning | open | - |
| WR-05 | warning | open | - |
| WR-06 | warning | open | - |
| IN-01 | info | open | - |
| IN-02 | info | open | - |
| IN-03 | info | open | - |
| IN-04 | info | open | - |
| IN-05 | info | open | - |
| IN-06 | info | open | - |
| IN-07 | info | open | - |
| IN-08 | info | open | - |

Dispositions: `open` (recorded, not yet triaged), `fixed`, `skipped`, `deferred`.
