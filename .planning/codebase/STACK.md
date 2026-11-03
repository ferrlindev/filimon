---
last_mapped_commit: 5122e6b9c4aa51ca49831917f1a1035fdc543870
last_mapped_at: 2026-11-03
---
# Technology Stack

**Analysis Date:** 2026-11-03

## Languages

**Primary:**
- Rust 1.85.0 - Core application; required for SmartModule WASM compilation (`Cargo.toml` line 4)

**Secondary:**
- WASM (Rust compiled) - SmartModule transformations in Fluvio pipeline
- Python 3.12 - Optional model downloader helper in Docker setup

## Runtime

**Environment:**
- Native binary compiled to Linux/macOS/Windows (via Rust)
- Docker deployment with Debian bookworm-slim runtime image

**Package Manager:**
- Cargo 1.85.0 (Rust package manager)
- Lockfile: `Cargo.lock` present and required (--locked flag used in Docker)

## Frameworks

**Core:**
- Fluvio 0.50 - Event streaming platform for pipeline coordination (`Cargo.toml` line 36)
- Tokio 1.0 (with full features) - Async runtime for concurrent operations (`Cargo.toml` line 30)

**CLI & Utilities:**
- Clap 4 (with derive and env features) - Command-line argument parsing (`Cargo.toml` line 19)
- Miette 7 (with fancy) - Enhanced error reporting and diagnostics (`Cargo.toml` line 31)

**Data Processing:**
- Scraper 0.21 - HTML parsing for content extraction (`Cargo.toml` line 21)
- Serde 1.0 + Serde_json 1.0 - JSON serialization/deserialization (`Cargo.toml` lines 22-23)

**File & System Watch:**
- Watchexec 8.0 - Filesystem event monitoring (`Cargo.toml` line 27)
- Watchexec-events 6.0 - Event type definitions (`Cargo.toml` line 28)
- Watchexec-signals 5.0 - Signal handling (`Cargo.toml` line 29)

**ML/NER:**
- GLiNER (via gline-rs 1.0) - Zero-shot Named Entity Recognition engine (`Cargo.toml` line 32)
- ORP 0.9 - ONNX Runtime bindings for model inference (`Cargo.toml` line 33)

**Logging & Tracing:**
- Tracing 0.1 - Structured logging framework (`Cargo.toml` line 38)
- Tracing-subscriber 0.3 (with env-filter) - Log filtering and output control (`Cargo.toml` line 39)

**Utilities:**
- Reqwest 0.12 (with blocking + rustls-tls, no default-features) - HTTP client for web crawling (`Cargo.toml` line 20)
- URL 2.0 - URL parsing and manipulation (`Cargo.toml` line 24)
- Regex 1.0 - Pattern matching for text processing (`Cargo.toml` line 25)
- Tempfile 3.0 - Temporary file/directory creation (`Cargo.toml` line 26)
- Futures-util 0.3 - Async utilities (`Cargo.toml` line 37)

## SmartModule Dependencies

**SmartModule Framework:**
- fluvio-smartmodule 0.50 - WASM framework for streaming transformations
  - Used in: `smartmodules/map-preprocess/Cargo.toml`
  - Used in: `smartmodules/map-extract/Cargo.toml`
  - Used in: `smartmodules/filter-relevance/Cargo.toml`

## Configuration

**Environment:**
- Runtime configuration via environment variables (see INTEGRATIONS.md)
- Fluvio cluster connection: `FLUVIO_CLUSTER_ADDR` env var (default: localhost:9003)
- Model paths: `TOKENIZER_PATH` and `MODEL_PATH` env vars

**Build:**
- Rust 1.85.0 edition 2024 (`Cargo.toml` line 4)
- Linting: Clippy (all + pedantic warnings) and unsafe_code warning (`Cargo.toml` lines 7-12)
- Release profile with LTO and optimization (`Cargo.toml` lines 49-52)
- WASM target: wasm32-unknown-unknown (for SmartModules)

**Workspace:**
- Monorepo structure with main binary and SmartModule crates (`Cargo.toml` lines 41-47):
  - Root: filimon (main application)
  - `smartmodules/filter-relevance`
  - `smartmodules/map-preprocess`
  - `smartmodules/map-extract`

## Platform Requirements

**Development:**
- Rust 1.85.0 (exact version required due to SmartModule WASM linker issue in newer versions)
- Fluvio CLI 0.18.1+ for cluster management
- SMDK (SmartModule Development Kit) for building transformations
- Docker + Docker Compose (for containerized deployment)

**Production:**
- Fluvio cluster (local or remote) with at least 1 SC and 1 SPU
- Linux/macOS/Windows with:
  - Libc (glibc or musl depending on build)
  - OpenSSL 3.x (or rustls-tls feature handles it)
  - ONNX Runtime 1.24 (auto-downloaded via ort crate)
- Minimum 2GB RAM (1.5GB for GLiNER small model + 500MB headroom)
- Recommended: 4+ CPU cores

## Docker Build Stack

**Build Stage: Rust 1-slim-bookworm**
- Base: Official Rust image on Debian bookworm
- Build tools: gcc, g++, pkg-config, libssl-dev, cmake
- ONNX Runtime: Downloaded at cargo build time via `ORT_STRATEGY=download`

**Runtime Stage: Debian bookworm-slim**
- Minimal dependencies: ca-certificates, libssl3
- ONNX Runtime library: Copied from builder stage and registered with ldconfig
- Binary size: ~15-20MB (filimon binary)
- Image size: ~100-150MB (with ONNX Runtime)

## Binary Output

**Main executable:** `fili` (compiled at `target/release/fili`)
- Entrypoint in Docker: `/usr/local/bin/fili`
- CLI binary supporting: produce, consume, watch, file, stream modes

---

*Stack analysis: 2026-11-03*
