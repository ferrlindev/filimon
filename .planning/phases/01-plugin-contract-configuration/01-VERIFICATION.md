---
phase: 01-plugin-contract-configuration
verified: 2026-11-03T12:22:51Z
status: human_needed
score: 64/67 must-haves verified
covered_files:
  - ".planning/phases/01-plugin-contract-configuration/01-01-PLAN.md"
  - ".planning/phases/01-plugin-contract-configuration/01-01-SUMMARY.md"
  - ".planning/phases/01-plugin-contract-configuration/01-02-PLAN.md"
  - ".planning/phases/01-plugin-contract-configuration/01-02-SUMMARY.md"
  - ".planning/phases/01-plugin-contract-configuration/01-03-PLAN.md"
  - ".planning/phases/01-plugin-contract-configuration/01-03-SUMMARY.md"
  - ".planning/phases/01-plugin-contract-configuration/01-04-PLAN.md"
  - ".planning/phases/01-plugin-contract-configuration/01-04-SUMMARY.md"
  - ".planning/phases/01-plugin-contract-configuration/01-05-PLAN.md"
  - ".planning/phases/01-plugin-contract-configuration/01-05-SUMMARY.md"
  - ".planning/phases/01-plugin-contract-configuration/01-06-PLAN.md"
  - ".planning/phases/01-plugin-contract-configuration/01-06-SUMMARY.md"
  - "Cargo.lock"
  - "Cargo.toml"
  - "examples/plugins/.env.example"
  - "examples/plugins/filimon.toml"
  - "examples/plugins/reddit.toml"
  - "examples/plugins/stub.toml"
  - "examples/plugins/youtube.toml"
  - "plugins/README.md"
  - "plugins/stub-fetcher/Cargo.toml"
  - "plugins/stub-fetcher/src/lib.rs"
  - "plugins/stub-source/Cargo.toml"
  - "plugins/stub-source/src/lib.rs"
  - "src/cli.rs"
  - "src/main.rs"
  - "src/plugins/config.rs"
  - "src/plugins/error.rs"
  - "src/plugins/mod.rs"
  - "src/plugins/secrets.rs"
  - "wit/host.wit"
  - "wit/types.wit"
  - "wit/worlds.wit"
covered_digest: "v4:sha256:13712f796d5452513b70aee4cd50f82a05832dcd2a5e8b3e4132c15e51aa1e09"
behavior_unverified: 0
overrides_applied: 0
re_verification:
  previous_status: gaps_found
  previous_score: 42/46
  gaps_closed:
    - "CONF-02 / phase goal: API keys and client secrets load from `.env` — the value a plugin receives in `init` is the value the operator wrote in `.env`, and the `.env` format `fili` loads is stable and documented"
    - "Coincidental reliance (IN-03): `fili --registry` default test no longer depends on FILI_REGISTRY being unset"
  gaps_remaining: []
  regressions: []
deferred:
  - truth: "`fili` itself loads and validates the registry at runtime (load_registry and Cli.registry have no non-test caller yet; the module carries a non-test dead_code allow)"
    addressed_in: "Phase 3 / Phase 4"
    evidence: "Phase 3 SC1: 'Source and fetcher components listed and enabled in filimon.toml are loaded'; Phase 4 SC2: 'fili plugins check <name> reports ... whether its TOML and required .env secrets are valid. A broken component or config exits non-zero'"
  - truth: "Allowlist enforcement point for a fetcher's outbound-http.send and allowlist host normalization (review WR-06, WR-01)"
    addressed_in: "Phase 5"
    evidence: "Phase 5 SC2: 'The host rejects, with a clear error, any request to a domain outside the source's allowlist ... This holds for both the default fetcher and a custom fetcher.' (FETCH-02)"
advisory:
  - finding: "IN-09: Unicode whitespace (e.g. U+00A0) delimits unquoted values and starts comments, so `K=abc<NBSP>#tail` reads as `abc`"
    category: other
    reason: "Reproduced by the verifier on a copy of parse_line. It matches the plan's explicit F-rule ('Whitespace means char::is_whitespace'), so no must-have is broken; README says only 'whitespace'. Resolve by restricting delimiters to space/tab or documenting Unicode whitespace."
    evidence_status: "probe reproduced; consistent with documented plan decision, so not a must-have failure"
