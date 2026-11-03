---
phase: 01-plugin-contract-configuration
plan: 04
subsystem: config
tags: [rust, toml, clap, dotenvy, plugins, cli, sample-config]

# Dependency graph
requires:
  - phase: 01-plugin-contract-configuration (01-01)
    provides: "plugins/stub-source and plugins/stub-fetcher crates (stub Settings: greeting, fetch_url; deny_unknown_fields)"
  - phase: 01-plugin-contract-configuration (01-02)
    provides: "plugins::config::load_registry_with_env, LoadedRegistry/PluginConfig, plugins::secrets::read_dotenv and Secret::expose"
provides:
  - "examples/plugins/: filimon.toml registry (sources stub, reddit, youtube; fetchers stub-fetcher, http), stub.toml, reddit.toml, youtube.toml and an empty-valued .env.example"
  - "plugins::tests::loads_sample_registry: the committed sample loads into typed config with per-plugin secret isolation (Phase SC3)"
  - "plugins::tests::sample_env_example_lists_every_declared_secret: .env.example stays equal to the set of [host.secrets] env var names"
  - "Global `fili --registry <PATH>` flag, env FILI_REGISTRY, default filimon.toml (D-13), accepted before or after the subcommand"
affects: [01-03, phase-3-host, phase-4-cli, phase-8-authoring-guide]

# Actuals (#2632)
actuals:
  tokens: 3300
  tasks: 2
  commits: 2
plan_head_before: a4cb7be11d7daeea01bf4106c46945e595dcfdc7
plan_head_after: 2a99044dd6cc7415d6c0cd6c10b91a0794a9c3c4

# Tech tracking
tech-stack:
  added: []
  patterns:
    - "Sample-config tests copy the committed examples/plugins files (CARGO_MANIFEST_DIR) into a TempDir and write .env only there"
    - "Path flags follow the existing tokenizer/model pattern: const DEFAULT_*, #[arg(long, env = ..., default_value = DEFAULT_*)]"

key-files:
  created:
    - examples/plugins/filimon.toml
    - examples/plugins/stub.toml
    - examples/plugins/reddit.toml
    - examples/plugins/youtube.toml
    - examples/plugins/.env.example
  modified:
    - src/plugins/mod.rs
    - src/cli.rs

key-decisions:
  - "No dead_code expect on Cli.registry: rustc and clippy report no never-read diagnostic for the clap-derived field, and an expect(dead_code) would itself fail as unfulfilled"
  - "registry_flag_feeds_loader uses an enabled source with a defined fetcher, so the fixture stays valid under the validation rules plan 01-03 adds"
  - "One commit per task (no separate RED commit): both tasks' 'implementation' cannot fail on an assertion before it exists (missing sample files or a missing struct field are fixture/build failures), so test discrimination was proven by mutation instead"

patterns-established:
  - "examples/plugins is the canonical operator sample; .env.example is test-locked to the declared secrets"

requirements-completed: [CONF-01, CONF-02]

coverage:
  - id: D1
    description: "Committed sample filimon.toml registry with per-plugin TOMLs (stub greeting, Reddit subreddits/interval/allowlist/backoff/2 secrets, YouTube channels/interval/1 secret) loads into typed config, and each plugin receives only the .env keys its own TOML names"
    requirement: "CONF-01"
    verification:
      - kind: unit
        ref: "src/plugins/mod.rs#tests::loads_sample_registry"
        status: pass
    human_judgment: false
  - id: D2
    description: "examples/plugins/.env.example lists exactly REDDIT_CLIENT_ID, REDDIT_CLIENT_SECRET and YOUTUBE_API_KEY with empty values; examples/plugins/.env is gitignored and absent"
    requirement: "CONF-02"
    verification:
      - kind: unit
        ref: "src/plugins/mod.rs#tests::sample_env_example_lists_every_declared_secret"
        status: pass
      - kind: other
        ref: "git check-ignore -q examples/plugins/.env && ! git check-ignore -q examples/plugins/.env.example && test ! -e examples/plugins/.env"
        status: pass
    human_judgment: false
  - id: D3
    description: "Global --registry flag with env FILI_REGISTRY and default filimon.toml, accepted before or after the subcommand, whose parsed path feeds load_registry_with_env"
    verification:
      - kind: unit
        ref: "src/cli.rs#tests::registry_flag_defaults_to_filimon_toml"
        status: pass
      - kind: unit
        ref: "src/cli.rs#tests::registry_flag_is_global_with_env"
        status: pass
      - kind: unit
        ref: "src/cli.rs#tests::registry_flag_after_subcommand"
        status: pass
      - kind: unit
        ref: "src/cli.rs#tests::registry_flag_feeds_loader"
        status: pass
      - kind: other
        ref: "cargo run --quiet --bin fili -- --help | grep --registry, FILI_REGISTRY, filimon.toml"
        status: pass
    human_judgment: false

