# Phase 4: Plugin CLI - Research

**Researched:** 2026-11-10
**Domain:** Rust CLI (clap 4 derive) over an existing wasmtime component host and TOML/.env plugin config loader
**Confidence:** HIGH (everything here is in-repo code that I read this session; the only external facts are the cargo env var and the clap empty-path probe, both checked)

<user_constraints>
## User Constraints (from CONTEXT.md)

No CONTEXT.md exists for this phase (the user ran plan-phase without discuss-phase).

### Locked Decisions
None from CONTEXT.md. The binding inputs are the roadmap success criteria SC1-SC4 (below), REQUIREMENTS.md CLI-01..CLI-04, and the Phase 3 decisions that carry forward (D-05..D-08 cursor and state-dir rules, Phase 2 D-01 social topic, Phase 1 name rule `[a-z0-9_-]`). The dispositions in `03-REVIEW-DISPOSITION.md` are also binding: WR-10 is "revisit when Phase 4 wires the run command"; WR-08/WR-09/IN-05/IN-06 are deferred past this phase.

### Claude's Discretion
Per the invocation: command names beyond the four in the success criteria, output format, flag names and exit codes are genuinely open. They are listed in "Open Questions (CLI design choices)" with a recommended default each, not decided silently.

### Deferred Ideas (OUT OF SCOPE)
- Real fetch routing, allowlist and timeout enforcement (Phase 5); `fetch` still returns `not-configured`.
- OAuth tokens (Phase 6); `poll` still receives access-token `none`.
- Per-plugin CPU/memory limits (v2 HOST-06), cross-poll dedupe (v2 HOST-07).
- WR-08, WR-09, IN-05, IN-06 (hostile-plugin hardening, revisit before Phase 7).
- Fixing existing `fluvio-wise` gaps (watch mode, batch-only NER); never modify `fluvio/`.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| CLI-01 | `fili plugins list`: registered plugins (source + fetcher), enabled state, last poll status | No place persists poll status today (finding F1). Add a status sidecar file per plugin written by the scheduler; `list` reads it without the state-dir lock (F2). Needs a registry-level listing that tolerates a broken plugin TOML (F3). |
| CLI-02 | `fili plugins check <name>`: component vs WIT world plus config/secrets, no polling | `loader::load_source` / `load_fetcher` already do compile + foreign-import preflight + `instantiate_pre` + export check, but are private (F4). Config problems come from `load_registry`, which is all-or-nothing and needs a per-plugin filter (F3). |
| CLI-03 | `fili plugins run-once <name>`: one poll, print items, no produce unless asked | Add a single-cycle path built on `InstanceFactory`/`PluginInstance` and `map_items`; do not call `run_plugin` (an infinite loop). `--produce` is the option name fixed by SC3 wording (F5). |
| CLI-04 | A run command polls every enabled source and produces to Fluvio until stopped | Wire `Host::load` + `Host::run` with `FluvioSink::connect_social`, a SIGINT/SIGTERM token, a shutdown grace period (WR-10), and the first caller for `--state-dir` (F6, F7). |
</phase_requirements>

## Summary

All the machinery exists as a library after Phase 3: `Host::load`/`Host::run`, `load_components`, `load_registry`, `FluvioSink`, `map_items`, `CursorStore`. Phase 4 is mostly wiring plus four small additions to host and config code that the CLI cannot work without: (1) last-poll status persistence (nothing records it today), (2) public single-component check and single-source load entry points, (3) a lenient registry listing and a per-plugin config-error filter, and (4) a bounded shutdown for the run command (WR-10). None of this needs a new crate.

The riskiest design points are not clap syntax. They are: `CursorStore::open` takes an exclusive directory lock, so `list` and a dry-run `run-once` must read state without it or they fail while `fili run` is up; `load_registry` returns every problem for every plugin or nothing, so `list`/`check` on a healthy plugin would fail because of an unrelated broken one unless the CLI uses a lenient path; and `main()` prints a decorative banner to stdout before every command, which corrupts `--json` output. Also, the real `fili run` binary is hard-wired to the production topic `filimon-social-items` (a Phase 3 security property), so no automated test may run the binary with `run`/`--produce`; the generic orchestration is tested with `VecSink` and an `#[ignore]` live test on a scratch topic, and the binary wiring needs a human UAT step.

**Primary recommendation:** Add `fili plugins {list,check,run-once}` plus top-level `fili run`; add a `<alias>.status.json` sidecar written by `run_plugin` after every cycle; make `list`/`check`/dry-run `run-once` strictly read-only on the state dir and lock-free; add a shutdown grace period to `Host::run`; suppress the banner for the new commands; add one `tests/plugins_cli.rs` integration test that spawns the binary via `CARGO_BIN_EXE_fili` for exit codes.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Argument parsing, subcommand dispatch, exit codes | CLI (`src/cli.rs`, `src/main.rs`) | - | clap derive owns syntax; `miette::Result` from `main` gives exit 1. |
| Rendering list/check/run-once output | CLI (`src/commands/`) | - | Presentation only; sanitizes every guest- or file-derived string. |
| Registry/TOML/secret validation | Config loader (`src/plugins/config.rs`, `secrets.rs`) | CLI filter | Already owns CONF-03 diagnostics; CLI only selects which problems belong to `<name>`. |
| Component vs world check | Host loader (`src/plugins/host/loader.rs`) | - | Only place with the linkers and the `filimon:plugin/` preflight; must be exposed, not re-implemented. |
| Single poll (`run-once`) | Host library (new single-cycle fn) | CLI | Reuses `InstanceFactory`, `map_items`, `ItemSink`; the CLI only prints. |
| Continuous polling, backoff, shutdown | Host library (`Host::run`, `run_plugin`) | CLI (signals) | Already isolates failures; CLI owns signal handling and the grace deadline policy. |
| Last-poll status persistence | State dir (`<alias>.status.json`) written by scheduler | CLI reader | The scheduler is the only code that sees cycle outcomes. |
| Producing items | Sink (`FluvioSink` / `VecSink`) | Fluvio broker | Production sink is pinned to `filimon-social-items`; keep it generic over `ItemSink` for tests. |

## Standard Stack

