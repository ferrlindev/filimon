---
last_mapped_commit: ee408de9974ea0cee802f332ee2386ef13b0428b
last_mapped_at: 2026-11-22
---
# Technology Stack

**Analysis Date:** 2026-11-22

## Languages

**Primary:**
- Rust (edition 2024, toolchain 1.96.0 pinned in `rust-toolchain.toml`, MSRV 1.95 in `Cargo.toml`) - CLI binary `fili` (`src/`), SmartModules, plugins

**Secondary:**
- WIT (WebAssembly Interface Types) - plugin contract in `wit/host.wit`, `wit/types.wit`, `wit/worlds.wit`
- Shell - `scripts/live-e2e.sh`, `scripts/register-compose-spu.sh`, `scripts/verify-authoring.sh`
- YAML / JSON - `transforms*.yaml`, `config.json`, `docker-compose.yml`

## Runtime

**Environment:**
- Native binary (tokio async runtime); Wasm components hosted via wasmtime
- Plugins compile to wasm32 components via cargo-component

**Package Manager:**
- Cargo
- Lockfile: present (`Cargo.lock`)

## Frameworks

**Core:**
- clap 4 (derive, env) - CLI (`src/cli.rs`, `src/args.rs`, `src/commands/`)
- tokio 1 (full) - async runtime
- fluvio 0.50 - streaming client (`src/streaming.rs`)
- wasmtime 48 (component-model, cranelift) - plugin host (`src/plugins/host/`)
- gline-rs 1.0 + orp 0.9 (ONNX Runtime via `ort` 2.0.0-rc.9) - GLiNER NER scoring (`src/scorer.rs`), model in `models/gliner_small-v2.1`
- reqwest 0.12 (blocking, rustls-tls) + scraper 0.21 - crawling (`src/crawler.rs`)
- miette 7 / thiserror 2 - error reporting; tracing + tracing-subscriber - logging
- watchexec 8 - file watching

**Testing:**
- Built-in `cargo test`; tokio `test-util` (paused time)

**Build/Dev:**
- Workspace members: `.`, `smartmodules/filter-relevance`, `smartmodules/map-preprocess`, `smartmodules/map-extract` (fluvio-smartmodule 0.50, cdylib)
- `plugins/` excluded from workspace; each plugin (`plugins/rss`, `reddit`, `youtube`, `http-fetcher`, `replay-fetcher`, `stub-*`, `fault-source`, `template`) has its own `[workspace]`, wit-bindgen-rt 0.41
- Lints: clippy `all` + `pedantic` warn, `unsafe_code` warn
- Profile `release-lto` (lto, opt-level s)
- `fluvio/` directory is a vendored Fluvio checkout (own Cargo.lock/Makefile)

## Key Dependencies

**Critical:**
- fluvio 0.50, wasmtime 48, gline-rs/orp/ort, reqwest, serde/serde_json, toml 1

**Infrastructure:**
- sha2, base64, getrandom 0.3, open 5 (PKCE/OAuth login in `src/plugins/auth/`), tokio-util, regex, url, chrono, tempfile
- Plugin-side: quick-xml 0.37 (RSS), time 0.3, percent-encoding 2 (YouTube)

## Configuration

**Environment:**
- `.env` (present, not read) and `.env.example`; keys: `REDDIT_CLIENT_ID`, `REDDIT_CLIENT_SECRET`, `YOUTUBE_API_KEY`, `REDDIT_USER_AGENT_CONTACT`
- Plugin secrets resolved from process env then `.env` (`src/plugins/secrets.rs`) via `[host.secrets]` aliases
- App config: `config.json`, `src/config.rs`, plugin registry `src/plugins/config.rs`; Fluvio client config in `docker/fluvio-config`
- Transform chains: `transforms.yaml`, `transforms-extract-only.yaml`, `transforms-filter-only.yaml`

**Build:**
- `Cargo.toml`, `rust-toolchain.toml`, `Dockerfile` (multi-stage; ORT_STRATEGY=download, ONNX Runtime 1.24), `docker-compose.yml`

## Platform Requirements

**Development:**
- Rust 1.96.0 with clippy/rustfmt, cargo-component for plugins, cmake/pkg-config/libssl for Docker build, Unix (file mode 0o600 usage)

**Production:**
- Docker Compose: `fluvio-sc`, `fluvio-spu` (infinyon/fluvio), `filimon`, optional `model-downloader` (profile `setup`)

---

*Stack analysis: 2026-11-22*