# Metrics
duration: 7min
completed: 2026-11-03
status: complete
---

# Phase 1 Plan 04: Sample Plugin Registry and --registry Flag Summary

**A committed `examples/plugins/` sample (stub, Reddit and YouTube sources, stub and http fetchers, empty `.env.example`) is proven by test to load into typed config with per-plugin secret isolation. A new global `fili --registry` flag (env `FILI_REGISTRY`, default `filimon.toml`) selects the registry.**

## Performance

- **Duration:** 7 min
- **Started:** 2026-11-03T10:16:06Z
- **Completed:** 2026-11-03T10:22:53Z
- **Tasks:** 2 of 2
- **Files modified:** 7 (5 created, 2 modified)

## Accomplishments
- `examples/plugins/filimon.toml` registers sources `stub`, `reddit` and `youtube` and fetchers `stub-fetcher` and `http`. Each source points at its own TOML. `stub.toml` sets only `greeting`, because the stub source denies unknown settings. `reddit.toml` sets subreddits, a 300 s interval, a two-host allowlist, backoff 60/3600/2.0 and two secrets. `youtube.toml` sets channels, a 900 s interval and one secret.
- `loads_sample_registry` copies the four committed TOMLs into a TempDir with a `.env` that also holds `UNRELATED_TOKEN=nope`. The test proves:
  - reddit's secrets are exactly `{client_id, client_secret}`, youtube's are exactly `{api_key}`, and stub's are empty.
  - No plugin sees `UNRELATED_TOKEN`, by key or by value.
  - Every component path resolves under the registry directory.
- `sample_env_example_lists_every_declared_secret` locks `.env.example` to the set of `[host.secrets]` env var names in the sample TOMLs.
- `Cli.registry` (`--registry`, global, `FILI_REGISTRY`, default `filimon.toml`) parses before or after `watch`. A parsed path loads a registry through `load_registry_with_env`. `fili --help` shows `[env: FILI_REGISTRY=] [default: filimon.toml]`.

## Task Commits

1. **Task 1 (tracer): sample registry, TempDir copy with .env, load_registry, typed config with per-plugin secret isolation (SC3).** Commit `bc41121` (feat).
2. **Task 2: global `--registry` flag (env FILI_REGISTRY, default filimon.toml) wired to the loader in tests (D-13).** Commit `2a99044` (feat).

**Plan metadata:** committed with this SUMMARY (docs(01-04)).

### Test-first record
- **Task 1:** this tracer's "implementation" is the data files. Before they exist, the test fails on file copy, which is a fixture failure and not a valid RED. I proved instead that the tests discriminate, by mutation. I added `leak = "UNRELATED_TOKEN"` to reddit's `[host.secrets]`, and both tests then failed on their planned assertions:
  - `loads_sample_registry` failed with left `["client_id", "client_secret", "leak"]` against right `["client_id", "client_secret"]`.
  - `sample_env_example_lists_every_declared_secret` failed because the declared set gained `UNRELATED_TOKEN`.
  - Result: `0 passed; 2 failed`. I restored the file, and the run returned to `2 passed`.
- **Task 2:** tests that name `cli.registry` cannot compile before the field exists, which is a build failure and not a valid RED. GREEN: `4 passed`.
- `tdd-red-evidence` was not run. It has no Rust libtest adapter, and this is a `type: execute` plan (see the tdd.md "Known gap — Rust" note).
- **Tracer gate:** this run is interactive, the mode is `end-of-phase`, and the tracer's `<verify>` is automated only. I re-ran the verify on the committed tree and it passed, so Task 2 went ahead with no checkpoint.

