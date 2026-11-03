---
last_mapped_commit: 5122e6b9c4aa51ca49831917f1a1035fdc543870
last_mapped_at: 2026-11-03
---
# Codebase Concerns

**Analysis Date:** 2026-11-03

## Tech Debt

**Dead Code Blocks:**
- Issue: `src/main.rs` contains ~215 lines of commented-out code (lines 103-175, 177-229, 232-317) for previous mode implementations (`run_produce`, `run_consume`, `run_watch`). These functions were refactored into `src/commands/` but the old implementations remain commented out instead of deleted.
- Files: `src/main.rs`
- Impact: Increases maintenance burden, confuses developers about which implementation is current, wastes bytes in the binary
- Fix approach: Delete all commented-out code blocks. The current implementations in `src/commands/produce.rs`, `src/commands/consume.rs`, and `src/commands/watch.rs` are the authoritative versions.

**Deprecated Fluvio API Usage:**
- Issue: `src/streaming.rs` uses deprecated Fluvio consumer API with `#[allow(deprecated)]` pragmas (lines 71, 156). The comments indicate this is known but not yet upgraded.
- Files: `src/streaming.rs` (lines 71, 156)
- Impact: Future Fluvio versions may remove these deprecated methods, forcing a breaking upgrade
- Fix approach: Update to the non-deprecated consumer API when Fluvio releases a stable replacement or in the next major version bump

**Invalid Rust Edition in Cargo.toml:**
- Issue: `Cargo.toml` specifies `edition = "2024"` (line 4), which is not a valid Rust edition. Valid editions are 2015, 2018, 2021. This will cause compilation failure.
- Files: `Cargo.toml`
- Impact: Project cannot compile with `cargo build`; this is a blocking issue
- Fix approach: Change `edition = "2024"` to `edition = "2021"` (the current stable edition)

**Incomplete Refactoring:**
- Issue: The transition from a unified CLI to a modular commands structure appears incomplete. Functions like `criteria()` and type conflicts suggest the refactoring wasn't fully validated before commit.
- Files: `src/main.rs`, `src/commands/`, `src/cli.rs`
- Impact: Inconsistent error handling patterns, unused code paths, potential logic errors in mode selection
- Fix approach: Complete the refactoring by removing the old mode implementations and ensuring all CLI routing points to `src/commands/*` modules

## Known Bugs

**Watch Mode Not Actually Producing to Fluvio:**
- Issue: `src/commands/watch.rs` at lines 112-114 has a TODO noting that the sync closure in Watchexec cannot easily await async operations. The detected HTML files are logged but never actually sent to Fluvio, only printed as "Would produce..."
- Files: `src/commands/watch.rs`
- Trigger: Running `fili watch` on a directory with HTML files
- Workaround: Use `fili file <paths>` to produce files instead
- Fix approach: Use a channel to send file paths from the watch thread to an async task that handles the actual Fluvio production

**Typo in Error Message:**
- Issue: `src/cli.rs` at line 46 has "Config fead failed" instead of "Config read failed"
- Files: `src/cli.rs`
- Impact: Minor; only affects user-facing error message clarity
- Fix approach: Change "fead" to "read" in the eprintln message

**Silent Error Handling in Streaming:**
- Issue: `src/streaming.rs` in the `process` method (lines 164-178) and `consume_batch` method (lines 192-206) silently log deserialization errors and stream errors without failing or notifying the caller that data was lost. Timeouts in consume_batch silently break the loop.
- Files: `src/streaming.rs`
- Trigger: Malformed JSON in Fluvio topic or network timeout during consumption
- Impact: Consumer may report false completion while silently dropping records; data loss goes unnoticed
- Fix approach: Either propagate deserialization/stream errors to the caller, or track and report a summary of dropped records

## Security Considerations

**Dangerous TLS Certificate Handling:**
- Risk: `src/crawler.rs` at line 64 enables `danger_accept_invalid_certs(true)` for the HTTP client. While documented as necessary for a sandboxed environment, this bypasses TLS certificate validation.
- Files: `src/crawler.rs`
- Current mitigation: Clear comment noting this is sandbox-only; feature should be disabled for production
- Recommendations: 
  - Make this configurable via environment variable or CLI flag
  - Add a compile-time feature gate to exclude this in release builds
  - Document requirement to remove this for any real deployment

**Test Files Committed to Repository:**
- Risk: Invalid JSON test file `test_config_panic.json` contains `not json` and is committed to the repository. While harmless, it's test debris that shouldn't be in version control.
- Files: `test_config_panic.json`
- Recommendations: Move to a `tests/fixtures/` directory or remove if no longer needed; add `test_config_*.json` to `.gitignore`

## Performance Bottlenecks

**HTTP Client Not Cached:**
- Problem: `src/crawler.rs` in `build_client()` creates a new `reqwest::blocking::Client` on every `fetch_page()` call (line 71). HTTP clients should be reused across requests for connection pooling and efficiency.
- Files: `src/crawler.rs`
- Cause: No singleton or connection pool pattern for HTTP clients
- Improvement path: Create a static or lazy-initialized HTTP client and reuse it across all fetch operations. This will reduce connection overhead and improve throughput.

