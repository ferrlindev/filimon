---
phase: 01-plugin-contract-configuration
verified: 2026-11-03T10:37:53Z
status: gaps_found
score: 42/46 must-haves verified
covered_files:
  - ".planning/phases/01-plugin-contract-configuration/01-01-PLAN.md"
  - ".planning/phases/01-plugin-contract-configuration/01-01-SUMMARY.md"
  - ".planning/phases/01-plugin-contract-configuration/01-02-PLAN.md"
  - ".planning/phases/01-plugin-contract-configuration/01-02-SUMMARY.md"
  - ".planning/phases/01-plugin-contract-configuration/01-03-PLAN.md"
  - ".planning/phases/01-plugin-contract-configuration/01-03-SUMMARY.md"
  - ".planning/phases/01-plugin-contract-configuration/01-04-PLAN.md"
  - ".planning/phases/01-plugin-contract-configuration/01-04-SUMMARY.md"
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
covered_digest: "v4:sha256:35fdf7a8ca326effe356849b0d267568781eefeb23ce7ed7abc9568c7a5679aa"
behavior_unverified: 0
overrides_applied: 0
gaps:
  - truth: "CONF-02 / phase goal: API keys and client secrets load from `.env` — the value a plugin receives in `init` is the value the operator wrote in `.env`, and the `.env` format `fili` loads is stable and documented"
    status: failed
    reason: "read_dotenv parses with dotenvy::from_read_iter, which applies shell-style `$VAR` substitution to unquoted and double-quoted values. Reproduced against the locked dotenvy 0.15.7: `PW=pa$$word` -> `pa`; `Q=\"x$HOME/y\"` -> `x/Users/jferrolino/y` (real process env); `S=abc$NOPE_UNSET_XYZ` -> `abc_UNSET_XYZ`; only `R='pa$$word'` survives verbatim. The corrupted value is non-empty, so it passes the EmptySecret check and is handed to the plugin with no diagnostic. Substitution reads std::env directly, bypassing the injected `env` lookup that load_registry_with_env documents as its only environment source. No test pins `$` behavior, and neither .env.example nor any doc tells operators to single-quote. Code review CR-01 (critical) is still `open` in 01-REVIEW-DISPOSITION.md. Not covered by any later roadmap phase."
    artifacts:
      - path: "src/plugins/secrets.rs"
        issue: "read_dotenv (line ~64) uses dotenvy::from_read_iter, a substituting parser; doc comment at lines 45-46 does not disclose the transformation"
      - path: "examples/plugins/.env.example"
        issue: "does not tell operators that `$` in a value is substituted unless single-quoted"
    missing:
      - "A .env value path that delivers secret text verbatim: either a non-substituting KEY=VALUE parser, or rejection (DotenvInvalid with line number only, no content) of any value containing an unescaped `$` outside single quotes"
      - "Regression tests: `PW=pa$$word` round-trips exactly or is rejected; `PW='pa$$word'` resolves to `pa$$word`; a `$NAME` reference never reads the real process environment (hermetic with the injected env closure)"
      - "Operator-facing note in .env.example (and plugins/README.md or loader docs) describing the quoting rule that results"
deferred:
  - truth: "`fili` itself loads and validates the registry at runtime (load_registry and Cli.registry have no non-test caller yet; the module carries a non-test dead_code allow)"
    addressed_in: "Phase 3 / Phase 4"
    evidence: "Phase 3 SC1: 'Source and fetcher components listed and enabled in filimon.toml are loaded'; Phase 4 SC2: 'fili plugins check <name> reports ... whether its TOML and required .env secrets are valid. A broken component or config exits non-zero'"
  - truth: "Allowlist enforcement point for a fetcher's outbound-http.send and allowlist host normalization (review WR-06, WR-01)"
    addressed_in: "Phase 5"
    evidence: "Phase 5 SC2: 'The host rejects, with a clear error, any request to a domain outside the source's allowlist ... This holds for both the default fetcher and a custom fetcher.' (FETCH-02)"
