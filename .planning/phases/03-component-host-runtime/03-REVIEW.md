---
phase: 03-component-host-runtime
reviewed: 2026-11-09T00:00:00Z
depth: standard
files_reviewed: 20
files_reviewed_list:
  - src/cli.rs
  - src/main.rs
  - src/streaming.rs
  - src/plugins/mod.rs
  - src/plugins/host/mod.rs
  - src/plugins/host/backoff.rs
  - src/plugins/host/cursor.rs
  - src/plugins/host/engine.rs
  - src/plugins/host/error.rs
  - src/plugins/host/instance.rs
  - src/plugins/host/loader.rs
  - src/plugins/host/map.rs
  - src/plugins/host/scheduler.rs
  - src/plugins/host/sink.rs
  - src/plugins/host/testing.rs
  - src/plugins/host/wasm_tests.rs
  - plugins/fault-source/src/lib.rs
  - Cargo.toml
  - rust-toolchain.toml
  - plugins/fault-source/Cargo.toml
findings:
  critical: 0
  warning: 7
  info: 4
  total: 11
  necessity: 0
status: issues_found
---

# Phase 3: Code Review Report

**Reviewed:** 2026-11-09
**Depth:** standard
**Files Reviewed:** 20
**Status:** issues_found

## Summary

I reviewed the component host runtime: loader preflight, engine and linker, scheduler state machine, cursor store, mapper, sink, CLI and logging wiring, and the fault-source fixture. I also ran `cargo test --offline plugins::host`. The result was 98 passed and 18 ignored; every ignored test needs prebuilt wasm fixtures or a Fluvio cluster.

The core threat-model items hold in code:
- Only registry-enabled components are opened.
- Foreign imports are rejected before instantiation.
- Every guest or wasmtime string that reaches `tracing` or a report goes through `clean_guest_text`.
- Traps, guest errors and task panics are isolated per plugin.
- The cursor is written atomically, only after the sink returned, with an alias check and a directory lock.

No critical issues were found. The remaining defects are about unbounded guest-controlled resources and a few robustness gaps:
- Nothing limits guest memory, items per poll, cursor size or key size.
- A silently failing epoch ticker disables the anti-wedge guarantee.
- A single oversized record key can wedge a plugin in an infinite duplicate-producing retry loop.
- Item URL scheme and platform are unvalidated.
- The sink's timeout path is not cancellation-safe.
- An empty `--state-dir` resolves to the current directory.

Known accepted gaps were not re-reported: guest infinite loop (HOST-06), no cross-poll dedupe (HOST-07), single-partition key assumption, interim `not-configured` fetch and none access token, and the pre-existing clippy warnings.

## Warnings

### WR-01: No resource limits on guest-controlled memory, item count, cursor size or log volume

**File:** `src/plugins/host/instance.rs:95-105`, `src/plugins/host/engine.rs:194-196`, `src/plugins/host/scheduler.rs:367-390`
**Issue:**
- The `Store` has no `limiter`, and `Config` sets no memory or table limits. A guest that calls `memory.grow` repeatedly can take up to the wasm32 maximum (4 GiB) per instance, and a rebuilt instance does it again. That is an OOM kill of the whole `fili` process, which defeats "a misbehaving plugin must not take down the host or other plugins". It is separate from the accepted "guest loops forever" gap, because a bounded loop can still exhaust memory.
- The host also accepts, without cap:
  - an unbounded `items` list per poll (`map_items` allocates all of it);
  - an unbounded `next_cursor`, which is base64-encoded, fsync'd and re-read every poll;
  - unlimited `log` calls, each up to 4096 characters, at `info` level under the default `fili::plugins=info` filter.
- Both the cursor and the log lines are caller-controlled disk and log volume.

**Fix:** Install a `ResourceLimiter` on every store, for example `StoreLimitsBuilder::new().memory_size(64 << 20).instances(1).tables(1).build()`. Pass it with `store.limiter(|state| &mut state.limits)`, storing the `StoreLimits` in `HostState`. Add caps for items per poll (for example 1000, dropping the rest with a counted warning) and for cursor bytes (for example 64 KiB). A larger cursor should become a poll failure and must not be persisted. Optionally rate-limit guest log lines per instance.

### WR-02: Epoch ticker thread spawn failure is silently swallowed, which removes the anti-wedge guarantee

**File:** `src/plugins/host/engine.rs:155-164`
**Issue:** `std::thread::Builder::spawn(...).ok()` discards the spawn error. If the thread cannot start (thread limit, resource exhaustion), `handle` is `None` and the epoch never advances. `epoch_deadline_async_yield_and_update(1)` then never fires, so a spinning guest never yields to tokio and holds a worker thread forever. With few workers, one spinner stalls every plugin and the shutdown path cannot run. The engine module header promises exactly this will not happen, yet `HostEngine::new()` returns `Ok`.