**Timeout in Consume Batch:**
- Problem: `src/streaming.rs` at line 192 uses a 5-second timeout per record in `consume_batch()`. With many slow-arriving records or network latency, this can cause premature exit even when more data is available.
- Files: `src/streaming.rs`
- Improvement path: Make timeout configurable and consider batch-level timeouts instead of per-record timeouts

## Fragile Areas

**Panic-Prone Config Loading in args.rs:**
- Files: `src/args.rs`
- Why fragile: The `produce()` method (line 17) and direct access to `.ls` (line 19) use `.unwrap()` that will panic if config file loading fails. The test intentionally triggers this panic (`test_produce_panic_file_not_found`, `test_produce_panic_invalid_json`), confirming the design is to panic rather than handle errors.
- Safe modification: Use Result<T, E> return types throughout the config path; avoid unwrap in production code paths. Current cli.rs correctly uses `unwrap_or_else()` with logging, but args.rs takes the opposite approach.
- Test coverage: Tests exist for panic scenarios but this is fragile design

**Error Handling in Smartmodules:**
- Files: `smartmodules/map-extract/src/lib.rs` (lines 208-209), `smartmodules/filter-relevance/src/lib.rs` (line 66)
- Why fragile: Uses `unwrap_or(0)` and `.unwrap()` in text extraction, which silently defaults to boundary values or panics on test data. If the HTML structure is malformed, extraction may fail silently.
- Safe modification: Add explicit error logging and consider partial extraction fallback strategies

## Scaling Limits

**Async Closure Limitations in Watch Mode:**
- Current capacity: Watch mode detects HTML changes but cannot scale beyond local file system operations due to inability to await async code in Watchexec's sync closure
- Limit: Only local file watching is practically supported; remote file sources would require a separate service
- Scaling path: Decouple file watching from production via a channel-based event queue that feeds into an async task

**SmartModule WASM Size Constraints:**
- Problem: NER model (~200-500MB) cannot run in WASM SmartModules; must run on consumer side. This limits where NER can be applied in the pipeline.
- Current capacity: NER works only with `--limit` mode (batch consumption), not streaming mode due to async closure constraints
- Scaling path: Either implement NER as a microservice or use a lighter NER model for WASM compatibility

## Dependencies at Risk

**Fluvio at 0.50 with API Deprecations:**
- Risk: Using Fluvio 0.50 which has documented deprecated APIs in use (`partition_consumer`). Future releases may remove these.
- Impact: Next Fluvio upgrade could break compilation
- Migration plan: Track Fluvio release notes; migrate to `stream_consumer` or replacement API when stable. Review fluvio-smartmodule 0.50.1 linker issue noted in README (Rust 1.96+ incompatibility).

**Reqwest 0.12 with rustls-tls:**
- Risk: Custom feature selection (rustls-tls instead of native-tls) increases supply chain risk and may have security patches lag
- Impact: TLS vulnerabilities may not be patched as quickly in rustls
- Migration plan: Consider using native-tls for production or ensure security monitoring of rustls releases

## Missing Critical Features

**Config File Not Loaded by Default:**
- Problem: Watch mode expects a `config.json` file in the current directory, but this file is not provided or documented. Users cannot easily discover how to configure watched directories.
- Blocks: Users cannot use watch mode without knowing about the `--config` flag or the CLI `--ls` override
- Fix: Provide a template `config.json.example` in the repo and document in README; make the error message more helpful

**No Integration Tests:**
- Problem: No tests verify that the CLI, streaming layer, and SmartModules work together end-to-end. All tests are unit-level for individual modules.
- Blocks: Refactoring risks breaking the pipeline without detection
- Fix: Add integration tests that spin up a test Fluvio cluster and verify data flows through all stages

## Test Coverage Gaps

**Untested Streaming Pipeline:**
- What's not tested: The actual data flow from producer → SmartModules → consumer, the JSON serialization/deserialization at each stage, end-to-end crawling
- Files: `src/streaming.rs`, `src/commands/produce.rs`, `src/commands/consume.rs`, all `smartmodules/*/src/lib.rs`
- Risk: Refactoring any part of the pipeline could silently break data compatibility without detection
- Priority: High — this is the core of the application

**No Error Path Testing for Fluvio Operations:**
- What's not tested: What happens when Fluvio cluster is unavailable, when serialization fails, when the topics don't exist
- Files: `src/streaming.rs`, `src/commands/produce.rs`, `src/commands/consume.rs`
- Risk: Errors may crash the application instead of providing helpful diagnostics
- Priority: High — affects reliability in production

**Incomplete SmartModule Tests:**
- What's not tested: Actual extraction logic (title, author, date, keywords), category inference accuracy, relevance scoring computation
- Files: `smartmodules/map-extract/src/lib.rs` (445 lines with no tests), `smartmodules/map-preprocess/src/lib.rs` (216 lines with no tests)
- Risk: Algorithm changes could silently degrade quality; edge cases in HTML parsing are not caught
- Priority: Medium — logic is somewhat stable but edge cases in HTML parsing are probable

---

*Concerns audit: 2026-11-03*
