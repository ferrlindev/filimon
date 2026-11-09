---
phase: 03-component-host-runtime
reviewed: 2026-11-09T00:00:00Z
depth: standard
files_reviewed: 9
files_reviewed_list:
  - plugins/README.md
  - plugins/fault-source/src/lib.rs
  - src/plugins/host/cursor.rs
  - src/plugins/host/engine.rs
  - src/plugins/host/instance.rs
  - src/plugins/host/map.rs
  - src/plugins/host/scheduler.rs
  - src/plugins/host/sink.rs
  - src/plugins/host/wasm_tests.rs
findings:
  critical: 0
  warning: 7
  info: 6
  total: 13
  necessity: 0
status: issues_found
---

# Phase 3: Code Review Report

**Reviewed:** 2026-11-09
**Depth:** standard
**Files Reviewed:** 9
**Status:** issues_found

## Summary

This is a re-review of the current source after the fixes for WR-01, WR-03 and WR-06. I read each file and checked every earlier finding against the code as it stands now. I also read `src/plugins/host/mod.rs`, `src/main.rs`, `src/output.rs` and wasmtime 48.0.8 (the version in `Cargo.lock`) where a claim depended on them. I did not run the test suite.

Fixes confirmed:
- **WR-01** (memory, items, cursor caps): a 64 MiB per-memory limiter with `trap_on_grow_failure`, a 1000-item cap and a 4096-byte cursor cap are present and unit-tested. Two gaps remain, reported as WR-08 and WR-09.
- **WR-03** (record key): a 512-byte key bound runs before the key is built, and the key length now counts toward `MAX_RECORD_BYTES`.
- **WR-06** (sink timeout): the bound now lives in `FluvioSink::send_batch`, starts after the lock is held, and `clear_errors` runs on both failure and expiry. The fix has a side effect, reported as WR-10.

Still open from the prior report, re-reported under the same id: WR-02, WR-04, WR-05, WR-07, IN-01, IN-02, IN-03, IN-04. The sources behind these (`engine.rs` ticker, `map.rs` URL and platform, `mod.rs` state dir, `output.rs`, `main.rs`, `cursor.rs`) are unchanged in the relevant lines.

New findings are WR-08 to WR-10 and IN-05 to IN-06. No critical issues.

## Warnings

### WR-02: Epoch ticker thread spawn failure is silently swallowed, which removes the anti-wedge guarantee

**File:** `src/plugins/host/engine.rs:196-205`
**Issue:** `std::thread::Builder::spawn(...).ok()` discards the spawn error. If the thread cannot start, `handle` is `None` and the epoch never advances, so `epoch_deadline_async_yield_and_update(1)` never fires. A spinning guest then never yields to tokio and holds a worker forever, and shutdown cannot drop its future. The module header promises this cannot happen, yet `HostEngine::new()` returns `Ok`.
**Fix:** Make `EpochTicker::start` return a `Result` and propagate it from `HostEngine::new`, so `Host::load` reports `HostError::Engine`:
```rust
let handle = std::thread::Builder::new()
    .name("fili-epoch-ticker".to_string())
    .spawn(move || { /* ... */ })
    .map_err(|e| wasmtime::format_err!("cannot start epoch ticker: {e}"))?;
Ok(Self { stop, handle: Some(handle) })
```

### WR-04: Item URL scheme is not restricted

**File:** `src/plugins/host/map.rs:77-79`
**Issue:** The module header says the host is the only gate, but the gate is only `url::Url::parse(&item.url).is_err()`. `javascript:`, `data:`, `file:` and `ftp:` URLs all pass and are produced to the topic as the item's canonical URL. A downstream consumer or UI that renders or fetches the URL inherits an attacker-controlled scheme from a compromised plugin. `valid_ids_are_not_trimmed_or_altered` (line 499) also pins that the stored URL keeps its surrounding spaces even though the parsed form does not, so a stricter consumer can disagree with the host.
**Fix:**
```rust
match url::Url::parse(item.url.trim()) {
    Ok(u) if matches!(u.scheme(), "http" | "https") && u.has_host() => {}
    _ => return Err(DropReason::InvalidUrl),
}
```
Add tests with `javascript:`, `file:` and `data:` URLs. Decide whether to store the trimmed URL.

### WR-05: Guest-chosen `platform` can impersonate another plugin or platform, and the key encoding is ambiguous

