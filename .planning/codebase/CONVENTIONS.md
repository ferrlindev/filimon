---
last_mapped_commit: ee408de9974ea0cee802f332ee2386ef13b0428b
last_mapped_at: 2026-11-22
---
# Coding Conventions

**Analysis Date:** 2026-11-22

## Naming Patterns

**Files:**
- snake_case Rust modules: `src/plugins/host/scheduler.rs`, `src/commands/consume.rs`.
- Test-only modules use a `_tests` suffix: `src/plugins/host/wasm_tests.rs`, `src/plugins/host/https_tests.rs`, `src/plugins/host/integration_tests.rs`; shared helpers in `src/plugins/host/testing.rs`.
- Binary is `fili` (`[[bin]]` in `Cargo.toml`), crate is `filimon`.

**Functions:** snake_case; test names are full sentences describing behavior (`step_with_zero_prior_failures_is_initial` in `src/plugins/host/backoff.rs`).

**Types:** PascalCase (`ConfigError`, `BackoffConfig`, `PluginSpec`); error enum variants name the problem (`RegistryUnreadable`, `PluginFileMalformed`).

## Code Style

**Formatting:** rustfmt via `rust-toolchain.toml` (channel 1.96.0, components clippy + rustfmt); edition 2024, `rust-version = "1.95"`. No `rustfmt.toml`, so defaults apply.

**Linting:** `Cargo.toml` sets `[lints.clippy] all = "warn"`, `pedantic = "warn"` and `[lints.rust] unsafe_code = "warn"`. Suppress with a justified attribute only:
```rust
#[allow(clippy::too_many_lines, reason = "one linear instance lifecycle loop")]
#[cfg_attr(not(test), allow(dead_code, reason = "used by the plugin host sink"))]
```
Always include `reason = "..."` (`src/plugins/host/scheduler.rs`, `src/streaming.rs`, `src/models.rs`).

## Import Organization

**Order:** (1) `std`, (2) external crates (`miette`, `tokio_util`, `tempfile`), (3) `crate::...` / `super::...`; blank line between groups. Imports are explicit, not glob, except `use super::*;` inside test modules.

**Path Aliases:** None; use `crate::` absolute paths.

## Error Handling

- Library-style errors use `thiserror::Error` plus `miette::Diagnostic` with a stable `code(fili::<area>::<name>)` and optional `help(...)`; see `src/plugins/error.rs`.
- Loading collects all problems into one `ConfigErrors` using `#[related]`, rather than failing on the first.
- Commands return `miette::Result` and convert with `miette::IntoDiagnostic` (`src/commands/run.rs`).
- Error text must carry names, paths, field paths and env var NAMES only; never secret values or raw `.env` lines (stated in `src/plugins/error.rs`; enforced by sentinel-token tests in `tests/auth_cli.rs`).
- Terminal-bound untrusted text goes through `sanitize_for_terminal` (`src/output.rs`).

## Logging

**Framework:** `tracing` with `tracing-subscriber` env-filter (`RUST_LOG`). Use structured fields with explicit targets:
```rust
tracing::warn!(plugin = %name, "plugin aborted at shutdown");
```
User-facing report output uses `println!` only in `src/output.rs`.

## Comments

- Every file starts with a boxed header banner: a `// ───…` rule, `// path/to/file.rs - purpose`, short explanation, closing rule. Copy this for new files.
- Doc comments (`///`) on public items and on test constants/helpers.
- Comments explain why (phase notes, requirement IDs like WIT-03).

## Function Design

**Size:** Linear, focused functions; long ones carry a `too_many_lines` allow with reason.
**Parameters:** Pass config structs (`BackoffConfig`, `HostConfig`) rather than long arg lists.
**Return Values:** `Result` with typed errors; domain enums for outcomes (`CallOutcome`, `PollError`).

## Module Design

**Exports:** `mod.rs` per directory (`src/plugins/mod.rs`, `src/plugins/host/mod.rs`, `src/commands/mod.rs`); `pub` limited to what other modules use.
**Barrel Files:** Not used beyond `mod.rs`.
**Workspace:** root crate plus `smartmodules/*`; `plugins/*` are excluded from the workspace (each has its own `Cargo.lock`, `rust-toolchain.toml` and `.cargo/config.toml` targeting `wasm32-unknown-unknown`).

---

*Convention analysis: 2026-11-22*