coincidental_reliance_items:
  - truth: "01-04: `fili --registry` default is `filimon.toml`"
    reason: undeclared-precondition
    harden: "cli::tests::registry_flag_defaults_to_filimon_toml returns early (reports pass) when FILI_REGISTRY is set in the test process; it held here only because FILI_REGISTRY was unset. Assert the default via Cli::command() get_default_values() instead (review IN-03)."
human_verification:
  - test: "Prohibition (test-tier, 01-01): no WIT parameter, record field or import carries an OAuth refresh token; the only per-poll credential is `access-token: option<string>`"
    expected: "Reviewer confirms by reading wit/*.wit; verifier ran `grep -niE 'refresh[a-z-]*\\s*:' wit/*.wit` (no match) and inspected poll/init signatures"
    why_human: "Fail-closed rule: test-tier prohibition with no committed, wired test enforcing it (the grep exists only as a plan acceptance gate). Flagged unverified-prohibition — human review recommended."
  - test: "Prohibition (test-tier, 01-01): no fetch-contract field or option disables or weakens TLS certificate validation"
    expected: "Reviewer confirms record `request` is exactly method/url/headers/body; verifier ran `grep -niE 'insecure|skip[-_]?verify|danger|verify|cert' wit/*.wit` (no match)"
    why_human: "Fail-closed rule: test-tier prohibition with no committed, wired test enforcing it. Flagged unverified-prohibition — human review recommended."
  - test: "Backstop truth (01-02 Edge CONF-01/concurrency): load_registry reads each config file exactly once and builds both the parsed value and its NamedSource from that one String"
    expected: "config.rs: registry read once via read_to_string and NamedSource built from registry_text; read_plugin reads each plugin file once and builds NamedSource from the same text; .env read once"
    why_human: "verification: backstop (non-inferable). Code inspection shows one read per file, but presence+wiring never qualifies; no held-out test exercises a concurrent edit. Marked insufficient_spec."
---

# Phase 1: Plugin Contract & Configuration Verification Report

**Phase Goal:** Plugin authors and operators have a stable, versioned contract to write against: WIT worlds for source and fetcher plugins, plus the registry, per-plugin TOML, and `.env` secret formats that `fili` loads and validates
**Verified:** 2026-11-03T10:37:53Z
**Status:** gaps_found
**Re-verification:** No — initial verification

