---
phase: 03-component-host-runtime
plan: 08
subsystem: plugins-host
tags: [rust, fluvio, validation, sink, untrusted-input]
requires:
  - phase: 03-component-host-runtime
    provides: "plan 03-04 tracer slice: Mapped, map_items, ItemSink, SinkRecord, VecSink"
provides:
  - "Validating map_items: per-item drop with content-free warning, size cap, key construction"
  - "FluvioSink (connect, connect_social, ItemSink impl) with send-all-then-flush and sticky-error recovery"
  - "TypedProducer::clear_errors"
  - "cfg(test) live_support: scratch_topic, delete_topic, ready_sink"
affects: [03-06 scheduler, 03-10]
tech-stack:
  added: []
  patterns:
    - "Drop-with-warning: plugin name, item index and a fixed reason label only"
    - "Producer lock held across send-all-and-flush; clear_errors on every failure"
key-files:
  created: []
  modified:
    - src/plugins/host/map.rs
    - src/plugins/host/sink.rs
    - src/streaming.rs
key-decisions:
  - "Key is `{platform}:{id}` after the platform fallback (RESEARCH A4, single-partition topic assumed; not confirmed with the user)"
  - "1_000_000-byte serialized cap under Fluvio's default max_request_size (RESEARCH A5, judgement threshold)"
  - "Dropped items never fail the poll or block the cursor; the count feeds PluginReport.dropped"
requirements-completed: [HOST-04]
status: complete
actuals:
  tokens: 18000
  tasks: 2
  commits: 2
plan_head_before: 0eff1cb38e8613d7ba6488e657314c6e84cb8afb
plan_head_after: 0f798ab361adf043e087e81756ff286dd184883f
commits: 2
completed: 2026-11-09
---

# Phase 3 Plan 08: Item validation and Fluvio sink Summary

Plugin items are validated before they can reach Fluvio (bad ones dropped with a content-free warning), and a `FluvioSink` produces survivors to `filimon-social-items` with send-all-then-flush and `clear_errors` recovery from fluvio's sticky producer error.

## Tasks

| Task | Name | Commit |
| ---- | ---- | ------ |
| 1 | Validate and map plugin items (map.rs) | 64174a1 |
| 2 | FluvioSink, clear_errors, live scratch-topic tests | 0f798ab |

## What was built

- `map.rs`: `MAX_RECORD_BYTES = 1_000_000`, `DropReason` with fixed labels, validating `map_items` (same signature). Drops: empty/whitespace id or url, `url::Url::parse` failure, serialization failure, record over the cap. Platform falls back to the plugin name when empty/whitespace; key is `{platform}:{id}`; valid id/url are never trimmed. No dedupe. Warning fields are `plugin`, `index`, `reason` only.
- `sink.rs`: `FluvioSink` holding `Arc<Mutex<TypedProducer>>`; `connect` runs `preflight` (names missing topic with `fluvio topic create`), `connect_social` uses `TOPIC_SOCIAL_ITEMS`. `send_batch` locks once, sends all, flushes, and calls `clear_errors` before returning `SinkError` on any send or flush failure. `live_support` helpers for later plans.
- `streaming.rs`: `TypedProducer::clear_errors` beside `flush`.

## Verification

- `cargo test -p filimon plugins::host::map`: 16 passed (the 15 named tests plus an extra `drop_reason_labels_are_fixed`).
- `cargo test -p filimon plugins::host::sink`: 2 passed, 2 ignored (`fluvio_sink_is_an_item_sink`, `vec_sink_keeps_batch_order` ok).
- `cargo clippy -p filimon --message-format=short`: no lines for `src/plugins/host/` and none mentioning `clear_errors`.
- Live group: a local Fluvio cluster was running; `cargo test -p filimon plugins::host::sink -- --ignored live_` ran and both `live_fluvio_sink_produces_records_in_order_with_keys` and `live_fluvio_sink_connect_names_a_missing_topic` passed. `fluvio topic list` showed no leftover `filimon-test-` topics afterwards. The sticky-error recovery path itself (`clear_errors` after a real produce failure) is not exercised by any test; it follows the fluvio 0.50.1 source behaviour.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 1 - Bug] Log-capture test flaky under parallel test threads**
- **Found during:** Task 1
- **Issue:** `drop_warning_names_plugin_index_and_reason_but_not_content` failed most runs in the parallel suite (empty log) though it passed alone: sibling tests reach the same `warn!` callsite on threads with no subscriber, and the first thread to hit it caches "never" interest.
- **Fix:** The test makes one warm-up `map_items` call so the callsite is registered, then `tracing::callsite::rebuild_interest_cache()`, before the real call. Stable over 11 consecutive runs.
- **Files modified:** src/plugins/host/map.rs
- **Commit:** 64174a1

**2. [Rule 3 - Clippy] doc backticks and by-ref `label`**
- `DropReason::label` takes `self` (Copy enum) per clippy's trivially-copy lint, and the cap doc comment was reworded; call sites unchanged.

**3. Test variant:** `live_fluvio_sink_connect_names_a_missing_topic` uses a never-created unique name directly, as the plan specified.

Otherwise the plan executed as written.

## Known Stubs

None.

## Threat Flags

None. `FluvioSink` only takes a topic through the `TOPIC_SOCIAL_ITEMS` constant in production code; live tests use scratch topics.

## Self-Check: PASSED

- FOUND: src/plugins/host/map.rs, src/plugins/host/sink.rs, src/streaming.rs
- FOUND commits: 64174a1, 0f798ab (2 commits since plan_head_before)
