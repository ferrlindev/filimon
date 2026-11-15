---
phase: "01"
slug: "plugin-contract-configuration"
status: verified
# threats_open = count of OPEN threats at or above workflow.security_block_on severity (the blocking gate)
threats_open: 0
asvs_level: 1
created: "2026-11-03"
---

# Phase 01 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| host → guest (WIT imports) | Plugins are untrusted third-party code; every capability is a WIT import | access token (sensitive), request records, log lines |
| crates.io → developer machine | Tool installs and guest dependencies are fetched and compiled locally | build tooling / source (supply chain) |
| operator files → fili | filimon.toml, plugin TOMLs and .env are operator-authored local files | config, secret values (sensitive) |
| fili → plugin init | Resolved secrets cross into untrusted guest code; per-plugin alias filtering | secret values (sensitive) |
| fili → logs / terminal / Debug | Diagnostics, tracing and rendered reports leave the process | alias and env var names only (non-sensitive) |
| repository → public | Committed sample files are published | sample config (must carry no credentials) |
| environment → CLI | `FILI_REGISTRY` / `--registry` choose which local file is parsed | file path (operator-trusted) |
| dev/CI environment → test process | Env vars such as FILI_REGISTRY leak into `cargo test` | test inputs |

---

## Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation | Status |
|-----------|----------|-----------|----------|-------------|------------|--------|
| T-01-01 | Elevation of privilege | stub component build target | high | mitigate | `plugins/stub-{source,fetcher}/.cargo/config.toml` set `target = "wasm32-unknown-unknown"`; no `wasi` in wit/*.wit | closed |
| T-01-02 | Information disclosure | wit/worlds.wit poll/init signatures | high | mitigate | `grep -niE 'refresh[a-z-]*\s*:' wit/*.wit` → no match; poll takes only `access-token: option<string>` (worlds.wit:27); UAT test 1 pass | closed |
| T-01-03 | Tampering | wit/types.wit request record | high | mitigate | `record request` = method/url/headers/body (types.wit:55-60); TLS-weakening grep → no match; UAT test 2 pass | closed |
| T-01-04 | Information disclosure | stub source init logging | medium | mitigate | plugins/stub-source/src/lib.rs:44-53 logs count + alias names only (`secrets.iter().map(\|(alias, _)\| ...)`) | closed |
| T-01-SC (01-01) | Tampering | cargo-component, wasm-tools, wit-bindgen-rt, toml, serde | high | mitigate | wit-bindgen-rt pinned `0.41` in both stub Cargo.toml; stub Cargo.lock files committed; RESEARCH legitimacy audit all [OK] | closed |
| T-01-05 | Information disclosure | Secret in Debug / tracing / diagnostics | high | mitigate | `impl Debug for Secret` writes `Secret(***)` (secrets.rs:34), no Display impl; tests `secret_debug_is_redacted`, `malformed_dotenv_reports_line_not_content` pass | closed |
| T-01-06 | Elevation of privilege | over-broad secret delivery | high | mitigate | `resolve_secrets` (secrets.rs:256) reads plugin's own aliases; test `only_aliased_secrets_resolved` passes | closed |
| T-01-07 | Information disclosure | process environment mutation | medium | mitigate | No `set_var`/`remove_var`/`dotenvy::dotenv` under src/plugins/ (custom `read_dotenv` into private map since 01-05); test `loading_does_not_mutate_process_env` passes | closed |
| T-01-08 | Information disclosure | plugin TOML aliasing unrelated env var | medium | mitigate | `tracing::info!(plugin, ?env_vars, "resolved plugin secrets")` (config.rs:348); residual (no name-prefix restriction) accepted — see AR-01 | closed |
| T-01-09 | Tampering | registry paths absolute / containing `..` | low | accept | Operator-authored local config, trusted at ASVS L1 — see AR-02 | closed |
| T-01-10 | Denial of service | huge or malformed config files | low | accept | Local operator files; parse failures fail closed via ConfigErrors — see AR-03 | closed |
| T-01-SC (01-02) | Tampering | toml, dotenvy, thiserror | high | mitigate | Legitimacy audit [OK]; versions constrained in Cargo.toml; dotenvy later removed (01-05) | closed |
| T-01-11 | Information disclosure | rendered miette reports | high | mitigate | Test `rendered_report_omits_secret_values` (config.rs:1505) passes | closed |
| T-01-12 | Tampering | plugin names for CLI lookup / state keys | medium | mitigate | `ConfigError::InvalidName` / `DuplicateName` enforced (config.rs:411, 424) | closed |
| T-01-13 | Tampering | silent partial load (fail-open) | medium | mitigate | Test `collects_all_problems` (config.rs:1439) passes; load returns `Err(ConfigErrors)` | closed |
| T-01-14 | Tampering | allowlist entries with scheme/port/path | medium | mitigate | `url::Host::parse` check per entry (config.rs:576) | closed |
| T-01-SC (01-03) | Tampering | package installs | low | accept | No package added — see AR-04 | closed |
| T-01-15 | Information disclosure | committed sample files | high | mitigate | examples/plugins/.env.example has empty values only; `.env` gitignored (`.gitignore:14`) and absent | closed |
| T-01-16 | Tampering | `--registry` / `FILI_REGISTRY` path selection | low | accept | Operator-chosen local path, trusted at L1 — see AR-05 | closed |
| T-01-SC (01-04) | Tampering | package installs | low | accept | No new package — see AR-04 | closed |
| T-01-17 | Information disclosure | `.env` value expansion | high | mitigate | `parse_line(line: &str)` (secrets.rs:132) takes no env input; tests `dotenv_never_expands_process_env`, `dotenv_dollar_outside_single_quotes_is_rejected` pass | closed |
| T-01-18 | Tampering | secret integrity `.env` → init | high | mitigate | Tests `dotenv_single_quoted_dollar_round_trips`, `dotenv_unquoted_dollar_is_rejected`, `dotenv_accepts_documented_forms`, `dotenv_rejects_ambiguous_forms` pass | closed |
| T-01-19 | Information disclosure | DotenvInvalid rejection paths | high | mitigate | Leak-marker assertions (`pa$$word`, `$$word`) in config.rs:942; tests pass | closed |
| T-01-20 | Denial of service | misleading diagnostics | low | mitigate | Tests `unusable_dotenv_suppresses_missing_secret`, `empty_process_var_names_process_env`, `empty_dotenv_value_names_dotenv_file` pass | closed |
| T-01-21 | Denial of service | pathological `.env` | low | accept | Linear single pass over operator-owned file — see AR-06 | closed |
| T-01-SC (01-05) | Tampering | cargo dependency changes | high | mitigate | Substituting crate removed; `dotenvy` absent from Cargo.toml | closed |
| T-01-22 | Tampering | `--registry` default evidence (D-13) | low | mitigate | Test asserts `Cli::command()` metadata (cli.rs:104, 117); CLI tests pass | closed |
| T-01-SC (01-06) | Tampering | cargo package installs | high | mitigate | No package change by this plan | closed |

*Status: open · closed · open — below high threshold (non-blocking)*
*Severity: critical > high > medium > low — only open threats at or above workflow.security_block_on count toward threats_open*
*Disposition: mitigate (implementation required) · accept (documented risk) · transfer (third-party)*

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-01 | T-01-08 (residual) | No env-var name-prefix restriction decided; plugin TOMLs are operator-owned and mappings are logged for audit | 01-02 PLAN threat model | 2026-11-03 |
| AR-02 | T-01-09 | Registry is operator-authored local config, trusted at ASVS L1; path-traversal hardening deferred | 01-02 PLAN threat model | 2026-11-03 |
| AR-03 | T-01-10 | Local operator inputs; failures fail closed through collected ConfigErrors | 01-02 PLAN threat model | 2026-11-03 |
| AR-04 | T-01-SC (01-03, 01-04) | Plans add no packages; reuse dependencies audited in 01-02 | 01-03/01-04 PLAN threat models | 2026-11-03 |
| AR-05 | T-01-16 | Path chosen by local operator; wrong path surfaces as RegistryUnreadable naming the path | 01-04 PLAN threat model | 2026-11-03 |
| AR-06 | T-01-21 | Single linear pass, no regex/backtracking, over an operator-owned file read once | 01-05 PLAN threat model | 2026-11-03 |

*Accepted risks do not resurface in future audit runs.*

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-11-03 | 28 | 28 | 0 | /gsd-secure-phase orchestrator (L1 grep-depth + `cargo test plugins` 49/49, `cargo test cli` 4/4) |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-11-03
