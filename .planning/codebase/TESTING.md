---
last_mapped_commit: 5122e6b9c4aa51ca49831917f1a1035fdc543870
last_mapped_at: 2026-11-03
---
# Testing Patterns

**Analysis Date:** 2026-11-03

## Test Framework

**Runner:**
- Rust built-in test harness (no external test runner)
- Run tests: `cargo test`
- Run tests with output: `cargo test -- --nocapture`
- Run tests with threads: `cargo test -- --test-threads=1` (for serial execution)

**Assertion Library:**
- Rust standard library: `assert!()`, `assert_eq!()`, `assert_ne!()`
- No external assertion library currently in use

**Run Commands:**
```bash
cargo test                    # Run all tests
cargo test --lib             # Run library tests only
cargo test -- --nocapture    # Show println! output
cargo test test_name          # Run specific test by name
```

## Test File Organization

**Location:**
- Tests are co-located with source code in `#[cfg(test)] mod tests` blocks
- Found in: `src/check.rs`, `src/args.rs`
- No separate `tests/` directory for integration tests

**Naming:**
- Test functions use `test_` prefix
- Descriptive names indicating what is tested
- Examples: `test_validate_with_all_valid`, `test_load_config_success`

**Structure:**
```
src/module.rs
├── pub struct/fn/impl definitions
├── private helpers
└── #[cfg(test)]
    mod tests {
        use super::*;
        
        #[test]
        fn test_case_name() { ... }
    }
```

## Test Structure

**Suite Organization:**
- All tests for a module grouped in single `#[cfg(test)] mod tests` block
- `use super::*;` imports everything from parent module
- Example from `src/args.rs:38-143`:

**Patterns:**

### Basic Test Pattern
```rust
#[test]
fn test_parse_default() {
    let args = Args::parse_from(vec!["prog"]);
    assert_eq!(args.ls, None);
    assert_eq!(args.config, "config.json");
}
```

### Setup Pattern
- Create temporary resources at test start
- Use `std::fs::File::create()` for temp files
- Clean up with `fs::remove_file()` at end
- Example from `src/args.rs:80-87`:
```rust
#[test]
fn test_load_config_success() {
    let config_path = "test_config_success.json";
    let mut file = File::create(config_path).unwrap();
    file.write_all(b"{\"ls\": [\"x\", \"y\"]}").unwrap();
    let res = load_config(config_path).unwrap();
    assert_eq!(res, vec!["x".to_string(), "y".to_string()]);
    fs::remove_file(config_path).unwrap();
}
```

### Teardown Pattern
- Cleanup in same test function (no separate teardown)
- Use `fs::remove_file()` to delete temp files
- Run unconditionally (not guarded by assertions)

### Assertion Pattern
- `assert_eq!(actual, expected)` for equality
- `assert_ne!()` for inequality
- `assert!()` for boolean conditions
- `.is_err()`, `.is_ok()` for `Result` checking
- `.is_none()`, `.is_some()` for `Option` checking
- Example from `src/args.rs:89-93`:
```rust
#[test]
fn test_load_config_file_not_found() {
    let res = load_config("nonexistent.json");
    assert!(res.is_err());
}
```

## Mocking

**Framework:** No external mocking framework
- Manual fixtures for known test cases
- File I/O mocking via actual temp files
- No use of `mockall`, `Mocker`, or similar libraries

**Patterns:**
- Create real files for I/O testing
- Pass different inputs to test multiple code paths
- Example from `src/check.rs:63-75`:
```rust
fn is_even_validator(s: &str) -> Result<String, ItemError> {
    match s.parse::<i32>() {
        Ok(num) if num % 2 == 0 => Ok(num.to_string()),
        Ok(_) => Err(ItemError {
            item: s.to_string(),
            message: "Odd number".to_string(),
        }),
        Err(_) => Err(ItemError {
            item: s.to_string(),
            message: "Not a number".to_string(),
        }),
    }
}

#[test]
fn test_validate_with_all_valid() {
    let items = vec!["2", "4", "6"];
    let result = items.validate_with(is_even_validator);
    assert_eq!(...);
}
```

**What to Mock:**
- File I/O: Create actual temp files for testing
- Simple functions: Pass different inputs, validate outputs
- Structures: Build test instances directly

**What NOT to Mock:**
- External service connections (not tested)
- Fluvio cluster interactions (require running infrastructure)
- Long-running async operations (use real timeouts)

## Fixtures and Factories

**Test Data:**
- Inline test helper functions for creating test objects
- Example from `src/args.rs:69-77`:
```rust
#[test]
fn test_produce_links_with_ls() {
    let mut args = Args {
        ls: Some(vec!["a".to_string(), "b".to_string()]),
        config: "config.json".to_string(),
    };
    let links = args.produce_links();
    assert_eq!(links, vec!["a", "b"]);
}
```

