---
phase: 07-reddit-youtube-plugins
plan: 10
subsystem: cli/auth
tags: [oauth, client_credentials, auth-cli, status, docs]
requires:
  - phase: 07-02
    provides: "[host.oauth] grant key and the client_credentials config rules"
  - phase: 07-03
    provides: "host-side app token fetch and in-memory cache (behavior the README documents)"
provides:
  - "OAuthDeclared::AppOnly from the lenient probe"
  - "fili auth status row `app-only` (table and --json)"
  - "fili auth login refusal for client_credentials plugins, before secrets, ports or directories"
  - "plugins/README.md App-only access section, grant and user_agent keys, Reddit and Google provider notes"
affects: [07-11]
tech-stack:
  added: []
  patterns:
    - "lenient probe reads one extra key (grant) from the untyped toml::Value, still no validation"
    - "app-only status row answers before the token store is read"
key-files:
  created: []
  modified:
    - src/plugins/config.rs
    - src/commands/auth.rs
    - tests/auth_cli.rs
    - plugins/README.md
key-decisions:
  - "The login refusal runs after find_login_source and before load_plugin, so a missing client secret never masks the real reason"
  - "An app-only row never reads the token store: a stale token file of the same alias is ignored and never shown"
requirements-completed: [RDDT-01, RDDT-03]
status: complete
duration: 55min
completed: 2026-11-11
commits: 3
plan_head_before: 042472605e9b78dc7a53753f2ae432d8b3abbed0
plan_head_after: 35844fd397138f182c7abfd71896a8ef94108146
actuals:
  tokens: 20000
  tasks: 2
  commits: 3
---

# Phase 7 Plan 10: client_credentials CLI honesty and operator docs Summary

**`fili auth login` refuses a `client_credentials` plugin with a clear reason before touching any secret, `fili auth status` shows it as `app-only`, and `plugins/README.md` documents the grant, `user_agent`, and the Reddit and Google provider notes.**

`commits` counts the three code and docs commits; the SUMMARY commit follows `plan_head_after`.

## What was built

- `config.rs`: new `OAuthDeclared::AppOnly`. `probe_oauth_sources` returns it when the plugin file's `[host.oauth]` has `grant = "client_credentials"` (string compare on the untyped table). Still no validation, no environment, no secrets, so `status` works before the client secret is set.
- `commands/auth.rs`: `StatusState::AppOnly` (label `app-only`). `collect_status` pushes the row first and `continue`s, so the token store is never read for such a plugin (expiry and refresh cells are `-`, JSON `expires_at` and `has_refresh_token` are null). `login_command` probes the registry right after `find_login_source` and, for an app-only source, returns `plugin `<name>` uses the client_credentials grant, so there is nothing to log in to; the host fetches its app token itself` before `load_plugin`, any port, or any directory. `logout_command` is unchanged.
- `tests/auth_cli.rs`: binary-level refusal (exit 1, empty stdout, no state dir, no missing-secret text) and `status --json` with a stale token file of the same alias.
- `plugins/README.md` (only the `## OAuth` section): `grant` and `user_agent` rows, "required when" wording for `authorize_url` and `client_secret_env`, the `### App-only access (client_credentials)` subsection, the `app-only` status state, the login/refusal wording, "What a plugin receives" sentence, Reddit web-app / User-Agent / approval notes, Google desktop-client and Testing-mode notes, and the replaced Known-limits bullet.

## TDD gate compliance

- RED `06c3fa8`: 4 unit tests fail on their planned assertions (`Yes` vs `AppOnly`; `logged-in` vs `app-only` in table and JSON; the missing-secret config error instead of the refusal). The `OAuthDeclared::AppOnly` variant and a match arm were added as a compile stub in this commit, because the new tests name the variant. The two CLI tests were committed in RED too but were not run against the RED tree (the integration-test binary was only built after GREEN); they pass after GREEN (`12 passed` in `auth_cli`).
- GREEN `97928f3`: probe, status row and login refusal.
- No REFACTOR commit (one clippy fix, identical match arms merged, went into GREEN).
- RED evidence was inspected by hand from `cargo test` output; the repo's Rust report is not a format the `tdd-red-evidence` classifier supports.

## Verification

- `cargo test -p filimon`: 666 + 12 + 12 passed, 0 failed. All tests named in Task 1 report `ok`, as do the existing `probe_oauth_sources_reports_declared_undeclared_and_unreadable`, `status_lists_each_state_without_token_values`, `status_json_is_an_array_with_no_token_fields` and `login_for_a_plugin_without_host_oauth_exits_nonzero_naming_the_plugin`.
- `cargo clippy -p filimon --all-targets`: no diagnostic for `src/commands/auth.rs` or `src/plugins/auth/`. Three pre-existing `Duration::from_secs` style warnings remain in `src/plugins/config.rs` test code (lines 1503-1506, not touched by this plan).
- Task 2 verify command and all acceptance greps pass; `git diff --quiet 75b078e -- wit Cargo.toml Cargo.lock` exits 0.

## Deviations from Plan

None in behavior. Two notes:

- **[Process] Pre-existing clippy warnings** in `src/plugins/config.rs` (lines 1503, 1505, 1506, `Duration::from_secs(120)`-style test code) make the plan's "no diagnostics for config.rs" criterion read strictly false, but the lines are not part of this plan's diff. Out of scope; not fixed.
- **[Process] `cargo fmt`** was not run crate-wide (the repo is not rustfmt-clean; see the 07-03 summary).

## Known Stubs

None.

## Threat Flags

None. The change adds no endpoint, auth path or file access; it removes one (login for a grant with no login). Mitigations T-07-42 and T-07-43 are covered by tests (sentinel tokens absent on both streams; refusal leaves no state directory and shows no missing-secret text). T-07-44: README examples use placeholders and `.env` key names only.

## Self-Check: PASSED

- FOUND: src/plugins/config.rs, src/commands/auth.rs, tests/auth_cli.rs, plugins/README.md (all modified in commits)
- FOUND commits: 06c3fa8 (test), 97928f3 (feat), 35844fd (docs)