**Fix:** Make `EpochTicker::start` return `wasmtime::Result<Self>` or `io::Result<Self>`, and propagate it from `HostEngine::new` so that `Host::load` returns `HostError::Engine`:
```rust
let handle = std::thread::Builder::new()
    .name("fili-epoch-ticker".to_string())
    .spawn(move || { /* ... */ })
    .map_err(|e| wasmtime::format_err!("cannot start epoch ticker: {e}"))?;
Ok(Self { stop, handle: Some(handle) })
```

### WR-03: Record key is guest-controlled, unbounded and not counted in the size cap; one bad record wedges the plugin in a duplicate-producing retry loop

**File:** `src/plugins/host/map.rs:70-89`, `src/plugins/host/scheduler.rs:372-394`
**Issue:**
- The size cap `MAX_RECORD_BYTES` checks only the serialized value. The Fluvio record also carries the key `"{platform}:{id}"`, and both parts are guest-controlled. `id` and `platform` are already inside the 1,000,000-byte value, so the key can be nearly as large as the value. A record of about 1.0 MB with a key of about 1.0 MB exceeds the ~1 MiB request limit that the headroom was meant to protect.
- `FluvioSink::send_batch` then fails with a permanent, record-specific error. The scheduler treats every sink error as transient (`SinkFailed`: back off, keep the instance, keep the cursor).
- Each retry re-sends the whole batch, including every valid record that precedes the bad one. Those valid records are duplicated on every retry, and the cursor never advances, so the plugin is stuck for good. The wedge itself is bounded by backoff (at most one retry per `max_secs`), but the duplicates grow without limit.

**Fix:**
- Bound the key and count it in the cap:
  - drop an item whose `platform.len() + 1 + id.len()` exceeds a small limit (for example 512 bytes), with a new `DropReason::KeyTooLong`;
  - include `key.len()` in the `MAX_RECORD_BYTES` comparison.
- Optionally distinguish record-specific permanent sink errors from broker unavailability, so a poison record is dropped instead of retried forever.

### WR-04: Item URL scheme is not restricted

**File:** `src/plugins/host/map.rs:58-63`
**Issue:** The mapper comment says "the consumer does no id or url validation, so the host is the only gate", but the gate is only `url::Url::parse(...).is_ok()`. `javascript:alert(1)`, `data:text/html,...`, `file:///etc/passwd` and `ftp:` all pass and are produced to the topic as the item's canonical URL. Any downstream consumer or UI that renders the URL as a link, or fetches it, inherits an attacker-controlled scheme from a (possibly compromised) plugin. The terminal printer sanitizes only control characters.

**Fix:** Accept only `http` and `https`, and optionally require a host:
```rust
match url::Url::parse(&item.url) {
    Ok(u) if matches!(u.scheme(), "http" | "https") && u.has_host() => {}
    _ => return Err(DropReason::InvalidUrl),
}
```
Add a test with `javascript:`, `file:` and `data:` URLs.

### WR-05: Guest-chosen `platform` can impersonate another plugin or platform, and the key encoding is ambiguous

**File:** `src/plugins/host/map.rs:65-70`
**Issue:** `platform` is taken verbatim from the guest whenever it is non-blank. A plugin registered as `acme` can emit `platform = "reddit"`. Its records then carry `platform=reddit` in the item and a `reddit:{id}` key. They are indistinguishable from the real reddit plugin's output and can collide with its keys. The host never records which plugin produced an item. The key `"{platform}:{id}"` is also ambiguous: `platform="a", id="b:c"` and `platform="a:b", id="c"` map to the same key.

**Fix:**
- Decide the policy explicitly. Either force `platform` to the registry alias, or validate it against `[a-z0-9_-]` (as aliases are) and reject or normalize anything else.
- Add the producing plugin to the key or to a header, for example `{plugin}:{platform}:{id}`, or percent-encode `id` and `platform` before joining.

### WR-06: `send_batch` is not cancellation-safe when the sink timeout drops it mid-flight

**File:** `src/plugins/host/sink.rs:76-93`, `src/plugins/host/scheduler.rs:222`
**Issue:** `send_records` wraps `send_batch` in `tokio::time::timeout`. On timeout the future is dropped between `send` calls or during `flush`, so:
- the `clear_errors()` call that the doc comment says "every failure" performs never runs on this path, and Fluvio 0.50.1 keeps returning the stored error until it is cleared;
- records already enqueued by earlier `producer.send` calls stay in the producer's accumulator and can be delivered later, while the scheduler re-polls and re-sends the same batch (extra duplicates beyond HOST-07's cross-poll case).