human_verification:
  - test: "Prohibition (test-tier, 01-01): no WIT parameter, record field or import carries an OAuth refresh token; the only per-poll credential is `access-token: option<string>`"
    expected: "Reviewer reads wit/*.wit and confirms. Verifier re-ran `grep -niE 'refresh[a-z-]*\\s*:' wit/*.wit` (no match); wit/ unchanged since prior verification"
    why_human: "Fail-closed rule: test-tier prohibition with no committed, wired test enforcing it. Flagged unverified-prohibition — human review recommended."
  - test: "Prohibition (test-tier, 01-01): no fetch-contract field or option disables or weakens TLS certificate validation"
    expected: "Reviewer confirms `record request` in wit/types.wit is exactly method/url/headers/body (verifier saw lines 55-60). `grep -niE 'insecure|skip[-_]?verify|danger|verify|cert' wit/*.wit` has no match"
    why_human: "Fail-closed rule: test-tier prohibition with no committed, wired test enforcing it. Flagged unverified-prohibition — human review recommended."
  - test: "Backstop truth (01-02 Edge CONF-01/concurrency, reaffirmed by 01-05 F1): each config file is read exactly once and both the parsed value and its NamedSource come from that one String"
    expected: "Exactly three read_to_string calls: config.rs:211 (registry), config.rs:500 (plugin file), secrets.rs:69 (.env); each parse and diagnostic source is built from that text"
    why_human: "verification: backstop (non-inferable). Presence+wiring never qualifies; no held-out test exercises a concurrent edit. Marked insufficient_spec."
---

# Phase 1: Plugin Contract & Configuration Verification Report

**Phase Goal:** Plugin authors and operators have a stable, versioned contract to write against: WIT worlds for source and fetcher plugins, plus the registry, per-plugin TOML, and `.env` secret formats that `fili` loads and validates
**Verified:** 2026-11-03T12:22:51Z
**Status:** human_needed
**Re-verification:** Yes — after gap closure (plans 01-05, 01-06)

Scope since the prior verification (`git diff --stat 4821ae5 HEAD`, excluding `.planning`): Cargo.lock (-7), Cargo.toml (-1), examples/plugins/.env.example (+4), plugins/README.md (+61), src/cli.rs, src/plugins/{config,error,secrets}.rs. `wit/`, `plugins/stub-*`, `src/main.rs`, `src/plugins/mod.rs` and the sample TOMLs are unchanged, so the earlier passing truths on them get a regression check only.

## Goal Achievement

### Roadmap Success Criteria

| # | Success Criterion | Status | Evidence |
|---|-------------------|--------|----------|
| SC1 | Versioned WIT package defines `source` and `fetcher` worlds that validate; exact exports and imports | ✓ VERIFIED (regression) | `wasm-tools component wit wit/` rc=0 this session; wit/ unchanged since prior full verification |
| SC2 | Stub source and fetcher build from the WIT and conform to their worlds | ✓ VERIFIED (regression) | Stub crates and WIT unchanged since prior fresh build and `targets` checks |
| SC3 | Sample registry loads into typed config; each plugin's secrets contain only its own `.env` keys | ✓ VERIFIED | `plugins::tests::loads_sample_registry` and `sample_env_example_lists_every_declared_secret` pass under the new parser. Value integrity, the prior gap, is now verified (row 46) |
| SC4 | Missing plugin TOML, malformed TOML or missing secret gives an error naming plugin and field | ✓ VERIFIED | `missing_plugin_file`, `malformed_plugin_toml`, `missing_secret`, `collects_all_problems` pass; `.env` problems are now one per bad line with no false MissingSecret cascade |

### Observable Truths

Rows 1-45 were verified in full previously. They are regression-checked here: all 45 pre-existing tests still pass in the single full run, and the files they rest on are unchanged unless noted.

