---
last_mapped_commit: ee408de9974ea0cee802f332ee2386ef13b0428b
last_mapped_at: 2026-11-22
---
# Testing Patterns

**Analysis Date:** 2026-11-22

## Test Framework

**Runner:**
- Built-in `cargo test`; async tests use `#[tokio::test]` (dev-dependency `tokio` with `test-util` for paused time, `#[tokio::test(start_paused = true)]`).
- Config: none beyond `Cargo.toml`; toolchain pinned in `rust-toolchain.toml`.

**Assertion Library:** std `assert!`, `assert_eq!`.

**Run Commands:**
```bash
cargo test                     # unit + tests/ integration (root crate)
cargo test -- --ignored        # openssl / network / fixture-dependent tests
cargo clippy --all-targets     # pedantic lints
cd plugins/rss && cargo test   # plugin crates are separate (not in workspace)
```
Coverage: no tooling configured. No CI workflow detected (`.github/` absent).

## Test File Organization

**Location:**
- Unit tests co-located: `#[cfg(test)] mod tests { use super::*; ... }` at the bottom of source files (about 50 files, e.g. `src/plugins/config.rs`, `src/plugins/host/scheduler.rs`, `src/commands/consume.rs`).
- Larger host suites live in sibling `*_tests.rs` modules: `src/plugins/host/wasm_tests.rs`, `https_tests.rs`, `integration_tests.rs`.
- Binary-level tests in `tests/auth_cli.rs`, `tests/plugins_cli.rs`.
- Plugin crates: unit tests in `plugins/*/src/*.rs`; replay tests in `plugins/reddit/tests/replay.rs`, `plugins/youtube/tests/replay.rs`, plus `fixtures_manifest.rs`.

**Naming:** behavior sentences in snake_case; ignored network groups use a `net_` prefix.

## Test Structure

**Suite Organization:**
```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn cfg(initial: u64, max: u64, multiplier: f64) -> BackoffConfig { ... }

    #[test]
    fn step_sequence_doubles_until_cap() { ... assert_eq!(got, want); }
}
```

**Patterns:**
- Small local builder helpers at the top of each test module.
- Table-style expected vectors compared in one `assert_eq!`.
- Temp dirs via `tempfile::TempDir` for registry/state/token files.
- CLI tests drive the real binary with `Command::new(env!("CARGO_BIN_EXE_fili"))`, stripping `FILI_REGISTRY`, `FILI_STATE_DIR`, `RUST_LOG` first (`fn fili()` in `tests/auth_cli.rs`), and use inline TOML string constants for plugin configs.
- Secret-safety: sentinel tokens (`ACCESS_SENTINEL`) asserted absent from both stdout and stderr.
- Time-dependent scheduler/backoff tests use paused tokio time.

## Mocking

**Framework:** None (no mockall). Hand-written fakes in `src/plugins/host/testing.rs` (cfg(test) only): `InstanceFactory`/`PluginInstance` fakes, `VecSink` as an `ItemSink`, `fixture_wasm(crate_dir, file)` locating prebuilt components under `plugins/<crate>/target/wasm32-unknown-unknown/release`.

**What to Mock:** the plugin instance, sink, HTTP fetcher (via replay-fetcher/stub-fetcher/fault-source plugins), clock (paused time).
**What NOT to Mock:** the CLI binary, config parsing, TOML loading; use real files in temp dirs. Fluvio cluster is not needed by tests.

## Fixtures and Factories

- HTML sample: `examples/fixtures/news-article.html`.
- Reddit/YouTube HTTP replay JSON in `plugins/reddit/tests/fixtures/` (documented in its `README.md`), with `.meta.json` for status codes (e.g. `rate_limited_429.meta.json`); `fixtures_manifest.rs` checks the manifest.
- Example plugin configs in `examples/plugins/*.toml`.

## Coverage

**Requirements:** None enforced.

## Test Types

**Unit Tests:** pure logic (backoff, cursors, PKCE, config validation, args/CLI parsing).
**Integration Tests:** `tests/*.rs` against the compiled binary; `src/plugins/host/integration_tests.rs` and `wasm_tests.rs` run real wasm components (need prebuilt fixtures, panic with build instructions otherwise).
**E2E Tests:** shell scripts `scripts/live-e2e.sh` and `scripts/verify-authoring.sh` (needs `cargo component`); require Fluvio/compose. Not run by `cargo test`.

## Common Patterns

**Ignored tests:** always with reason:
```rust
#[ignore = "needs the openssl binary: see plugins/README.md"]
```

**Async Testing:**
```rust
#[tokio::test(start_paused = true)]
async fn ... { /* advance paused time, assert on report */ }
```

**Error Testing:** assert on diagnostic code/text of `ConfigError` variants and on absence of secrets in output.

---

*Testing analysis: 2026-11-22*
