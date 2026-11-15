---
phase: 04-plugin-cli
reviewed: 2026-11-10T00:00:00Z
depth: standard
files_reviewed: 19
files_reviewed_list:
  - README.md
  - plugins/README.md
  - src/cli.rs
  - src/commands/mod.rs
  - src/commands/plugins.rs
  - src/commands/run.rs
  - src/main.rs
  - src/plugins/config.rs
  - src/plugins/error.rs
  - src/plugins/host/cursor.rs
  - src/plugins/host/engine.rs
  - src/plugins/host/error.rs
  - src/plugins/host/loader.rs
  - src/plugins/host/mod.rs
  - src/plugins/host/once.rs
  - src/plugins/host/scheduler.rs
  - src/plugins/host/testing.rs
  - src/plugins/mod.rs
  - tests/plugins_cli.rs
findings:
  critical: 1
  warning: 3
  info: 4
  total: 8
  necessity: 0
status: issues_found
---

# Phase 04: Code Review Report

**Reviewed:** 2026-11-10
**Depth:** standard
**Files Reviewed:** 19
**Status:** issues_found

## Summary

Reviewed the phase diff (`e4c4938..HEAD`) for the plugin CLI: `plugins list|check|run-once`, `fili run`, the bounded-shutdown supervisor, the status sidecar and the plugin-scoped config loader. Necessity assessment: no necessity findings.

Mostly sound. The following hold up under tracing:

- The cursor is written only after `send_records` returns, in both the scheduler (`scheduler.rs`) and `commit_once` (`once.rs`). No `.await` sits between the send and the write, so `abort_all` cannot split them.
- Read-only commands (`list`, `check`, dry-run `run-once`) take no lock and create no files. `read_status` and `read_cursor_file` are lock-free.
- File-derived strings go through `clean_guest_text` or `sanitize_for_terminal` in the CLI output paths.
- Secret values never reach `check` output or the `.env` diagnostics. Only alias and env-var names are printed.
- The supervisor's biased select, its lazily armed grace deadline and its abort handling are correct.
- `cargo check` shows no warnings from phase files.

One real defect exists in `fili run` signal forwarding: it can leave the process impossible to stop with Ctrl-C. Three warnings follow, covering the shutdown summary, the status file and secret redaction.

## Critical Issues

### CR-01: `forward_signals` prints before cancelling, so a broken stdout leaves `fili run` un-stoppable by signals

**File:** `src/commands/run.rs:295-310`
**Issue:** `forward_signals` calls `println!` before `shutdown.cancel()` and before `abort.cancel()`. `println!` panics on a write error such as EPIPE. This is a normal situation: `fili run 2>&1 | tee run.log`, or any pipe whose reader exits. Ctrl-C sends SIGINT to the whole foreground process group, so `tee` dies at the same moment.

The signal task then panics inside the first `println!`, before `shutdown.cancel()`. The host never receives a stop request. Tokio keeps the SIGINT and SIGTERM handlers installed for the process lifetime, even after the `Signal` streams drop with the panicked task. The default "terminate" action is never restored.

A second Ctrl-C or a SIGTERM is now swallowed, and the process runs until SIGKILL. The panic is also silent: the task's `JoinHandle` is only `abort()`ed, never awaited. Every other progress line in `run_with_sink` already uses `writeln!(..).ok()` with the comment "a closed stdout must not stop the run". Only this path was missed. The `println!` at `run.rs:258` is the same hazard, but it runs before any handler is installed.

**Fix:**
```rust
pub(crate) async fn forward_signals<S: SignalSource>(
    mut source: S,
    shutdown: CancellationToken,
    abort: CancellationToken,
) {
    source.next().await;
    shutdown.cancel(); // act first, report second
    writeln!(
        std::io::stdout(),
        "[Run] stopping: waiting up to {} s for in-flight sends; press Ctrl+C again to stop now",
        DEFAULT_SHUTDOWN_GRACE.as_secs()
    )
    .ok();

    source.next().await;
    abort.cancel();
    writeln!(std::io::stdout(), "[Run] stopping now: in-flight sends are aborted").ok();
}
```
Also use `writeln!(..).ok()` for the connect banner in `run`. Add a test that drives `forward_signals` with a stdout that errors, or at minimum asserts the token is cancelled before any print.

## Warnings

### WR-01: Aborted (and panicked) plugins report zero polls, items and failures in the shutdown summary

**File:** `src/plugins/host/mod.rs:299-322` (`record_joined`), consumed by `src/commands/run.rs:104-146`
**Issue:** `PluginReport` is a local owned by each `run_plugin` task. When `abort_all()` cancels a task its report is lost. `record_joined` substitutes `PluginReport { disabled: Some("aborted at shutdown"), ..Default::default() }`, so `fili run` prints `POLLS 0  SENT 0  DROPPED 0  FAILURES 0` for a plugin that may have polled for days and produced thousands of items.

The aborted case is the one an operator most needs accurate numbers for, since the summary warns that "an aborted send may replay its last batch". The summary table contradicts the status files and the Fluvio topic. The panic branch has the same problem and predates this phase, but the new abort branch makes it much more reachable. `run_outcome` is unaffected, because it keys on `disabled`.

**Fix:** Have `run_plugin` update a report shared with the supervisor, for example `Arc<Mutex<PluginReport>>` created in `supervise` and cloned into the task. On cancel or panic, take a snapshot and set `disabled`. Alternatively, mark the counters as unknown (`-`) in `state_cell` and the summary when the state is aborted, rather than printing misleading zeros.