**File:** `src/plugins/host/map.rs:81-91`
**Issue:** A non-blank guest `platform` is used verbatim, so a plugin registered as `acme` can emit `platform = "reddit"`. Its records carry `platform=reddit` and a `reddit:{id}` key, indistinguishable from the real reddit plugin and able to collide with its keys. The host never records which plugin produced an item. The key `"{platform}:{id}"` is also ambiguous: `("a", "b:c")` and `("a:b", "c")` both produce `a:b:c`.
**Fix:** Either force `platform` to the registry alias or validate it against `[a-z0-9_-]`. Make the key unambiguous: include the plugin (`{plugin}:{platform}:{id}`), or percent-encode `platform` and `id` before joining.

### WR-07: An empty `--state-dir` / `FILI_STATE_DIR` silently resolves to the current directory

**File:** `src/plugins/host/mod.rs:52-57` (consumed by `src/plugins/host/cursor.rs:59-70`)
**Issue:** `resolve_state_dir` uses the flag as given. For an empty path, `create_dir_all("")` succeeds and `"".join(".lock")` is `.lock`, so the lock file and every `<alias>.json` land in the process working directory. The tests in `mod.rs` cover a relative path but not the empty path.
**Fix:** Reject an empty path in `resolve_state_dir` or `CursorStore::open` as `HostError::StateDir` ("state directory must not be empty"), or require a non-empty value in clap.

### WR-08: The per-poll item cap does not bound host memory: list lifting copies every item before `map_items` runs

**File:** `src/plugins/host/map.rs:120-132`, `src/plugins/host/instance.rs:125`
**Issue:** `MAX_ITEMS_PER_POLL` is applied in `map_items`, after wasmtime has already lifted the whole `list<item>` into `Vec<WitItem>`. Lifting copies each string out of guest memory (`WasmStr`, then an owned `String`), and the canonical ABI does not require different items to point at different bytes. A guest with the 64 MiB memory cap can build a list of several hundred thousand 96-byte item records whose strings all alias one large string. The host then allocates the product of the two, which can be hundreds of GiB. The 64 MiB guest cap does not bound host-side memory, so the OOM-of-the-whole-process scenario that WR-01 targeted is still reachable. The 1000-item and 1,000,000-byte-record caps run too late to help.
**Fix:** Bound the amount lifted. Options:
- Change the WIT `poll-response.items` to a bounded shape, or declare `items` as `WasmList<Item>` in a hand-written binding, so the host reads at most `MAX_ITEMS_PER_POLL` elements.
- Alternatively, shrink the per-instance memory limit far enough that aliasing cannot amplify to a harmful size, and say plainly in `plugins/README.md` that host memory is not bounded by the guest cap.

### WR-09: The "64 MiB per instance" ceiling is per memory, so one instance can hold up to 512 MiB

**File:** `src/plugins/host/engine.rs:64-92`, `plugins/README.md:222-226`, `plugins/README.md:282`
**Issue:** `StoreLimitsBuilder::memory_size` limits each linear memory, not the sum. `MAX_MEMORIES = 8` lets a hand-built component use 8 x 64 MiB = 512 MiB per instance. The constant's doc comment ("one instance's linear memory"), the README ("a fixed 64 MiB memory ceiling" per instance) and the new tests all assume a total. `memory_growing(0, GUEST_MEMORY_LIMIT_BYTES, ...)` tests only a single memory, so the gap is not caught. A rebuild loop also gets a fresh 512 MiB each time.
**Fix:** Either lower `MAX_MEMORIES` to the real need (cargo-component output typically uses one or two) and compute the cap as `GUEST_MEMORY_LIMIT_BYTES / MAX_MEMORIES`, or enforce a total in a small custom `ResourceLimiter` that sums grown memory across calls. Then correct the doc comment and the README so they state what is enforced.

### WR-10: Sink sends are never raced against shutdown and lock waiting is unbounded, so shutdown can take N x 60 s

