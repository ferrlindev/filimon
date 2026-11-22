---
last_mapped_commit: ee408de9974ea0cee802f332ee2386ef13b0428b
last_mapped_at: 2026-11-22
---
# Codebase Structure

**Analysis Date:** 2026-11-22

## Directory Layout

```
filimon/
├── src/                  # Main `fili` binary
│   ├── main.rs           # Entry, logging, subcommand routing
│   ├── cli.rs, args.rs   # clap definitions
│   ├── commands/         # auth, consume, plugins, produce, run, watch
│   ├── crawler.rs, check.rs, models.rs, scorer.rs, streaming.rs, output.rs, config.rs
│   └── plugins/          # config, secrets, error, auth/, host/
├── plugins/              # Wasm guest crates (each standalone, own Cargo.lock)
│   ├── rss/ reddit/ youtube/        # source plugins
│   ├── stub-source/ fault-source/   # test sources
│   ├── http-fetcher/ replay-fetcher/ stub-fetcher/  # fetchers
│   └── template/                    # starter plugin
├── smartmodules/         # filter-relevance, map-extract, map-preprocess
├── wit/                  # host.wit, types.wit, worlds.wit
├── examples/plugins/     # filimon.toml registry + per-plugin tomls
├── examples/fixtures/    # sample HTML
├── tests/                # CLI integration tests (auth_cli.rs, plugins_cli.rs)
├── scripts/              # live-e2e.sh, register-compose-spu.sh, verify-authoring.sh
├── docker/, Dockerfile, docker-compose.yml  # Fluvio cluster + service
├── docs/plugin-guide.md
├── transforms*.yaml      # SmartModule chain definitions
├── config.json
├── models/               # GLiNER model (gitignored)
├── fluvio/, target/, output/  # gitignored
└── .planning/            # GSD planning artifacts
```

## Directory Purposes

**`src/commands/`:** one file per subcommand, exported via `src/commands/mod.rs`.
**`src/plugins/host/`:** wasm runtime; unit tests in `*_tests.rs` and `testing.rs` (cfg(test)).
**`src/plugins/auth/`:** OAuth client, PKCE, callback server, token store, providers.
**`plugins/<name>/`:** guest crate; larger ones split into `component.rs`, `poll.rs`, `cursor.rs`, `http.rs`, `settings.rs`, `api.rs`, `feed.rs`; tests in `tests/replay.rs` with `tests/fixtures/`.
**`wit/`:** the only guest/host contract; change here affects `src/plugins/host/engine.rs` and every plugin.

## Key File Locations

**Entry Points:** `src/main.rs`; workspace is declared in `Cargo.toml` (`[workspace]`).
**Configuration:** `Cargo.toml`, `rust-toolchain.toml`, `config.json`, `examples/plugins/filimon.toml`, `.env.example`.
**Core Logic:** `src/plugins/host/scheduler.rs`, `src/streaming.rs`, `src/models.rs`.
**Testing:** inline `#[cfg(test)]` modules, `tests/*.rs`, `plugins/*/tests/`.

## Naming Conventions

**Files:** snake_case `.rs`; test-only modules `*_tests.rs`; plugin crates kebab-case directories.
**Directories:** lowercase kebab-case (`stub-source`, `map-extract`).
**Topics:** `filimon-<name>` constants `TOPIC_*` in `src/streaming.rs`.
**Source file header:** a banner comment block naming the file and its purpose.

## Where to Add New Code

**New subcommand:** variant in `src/cli.rs`, handler `src/commands/<name>.rs`, export in `src/commands/mod.rs`, route in `src/main.rs`; CLI tests in `tests/`.
**New source plugin:** copy `plugins/template/`, add `examples/plugins/<name>.toml` and register in `examples/plugins/filimon.toml`; follow `plugins/rss/` layout; see `docs/plugin-guide.md`.
**New SmartModule:** `smartmodules/<name>/` with `SmartModule.toml`; wire in `transforms*.yaml`.
**Host feature:** `src/plugins/host/<file>.rs`, declare in `src/plugins/host/mod.rs`.
**Shared helpers:** `src/output.rs` or `src/models.rs`.

## Special Directories

**`target/`, `fluvio/`, `models/`, `output/`:** generated or vendored, gitignored.
**`plugins/*/tests/fixtures/`:** recorded HTTP responses for replay tests, committed.
**`.planning/`, `.project/`, `.gsd-path/`:** pipeline artifacts, not product code.

---

*Structure analysis: 2026-11-22*