## Files Created/Modified
- `examples/plugins/filimon.toml`: sample registry. Paths resolve relative to its own directory.
- `examples/plugins/stub.toml`: stub source host values and `greeting`.
- `examples/plugins/reddit.toml`: Reddit host values, backoff, two secret aliases, subreddits and listing.
- `examples/plugins/youtube.toml`: YouTube host values, one secret alias and channels.
- `examples/plugins/.env.example`: the three secret env var names with empty values, plus a comment line.
- `src/plugins/mod.rs`: `#[cfg(test)] mod tests` with the two sample tests (162 lines added, nothing else changed).
- `src/cli.rs`: the `DEFAULT_REGISTRY` constant, the `registry` field and `#[cfg(test)] mod tests` with four tests. Numstat is `78 0`, so no existing line changed.

## Decisions Made
- I added no `expect(dead_code)` to `Cli.registry`. Neither rustc nor clippy reports the clap-derived field as never read, and an unfulfilled `expect` would itself warn.
- The `registry_flag_feeds_loader` fixture is an enabled source that references a defined fetcher. It will stay valid once plan 01-03's rule "every enabled source has a defined, enabled fetcher" lands.
- Each task is one commit rather than a separate RED/GREEN pair. For both tasks, the pre-implementation failure would be a fixture or build failure rather than an assertion RED. This also keeps Task 1's literal `git show --name-only HEAD` acceptance check true at its own commit.

## Deviations from Plan

None. The plan was executed as written. (The single-commit-per-task choice is recorded under Decisions Made.)

## Issues Encountered
- `cargo clippy --bin fili` reports three warnings in `src/cli.rs`, at lines 17 and 21 (doc backticks) and line 51 (`format!` inlining). All three are on baseline lines that my one-line constant shifted down. None mentions `registry`, so the plan's clippy gate passes. They are out of scope and were not touched.
- The test-build dead-code warnings, including `src/plugins/config.rs:198` `load_registry`, predate this plan. Plan 01-02 documented them.

## Verification Results
- `cargo test --bin fili`: 26 passed (20 baseline and 6 new).
- `cargo test --bin fili plugins::tests::`: 2 passed. `cargo test --bin fili cli::tests::`: 4 passed. Each named test reports `1 passed` with `--exact`.
- `git check-ignore -q examples/plugins/.env` passes, `.env.example` is not ignored, and `examples/plugins/.env` does not exist.
- `fili --help` lists `--registry <REGISTRY> ... [env: FILI_REGISTRY=] [default: filimon.toml]`.
- The clippy gate finds no `src/plugins/` line and no cli.rs line that mentions `registry`. `rustfmt --check --edition 2024` is clean on `src/plugins/mod.rs` and `src/cli.rs`.
- Every non-comment line of `.env.example` ends with `=`.

## Shared files not touched
- In worktree mode, `.planning/STATE.md`, `.planning/ROADMAP.md` and `.planning/REQUIREMENTS.md` are left for the orchestrator's post-wave writes. `requirements-completed` lists CONF-01 and CONF-02. I did not touch `src/plugins/error.rs` or `src/plugins/config.rs`, which plan 01-03 owns.

## User Setup Required
None. Operators copy `examples/plugins/.env.example` to `.env` next to their registry only when they run the real Reddit or YouTube plugins in later phases.

## Next Phase Readiness
- Phases 3 and 4 can read `cli.registry` and pass it to `load_registry`.
- Plan 01-03's validation rules should accept the sample. Names are lowercase `[a-z0-9_-]`, source and fetcher names are disjoint, every enabled source references a defined and enabled fetcher, allowlists hold bare hosts, and reddit's backoff max is at least initial with a multiplier of at least 1.0. After the wave merges, `loads_sample_registry` is the check that confirms it.

---
*Phase: 01-plugin-contract-configuration*
*Completed: 2026-11-03*

## Self-Check: PASSED

- Files present: examples/plugins/{filimon,stub,reddit,youtube}.toml, examples/plugins/.env.example, src/plugins/mod.rs, src/cli.rs, 01-04-SUMMARY.md
- Commits on branch: bc41121, 2a99044 (tasks), 48f15dc (SUMMARY)
- Plan verification re-run: cargo test --bin fili 26 passed; fili --help shows --registry with FILI_REGISTRY and filimon.toml; examples/plugins/.env ignored and absent