### WR-02: A failed cursor write is recorded as `ok: true` in the status file by the scheduler, but `ok: false` by `run-once`

**File:** `src/plugins/host/scheduler.rs:466-482` and `src/commands/plugins.rs:686-698`
**Issue:** When the sink succeeded but `cursors.write` failed, the scheduler logs an error, bumps `report.failures` and falls through to `CycleOutcome::PollOk`. `record_status(.., &PollOk, sent)` then writes `ok: true` with no error. `fili plugins list` shows a healthy `ok (N items)` while the next restart will replay that batch, since the cursor never advanced.

`commit_and_record` in `run-once --produce` handles the same condition correctly: it records `ok:false` with "produced N but the cursor could not be saved". The two paths disagree on the one status that matters for the at-least-once guarantee.

**Fix:** In the scheduler, track a `cursor_error: Option<String>` from the failed write. After computing `outcome`, call `record_status_text(&cursors, &spec.name, false, Some(text), sent)` in that case, instead of `record_status` with `PollOk`. Keep the control flow unchanged (A6: keep running).

### WR-03: Guest-supplied error text is persisted and printed without redacting the secrets that were passed to `init`

**File:** `src/plugins/host/scheduler.rs:181-201` (`describe`), `src/plugins/host/cursor.rs:306-328` (`write_status`), `src/commands/plugins.rs:118-139`, `src/commands/plugins.rs:197-217`
**Issue:** `describe` cleans and clips guest strings (control characters, 512 characters) but does not remove secret values. Guest `init` receives the plaintext secrets, and a guest that wraps an upstream error such as "401 for token=abc123" returns it in `PollError::{Transient,Config,Fatal}`. That text now goes to three new places that earlier phases did not use:

- a durable `<alias>.status.json`,
- `fili plugins list` and `--json` output, which operators routinely paste into tickets,
- `run-once` error output.

Phase 3 already logged the text. Phase 4 makes the leak persistent and easy to copy, and it contradicts the stated goal that secrets are never printed. The `check` path is clean, because it never calls the guest.

**Fix:** In `expose_secrets`' caller, keep the secret values and add a `redact(text, &secrets)` step. Replace every occurrence of a non-empty secret value with `***`, applied inside `describe` or at each `record_status` call site. Alternatively, document the residual risk in `plugins/README.md` under the status-file bullet, if redaction is deliberately out of scope.

## Info

### IN-01: Sink and scheduler docs still say the caller never drops `send_batch`, but `abort_all` now does

**File:** `src/plugins/host/sink.rs:41-43`, `src/plugins/host/sink.rs:83-86`, `src/plugins/host/scheduler.rs:282-284`
**Issue:** These comments state that the caller does not cancel the future because "dropping it mid-flight would skip the implementation's cleanup", meaning `clear_errors`. The sink is shared by all plugins, and Fluvio 0.50.1 keeps returning a stored producer error until it is cleared. The supervisor now drops the future by design (abort at the grace deadline or a second signal).

This is harmless today because the process exits straight after, so no later batch sees the stale error. The invariant is no longer true, though, and a future caller that reuses the sink after `run_with_abort` would hit it.

**Fix:** Update the three comments to say that `Host::supervise` may drop an in-flight send at shutdown and that the sink must not be reused afterwards. Optionally note this on `Host::run_with_abort`.

### IN-02: README documents the `fili run` startup order incorrectly

**File:** `plugins/README.md` (`fili run` section), `src/commands/run.rs:250-262`
**Issue:** The README says `fili run` "takes the state-directory lock ..., connects to Fluvio and loads every enabled source component". The code connects to Fluvio first (`FluvioSink::connect_social().await?`) and only then locks the state directory inside `Host::load`. A second `fili run` on a busy state directory therefore opens a Fluvio connection and prints "Connecting..." before it fails with `StateDirLocked`. `run-once --produce` does lock first.

**Fix:** Either move a lock probe (`CursorStore::open`, then pass the store into the host) ahead of the connect so `fili run` fails fast like `run-once --produce`, or correct the README order.

### IN-03: `read_cursor_file` reads the file with no size bound, unlike `read_status`

**File:** `src/plugins/host/cursor.rs:182-198`
**Issue:** `read_status` limits input to `MAX_STATUS_BYTES`. `read_cursor_file` now runs lock-free from `run-once` and uses an unbounded `std::fs::read`. A cursor file is at most about 5.5 KB, since `MAX_CURSOR_BYTES` is 4096 and base64 expands it. A corrupt or hostile file, or a FIFO planted at `<alias>.json`, makes a dry run read without limit or hang. The `.status.json` reader handles the first case.

**Fix:** Read through `File::open(..).take(MAX_CURSOR_FILE_BYTES + 1)` and return `CursorCorrupt("state file is too large")` over the limit, mirroring `read_status`.

### IN-04: `check` prints host diagnostics unsanitized, unlike `run`

**File:** `src/commands/plugins.rs:502-509`
**Issue:** `run.rs` passes `failure.error.to_string()` through `clean_guest_text`. `write_diagnostics` prints `miette::Report::new(error)` for `HostError` and `ConfigErrors` directly. These embed component paths and, for config errors, rendered TOML snippets. A control character or ESC in a registry-supplied path reaches the terminal raw. The imports list is safe because it is `{:?}`-formatted. The registry is operator-controlled, so the risk is low. It is still inconsistent with the file's own header ("everything that came from a file ... passes through `sanitize_for_terminal`").

**Fix:** Render the report to a `String` and pass it through `sanitize_for_terminal`. Keep newlines, for example by sanitizing per line.

---

_Reviewed: 2026-11-10_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