**File:** `src/plugins/host/scheduler.rs:211-230` and `:390-392`, `src/plugins/host/sink.rs:87-111`
**Issue:** The WR-06 fix moved the timeout inside `send_batch` and starts it after the producer lock is held. The scheduler no longer wraps `send_records` in any timeout, and the call is deliberately not raced against `shutdown` ("a stop must not split send from persist"). `self.producer.lock().await` has no bound. With a stuck broker, plugin A holds the lock for 60 s and then fails, plugin B acquires it and holds it for its own 60 s, and so on. Every plugin that was already inside `send_batch` when `shutdown` fired must run its full sequence, so `Host::run` (which joins all tasks with no grace period) can take about (number of plugins x 60 s) to return. Before the fix, the outer timeout covered lock waiting and capped each plugin at 60 s. The README's "cancelling the host returns within seconds" claim is true only for a spinning guest, not for a slow sink.
**Fix:** Pick the intent and enforce it. For example, add an overall deadline for the lock acquisition (`tokio::time::timeout(LOCK_WAIT, self.producer.lock())`, returning `SinkError("producer busy")`), and document the worst-case shutdown time in the README. Alternatively, race the lock acquisition (not the send) against `shutdown` and return early so no new send starts after cancellation.

## Info

### IN-01: `clean_guest_text` leaves Unicode line separators and bidi controls in place

**File:** `src/plugins/host/engine.rs:104-114` (relies on `src/output.rs:49-57`)
**Issue:** Only `char::is_control()` characters (Cc) are removed. U+2028, U+2029, bidi overrides and isolates (U+202A-U+202E, U+2066-U+2069) and zero-width characters pass through, so a guest can forge a log line in a viewer that treats U+2028 as a line break or reorder text visually.
**Fix:** In `clean_guest_text`, map U+2028 and U+2029 to a space and drop the bidi and zero-width ranges.

### IN-02: Invalid `RUST_LOG` is discarded silently and per-item drop warnings are unbounded

**File:** `src/main.rs:63-69`, `src/plugins/host/map.rs:135-147`
**Issue:** `log_filter` falls back to the default for an unparsable `RUST_LOG` without telling the operator. `map_items` still emits one `warn!` per dropped item, up to `MAX_ITEMS_PER_POLL` lines per poll, repeated every poll.
**Fix:** Print a one-line stderr note for a non-blank invalid `RUST_LOG`. Aggregate per-reason drop counts into one warning per poll (counts only).

### IN-03: The state-file reader treats a missing `cursor` key as a first poll

**File:** `src/plugins/host/cursor.rs:32-37`, `:135-141`
**Issue:** `cursor: Option<String>` makes serde deserialize `{"version":1,"plugin":"a"}` as `cursor: None`, which resets the plugin to a first poll. The module contract says a bad file is never reset automatically.
**Fix:** Require the key: deserialize to `Option<Option<String>>` (or use `deserialize_with`) and treat a missing key as `corrupt("missing cursor")`.

### IN-04: The core isolation tests are all `#[ignore]` and depend on a manual fixture build

**File:** `src/plugins/host/wasm_tests.rs:33-498`
**Issue:** Every real-wasm test, including the new `wasm_guest_memory_growth_past_the_cap_is_refused_and_isolated`, is ignored, and the repository has no `.github/workflows` directory to run them. A plain `cargo test` verifies the limiter only against `StoreLimits` directly (`engine.rs` tests) and never against a real guest `memory.grow`. `wasm_spinning_guest_...` also depends on 2.5 s of wall time.
**Fix:** Add a CI job that builds the three fixtures and runs `cargo test -- --ignored wasm_`, or commit the small fixture `.wasm` files. Give the spin test slack.

### IN-05: Items over the per-poll cap are lost permanently while an oversize cursor disables the plugin

**File:** `src/plugins/host/map.rs:120-132`, `src/plugins/host/scheduler.rs:367-384`
**Issue:** An over-cap cursor is treated as a plugin fault and nothing is persisted. An over-cap item list, by contrast, is truncated and the cursor still advances, so a plugin that returns 1001 items loses the last item for good with only a `warn!`. The two caps protect the same boundary but fail in opposite ways. The README documents the behavior ("a plugin should page its own results"), so this is a design choice, not a defect in the code as written.
**Fix:** Consider the same fault policy as the cursor cap (fail the poll without advancing the cursor), or at least surface the loss in `PluginReport` as its own counter instead of folding it into `dropped`.

### IN-06: Misleading variable name in the `grow` fixture mode

**File:** `plugins/fault-source/src/lib.rs:205-208`
**Issue:** `sum` is the number of touched pages (`.count()`), not a sum, and the message reads "({sum} pages)". The `for i in (0..block.len()).step_by(4096)` loop is also an index loop that clippy flags in other crates.
**Fix:** Rename `sum` to `pages`, and iterate with `block.iter_mut().step_by(4096)` setting each element to 1.

---

_Reviewed: 2026-11-09_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