**Location:**
- Test helper functions defined within `#[cfg(test)] mod tests` block
- Reused across multiple tests in same module

## Coverage

**Requirements:** None enforced (no coverage tool configured)

**View Coverage:**
- No built-in coverage view
- Would need to add via `cargo tarpaulin` or `cargo llvm-cov` if desired

**Current Coverage:**
- `src/check.rs`: Full coverage with 7 tests (module logic, partition_map trait, error display)
- `src/args.rs`: Good coverage with 9 tests (CLI parsing, config loading, panic cases)
- Other modules: No tests defined (`src/models.rs`, `src/output.rs`, `src/crawler.rs`, `src/streaming.rs`, etc.)

## Test Types

**Unit Tests:**
- Scope: Individual functions and structs
- Approach: Test business logic in isolation
- Examples:
  - `test_validate_with_*` tests: Validate the custom `partition_map` trait
  - `test_parse_*` tests: Validate CLI argument parsing
  - `test_load_config_*` tests: Validate JSON config file loading
  - `test_item_error_display` test: Validate error formatting

**Integration Tests:**
- Scope: Not currently implemented
- Would require: Real Fluvio cluster, full pipeline coordination
- Location: Would go in `tests/` directory if added
- Candidates: Full `produce` → `consume` flow, SmartModule processing

**E2E Tests:**
- Framework: Not used
- Note: Project uses Fluvio for streaming; true E2E would require running cluster
- Shell-based tests exist in `fluvio/tests/` (external test suite)

## Common Patterns

**Async Testing:**
- No async tests currently (tests are blocking)
- Would use `#[tokio::test]` for async test support if needed
- Example structure (not in codebase):
```rust
#[tokio::test]
async fn test_async_operation() {
    let result = some_async_fn().await;
    assert!(result.is_ok());
}
```

**Error Testing:**
- Test `Result::Err` cases with `.is_err()`:
```rust
#[test]
fn test_load_config_file_not_found() {
    let res = load_config("nonexistent.json");
    assert!(res.is_err());
}
```

- Test panic with `#[should_panic]` attribute:
```rust
#[test]
#[should_panic]
fn test_produce_panic_file_not_found() {
    let mut args = Args {
        ls: None,
        config: "nonexistent.json".to_string(),
    };
    args.produce();  // Should panic
}
```

**Data Validation Tests:**
- Create instances with known data
- Call validation functions
- Assert expected behavior
- Example from `src/check.rs:102-115`:
```rust
#[test]
fn test_validate_with_mixed() {
    let items = vec!["2", "1", "4", "three", "6"];
    let result = items.validate_with(is_even_validator);
    assert_eq!(
        result.valid,
        vec!["2".to_string(), "4".to_string(), "6".to_string()]
    );
    assert_eq!(result.invalid.len(), 2);
    assert_eq!(result.invalid[0].item, "1");
    assert_eq!(result.invalid[0].message, "Odd number");
}
```

## Test Recommendations (Gaps)

**Critical Missing Coverage:**
1. `src/streaming.rs` - No tests for Fluvio integration
   - Would need: Running Fluvio cluster or mock client
   - Risk: Stream producer/consumer logic uncovered
   
2. `src/crawler.rs` - No tests for web crawling logic
   - Would need: Mock HTTP client or VCR-style recordings
   - Risk: Link extraction, priority scoring, URL normalization untested
   
3. `src/models.rs` - No tests for data structures
   - Would need: Serialization/deserialization round-trip tests
   - Risk: Silent data corruption in Fluvio pipeline
   
4. `src/output.rs` - No tests for formatting/output
   - Would need: File I/O and formatting verification
   - Risk: User-facing output could be malformed

**Recommended Next Steps:**
1. Add `dev-dependencies` section to `Cargo.toml`:
   ```toml
   [dev-dependencies]
   tokio = { version = "1", features = ["full"] }
   serde_json = "1"
   tempfile = "3"
   ```

2. Add async test support:
   ```rust
   #[tokio::test]
   async fn test_streaming_operations() { ... }
   ```

3. Test data serialization:
   ```rust
   #[test]
   fn test_raw_page_serialization() {
       let page = RawPage { ... };
       let json = serde_json::to_string(&page).unwrap();
       let restored: RawPage = serde_json::from_str(&json).unwrap();
       assert_eq!(page, restored);
   }
   ```

4. Mock Fluvio client for integration tests (consider: `mockito` or manual mocks)

---

*Testing analysis: 2026-11-03*