**Plan 01-01 (WIT-01, WIT-02): rows 1-12** ✓ VERIFIED (regression). wit/ and stub crates unchanged; `wasm-tools component wit wit/` rc=0.

**Plan 01-02 (CONF-01, CONF-02): rows 13-26**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 13-20, 22, 23, 25, 26 | Typed load, defaults, alias-only secrets, disabled sources, redaction, workspace exclude, empty and ordering edges, D-12 precedence, no env mutation | ✓ VERIFIED (regression) | All named tests pass; `grep -rnE 'set_var\|remove_var' src/plugins/` has no match |
| 21 | Edge concurrency (backstop): one read per file | ⚠️ insufficient_spec | Three `read_to_string` calls (config.rs:211, :500; secrets.rs:69). Backstop truth needs held-out evidence, so it goes to the human items |
| 24 | Edge encoding: case-sensitive lookup; `export X="a b"` gives `a b` | ✓ VERIFIED | Superseded by the literal parser: `env_lookup_is_case_sensitive`, `dotenv_export_prefix_and_double_quotes` (renamed), `dotenv_accepts_documented_forms` |

**Plan 01-03 (CONF-03): rows 27-35** ✓ VERIFIED (regression). Every named diagnostic test passes. `collects_all_problems` passes unchanged.

**Plan 01-04 (CONF-01, CONF-02): rows 36-40**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 36-38, 40 | Sample registry, isolation, `.env.example` names, `--registry` feeds loader | ✓ VERIFIED (regression) | Tests pass; `.env.example` non-comment lines are still the three empty `KEY=` lines |
| 39 | `--registry` / FILI_REGISTRY / default `filimon.toml` | ✓ VERIFIED | Coincidental reliance removed: cli.rs:102-113 asserts the default from `Cli::command()` metadata via `get_default_values()` with no early return. The full suite ran with `FILI_REGISTRY=/nonexistent/elsewhere.toml` exported, and all 4 `cli::tests::` passed. `fili --help` shows `[env: FILI_REGISTRY=] [default: filimon.toml]` |

**Prohibitions from 01-01..01-03: rows 41-45**

| # | Prohibition | Tier | Status | Evidence |
|---|-------------|------|--------|----------|
| 41 | No secret values in logs/Debug/diagnostics | test | ✓ VERIFIED | `secret_debug_is_redacted`, `rendered_report_omits_secret_values`, `malformed_dotenv_reports_line_not_content` pass |
| 42 | `.env` never copied into process env | test | ✓ VERIFIED | `loading_does_not_mutate_process_env` |
| 43 | Never silently skip an invalid plugin | test | ✓ VERIFIED | `collects_all_problems`, `invalid_plugin_name` |
| 44 | No refresh token in WIT (AUTH-03) | test | ⚠️ FLAGGED unverified-prohibition | Holds by grep; no committed test enforces it (fail-closed) |
| 45 | No TLS-weakening option in fetch contract | test | ⚠️ FLAGGED unverified-prohibition | Holds by grep; no committed test enforces it (fail-closed) |

**Requirement-derived truth (prior gap) — merged with 01-05 truth 1**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 46 | CONF-02 / goal: `.env` values reach the plugin exactly as written between delimiters, or the line is rejected; the format is stable and documented | ✓ VERIFIED | `dotenvy` is gone: `git grep dotenvy -- Cargo.toml src/` is empty, there is no `name = "dotenvy"` in Cargo.lock, and the Cargo.lock diff is 0 added / 7 deleted lines. The new parser (secrets.rs:68-236) is hand-written and takes only line text. `dotenv_single_quoted_dollar_round_trips` delivers `pa$$word` end to end. A verifier probe on a copy of `parse_line` confirmed: `PW=pa$$word`, `Q="x$HOME/y"` and `S=abc$NOPE` are rejected with the DOLLAR reason; `R='pa$$word'` gives `pa$$word`; `B='${PATH}'` gives `${PATH}` |