### Core
No new dependencies. Everything below is already in `Cargo.toml` / `Cargo.lock`. [VERIFIED: Cargo.lock]

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| clap (derive, env) | 4.5.48 | New `Plugins`/`Run` subcommands | Already the CLI framework; global flags `--registry`, `--state-dir` exist. |
| tokio (full) | 1.47.1 | `tokio::signal` for SIGINT/SIGTERM, `select!`, timeouts | `full` is enabled in Cargo.toml line 31; `signal` is part of `full`. [ASSUMED: tokio `full` feature set includes `signal`; confirm with `cargo check`] |
| tokio-util | 0.7.18 | `CancellationToken` shutdown (already used by `Host::run`) | Existing pattern. |
| miette | 7.6.0 | Error reports, exit code 1 via `main() -> miette::Result<()>` | Existing diagnostics pattern; `HostError`, `ConfigError(s)` already derive `Diagnostic`. |
| serde_json | 1.0.150 | `--json` output, status file | Already used for state and records. |
| chrono | 0.4.45 | RFC 3339 UTC timestamp for status `at` | Already used in `src/models.rs`; reuse `to_rfc3339_opts(SecondsFormat::Secs, true)`. |
| wasmtime | 48.0.8 | Component check and run-once | Pinned by Phase 3. |
| tempfile | 3.23.0 | Atomic status write (temp + rename), test tempdirs | Same pattern as `CursorStore::write`. |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Hand-formatted table | `comfy-table` / `tabled` | New dependency for a 4-column table; not worth a legitimacy review. |
| `assert_cmd` | `env!("CARGO_BIN_EXE_fili")` + `std::process::Command` | No new dependency; cargo sets the variable for integration tests. [CITED: doc.rust-lang.org/cargo/reference/environment-variables.html] |
| Status in the cursor file | Separate `<alias>.status.json` | See F1: sidecar leaves the D-05 cursor format untouched. |

**Installation:** none.

## Package Legitimacy Audit

No external packages are installed or added by this phase, so the Package Legitimacy Gate does not apply.

| Package | Registry | Age | Downloads | Source Repo | Verdict | Disposition |
|---------|----------|-----|-----------|-------------|---------|-------------|
| (none) | - | - | - | - | - | - |

