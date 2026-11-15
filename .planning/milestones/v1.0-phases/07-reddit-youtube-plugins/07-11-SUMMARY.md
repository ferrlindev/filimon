---
phase: 07-reddit-youtube-plugins
plan: 11
subsystem: plugins
tags: [wasm-plugins, sample-registry, reddit, youtube, oauth, docs]
status: complete

requires:
  - phase: 07-reddit-youtube-plugins
    provides: reddit and youtube plugin crates (07-02 to 07-09), host OAuth and client_credentials support (07-10)
provides:
  - five sample source instances over two components (reddit, reddit-user, youtube, youtube-user, stub)
  - host sample tests pinning shared components, read-only scopes, official hosts, accepted settings keys and the .env.example key set
  - operator documentation for both plugins in plugins/README.md
affects: [07-12 real-capture checkpoint, operators registering the samples]

actuals:
  tokens: 9500
  tasks: 2
  commits: 2
plan_head_before: f804377fab7c114ffd3dc99d6b28f3a83fe51c11
plan_head_after: 3baa5bd4d58d17d0fecb96b7dff8ddbe727a15aa

tech-stack:
  added: []
  patterns:
    - "one component, several registry instances: the instance name keys cursor and token files"

key-files:
  created:
    - examples/plugins/reddit-user.toml
    - examples/plugins/youtube-user.toml
  modified:
    - examples/plugins/reddit.toml
    - examples/plugins/youtube.toml
    - examples/plugins/filimon.toml
    - examples/plugins/.env.example
    - src/plugins/mod.rs
    - tests/plugins_cli.rs
    - plugins/README.md

key-decisions:
  - "Reddit client secret moved from [host.secrets] to [host.oauth] client_secret_env; REDDIT_CLIENT_ID dropped from .env.example (the id is public and lives in the TOML)"
  - "Documentation maps the roadmap wording `fili auth login reddit/youtube` to the reddit-user and youtube-user instances"

patterns-established:
  - "Sample-registry tests derive declared env vars from both [host.secrets] and [host.oauth] client_secret_env"

requirements-completed: [RDDT-01, RDDT-02, RDDT-03, YTUB-01, YTUB-02, YTUB-03, TEST-01]

duration: ~25min
completed: 2026-11-11
---

# Phase 7 Plan 11: Sample registry and operator docs Summary

Five sample instances (app-only and PKCE Reddit, API-key and PKCE YouTube, plus the stub) over two components, with host tests that pin their shape and a plugins/README.md section covering build, settings, cursor limits, rate limits and the YouTube quota formula.

## Tasks

| Task | Name | Commit |
|------|------|--------|
| 1 | Sample instances, registry, .env.example, host sample tests | cfa92c2 |
| 2 | Reddit and YouTube section of plugins/README.md | 3baa5bd |

## What was built

- `reddit.toml`: app-only (`client_credentials`), allowlist `oauth.reddit.com` only, `[host.oauth]` with the host-only secret env, `subreddits`, `listings`, `user_agent_contact`; no `[host.secrets]`.
- `reddit-user.toml`: PKCE, scopes `read` and `history`, `duration = permanent`, `feed = "home"`.
- `youtube-user.toml`: PKCE, scope `youtube.readonly`, `access_type = offline`, `subscriptions = true`.
- `filimon.toml`: `reddit` and `reddit-user` share `reddit_source.wasm`; `youtube` and `youtube-user` share `youtube_source.wasm`, both pointing at the crates' build output.
- Tests: `loads_sample_registry` and `sample_env_example_lists_every_declared_secret` updated; new `sample_instances_of_one_component_share_it`, `sample_user_instances_request_read_only_scopes_and_official_hosts`, `sample_settings_use_only_keys_the_plugins_accept`; `list_shows_the_sample_registry_plugins` expects seven names.
- README: build and conformance lines, native test command with the explicit host triple, instances table, settings tables, item mapping, cursor limits, rate limits, quota formula with a worked table (computed with the formula; 71 playlists at 600 s exceeds 10,000 units), access notes and fixture provenance. The stale `client_id = "REDDIT_CLIENT_ID"` example in "Plugin secrets" was changed to `api_key = "YOUTUBE_API_KEY"`.

## Verification

- `cargo test -p filimon plugins::tests`: 53 passed, all five sample tests ok.
- `cargo test -p filimon --test plugins_cli list_shows_the_sample_registry_plugins`: ok.
- All Task 1 and Task 2 acceptance greps pass; `git diff 75b078e -- wit Cargo.toml Cargo.lock` is empty.
- The full `cargo test -p filimon` suite was not run to completion in this worktree; only the targeted groups above were.

## Deviations from Plan

None - plan executed as written. One incident: `cargo fmt` run in the worktree reformatted unrelated files; they were restored with per-file `git checkout --` before any commit, so only plan files are committed.

## Known Stubs

None. The sample TOMLs hold placeholder client ids and contacts (`your-reddit-web-app-client-id`, `u/yourname`) by design, as operator-fill values.

## Threat Flags

None.

## Self-Check: PASSED

- Created files exist: reddit-user.toml, youtube-user.toml, this SUMMARY.
- Commits cfa92c2 and 3baa5bd are in HEAD history.