**Plan 01-05 (CONF-02, CONF-03): rows 47-62**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 47 | `ALPHA_TOKEN=pa$$word` gives exactly one DotenvInvalid at line 1; no `pa$$word`/`$$word` in report or Debug | ✓ VERIFIED | `dotenv_unquoted_dollar_is_rejected` (asserts len 1, reason contains `single-quote`, checks both leak strings) |
| 48 | CR-01 reproduction lines rejected at line 1; `R='pa$$word'` literal | ✓ VERIFIED | `dotenv_dollar_outside_single_quotes_is_rejected` |
| 49 | Parsing never consults any environment; `'$PATH'`/`'${PATH}'` literal; crate removed | ✓ VERIFIED | `dotenv_never_expands_process_env` (the closure asserts it is never called with `PATH`); dependency-removal greps above |
| 50 | Accepted forms F1-F7 yield documented values | ✓ VERIFIED | `dotenv_accepts_documented_forms` uses full-map equality over 17 keys, including decomposed Unicode |
| 51 | Rejected forms R1-R8 give one DotenvInvalid at their line, fixed reason, no value text | ✓ VERIFIED | `dotenv_rejects_ambiguous_forms` (13 cases, exact reason equality, `SECRETx` omitted) and `dotenv_duplicate_key_is_rejected` |
| 52 | Line numbers counted (IN-02) | ✓ VERIFIED | `dotenv_line_number_is_counted`; `fn line_containing` absent; `enumerate()` at secrets.rs:80 |
| 53 | WR-02: unusable `.env` reports only DotenvInvalid, no MissingSecret | ✓ VERIFIED | `malformed_dotenv_reports_line_not_content` (len 1), `unusable_dotenv_suppresses_missing_secret`; config.rs pass 2 passes `None` and records problems |
| 54 | WR-03: empty process var keeps D-12 precedence; EmptySecret names its origin and gives a working fix | ✓ VERIFIED | `empty_process_var_names_process_env`, `empty_dotenv_value_names_dotenv_file`; error.rs EmptySecret has `origin` and `#[help] fix` |
| 55 | Operators can find the format in `.env.example` and README `## Plugin secrets` | ✓ VERIFIED | Read both files: they state "never expands", show `KEY='pa$$word'`, list rejected forms including duplicate keys, and name the process environment as the escape hatch |
| 56 | Prior behavior holds; full suite passes | ✓ VERIFIED | `cargo test --bin fili`: 59 passed, 0 failed (45 prior + 14 new) |
| 57 | Edge adjacency: process wins even when empty; duplicate key rejected at second line naming key and first line | ✓ VERIFIED | `process_env_overrides_dotenv`, `empty_process_var_names_process_env`, `dotenv_duplicate_key_is_rejected` |
| 58 | Edge empty: four empty forms give ""; EmptySecret when aliased; empty/missing file gives an empty map | ✓ VERIFIED | `dotenv_accepts_documented_forms`, `empty_secret_value_is_error`, `missing_dotenv_file_is_empty` (secrets module) |
| 59 | Edge encoding: exact UTF-8, no trimming inside quotes, no normalisation, CRLF + BOM | ✓ VERIFIED | `dotenv_accepts_documented_forms` (`PADDED`, `DECOMPOSED`), `dotenv_accepts_crlf_and_bom`, `env_lookup_is_case_sensitive` |
| 60 | Edge ordering: duplicates rejected; DotenvInvalid in ascending line order; alias order kept | ✓ VERIFIED | `dotenv_reports_every_bad_line_in_order`, `secrets_iterate_in_alias_order` |
| 61 | Edge concurrency: parser reads no environment; loading never mutates it | ✓ VERIFIED | `dotenv_never_expands_process_env`, `loading_does_not_mutate_process_env` |
| 62 | Edge CONF-03 concurrency: registry problems, then DotenvInvalid in line order, then per-source; stable across loads | ✓ VERIFIED | `dotenv_problems_keep_line_order` (exact sequence InvalidName, line 2, line 4; two loads compared by Debug) |