Baseline: commit `12c57b9` (user's fluvio-wise baseline). Phase work is `12c57b9..HEAD` (`10ef695`). `git diff 12c57b9 HEAD` confirms Cargo.toml gained only 3 deps plus `exclude = ["plugins"]`, `src/main.rs` only `mod plugins;`, and `src/cli.rs` 78 added / 0 deleted lines.

## Goal Achievement

### Roadmap Success Criteria

| # | Success Criterion | Status | Evidence |
|---|-------------------|--------|----------|
| SC1 | Versioned WIT package defines `source` and `fetcher` worlds, both validate with wasm-tools; source exports init(settings, secrets)/poll(cursor, access-token), imports fetch and log; fetcher exports fetch, imports low-level outbound HTTP | ✓ VERIFIED | `wasm-tools component wit wit/` exit 0 (wasm-tools 1.263.0); single package line `filimon:plugin@0.1.0` across 3 files; worlds.wit lines 7-39 show exact signatures; fetcher world `import outbound-http; export fetch;` |
| SC2 | Stub source and stub fetcher generate bindings from the WIT alone via `cargo component` and compile to valid components for their world | ✓ VERIFIED | Fresh `cargo component build --release` (cargo-component 0.21.1) of both crates rc=0; `targets -w source stub_source.wasm` rc=0; `targets -w fetcher stub_fetcher.wasm` rc=0; cross checks rc=1 both directions; Cargo.toml `path = "../../wit"`; no `wasi:` imports |
| SC3 | Sample filimon.toml -> per-plugin TOMLs loads into typed config; each plugin's secrets contain only the .env keys its own TOML names | ✓ VERIFIED (key isolation) | `plugins::tests::loads_sample_registry` passes: reddit {client_id, client_secret}, youtube {api_key}, stub {}, UNRELATED_TOKEN absent by key and value. Value integrity is a separate CONF-02 gap (row 46) |
| SC4 | Missing plugin TOML, malformed TOML, or missing .env secret -> error naming plugin and field/key | ✓ VERIFIED | Tests `missing_plugin_file` (renders `sources.alpha.config`), `malformed_plugin_toml`, `unknown_host_key`, `zero_poll_interval`, `missing_host_table`, `missing_secret` (renders `host.secrets.client_id`, `REDDIT_CLIENT_ID`, `declared here`, `reddit.toml`) all pass |

### Observable Truths (plan must_haves)

**Plan 01-01 (WIT-01, WIT-02)**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | `wasm-tools component wit wit/` exits 0 with package `filimon:plugin@0.1.0`, worlds `source` and `fetcher` | ✓ VERIFIED | rc=0, ran this session |
| 2 | source exports init/poll with exact signatures, imports only log and fetch | ✓ VERIFIED | Component world printout: imports types (type-only), log, fetch; exports init, poll with exact signatures |
| 3 | fetcher exports `fetch` interface, no init, imports log and outbound-http | ✓ VERIFIED | worlds.wit:34-39 |
| 4 | poll-error and fetch-error cases exact | ✓ VERIFIED | types.wit:33-44 and 70-83 |
| 5 | stub-source builds from ../../wit and `targets -w source` accepts it | ✓ VERIFIED | Fresh build + targets rc=0 |
| 6 | stub-fetcher same; stub-source rejected against fetcher | ✓ VERIFIED | targets rc=0; cross check rc=1 |
| 7 | stub-source imports filimon:plugin/fetch and /log, no wasi | ✓ VERIFIED | `wasm-tools component wit` printout, no `wasi:` |
| 8 | No interface shares a name with a world; identical package line in every file | ✓ VERIFIED | interfaces: types, log, fetch, outbound-http; worlds: source, fetcher; `sort -u` of package lines = 1 |
| 9 | Edge empty: cursor none = first poll, items may be empty, docs state it | ✓ VERIFIED | types.wit:24-31, worlds.wit:21-27 |
| 10 | Edge ordering: items produced in list order | ✓ VERIFIED | types.wit:25-26 |
| 11 | Edge concurrency: init once before poll; no concurrent poll | ✓ VERIFIED | worlds.wit:3-6, 13, 26-27 |
| 12 | Edge WIT-02 concurrency: fetcher holds no state | ✓ VERIFIED | worlds.wit:31-33 |

**Plan 01-02 (CONF-01, CONF-02)**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 13 | load_registry_with_env returns typed LoadedRegistry, paths registry-relative | ✓ VERIFIED | `loads_minimal_registry` pass; config.rs base_dir.join |
| 14 | [host] -> HostConfig with defaults; [settings] re-serialised | ✓ VERIFIED | `loads_minimal_registry`, `absent_settings_becomes_empty_string` |
| 15 | Secrets exactly the [host.secrets] aliases, keyed by alias; process env then .env | ✓ VERIFIED | `only_aliased_secrets_resolved`, `process_env_overrides_dotenv` |
| 16 | Disabled source listed with plugin None, its files never read | ✓ VERIFIED | `disabled_source_is_not_loaded` (config "does-not-exist.toml") |
| 17 | Secret values never in Debug, tracing or diagnostics; .env errors by line number | ✓ VERIFIED | `secret_debug_is_redacted`, `malformed_dotenv_reports_line_not_content`, `rendered_report_omits_secret_values`; tracing::info logs env var names only (config.rs finish_plugin) |
| 18 | Root workspace excludes plugins/; `cargo test --bin fili` passes incl. pre-existing tests | ✓ VERIFIED | Cargo.toml `exclude = ["plugins"]`; full run 45 passed, 0 failed |
| 19 | Edge empty registry / absent settings | ✓ VERIFIED | `empty_registry_loads`, `absent_settings_becomes_empty_string` |
| 20 | Edge ordering: BTreeMaps | ✓ VERIFIED | config.rs LoadedRegistry fields |
| 21 | Edge concurrency (backstop): each file read once, parse + NamedSource from one String | ⚠️ insufficient_spec | One read per file observed by inspection; backstop truth needs held-out evidence — human item |
| 22 | Edge adjacency: process env wins | ✓ VERIFIED | `process_env_overrides_dotenv` |
| 23 | Edge empty: EmptySecret; no [host.secrets] -> empty map; missing .env not an error | ✓ VERIFIED | `empty_secret_value_is_error`, `missing_dotenv_file_is_empty`, sample stub secrets empty |
| 24 | Edge encoding: case-sensitive lookup; dotenvy parse (`export X="a b"` -> `a b`) | ✓ VERIFIED | `env_lookup_is_case_sensitive`, `dotenv_values_parsed_by_dotenvy` (but see gap row 46: "as dotenvy parses them" includes `$` substitution) |
| 25 | Edge ordering: secrets in alias order | ✓ VERIFIED | `secrets_iterate_in_alias_order` |
| 26 | Edge concurrency: process env never mutated | ✓ VERIFIED | `loading_does_not_mutate_process_env`; no set_var/remove_var under src/plugins/ |

**Plan 01-03 (CONF-03)**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 27 | Missing plugin TOML names plugin, `sources.<name>.config`, path, registry label | ✓ VERIFIED | `missing_plugin_file` |
| 28 | Malformed plugin TOML cases each name plugin + file with span | ✓ VERIFIED | `malformed_plugin_toml`, `unknown_host_key`, `zero_poll_interval`, `missing_host_table` |
| 29 | Missing secret names plugin, `host.secrets.<alias>`, env var, `declared here` | ✓ VERIFIED | `missing_secret` |
| 30 | Unknown/disabled fetcher from enabled source names `sources.<name>.fetcher` with span | ✓ VERIFIED | `unknown_fetcher`, `disabled_fetcher`, `unknown_fetcher_on_disabled_source_ignored` |
| 31 | Invalid names, cross-table duplicates, non-host allowlist, bad backoff are errors | ✓ VERIFIED | `invalid_plugin_name`, `duplicate_plugin_name`, `invalid_allowlist_entry`, `invalid_backoff` |
| 32 | One ConfigErrors with every problem; one `fili::config::` diagnostic each; fail closed | ✓ VERIFIED | `collects_all_problems` (5 problems, 5 codes, Err) |
| 33 | Rendered report never contains a secret value | ✓ VERIFIED | `rendered_report_omits_secret_values` |
| 34 | Edge adjacency: DuplicateName; duplicate table -> RegistryMalformed | ✓ VERIFIED | `duplicate_plugin_name`, `duplicate_table_is_malformed` |
| 35 | Edge concurrency: deterministic problem order | ✓ VERIFIED | `collects_all_problems` asserts exact sequence twice |

**Plan 01-04 (CONF-01, CONF-02)**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 36 | Sample registers stub/reddit/youtube + stub-fetcher/http, each source its own TOML | ✓ VERIFIED | examples/plugins/*.toml read; `loads_sample_registry` |
| 37 | Sample + .env loads; per-plugin secrets exact; UNRELATED_TOKEN never delivered | ✓ VERIFIED | `loads_sample_registry` |
| 38 | .env.example lists exactly declared env var names, empty values; .env gitignored | ✓ VERIFIED | `sample_env_example_lists_every_declared_secret`; `git check-ignore` .env rc=0, .env.example rc=1; no examples/plugins/.env |
| 39 | `--registry` / FILI_REGISTRY / default filimon.toml, before or after subcommand | ✓ VERIFIED (coincidental-reliance) | 4 cli tests pass; `fili --help` shows `[env: FILI_REGISTRY=] [default: filimon.toml]`. Default test is vacuous if FILI_REGISTRY is set (it was unset here) |
| 40 | Path parsed from `--registry` feeds load_registry_with_env | ✓ VERIFIED | `registry_flag_feeds_loader` |

**Prohibitions folded into the score as truths (rows 41-45)**

| # | Prohibition | Tier | Status | Evidence |
|---|-------------|------|--------|----------|
| 41 | 01-02: no secret values in logs/Debug/diagnostics (incl. .env parse errors) | test | ✓ VERIFIED | Three wired tests above pass |
| 42 | 01-02: .env never copied into process env | test | ✓ VERIFIED | `loading_does_not_mutate_process_env` |
| 43 | 01-03: never silently skip an invalid plugin | test | ✓ VERIFIED | `collects_all_problems`, `invalid_plugin_name` (InvalidName reported, load Err) |
| 44 | 01-01: no refresh token in WIT (AUTH-03) | test | ⚠️ FLAGGED unverified-prohibition | Holds by inspection and grep this session, but no committed test enforces it — fail-closed: not counted as verified; human review |
| 45 | 01-01: no TLS-weakening option in fetch contract | test | ⚠️ FLAGGED unverified-prohibition | Same as row 44 |

**Requirement-derived truth**

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 46 | CONF-02 / goal: `.env` secrets reach the plugin as written; the `.env` format is stable and documented | ✗ FAILED | dotenvy `$` substitution reproduced (see gaps). `pa$$word` -> `pa`, silently accepted |

**Score:** 42/46 truths verified (row 46 failed; row 21 insufficient_spec; rows 44-45 flagged unverified-prohibitions, not counted; row 39 coincidental-reliance advisory, counted)

### Deferred Items

| # | Item | Addressed In | Evidence |
|---|------|-------------|----------|
| 1 | `fili` has no non-test caller of load_registry / Cli.registry yet | Phase 3 / 4 | Phase 3 SC1; Phase 4 SC2 (`fili plugins check` exits non-zero on broken config) |
| 2 | outbound-http.send allowlist attribution and allowlist normalization (WR-06, WR-01) | Phase 5 | Phase 5 SC2 / FETCH-02 |

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `wit/types.wit`, `wit/host.wit`, `wit/worlds.wit` | WIT package | ✓ VERIFIED | gsd verify.artifacts 7/7 (01-01) |
| `plugins/stub-source/*`, `plugins/stub-fetcher/*`, `plugins/README.md` | Stub guests + docs | ✓ VERIFIED | Built fresh; README has all required commands |
| `src/plugins/{mod,config,secrets,error}.rs` | Loader, secrets, diagnostics | ✓ VERIFIED | 5/5 (01-02), 2/2 (01-03); 2076 lines, substantive |
| `examples/plugins/*` (5 files) | Sample config | ✓ VERIFIED | 6/6 (01-04), tracked in git |
| `src/cli.rs` | `--registry` flag | ✓ VERIFIED | Field + const + 4 tests |
| `Cargo.toml` | deps + exclude | ✓ VERIFIED | Baseline diff shows only phase hunks |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| stub-source/Cargo.toml | wit/worlds.wit | `world = "source"`, path ../../wit | ✓ WIRED | gsd + fresh build |
| stub-fetcher/Cargo.toml | wit/worlds.wit | `world = "fetcher"` | ✓ WIRED | gsd + fresh build |
| stub-source/src/lib.rs | wit/host.wit | `fetch(&request)` | ✓ WIRED | Component imports filimon:plugin/fetch |
| wit/worlds.wit | wit/host.wit | import/export fetch | ✓ WIRED | |
| src/main.rs | src/plugins/mod.rs | `mod plugins;` | ✓ WIRED | gsd tool reported false negative (anchored regex); `grep -n '^mod plugins;' src/main.rs` -> line 25, and module tests compile/run in the fili bin |
| config.rs | secrets.rs | read_dotenv / resolve_secrets | ✓ WIRED | |
| config.rs | error.rs | ConfigErrors, UnknownFetcher/InvalidValue, render_report | ✓ WIRED | |
| plugins/mod.rs tests | examples/plugins | CARGO_MANIFEST_DIR copy | ✓ WIRED | |
| cli.rs | config.rs | `load_registry_with_env(&cli.registry` | ✓ WIRED (test-only) | Production wiring deferred (Phase 3/4) |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| LoadedRegistry.sources[*].plugin.secrets | Secret values | process env closure, then `.env` via dotenvy::from_read_iter | Yes for plain values; **values containing `$` are rewritten** | ⚠️ FLOWING with corruption (gap) |
| PluginConfig.settings_toml | `[settings]` | plugin TOML -> toml::to_string | Yes | ✓ FLOWING |
| HostConfig | `[host]` | plugin TOML typed parse | Yes | ✓ FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| WIT validates | `wasm-tools component wit wit/` | rc=0 | ✓ PASS |
| Stub builds + conformance | `cargo component build --release` x2; `wasm-tools component targets wit -w {source,fetcher}` | rc=0/0; cross checks rc=1/1 | ✓ PASS |
| Whole fili test suite (run once) | `cargo test --bin fili` | 45 passed, 0 failed | ✓ PASS |
| Clippy on phase code | `cargo clippy --bin fili --message-format=short` | rc=0; no src/plugins/ lines; 3 src/cli.rs warnings on baseline lines 17/21/51 (blame: 12c57b9) | ✓ PASS |
| Help output | `cargo run --bin fili -- --help` | `--registry ... [env: FILI_REGISTRY=] [default: filimon.toml]` | ✓ PASS |
| CR-01 reproduction | scratch crate outside repo, dotenvy =0.15.7, from_read_iter | `PW=pa$$word`->`pa`; `Q="x$HOME/y"`->expanded real HOME; `S=abc$NOPE_UNSET_XYZ`->`abc_UNSET_XYZ`; `R='pa$$word'`->verbatim | ✗ FAIL (gap) |

### Probe Execution

No probes declared in PLAN/SUMMARY and no `scripts/*/tests/probe-*.sh` apply. Step 7c: not applicable.

### Requirements Coverage

| Requirement | Source Plan | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| WIT-01 | 01-01 | Versioned WIT `source` world, init/poll, fetch + log imports | ✓ SATISFIED | Truths 1-2, 5, 7-11 |
| WIT-02 | 01-01 | `fetcher` world exports fetch, imports low-level outbound HTTP | ✓ SATISFIED | Truths 3, 6, 12 |
| CONF-01 | 01-02, 01-04 | Per-plugin TOML referenced from filimon.toml | ✓ SATISFIED | Truths 13-14, 36-37 |
| CONF-02 | 01-02, 01-04 | Secrets load from `.env`; each plugin gets only its named secrets | ✗ PARTIAL / BLOCKED | Isolation satisfied (15, 37); value loading fails for `$`-containing values (46) |
| CONF-03 | 01-03 | Clear error naming plugin and field | ✓ SATISFIED (warning WR-02) | Truths 27-35 |

Orphaned requirements: none. REQUIREMENTS.md maps exactly WIT-01, WIT-02, CONF-01, CONF-02, CONF-03 to Phase 1, and every one is claimed by a plan.

### Anti-Patterns Found

No TBD/FIXME/XXX/TODO/HACK/PLACEHOLDER markers in any phase-modified file. No stubs on user-visible paths (stub crates are intentional fixtures).

Code-review findings weighed against must-haves (01-REVIEW.md; all 15 dispositions still `open`):

| ID | File | Severity (this report) | Impact on goal |
|----|------|------------------------|----------------|
| CR-01 | src/plugins/secrets.rs:64 | 🛑 Blocker | Defeats CONF-02 value correctness; see gap. Independently reproduced |
| WR-02 | src/plugins/config.rs:258-266 | ⚠️ Warning | Malformed `.env` yields DotenvInvalid plus one false MissingSecret per alias with wrong help. SC4 still holds (real error present), but clarity degraded; test asserts `.any()` only (config.rs:890) |
| WR-03 | src/plugins/secrets.rs:140-147, error.rs:96-100 | ⚠️ Warning | Empty process var shadows `.env`; EmptySecret help suggests a fix that cannot work. Not silent |
| WR-04 | src/plugins/config.rs:195 | ⚠️ Warning | Non-UTF-8 process var silently falls through to `.env`, contradicting "process env wins" |
| WR-05 | src/plugins/config.rs:84-104, 582-606 | ⚠️ Warning | No upper bounds on durations/multiplier; can panic Phase 3 backoff math. No SC requires bounds |
| WR-06 | wit/host.wit:22-27 | ⚠️ Warning (deferred to Phase 5) | Doc-only contract gap; ABI unaffected |
| WR-01 | src/plugins/config.rs:569-580 | ⚠️ Warning (deferred to Phase 5) | Fail-closed; matching is Phase 5 |
| IN-01..IN-08 | various | ℹ️ Info | IN-03 recorded as coincidental-reliance on truth 39 |

### Human Verification Required

1. **Prohibition: no refresh token in WIT (01-01, test-tier, no wired test)**
   **Test:** Read wit/*.wit.
   **Expected:** Only `access-token: option<string>` crosses into `poll`; no field/param/import carries a refresh token.
   **Why human:** Fail-closed: test-tier prohibition with no committed enforcement.

2. **Prohibition: no TLS-weakening option (01-01, test-tier, no wired test)**
   **Test:** Read `record request` in wit/types.wit.
   **Expected:** Exactly method, url, headers, body.
   **Why human:** Same as above.

3. **Backstop truth: single read per config file (01-02)**
   **Test:** Read config.rs load_registry_with_env and read_plugin, secrets.rs read_dotenv.
   **Expected:** One `read_to_string` per file; parse and NamedSource built from the same String.
   **Why human:** verification: backstop — presence is not evidence.

### Gaps Summary

The WIT contract (SC1, SC2), typed loading with per-plugin key isolation (SC3), and plugin-and-field diagnostics (SC4) are all genuinely delivered and backed by a fresh build, conformance checks, and 45 passing tests. The phase falls short on one point in its own goal, "`.env` secret formats that `fili` loads": the `.env` reader silently rewrites any secret containing `$` (shell-style substitution from dotenvy). It also reads the real process environment during that rewrite. The corrupted value is non-empty, so validation passes and the plugin later fails authentication with no host diagnostic. Plan 01-02 delegated value semantics to "as dotenvy parses them" without deciding on substitution, so every plan-level test passes while the requirement-level behavior is wrong. This is code review CR-01, still open.

**This may be acceptable as an explicit format decision.** If the user decides `.env` follows dotenv shell semantics (single-quote any value containing `$`), the gap can be closed by documenting that rule in `.env.example` and the loader docs and pinning it with tests. To accept the deviation without code changes instead, add to the frontmatter:

```yaml
overrides:
  - must_have: "CONF-02 / phase goal: API keys and client secrets load from `.env` — the value a plugin receives in `init` is the value the operator wrote in `.env`, and the `.env` format `fili` loads is stable and documented"
    reason: "Accept dotenv shell semantics: `$VAR` substitution applies to unquoted and double-quoted values; operators single-quote secrets containing `$`"
    accepted_by: "{name}"
    accepted_at: "{ISO timestamp}"
```

Warnings WR-02 through WR-05 do not defeat a success criterion, but WR-02 and WR-03 directly degrade CONF-03/CONF-02 diagnostic clarity and are cheap to fold into the same gap-closure plan.

---

_Verified: 2026-11-03T10:37:53Z_
_Verifier: Claude (gsd-verifier)_