**Packages removed due to [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

## Findings Driving The Plan (read these first)

### F1. What Phase 3 persists, and where last-poll status can go
`StateFile` is exactly `{version: u32, plugin: String, cursor: Option<String>}` and is written to `<state_dir>/<alias>.json`. [VERIFIED: src/plugins/host/cursor.rs:30-37] The reader ignores unknown fields (no `deny_unknown_fields`; test `unknown_fields_are_ignored`, cursor.rs:286-295). A test pins the written keys: `assert_eq!(keys, vec!["cursor", "plugin", "version"])` (cursor.rs:367), so extending that file means editing that test. Nothing in the scheduler records poll time or outcome: `PluginReport` (polls, items_sent, dropped, failures, disabled) is in-memory and returned only when the host stops. [VERIFIED: src/plugins/host/scheduler.rs:62-71] plugins/README.md states "The last poll's status is not persisted. The state file is versioned and tolerant of unknown keys so Phase 4 can add it." [VERIFIED: plugins/README.md:294-295]

Two options:
- A. Add `last_poll` to the cursor file. Reads stay tolerant, but every failure cycle must rewrite the file with the cursor, so a status write can persist a cursor the commit-after-flush rule (D-07) did not intend, and the corrupt-file rule (D-08, never rewrite a bad file) gets an exception.
- B (recommended). A sidecar `<state_dir>/<alias>.status.json`: `{"version":1,"plugin":"<alias>","at":"<RFC3339 UTC>","ok":true|false,"error":null|"<cleaned text>","items":<u64 sent|null>}`. Plugin aliases are limited to `[a-z0-9_-]` (`is_valid_name`, config.rs:386-393), so `foo.status.json` can never equal another alias's `<alias>.json`, and `.lock` cannot be an alias. The cursor format (costly to reverse, D-05) is untouched; a status write failure can never affect a cursor; a corrupt cursor file still disables the plugin and its status file still records why.

Write policy: after every cycle in `run_plugin` (PollOk, Guest, InstanceFatal, BuildFailed, SinkFailed, and the disable path including the corrupt-cursor early return), via a new `CursorStore::write_status(alias, &PollStatus)` using the same temp-file-and-rename helper. A status write error is logged at warn (cleaned text) and never fails the cycle or disables the plugin. Use the wall clock (`chrono::Utc::now()`); scheduler tests run under tokio paused time, so they must assert `ok`/`error`/`items`, never the timestamp value. The `error` text is `describe(&outcome)` (already cleaned and clipped to 512 chars).

### F2. `CursorStore::open` takes an exclusive lock; read-only commands must not
`CursorStore::open` creates the directory, creates `<dir>/.lock` and `try_lock()`s it, returning `HostError::StateDirLocked` if another process holds it. [VERIFIED: src/plugins/host/cursor.rs:59-84] `Host::load` calls it. Consequences:
- `fili plugins list` must not use `CursorStore::open` or `Host::load`; add a lock-free free function `read_status(state_dir, alias) -> Result<Option<PollStatus>, ...>` (plain `std::fs::read`; writes are atomic renames so a concurrent read is safe). It must not create the directory: a missing dir or file means "never polled".
- A corrupt, unreadable or oversize status file must render as `unreadable (<reason>)` in the `list` row, not fail the command. Treat file content as untrusted at read time: pass it through `clean_guest_text` / `sanitize_for_terminal` before printing; cap the read (for example 64 KiB).
- `run-once` without `--produce` must read the saved cursor lock-free (split the parse logic of `CursorStore::read` into a free function over `(dir, alias)` that `CursorStore::read` calls) and must never write or open the lock. `run-once --produce` and `run` use `CursorStore::open`, so a second concurrent producer on the same state dir fails fast with `StateDirLocked`, which is the intended protection.

### F3. Config loading is all-or-nothing, and `Host::load` silently skips what it cannot load
`load_registry_with_env` collects all problems and returns either a full `LoadedRegistry` or `Err(ConfigErrors)`; "a partial `LoadedRegistry` is never returned". [VERIFIED: src/plugins/config.rs:9-10, 310-316] For a disabled source, `plugin` is `None` and its TOML is never read ("`plugin` is `Some` exactly when the source is enabled", config.rs:139). Fetchers have no config file (`LoadedFetcher` has `name`, `component`, `enabled`). `.env` is read only when some enabled source declares secrets (config.rs:256-272).

Implications and recommended handling:
- `list` must not fail because one plugin's TOML is broken. Add a registry-level listing in `config.rs` (parse `filimon.toml` only: names, kind, enabled, component, fetcher) that does not read plugin TOMLs or `.env`. `RegistryUnreadable` / `RegistryMalformed` still fail `list` (nothing to list).
- `check <name>`: resolve `<name>` from that listing (so a component check does not depend on other plugins), then run the full `load_registry` and keep only problems for `<name>` plus registry-level problems (`RegistryUnreadable`, `RegistryMalformed`, `DotenvInvalid`). Add `ConfigError::plugin(&self) -> Option<&str>`; every variant except `RegistryUnreadable`, `RegistryMalformed`, `DotenvInvalid` carries a `plugin` field. [VERIFIED: src/plugins/error.rs:16-172] `DuplicateName`/`InvalidName` carry `plugin` too and are correctly attributed.
- A disabled source: the config loader skips its TOML and secrets, so `check` can validate its component but cannot validate its config; see Open Question 3.
- `Host::load` skips a source with no loaded plugin config or a failing component and only reports it in `HostReport.load_failures` at the end of the run. `fili run` needs those at startup: add `Host::load_failures()` and `Host::loaded_sources()` accessors so the run command prints them before polling begins and can refuse to start when zero sources loaded.
- The strict load means one broken enabled plugin TOML blocks `fili run` entirely (CONF-03 error naming plugin and field). That is acceptable and recommended for `run` (explicit and safe); document that `enabled = false` is how an operator skips a plugin.

### F4. The component-vs-world check already exists; expose it
`load_source` and `load_fetcher` (loader.rs:146-172) do: `Component::from_file` (compile), `preflight_imports` (anything outside `filimon:plugin/` is `HostError::ForeignImports`), `source_linker().instantiate_pre` then `SourcePre::new` (or the fetcher equivalents), mapping failure to `HostError::WrongWorld`. [VERIFIED: src/plugins/host/loader.rs:121-172] They are private `fn`s. They do not call `init` or `poll`, so they satisfy "without polling". Expose a `pub fn check_component(kind: PluginKind, name, path, &HostEngine) -> Result<(), HostError>` (or make the two functions `pub(crate)`) and have `check` call it. Do not re-implement world checking or shell out to `wasm-tools`. `PluginKind::world()` is private (loader.rs:40-46); the "declared world" is the registry table the entry sits in (`[sources.*]` is `source`, `[fetchers.*]` is `fetcher`).

`check` must not call guest `init`. `init` runs guest code with real secrets and validates plugin-specific `[settings]` (stub-source uses `deny_unknown_fields`, plugins/stub-source/src/lib.rs). Settings errors surface as a `config` poll-error in `run-once`; see Open Question 5.

### F5. `run-once` is a new single-cycle path, not `run_plugin`
`run_plugin` loops until shutdown or disable. [VERIFIED: src/plugins/host/scheduler.rs:295-431] The traits `InstanceFactory`/`PluginInstance` (instance.rs:56-77) are the seam. A single cycle is: `factory.instantiate(name)` then `instance.init(&settings_toml, &secrets)` then `instance.poll(cursor)` then `map_items(name, response.items)`; with `--produce`, `sink.send_batch` then `cursors.write(..)` in that order (D-07). Reuse `describe(&CycleOutcome)` (pub, scheduler.rs:181) for failure text and `clean_guest_text` for guest strings. Put it in a new host-library function (testable with `FakeFactory` from `testing.rs` and `VecSink`), not inside the command. Load only the one component (`load_source`), not `Host::load`, which compiles every enabled component and opens the state dir and lock.

Details the planner must pin:
- The oversize-cursor rule (`MAX_CURSOR_BYTES`, scheduler.rs:371-384) must apply in `run-once --produce` too, or the cursor written could be one the scheduler would refuse. Share the check rather than copying it.
- The secrets vector is built the same way as `run_plugin` (scheduler.rs:319-323: `(alias, secret.expose().to_string())`); this is the only place a secret value is read.
- Print mapped records (what would be produced) plus the dropped count, not the raw WIT items. Mapped `SourceItem` carries `source_type, platform, id, url, title, text, author, published (RFC 3339), raw`. [VERIFIED: src/models.rs:116-138]
- A non-Ok cycle (guest error, trap, build failure) prints the `describe` text and exits non-zero.
- No timeout exists for a spinning guest; `epoch_deadline_async_yield_and_update` makes the future droppable, so `run-once` must `select!` the cycle against Ctrl-C.

### F6. `fili run` wiring and the WR-10 shutdown carry-over
`Host::run(self, sink, shutdown)` joins all tasks with no grace period. [VERIFIED: src/plugins/host/mod.rs:129-184] A sink send is deliberately not raced against shutdown and the producer lock wait is unbounded (WR-10, deferred "revisit when Phase 4 wires the run command"). With a stuck broker, shutdown can take about N x 60 s (`SINK_TIMEOUT` = 60 s, sink.rs:22), while plugins/README.md says "cancelling the host returns within seconds" (README:287-289), true only for a spinning guest.

Recommended: add a bounded grace period in `Host::run` (via `HostOptions` field, default 10 s): after `shutdown` fires, wait up to the grace for tasks to finish; then `abort_all()` and report each aborted plugin with `disabled: Some("aborted at shutdown")`. Aborting mid-send is safe because the cursor is written only after the sink returned (D-07): the worst case is the already-documented at-least-once replay. A second SIGINT during the grace should abort immediately. Note `HostOptions { state_dir }` is built with a struct literal in `wasm_tests.rs` (line 38-41) and elsewhere; a new field means updating those call sites (or providing a constructor). To test the grace without wasm, extract the supervision loop into a generic function over `InstanceFactory` (today `Host.plugins` holds the concrete `WasmtimeFactory`) and test it with `FakeFactory` plus a never-returning sink under `start_paused = true`. Correct the README sentence to state the grace and the abort behavior.

Run command shape (recommended order, fail fast, nothing held until needed): load registry (strict) then connect `FluvioSink::connect_social()` (preflight: topic exists, error already names `fluvio topic create`) then `Host::load` (creates state dir, takes the lock, compiles components) then print loaded/failed lines then `Host::run` with a token cancelled by SIGINT or SIGTERM (`tokio::signal::ctrl_c`, and `tokio::signal::unix::signal(SignalKind::terminate())` under `#[cfg(unix)]`). Print a per-plugin summary from `HostReport` on stop. Keep `run` generic over `ItemSink` (`run_with_sink`) so tests inject `VecSink`.

### F7. `--state-dir`, WR-07 and the dead-code allows
The global flag is `Option<PathBuf>` with `env = "FILI_STATE_DIR"` and `#[cfg_attr(not(test), allow(dead_code, ...))]`. [VERIFIED: src/cli.rs:43-49] `HostOptions::resolve_state_dir` returns the flag "exactly as given". [VERIFIED: src/plugins/host/mod.rs:51-57] I probed clap 4.5.48 (the locked version) in a scratch crate: an empty value for an `Option<PathBuf>` arg is rejected by clap itself, both as `--state-dir ""` and as `FILI_STATE_DIR=` (`kind: InvalidValue`, value `""`), while `"  "` (whitespace) is accepted and `"x"` is accepted. [VERIFIED: scratch probe at /tmp/clap-probe, output above]. So WR-07 is already unreachable through the CLI; it remains reachable only by calling `resolve_state_dir`/`CursorStore::open` with an empty path directly. Settle it by adding a defensive reject of an empty path in `resolve_state_dir` or `CursorStore::open` (as `HostError::StateDir`, "state directory must not be empty") with a unit test, plus a CLI test `Cli::try_parse_from(["fili","--state-dir","","run"]).is_err()`. Use argument metadata (not the live environment) for env assertions, as the existing cli tests do. Whitespace-only is an operator oddity; reject it too (`trim().is_empty()`) if cheap, otherwise accept.

When wiring, remove the two blanket dead-code allows: `#[cfg_attr(not(test), allow(dead_code, ...))]` on `state_dir` (cli.rs:45-48) and the module-level `#![cfg_attr(not(test), allow(dead_code, ...))]` in `src/plugins/mod.rs:10-16`. Some items may still be unused until Phase 5 (for example fetcher-only fields); narrow with an item-level `#[allow(dead_code, reason = "...")]` rather than keeping the blanket. `Cargo.toml` enables `clippy::pedantic = "warn"`, so run `cargo clippy --all-targets` and keep new code warning-free.

### F8. Output channels
- `main()` prints a three-line banner with `println!` to stdout before dispatching any command. [VERIFIED: src/main.rs:87-89] That corrupts machine-readable stdout. Recommended: skip the banner for `Command::Plugins{..}` (and optionally `Run`); leave it for existing commands so their output is unchanged.
- Logging goes to stderr only. The default filter is `fili::plugins=info` (main.rs:57-59), so tracing events from new code under `fili::commands::...` are filtered out by default. Command-level messages therefore use `println!`/`eprintln!` as the existing commands do, not `tracing`. Scheduler and guest log lines (targets under `fili::plugins::host::...`) do appear by default.
- `finish_plugin` logs `tracing::info!(plugin, ?env_vars, "resolved plugin secrets")` (names only) on every successful load (config.rs:348), so every plugins command prints one stderr line per secret-bearing enabled plugin. It leaks no values; accept it or demote it to `debug` (one-line change) if it makes `check` output noisy.
- Every item field printed to a terminal (title, text, author, url, raw) is guest-controlled: pass through `sanitize_for_terminal` / `clean_guest_text` and clip in human format (Phase 2 convention: "Every record-derived string goes through `sanitize_for_terminal`", output.rs:64-66). `--json` output through `serde_json` is escape-safe.

## Architecture Patterns

### System Architecture Diagram

```
                                  fili <args>
                                       |
                               clap parse (cli.rs)
        +----------+----------+--------+---------+-----------------+
        |          |          |                  |                 |
   plugins list  plugins    plugins          run (top-level)   (existing:
        |        check      run-once              |             produce, consume,
        |          |          |                   |             watch, file)
        v          v          v                   v
  registry     registry   registry          registry (strict load_registry)
  listing      listing    listing                 |
  (filimon     + full     + full                  v
   .toml only) load, filter load, one       FluvioSink::connect_social --> Fluvio :9003
        |      to <name>  source             (topic preflight)             (filimon-social-items)
        |          |          |                   |
        v          v          v                   v
  read_status  check_      load_source       Host::load (state dir + lock,
  (no lock,    component   -> init           compile all enabled) -- failures printed now
   no mkdir)   (compile,   -> poll                |
        |      preflight,  -> map_items           v
        |      world)         |             Host::run: one task per source
        v          |          v                   |  poll -> map_items -> sink -> cursor write
   table / JSON    v      prints items            |  -> status write (<alias>.status.json)
   to stdout   exit 0/1   (sanitized) ;           v
                          --produce: lock +   SIGINT/SIGTERM -> cancel -> grace -> abort
                          sink + cursor       -> per-plugin summary on stdout
                          write + status
```

### Recommended Project Structure
```
src/
├── cli.rs                    # + Command::Plugins{PluginsCommand}, Command::Run; remove dead_code allow
├── main.rs                   # dispatch; skip banner for plugins commands
├── commands/
│   ├── mod.rs                # + pub mod plugins; pub mod run
│   ├── plugins.rs            # list / check / run_once (render fns take impl Write)
│   └── run.rs                # run (generic run_with_sink + thin FluvioSink wiring)
└── plugins/
    ├── config.rs             # + registry listing fn, ConfigError::plugin()
    ├── error.rs              # + ConfigError::plugin()
    └── host/
        ├── cursor.rs         # + PollStatus, write_status, read_status (lock-free), lock-free cursor read
        ├── loader.rs         # + pub check_component / load one source
        ├── once.rs           # single-cycle poll used by run-once (new)
        ├── scheduler.rs      # + status writes after each cycle
        └── mod.rs            # + load_failures()/loaded_sources() accessors, shutdown grace
tests/
└── plugins_cli.rs            # spawns env!("CARGO_BIN_EXE_fili"); exit codes and stdout shape
```
(`once.rs` is a suggestion; the planner may place the function in `mod.rs`.)

### Pattern 1: Render functions over `impl Write`
**What:** each command computes a plain data struct (`ListRow`, `CheckReport`, `RunOnceOutcome`), and a separate `render(&data, &mut impl Write)` formats it.
**When to use:** all four commands, so unit tests assert exact text and JSON shape without spawning the binary.
**Example:**
```rust
// Source: pattern derived from src/output.rs and src/cli.rs tests in this repo
pub fn render_list(rows: &[ListRow], out: &mut impl std::io::Write) -> std::io::Result<()> { /* ... */ }
```

### Pattern 2: Unknown name is a CLI error that lists what exists
**What:** `<name>` is only ever used as a key into the registry's maps; it is never joined into a path. On a miss, return a diagnostic naming the registered sources and fetchers.
**When to use:** `check` and `run-once`. A fetcher given to `run-once` is a distinct clear error ("is a fetcher; run-once polls sources").

### Anti-Patterns to Avoid
- **Using `Host::load` for `check`, `list` or dry-run `run-once`:** it creates the state dir, takes the lock and compiles every component. Use the narrow entry points.
- **Calling `CursorStore::open` for any read-only command:** fails with `StateDirLocked` while `fili run` is up.
- **Printing the whole config error set for `check <name>`:** filter to the plugin plus registry-level problems.
- **Re-implementing world/import checks or shelling to `wasm-tools`:** drift from what the host actually accepts.
- **Running the real `fili run`/`--produce` binary in automated tests:** it writes to `filimon-social-items` (the production sink takes no topic argument by design).
- **Writing to stdout from `tracing` or adding the banner to machine output.**

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| WIT world conformance | An export/import walker or a `wasm-tools` subprocess | `loader::load_source`/`load_fetcher` (expose as `check_component`) | Already covers foreign imports, wrong world, compile errors with diagnostics and tests. |
| Config/secret validation | A second TOML/.env parser | `load_registry` + `ConfigError::plugin()` filter | CONF-03 diagnostics, span labels and secret redaction already exist. |
| Atomic file write | `fs::write` | The `tempfile::NamedTempFile::new_in` + `persist` helper pattern from `CursorStore::write` | Readers (`list`) may race the writer. |
| Item to record mapping and validation | Custom printing of raw WIT items | `map_items` | Same drops and caps as production; shows what would really be produced. |
| Producer error recovery | Custom retry | `FluvioSink::send_batch` (already bounded, clears sticky errors) | Fluvio 0.50.1 errors are sticky; Phase 3 fixed it. |
| Terminal-safe text | Ad-hoc escaping | `sanitize_for_terminal` / `clean_guest_text` | Strips ESC and control chars; clips by character. |
| Signal handling | A signal crate | `tokio::signal` + `CancellationToken` | Already in the dependency set. |
| CLI integration tests | `assert_cmd` | `env!("CARGO_BIN_EXE_fili")` + `std::process::Command` | No new dependency. |
| Table rendering | A table crate | `format!("{:<w$}")` columns | Four columns. |

**Key insight:** every hard problem here (isolation, cursors, config diagnostics, world checks) is solved in Phases 1 and 3. Phase 4 risk is in the seams (locking, all-or-nothing loading, stdout hygiene, shutdown bound), not in new algorithms.

## Runtime State Inventory

Not a rename/refactor phase; omitted. State added by this phase: new files `<state_dir>/<alias>.status.json`. No existing stored data, OS registration, secret name or build artifact changes name. The `.fili-state/` directory, `.lock` and `<alias>.json` formats are unchanged.

## Common Pitfalls

### Pitfall 1: `list` dies while `fili run` is running
**What goes wrong:** `list` calls `CursorStore::open` and gets `StateDirLocked`.
**Why:** the lock is held for the host's lifetime (cursor.rs:51-84).
**How to avoid:** lock-free `read_status`; test it by holding a `CursorStore` open and calling `list`.
**Warning signs:** `list` creating `.fili-state/` or `.lock` in a fresh checkout.

### Pitfall 2: One broken plugin TOML hides every plugin
**What goes wrong:** `fili plugins list` prints only a config error report.
**Why:** `load_registry` is all-or-nothing.
**How to avoid:** registry-level listing for `list`; filtered problems for `check <name>`.
**Warning signs:** `check good` exits non-zero because of `bad`.

### Pitfall 3: Banner and logs corrupt `--json`
**What goes wrong:** stdout starts with box-drawing characters.
**Why:** `main` prints the banner unconditionally (main.rs:87-89).
**How to avoid:** skip the banner for `plugins`; integration test parses stdout as JSON.

### Pitfall 4: Status written without a cursor change, or cursor persisted by a status write
**What goes wrong:** option A corrupts D-07 semantics.
**How to avoid:** sidecar file (F1). Test: a failed sink send leaves the cursor file byte-identical while the status file says error.

### Pitfall 5: `run-once` without `--produce` moves the cursor
**What goes wrong:** items are skipped on the next real run.
**How to avoid:** dry run takes no lock and writes nothing; test asserts the state dir is unchanged (and not created).

### Pitfall 6: Terminal escape injection through item text
**What goes wrong:** a plugin returns ESC sequences in `title`/`text`; `run-once` prints them raw.
**How to avoid:** sanitize and clip every printed field; unit test with `"\u{1b}[31m"`.

### Pitfall 7: Slow or never-ending shutdown
**What goes wrong:** Ctrl-C with a stuck broker takes N x 60 s (WR-10).
**How to avoid:** grace period plus abort; second signal aborts at once; paused-time test with a never-returning sink.

### Pitfall 8: Tests that publish to the production topic
**What goes wrong:** an integration test runs `fili run`/`--produce` and writes to `filimon-social-items`.
**How to avoid:** automated tests use `VecSink` (generic `run_with_sink`) or an `#[ignore] live_` test with `scratch_topic`; the binary-level `run`/`--produce` check is a manual UAT item.

### Pitfall 9: Test filter collisions
**What goes wrong:** libtest ORs filters, and names containing `wasm_` or `live_` get swept into the other ignored group (03-VALIDATION.md filter note).
**How to avoid:** name real-wasm CLI tests `wasm_...`, cluster tests `live_...`, and keep non-ignored tests free of those substrings (including module names).

### Pitfall 10: Secret values in output
**What goes wrong:** `check` echoes resolved secrets.
**How to avoid:** print alias and env var NAMES only; `ResolvedSecrets` redacts in Debug; a test sets a distinctive secret value and asserts it never appears in stdout or stderr.

## Code Examples

### Reading the status file lock-free (shape to implement)
```rust
// Source: shape derived from src/plugins/host/cursor.rs (CursorStore::read/write) in this repo
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PollStatus {
    pub version: u32,
    pub plugin: String,
    pub at: String,          // RFC 3339 UTC, seconds, e.g. 2026-11-10T09:14:03Z
    pub ok: bool,
    pub error: Option<String>,
    pub items: Option<u64>,
}
// <state_dir>/<alias>.status.json ; atomic write like CursorStore::write; read with plain std::fs::read.
```

### Process-level test of an exit code (no new dependency)
```rust
// Source: https://doc.rust-lang.org/cargo/reference/environment-variables.html (CARGO_BIN_EXE_<name>)
let out = std::process::Command::new(env!("CARGO_BIN_EXE_fili"))
    .args(["--registry", registry.to_str().unwrap(), "plugins", "check", "stub"])
    .env_remove("FILI_STATE_DIR")
    .output()
    .expect("run fili");
assert!(!out.status.success());
```

### Shutdown signal to a `CancellationToken`
```rust
// Source: tokio::signal docs; pattern only, unix-gated branch is required for SIGTERM
let token = tokio_util::sync::CancellationToken::new();
let on_signal = token.clone();
tokio::spawn(async move {
    let _ = tokio::signal::ctrl_c().await;
    on_signal.cancel();
});
```
(The `tokio::signal` API is tagged [ASSUMED] from training knowledge; confirm with `cargo check` when implementing.)

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `assert_cmd` for binary tests | `CARGO_BIN_EXE_<name>` env var | long-standing cargo feature | no dev-dependency needed |
| Banner on every run | Banner only for legacy commands | this phase | keeps stdout parseable |

**Deprecated/outdated:** none relevant.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | tokio `full` includes the `signal` feature and `tokio::signal::unix::signal(SignalKind::terminate())` is available | Standard Stack, F6 | `cargo check` fails; add `signal` to the tokio features. Low risk. |
| A2 | Aborting a task mid-`send_batch` at shutdown is acceptable (at-least-once replay only) | F6 | If the user wants no abort ever, the grace must be unbounded and WR-10 stays open. Safe by D-07, but it is a behavior choice; see Open Question 8. |
| A3 | A 64 KiB read cap on the status file is a sensible bound | F2 | Cosmetic. |
| A4 | Exit code 1 for any failure (miette default) is enough; no distinct codes | Open Question 9 | Scripts wanting to tell "config error" from "component error" would need codes. |
| A5 | `tokio::signal::ctrl_c` cancels cleanly while `Host::load` compiles components (blocking) | F6 | Ctrl-C during compile is delayed until compile ends; acceptable. |

## Open Questions (CLI design choices)

Each has a recommended default; the planner should adopt the default unless the user rules otherwise (plan-phase may surface them to the user).

1. **Name and shape of the run command (CLI-04).**
   - What we know: roadmap says "`fili plugins list | check | run-once` and a run command"; existing top-level commands are verbs (`produce`, `consume`, `watch`, `file`).
   - Recommendation: top-level `fili run` (no flags beyond the global `--registry`/`--state-dir`). Alternative `fili plugins run` is rejected: it blurs "inspect" with "operate".
2. **Output format.**
   - Recommendation: human table by default; `--json` on `list` (JSON array) and on `run-once` (one `SourceItem` per line). `check` is human only. Human item output clips long fields (for example 200 chars) and shows `platform`, `id`, `url`, `title`, `author`, `published`, a text preview.
3. **Disabled plugins in `check` and `run-once`.** The config loader never reads a disabled source's TOML or secrets.
   - Recommendation: `check` of a disabled source validates the component only and prints `config: skipped (plugin is disabled)` (exit 0 if the component passes); `run-once` of a disabled source fails with "plugin `x` is disabled in filimon.toml; set `enabled = true` to run it". Validating a disabled plugin's config would need a loader flag that forces it on; defer unless the user wants it.
4. **`run-once` starting cursor.**
   - Recommendation: both modes start from the saved cursor (dry run reads it lock-free, never writes), so the dry run shows what the next real poll would fetch. Alternative: dry run always starts from `none` (stable output, but differs from what `--produce` fetches). Do not add a `--from-start` flag in this phase.
5. **Does `check` run guest `init`?** Recommendation: no (it says "without polling"; `init` executes guest code with secrets). Plugin-specific `[settings]` errors show up in `run-once` as a `config` error. Revisit with an opt-in `--init` later.
6. **Per-plugin status shape.** Recommendation: F1 option B with `items` included; `list` renders `2026-11-10T09:14:03Z ok (3 items)` or `... error: <text clipped to ~80>`, and `-` when never polled. Fetchers show `n/a` (they are never polled).
7. **Status for a disabled-by-fault plugin.** Recommendation: record an `ok:false` status with the disable reason (cleaned) so `list` shows why; no separate `disabled` field this phase.
8. **Shutdown grace.** Recommendation: constant 10 s in `HostOptions`, second signal aborts immediately, exit 0 on an operator-initiated stop even if tasks were aborted (a warning names them). No `--shutdown-grace` flag.
9. **Exit codes.** Recommendation: 0 success; 1 for every failure via `miette` (check failed, config invalid, unknown plugin, topic missing, state dir locked, no source loaded, every plugin disabled); 2 for clap usage errors (clap default). `fili run` exits 1 when zero sources loaded or when every plugin ended disabled; otherwise 0.
10. **Startup partial failure in `run`.** Recommendation: components that fail to load are printed at startup (error level, naming plugin and path) and the rest run (consistent with HOST-05); zero loaded sources is an error.
11. **Demote the "resolved plugin secrets" info log?** Recommendation: leave as is unless it clutters `check` output in UAT.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust toolchain | build, tests | yes | cargo/rustc 1.96.0 | - |
| cargo-component | rebuilding plugin fixtures | yes | on PATH | - |
| wasm-tools | conformance spot checks | yes | on PATH | - |
| Built fixtures (`stub_source.wasm`, `stub_fetcher.wasm`, `fault_source.wasm`) | `wasm_` tests, `run-once` demo | yes | present under `plugins/*/target/wasm32-unknown-unknown/release/` | rebuild per plugins/README.md |
| Fluvio cluster | `live_` tests, manual `run` UAT | yes | 127.0.0.1:9003 accepts connections; `fluvio` CLI on PATH | - |

**Missing dependencies with no fallback:** none.
**Missing dependencies with fallback:** none. I did not check whether `filimon-social-items` exists on the cluster and did not touch the cluster.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in harness; `#[tokio::test(start_paused = true)]` for scheduler/shutdown (`test-util` present); new `tests/plugins_cli.rs` integration test driving the binary via `CARGO_BIN_EXE_fili` |
| Config file | none (Cargo.toml); `tests/` does not exist yet |
| Quick run command | `cargo test -p filimon plugins::` and `cargo test -p filimon cli::tests` and `cargo test -p filimon commands::` |
| Full suite command | `cargo test -p filimon` (last reported baseline: 255 passed, 29 ignored) |
| Wasm group | `cargo test -p filimon -- --ignored wasm_` (single substring on purpose) |
| Live group | `cargo test -p filimon -- --ignored live_` (scratch `filimon-test-*` topics only) |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| CLI-01 | `list` rows for source, fetcher, disabled, never-polled, ok, error, unreadable status; works with a broken plugin TOML; works while the state-dir lock is held; creates nothing | unit (render + data) | `cargo test -p filimon commands::plugins` | no, Wave 0 |
| CLI-01 | Scheduler writes status after PollOk, guest error, trap, sink failure, disable, corrupt cursor; failure leaves cursor file byte-identical | unit (fakes, paused time) | `cargo test -p filimon plugins::host::scheduler` | partly (module exists), new tests Wave 0 |
| CLI-01 | `read_status`/`write_status` round trip, atomic, tolerant of garbage, oversize cap, no `.status` collision with cursor names | unit | `cargo test -p filimon plugins::host::cursor` | module exists, new tests |
| CLI-01 | `fili plugins list` and `--json` exit 0 and stdout is exactly the table / valid JSON (no banner) | integration (binary) | `cargo test -p filimon --test plugins_cli list` | no, Wave 0 |
| CLI-02 | check passes for a good source and fetcher component (real wasm) | `wasm_` | `cargo test -p filimon -- --ignored wasm_check` | no, Wave 0 |
| CLI-02 | wrong world, garbage bytes, WASI import each fail and name the plugin and the problem (`ForeignImports`, `WrongWorld`, `ComponentLoad`) | unit (garbage and `WASI_IMPORT_COMPONENT` bytes, as in loader tests) | `cargo test -p filimon commands::plugins::tests::check` | no, Wave 0 |
| CLI-02 | missing secret, bad TOML, bad `[host]` value each fail naming plugin and field; problems of other plugins are not reported; registry-level problems are | unit with `load_registry_with_env` | `cargo test -p filimon commands::plugins::tests::check` | no, Wave 0 |
| CLI-02 | `check` never polls and never creates the state dir | unit (state dir absent afterwards) | same | no, Wave 0 |
| CLI-02 | exit code non-zero on failure, zero on success; secret value never printed | integration (binary) | `cargo test -p filimon --test plugins_cli check` | no, Wave 0 |
| CLI-03 | single poll prints mapped items; dropped count shown; sanitized and clipped; `--json` is one `SourceItem` per line | unit with `FakeFactory` | `cargo test -p filimon plugins::host::once` | no, Wave 0 |
| CLI-03 | no `--produce`: sink never called, state dir untouched; `--produce`: send then cursor write, cursor unchanged on sink failure, lock held | unit (`VecSink`, failing sink) | same | no, Wave 0 |
| CLI-03 | guest error, trap and rate-limited produce non-zero outcome with `describe` text | unit | same | no, Wave 0 |
| CLI-03 | real stub component: `run-once stub` prints `stub-0` without Fluvio | `wasm_` (binary) | `cargo test -p filimon -- --ignored wasm_run_once` | no, Wave 0 |
| CLI-04 | `Host::run` returns within the grace and reports aborted plugins when the sink never returns | unit (paused time, generic supervisor) | `cargo test -p filimon plugins::host::tests` | no, Wave 0 |
| CLI-04 | `run_with_sink` polls all enabled sources, skips disabled, prints load failures at startup, errors on zero sources | unit (`VecSink`) / `wasm_` | `cargo test -p filimon commands::run` | no, Wave 0 |
| CLI-04 | run against a scratch topic produces records; stop; restart resumes | `live_` (scratch topic, `FluvioSink::connect(scratch)`) | `cargo test -p filimon -- --ignored live_run` | no, Wave 0 |
| CLI-04 | the real `fili run` binary on `filimon-social-items`, Ctrl-C stops it | manual UAT only (writes to the production topic; user runs it) | n/a | n/a |
| WR-07 | empty `--state-dir` rejected by clap and by `resolve_state_dir`/`CursorStore::open` | unit | `cargo test -p filimon cli::tests` and `plugins::host::tests` | module exists |
| CLI surface | all new subcommands and flags parse; `--state-dir` and `--registry` work before and after the subcommand; `Plugins` requires a subcommand | unit | `cargo test -p filimon cli::tests` | module exists, new tests |

### Sampling Rate
- **Per task commit:** `cargo test -p filimon plugins::` plus the module under change (`cli::tests` or `commands::`).
- **Per wave merge:** `cargo test -p filimon` and `cargo clippy --all-targets`.
- **Phase gate:** full suite green, then `-- --ignored wasm_`, then (cluster up) `-- --ignored live_`; report any group that could not run as NOT RUN. Plain `cargo test` does not exercise real wasm.

### Wave 0 Gaps
- [ ] `tests/plugins_cli.rs`: binary-level exit-code and stdout tests (directory `tests/` is new).
- [ ] Unit-test helpers for registry fixtures in `commands/plugins.rs` tests (reuse `testing.rs::write_registry` and `load_test_registry`; they are `cfg(test)` and `pub(crate)`).
- [ ] A generic supervisor in `host/mod.rs` testable with `FakeFactory` (precondition for the paused-time shutdown test).
- [ ] Update every `HostOptions { state_dir }` literal if a field is added.
- No framework install needed.

## Security Domain

`security_enforcement` is enabled in `.planning/config.json` (`security_asvs_level: 1`, `security_block_on: high`).

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | no | OAuth is Phase 6 |
| V3 Session Management | no | - |
| V4 Access Control | partial | The registry is the only source of loadable components; `<name>` is a map key only, never a path |
| V5 Input Validation | yes | clap value parsers; registry name rule `[a-z0-9_-]`; sanitize and clip guest and file-derived text; bounded status file read |
| V6 Cryptography | no | TLS and tokens are later phases |
| V7 Error Handling and Logging | yes | Names only (aliases, env var names), never secret values, settings or item content in logs and errors; existing `ConfigError`/`HostError` already follow this |
| V12 Files and Resources | yes | Atomic writes; read-only commands create nothing; lock respected; no deletion or repair of bad state files (D-08) |
| V14 Configuration | yes | Strict config load for `run`; clear named errors |

### Known Threat Patterns for this stack

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Terminal escape injection through plugin-supplied item text or a tampered status file | Tampering | `sanitize_for_terminal`/`clean_guest_text`, clip in human output, JSON via serde |
| Secret disclosure in `check`, `list`, `run-once` output or logs | Information disclosure | Print alias and env var names only; test with a distinctive secret value asserting it never appears on stdout or stderr |
| Path traversal via the plugin name argument | Tampering | Look up `<name>` in the registry map first; state file names come only from registry aliases |
| A dry run or `check` mutating state (cursor advance, directory creation) | Tampering | Read-only code paths; tests assert the state dir is unchanged |
| Two producers sharing a cursor | Tampering / DoS | `CursorStore::open` lock for `run` and `run-once --produce` |
| Test or CLI writing to the production topic | Tampering | Production sink pinned to `filimon-social-items` with no topic parameter; tests use `VecSink` or scratch topics |
| Unbounded shutdown (stuck broker) | DoS | Grace period then abort (cursor safe by D-07) |
| Oversized status or state file read | DoS | Size cap on read |

## Sources

### Primary (HIGH confidence)
- In-repo files read this session: `src/cli.rs`, `src/main.rs`, `src/commands/mod.rs`, `src/plugins/mod.rs`, `src/plugins/config.rs` (lines 1-640), `src/plugins/error.rs`, `src/plugins/secrets.rs` (resolve), `src/plugins/host/{mod,cursor,engine,instance,loader,error,map,sink,testing,wasm_tests}.rs`, `src/plugins/host/scheduler.rs` (lines 1-700), `src/models.rs` (SourceItem, time helpers), `src/streaming.rs` (topics, producer), `src/output.rs` (sanitize), `Cargo.toml`, `Cargo.lock`, `plugins/README.md`, `wit/*.wit`, `examples/plugins/*`.
- `.planning/REQUIREMENTS.md`, `.planning/STATE.md`, `.planning/ROADMAP.md` Phase 4, `03-CONTEXT.md`, `03-VERIFICATION.md`, `03-REVIEW.md` (WR-07, WR-10), `03-REVIEW-DISPOSITION.md`, `03-VALIDATION.md`.
- Scratch probe of clap 4.5.48 empty `PathBuf` handling (flag and env), run in `/tmp/clap-probe` (outside the repo).
- https://doc.rust-lang.org/cargo/reference/environment-variables.html (`CARGO_BIN_EXE_<name>`).

### Secondary (MEDIUM confidence)
- none

### Tertiary (LOW confidence)
- `tokio::signal` API shape and `full` feature membership from training knowledge (A1).

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH, no new dependencies; versions read from `Cargo.lock`.
- Architecture: HIGH, every integration seam was read in source; the recommended additions are small and follow existing patterns.
- Pitfalls: HIGH for lock, all-or-nothing load, banner, topic safety (all read in code); MEDIUM for the grace-period design (behavior choice, see Open Question 8).

**Research date:** 2026-11-10
**Valid until:** 2026-12-10 (stable; invalidated if Phase 3 code changes before planning)

## Project Constraints (from CLAUDE.md)

`./.claude/CLAUDE.md` imports `AGENTS.md` and `WORKFLOW.md`, which describe the GSD Path pipeline (a separate `.project/` workflow with a router and role agents). This repository's active workflow is GSD (`.planning/`); `.project/` does not exist here, so the Path-specific rules (STATE.md in `.project/`, archive transactions, INTENT.md) do not apply to this phase. The directives that do carry over:
- Files are the only memory: start from disk; report an absent input rather than inventing it.
- Claims need checked sources and verdicts need reproduced evidence; never report a command as passing unless it ran; report partial results plainly.
- Preserve uncertainty (`NEEDS-USER`/`RESEARCH`/confidence); record user corrections verbatim.
- Any user-facing checkpoint uses the Outcome / Review (absolute-path Markdown link) / Next shape, and every choice names a recommended option listed first.
- Project conventions from `.planning/codebase/CONVENTIONS.md`: Rust 2024, `clippy::all` and `pedantic` warn, module header banner comments with the `// ───` style, imports ordered std, external, `crate::`.
- Standing user constraints from the invocation: Fluvio tests are `#[ignore]` and use scratch `filimon-test-*` topics only; never modify `fluvio/`; do not fix the known `fluvio-wise` gaps; Rust pinned to 1.96.0, wasmtime 48.