**Plan 01-06 (CONF-01): rows 63-64** (01-06 truth 1 is merged into row 39)

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 63 | With FILI_REGISTRY exported, all four cli tests run and pass; no early return | ✓ VERIFIED | Full suite run with `FILI_REGISTRY=/nonexistent/elsewhere.toml`: 4/4 `cli::tests::` ok; diff shows the early return removed |
| 64 | Production CLI unchanged; only test code in cli.rs changed | ✓ VERIFIED | `git diff 4821ae5 HEAD -- src/cli.rs` touches only `registry_flag_defaults_to_filimon_toml`; `--help` output unchanged |

**Prohibitions from 01-05: rows 65-67**

| # | Prohibition | Tier | Status | Evidence |
|---|-------------|------|--------|----------|
| 65 | MUST NOT deliver a `.env` value differing from the text between delimiters; reject instead | test | ✓ VERIFIED (wired tests) | `dotenv_single_quoted_dollar_round_trips`, `dotenv_accepts_documented_forms`, `dotenv_rejects_ambiguous_forms`. Note advisory IN-09: under the documented rule that "whitespace" means `char::is_whitespace`, U+00A0 counts as a delimiter |
| 66 | MUST NOT expand `$NAME`/`${NAME}` | test | ✓ VERIFIED (wired tests) | `dotenv_never_expands_process_env`, `dotenv_dollar_outside_single_quotes_is_rejected` |
| 67 | MUST NOT include rejected-line value text in diagnostics/logs/Debug | test | ✓ VERIFIED (wired tests) | `assert_omits` in `dotenv_rejects_ambiguous_forms`, `dotenv_duplicate_key_is_rejected`, `dotenv_unquoted_dollar_is_rejected`. The duplicate reason does carry the key name, which the plan allows (IN-10) |

**Score:** 64/67 truths verified (row 21 insufficient_spec; rows 44-45 flagged unverified-prohibitions; 0 present-but-behavior-unverified)

### Deferred Items

| # | Item | Addressed In | Evidence |
|---|------|-------------|----------|
| 1 | `fili` has no non-test caller of load_registry / Cli.registry yet | Phase 3 / 4 | Phase 3 SC1; Phase 4 SC2 |
| 2 | outbound-http.send allowlist attribution and allowlist normalization (WR-06, WR-01) | Phase 5 | Phase 5 SC2 / FETCH-02 |

### Advisory (New Scope, Unevidenced)