The stale error self-heals only because the next batch hits it, fails once more and then clears it. That costs an extra backoff cycle for every plugin sharing the producer. Time spent waiting on the producer `Mutex` also counts against the 60 s, so a healthy plugin can be reported as `SinkFailed` because another plugin's flush was slow.

**Fix:** Do the bounding inside `send_batch`: apply the timeout around the send and flush while holding the lock, and on expiry call `producer.clear_errors().await` before returning `SinkError("send timed out")`. Keep the outer timeout only as a backstop. Starting the lock-wait timer after the lock is acquired would also stop one slow plugin from failing the others.

### WR-07: An empty `--state-dir` / `FILI_STATE_DIR` silently resolves to the current directory

**File:** `src/plugins/host/mod.rs:52-57` (consumed by `src/plugins/host/cursor.rs:55-66`)
**Issue:** `resolve_state_dir` uses the flag "exactly as given". For `""`, `std::fs::create_dir_all("")` returns `Ok(())` (verified with rustc), and `"".join(".lock")` is `.lock`. The lock file and every `<alias>.json` cursor file would be written into the process's working directory, mixed with unrelated files. The existing tests cover a relative path but not the empty path.

**Fix:** Reject an empty path in `resolve_state_dir` or in `CursorStore::open`. For example, treat `Some(p) if p.as_os_str().is_empty()` as a `HostError::StateDir` ("state directory must not be empty"). Alternatively, filter it out in clap with a `value_parser` that requires a non-empty value.

## Info

### IN-01: `clean_guest_text` leaves Unicode line separators and bidi controls in place

**File:** `src/plugins/host/engine.rs:71-81` (relies on `src/output.rs:49-57`)
**Issue:** The sanitizer drops only `char::is_control()` characters (Cc). U+2028 and U+2029 (category Zl and Zp), bidi overrides and isolates (U+202A-U+202E, U+2066-U+2069) and zero-width characters pass through. Some log viewers and terminals treat U+2028 as a line break, so a guest can forge a log line. A bidi override can visually reorder an error message. Escape sequences and CR/LF, which are the main concern, are handled.
**Fix:** Also map U+2028 and U+2029 to a space, and drop `\u{200B}-\u{200F}`, `\u{202A}-\u{202E}` and `\u{2066}-\u{2069}`. Do it in `clean_guest_text` if changing the shared `sanitize_for_terminal` is out of scope.

### IN-02: Invalid `RUST_LOG` is discarded silently and per-item drop warnings are unbounded

**File:** `src/main.rs:63-69`, `src/plugins/host/map.rs:100-113`
**Issue:**
- `log_filter` falls back to the default for an unparsable `RUST_LOG` without telling the operator, so a typo (`fili::plugin=debug`) looks like debug logging silently doing nothing.
- `map_items` emits one `warn!` per dropped item. A plugin returning many bad items makes one log line per item on every poll.
**Fix:**
- Print a one-line stderr note when `RUST_LOG` is non-blank but invalid.
- Aggregate per-reason drop counts into a single warning per poll (counts only, no content).

### IN-03: The state-file reader treats a missing `cursor` key as a first poll

**File:** `src/plugins/host/cursor.rs:28-33, 131-137`
**Issue:** `cursor: Option<String>` deserializes a state file with no `cursor` key (for example `{"version":1,"plugin":"a"}` left by manual editing or a buggy tool) as `None`. That silently resets the plugin to a first poll, which the module contract says never happens automatically ("never reset automatically"). A truncated file is still correctly rejected as corrupt.
**Fix:** Require the key to be present. Use a `#[serde(deserialize_with = ...)]` that rejects a missing field, or deserialize into `Option<Option<String>>` and treat the outer `None` as `corrupt("missing cursor")`.

### IN-04: The core isolation tests are all `#[ignore]` and depend on a manual fixture build

**File:** `src/plugins/host/wasm_tests.rs:33-447`, `src/plugins/host/loader.rs:440-552`
**Issue:** Every real-wasm test is ignored, and no CI workflow is present in the repository to run them with the prebuilt fixtures (cargo-component 0.21.1, wasm-tools 1.263.0, per `plugins/README.md`). Real-wasm behaviour is therefore unverified by a plain `cargo test`:
- trap isolation and rebuild;
- the spinning-guest anti-wedge;
- WrongWorld classification;
- the end-to-end cursor round trip.

The default suite covers the same rules only against fakes. The one real-clock test (`wasm_spinning_guest...`) also depends on 2.5 s of wall time, so it can flake on a loaded runner.
**Fix:** Add a CI job that builds the three fixtures and runs `cargo test -- --ignored plugins::host`. Alternatively, commit the small fixture `.wasm` files. Give the spin test slack, or assert on poll counts after an explicit quiescence point.

---

_Reviewed: 2026-11-09_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