| # | Finding | Category | Why Advisory |
|---|---------|----------|--------------|
| 1 | IN-09: Unicode whitespace delimits unquoted values (`K=abc<NBSP>#tail` reads as `abc`) | other | Reproduced, but it follows the plan's explicit whitespace rule, so no must-have fails. Documentation and implementation detail to triage |

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/plugins/secrets.rs` | Literal parser, `fn parse_line`, cascade suppression, origin-aware EmptySecret | ✓ VERIFIED | gsd verify.artifacts 5/5 (01-05); read in full |
| `src/plugins/error.rs` | EmptySecret `origin` + `#[help] fix` | ✓ VERIFIED | error.rs EmptySecret variant; old static help removed |
| `src/plugins/config.rs` | `Option<HashMap>` wiring; e2e `.env` tests | ✓ VERIFIED | Pass 2/3 read |
| `examples/plugins/.env.example` | Quoting rule | ✓ VERIFIED | 4 comment lines + 3 unchanged keys |
| `plugins/README.md` | `## Plugin secrets` section | ✓ VERIFIED | Section present before `## Contract version` |
| `src/cli.rs` | `get_default_values` assertion | ✓ VERIFIED | gsd verify.artifacts 1/1 (01-06) |
| WIT, stubs, sample TOMLs | (prior) | ✓ VERIFIED (regression) | Unchanged |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| config.rs | secrets.rs | `read_dotenv(` once; Err → DotenvInvalid appended, `None` passed on | ✓ WIRED | gsd tool verified; code at config.rs pass 2 |
| secrets.rs | error.rs | `EmptySecret {` with origin/fix; DotenvInvalid per line | ✓ WIRED | gsd tool verified |
| .env.example | secrets.rs | commented sample parses under new format | ✓ WIRED | `sample_env_example_lists_every_declared_secret` passes |
| cli.rs test | Cli.registry / DEFAULT_REGISTRY | `Cli::command()` → `get_default_values()` | ✓ WIRED | gsd tool false negative (parenthetical in `from:` path); manually confirmed cli.rs:104-112 |
| Prior links (stubs→WIT, main→plugins, config→error, cli→config) | | | ✓ WIRED (regression) | Unchanged files |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| LoadedRegistry.sources[*].plugin.secrets | Secret values | injected env closure, then `.env` via literal `read_dotenv` | Yes, verbatim or rejected | ✓ FLOWING (prior corruption fixed) |
| PluginConfig.settings_toml | `[settings]` | plugin TOML | Yes | ✓ FLOWING |
| HostConfig | `[host]` | plugin TOML typed parse | Yes | ✓ FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Full fili suite (run once, FILI_REGISTRY exported) | `FILI_REGISTRY=/nonexistent/elsewhere.toml cargo test --bin fili` | 59 passed, 0 failed; all 30 named tests checked against saved output | ✓ PASS |
| Clippy on phase code | `cargo clippy --bin fili --message-format=short` | rc=0; no line under `src/plugins/` or `src/cli.rs` (remaining warnings are in baseline files) | ✓ PASS |
| WIT validates | `wasm-tools component wit wit/` | rc=0 | ✓ PASS |
| Help output | `cargo run --bin fili -- --help` | `--registry ... [env: FILI_REGISTRY=] [default: filimon.toml]` | ✓ PASS |
| Parser probe (copy of secrets.rs:113-236 in /tmp, outside repo) | `rustc probe.rs && ./probe` | `$` forms rejected; single-quoted `$` literal; `K="it's"` gives `it's`; `K='it's'` rejected (IN-11); bare `dGhpc2lzYXNlY3JldA==` accepted as key with value `=` (IN-10); NBSP truncation (IN-09) | ✓ PASS (gap behavior) / info findings confirmed |
| SUMMARY commits exist | `gsd query verify.commits` (7 hashes) | all_valid | ✓ PASS |

### Probe Execution

No probes declared in the PLAN or SUMMARY files, and no `scripts/*/tests/probe-*.sh` apply. Step 7c: not applicable.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| WIT-01 | 01-01 | Versioned WIT `source` world | ✓ SATISFIED | Rows 1-2, 5, 7-11 (regression) |
| WIT-02 | 01-01 | `fetcher` world with outbound-HTTP import | ✓ SATISFIED | Rows 3, 6, 12 (regression) |
| CONF-01 | 01-02, 01-04, 01-06 | Per-plugin TOML referenced from filimon.toml | ✓ SATISFIED | Rows 13-14, 36-40, 63-64 |
| CONF-02 | 01-02, 01-04, 01-05 | Secrets load from `.env`; each plugin gets only its own | ✓ SATISFIED | Isolation (15, 37) and value integrity (46-61, 65-67) |
| CONF-03 | 01-03, 01-05 | Clear error naming plugin and field | ✓ SATISFIED (warning WR-09) | Rows 27-35, 51-53, 62 |

Orphaned requirements: none. REQUIREMENTS.md maps exactly these five IDs to Phase 1, and plans claim all of them. Bookkeeping note: REQUIREMENTS.md marks CONF-01..03 Complete, which this verification supports. WIT-01 and WIT-02 are still `[ ]` / Pending even though both are satisfied.

### Anti-Patterns Found

No TBD/FIXME/XXX/TODO/HACK/PLACEHOLDER markers in any phase-modified file (grep rc=1). No stubs on user-visible paths.

Code review 01-REVIEW.md (incremental, 0 critical / 6 warning / 5 info), weighed against the must-haves:

| ID | File | Severity (this report) | Impact on goal |
|----|------|------------------------|----------------|
| CR-01, WR-02, WR-03, IN-02, IN-03 | secrets.rs, config.rs, error.rs, cli.rs | Resolved | Independently confirmed (rows 39, 46-54). 01-REVIEW-DISPOSITION.md still lists them `open`; that is bookkeeping, not code |
| WR-07 | config.rs:256-272, secrets.rs:80-102 | ⚠️ Warning | One bad line for a key fili never reads fails the whole load, even when the process environment supplies every secret. This is fail-closed, not a wrong value, and the README states it ("One bad line makes the whole file unusable"). But the advice to keep fili's `.env` separate has no knob other than moving the registry. It degrades operability with a shared docker-compose `.env`; it does not defeat a success criterion. Recommend triage before Phase 3/4 wires production loading |
| WR-08 | secrets.rs:247-250, 294-295 | ⚠️ Warning (latent) | `resolve_secrets(None)` can return Ok with an alias missing. The only caller always fails the load on that path, because registry_problems is non-empty, so there is no current fail-open. A risk for future pub callers |
| WR-09 | error.rs:81-84 | ⚠️ Warning | For an env var name outside `[A-Za-z_][A-Za-z0-9_.]*` (e.g. `REDDIT-TOKEN`), MissingSecret's help prescribes a `.env` line that the parser rejects. It still names plugin and field (SC4 holds); only the help is wrong in this edge case |
| WR-04, WR-05 | config.rs | ⚠️ Warning (carried) | Unchanged; no success criterion depends on them |
| WR-01, WR-06 | config.rs, wit/host.wit | ⚠️ Warning (deferred, Phase 5) | Unchanged |
| IN-09 | secrets.rs | 📋 Advisory | See Advisory table |
| IN-10, IN-11, IN-12, IN-07 | secrets.rs, .env.example, config.rs | ℹ️ Info | IN-10 (bare base64 line accepted as key with value `=`) and IN-11 (circular `'` guidance) were confirmed by the probe; neither alters a delivered secret value |

### Human Verification Required

1. **Prohibition: no refresh token in WIT (01-01, test-tier, no wired test)**
   **Test:** Read wit/*.wit.
   **Expected:** Only `access-token: option<string>` crosses into `poll`; no field, parameter or import carries a refresh token.
   **Why human:** Fail-closed: a test-tier prohibition with no committed enforcement.

2. **Prohibition: no TLS-weakening option (01-01, test-tier, no wired test)**
   **Test:** Read `record request` in wit/types.wit (lines 55-60).
   **Expected:** Exactly method, url, headers, body.
   **Why human:** Same as above.

3. **Backstop truth: single read per config file (01-02, reaffirmed by 01-05 F1)**
   **Test:** Read config.rs:211 and :500 and secrets.rs:69.
   **Expected:** One `read_to_string` per file; the parse and the NamedSource are built from the same String.
   **Why human:** `verification: backstop`, where presence is not evidence.

These three items carry over unchanged from the prior verification. Neither gap-closure plan touched them. Adding one `#[test]` that greps wit/*.wit for the two prohibitions would turn items 1-2 green on the next verification.

### Gaps Summary

The only prior gap, CONF-02 `.env` value corruption (CR-01), is closed. The substituting `dotenvy` crate is removed from Cargo.toml, Cargo.lock and src/. A hand-written literal parser delivers each value verbatim or rejects its line, with a line number and a fixed reason that never carries the value. Fourteen new tests pin this end to end, and the verifier reproduced the key cases on an independent copy of the parser. The coincidental-reliance item (IN-03) is also closed: the default-registry test asserts on clap metadata and passed with FILI_REGISTRY exported. The full suite is 59/59, clippy is clean on phase code, and the WIT still validates.

No must-have fails. Status is `human_needed` only because of the three unchanged fail-closed items above: two test-tier WIT prohibitions with no wired test, and one backstop truth.

The orchestrator may keep CONF-01..03 marked Complete. The new review warnings WR-07 (one unrelated bad `.env` line fails the load even when the process environment supplies everything) and WR-09 (MissingSecret help can prescribe a rejected line) are the main open design concerns. They do not defeat a success criterion, but they are worth triaging before Phase 3/4 adds production callers of the loader.

---

_Verified: 2026-11-03T12:22:51Z_
_Verifier: Claude (gsd-verifier)_
