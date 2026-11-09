---
phase: "3"
slug: "component-host-runtime"
status: draft
# threats_open = count of OPEN threats at or above workflow.security_block_on severity (the blocking gate)
threats_open: 1
asvs_level: 1
created: "2026-11-09"
---

# Phase 3 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.
> Register source: `<threat_model>` blocks in 03-01 through 03-10 PLAN.md (authored at plan time). No `## Threat Flags` were raised in any SUMMARY.md. Audited at ASVS L1 (L2 depth where the code review touched a threat) against the integrated tree on branch fluvio-wise.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| user shell to machine-local rustup settings | A setting outside the repository changes which compiler builds the project | (see description) |
| test fixture to host tests | A deliberately misbehaving guest (trap, spin) runs inside the host under test | (see description) |
| crates.io to build tree | Third-party crates (wasmtime and its dependency tree) enter the repository's lock file | (see description) |
| toolchain selection to build output | Which rustc builds which crate decides whether the host is supportable and SmartModules link | (see description) |
| guest component to host (wasmtime Linker) | Untrusted guest code runs in-process and can call only the imports the Linker defines | (see description) |
| guest output to host data and logs | Items, error strings, log lines and trap text are untrusted input | (see description) |
| host process to local filesystem | Cursor files are written under an operator-chosen directory using names from the registry | (see description) |
| registry TOML to component loader | Only the registry decides which `.wasm` files are opened | (see description) |
| environment variables to process configuration | `RUST_LOG` and `FILI_STATE_DIR` come from the operator's environment | (see description) |
| host log output to the operator's terminal | Everything the host logs is read by a person on a terminal | (see description) |
| guest errors and traps to host control flow | A plugin's return values and traps decide how often and whether the host retries it | (see description) |
| guest-supplied strings to the operator's terminal | Error text and trap messages are untrusted | (see description) |
| registry entry to opened file | Only the registry's enabled entries cross into the compiler | (see description) |
| component binary to host | Import names and compile errors derived from an untrusted binary reach logs and diagnostics | (see description) |
| guest items to Fluvio | Plugin output becomes records on a shared topic read by the consumer | (see description) |
| host to Fluvio cluster | A down or failing cluster can hang or poison the producer | (see description) |
| state directory contents to host | Files an operator or another process can edit or replace are read at startup | (see description) |
| two fili processes to one state directory | Concurrent hosts could double-poll and overwrite each other's cursors | (see description) |
| deliberately hostile guests (trap, spin, fatal) to the host runtime | The fault fixture plays the part of a misbehaving third-party plugin | (see description) |
| tests to the shared Fluvio cluster | The live test writes to a real cluster | (see description) |

---

## Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation | Status |
|-----------|----------|-----------|----------|-------------|------------|--------|
| T-3-01 (03-01) | Tampering | machine-local `rustup override` for this checkout | low | accept | Only the user runs the command, with the reason stated in the checkpoint. Reversible with `rustup override set 1.85.0`. No repository file changes in this plan. — verified: 03-01-SUMMARY; `rustup override list` = no overrides | closed |
| T-3-SC (03-01) | Tampering | npm/pip/cargo installs | high | mitigate | No packages are installed in this plan. Installs happen in plan 03-03, behind the RESEARCH `## Package Legitimacy Audit` (wasmtime, base64, tokio-util: all OK;  — verified (no dependency change in this plan) | closed |
| T-3-02 (03-02) | Denial of Service | fault-source `trap` and `spin` modes | low | accept | Test-only crate: never referenced from `examples/plugins/` registries, its `.wasm` output is gitignored, and the README labels it a test fixture. The host's iso — verified: plugins/README.md:15-21; .gitignore /target/ | closed |
| T-3-03 (03-02) | Information Disclosure | fault-source log lines | low | mitigate | The fixture ignores the `secrets` argument and logs only the mode name and poll counters; acceptance criteria require the source to bind secrets as unused. — verified: fault-source lib.rs:84,99,152 | closed |
| T-3-SC (03-02) | Tampering | npm/pip/cargo installs | high | mitigate | No new dependency version: the lock file is the stub-source lock with the root package renamed, verified by the normalised `diff` gate. Dependencies (`wit-bindg — verified (no dependency change in this plan) | closed |
| T-3-04 (03-03) | Tampering | wasmtime version resolution under the MSRV-aware resolver | high | mitigate | Raise `rust-version` to 1.95 and require `wasmtime = "48"`; gate on `Cargo.lock` containing a 48.x wasmtime and no 30.x-47.x entry (an unsupported 34.0.2 would  — verified: Cargo.toml:5,45; Cargo.lock wasmtime 48.0.8 only | closed |
| T-3-05 (03-03) | Tampering | SmartModule build regression from the toolchain move | medium | mitigate | Nested 1.85.0 pins in all three SmartModule directories; Task 2 proves them with a version check per directory and a real `smdk build`. — verified: nested rust-toolchain.toml 1.85.0; smdk build in 03-03-SUMMARY | closed |
| T-3-06 (03-03) | Elevation of Privilege | wasmtime default features expanding what a plugin or the hos | medium | mitigate | `default-features = false` with an explicit six-feature list; acceptance criteria grep that no WASI crate and none of the three excluded features appear in Carg — verified: Cargo.toml:45 six features, no wasi/wat/cache in lock | closed |
| T-3-SC (03-03) | Tampering | npm/pip/cargo installs (wasmtime 48, base64 0.22, tokio-util | high | mitigate | RESEARCH `## Package Legitimacy Audit` lists all three as OK with no [ASSUMED], [SUS] or [SLOP] row; base64 and tokio-util are already in Cargo.lock transitivel — verified (no dependency change in this plan) | closed |
| T-3-07 (03-04) | Elevation of Privilege | `HostEngine` linker | high | mitigate | The `Linker` is populated only through the generated `Source::add_to_linker` (the `filimon:plugin/*` interfaces); no WASI crate exists in Cargo.toml (plan 03-03 — verified: engine.rs:198-204 no WASI linked | closed |
| T-3-08 (03-04) | Elevation of Privilege | `loader::load_components` | high | mitigate | Iterates only `registry.sources` entries with `enabled && plugin.is_some()`; there is no directory scan in `loader.rs` (`read_dir` is absent there, acceptance-c — verified: loader.rs:83-116 enabled-only; no read_dir | closed |
| T-3-09 (03-04) | Information Disclosure | `PluginSpec`, `init` call, logs | high | mitigate | Secrets are `Secret` values (Debug prints `Secret(***)`); `Secret::expose()` is called only while building the `init` argument list inside `run_plugin`; no log  — verified: secrets.rs:34-37; expose() only scheduler.rs:319 | closed |
| T-3-10 (03-04) | Tampering | guest `log` import and guest error text reaching the termina | medium | mitigate | Every guest string passes `clean_guest_text` (`sanitize_for_terminal` plus a character clip: 4096 for log lines, 512 for error text) before it reaches `tracing` — verified: engine.rs:71-117 clean_guest_text | closed |
| T-3-11 (03-04) | Tampering | cursor file path derived from the plugin name | medium | mitigate | The path is only ever `dir.join("<alias>.json")` with `alias` from `LoadedRegistry` (names limited to `[a-z0-9_-]` by `is_valid_name`); no other source of alias — verified: cursor.rs:85-87; config.rs:388-393 alias charset | closed |
| T-3-12 (03-04) | Denial of Service | a guest that never yields starving a tokio worker | medium | mitigate | Epoch interruption: engine config enables it, every Store sets deadline 1 with async yield-and-update, a 10 ms ticker thread advances the epoch, so every `call_ — verified: engine.rs:151-195, instance.rs:98 epoch yield; wasm spin test (residual WR-02) | closed |
| T-3-13 (03-04) | Denial of Service | a guest allocating unbounded memory or burning CPU without l | medium | accept | Per-plugin CPU and memory limits are deferred to v2 HOST-06 (CONTEXT, Deferred Ideas); epoch yield only keeps the runtime responsive and does not kill a looping — verified (no dependency change in this plan) | open — below high threshold (non-blocking) |
| T-3-SC (03-04) | Tampering | npm/pip/cargo installs | high | mitigate | This plan adds no new package: wasmtime, base64 and tokio-util arrived in plan 03-03 behind the RESEARCH package-legitimacy audit and a Cargo.lock gate. — verified (no dependency change in this plan) | closed |
| T-3-14 (03-05) | Information Disclosure | tracing subscriber and the default filter | low | mitigate | Default directive limited to `fili::plugins=info`; stderr only; the only existing plugin-target event prints env var NAMES, never values; the host never logs se — verified: main.rs:55-77 plugin-only default, stderr | closed |
| T-3-15 (03-05) | Tampering | `--state-dir` / `FILI_STATE_DIR` pointing at an arbitrary di | low | accept | The operator controls the process and its environment. The directory only receives `<alias>.json` files with `version`, `plugin` and `cursor` (no secrets), and  — verified: README.md:226-239; accepted (residual WR-07, low) | closed |
| T-3-SC (03-05) | Tampering | npm/pip/cargo installs | high | mitigate | No new package: `tracing-subscriber` with `env-filter` is already a direct dependency. — verified (no dependency change in this plan) | closed |
| T-3-16 (03-06) | Denial of Service | retry loop for a failing plugin | high | mitigate | Every failure action waits `backoff::step` (>= `initial`, which config validation makes non-zero) or the larger retry-after, capped at `max`; the counter satura — verified: backoff.rs:27-34; config.rs:101,590,599; scheduler.rs:171 | closed |
| T-3-17 (03-06) | Denial of Service | reuse of a trapped instance (every later call returns `Canno | medium | mitigate | `InstanceFatal` and `BuildFailed` set `discard_instance`, so the loop drops the Store and builds a fresh instance then re-runs `init`; tests `sched_trap_rebuild — verified: scheduler.rs:168,283-324 rebuild+reinit | closed |
| T-3-18 (03-06) | Tampering | guest error and trap text in logs (terminal injection) | medium | mitigate | `describe` passes every guest or wasmtime string through `clean_guest_text(.., 512)`; tests `describe_cleans_control_characters`, `fatal_disables_with_cleaned_r — verified: scheduler.rs:181-200; instance.rs:45,104 | closed |
| T-3-19 (03-06) | Repudiation | a plugin failing or being disabled without a trace | medium | mitigate | Every backoff logs a `warn!` and every disable an `error!`, each with `plugin=<alias>`; test `sched_logs_name_the_plugin_on_backoff_trap_and_disable`. — verified: scheduler.rs:206-276 plugin= logs | closed |
| T-3-SC (03-06) | Tampering | npm/pip/cargo installs | high | mitigate | No new package in this plan. — verified (no dependency change in this plan) | closed |
| T-3-20 (03-07) | Elevation of Privilege | a plugin importing WASI or any capability fili did not inten | high | mitigate | No WASI is linked; `preflight_imports` rejects every import outside `filimon:plugin/` before instantiation with a named `ForeignImports` error; test `component_ — verified: loader.rs:178-226 preflight_imports rejects non-filimon imports | closed |
| T-3-21 (03-07) | Elevation of Privilege | loading an unlisted or disabled `.wasm` | high | mitigate | `load_components` filters on `enabled` for sources (and `plugin.is_some()`) and fetchers, records every opened path, and contains no directory scan; tests open  — verified: loader.rs unlisted/disabled never opened (test) | closed |
| T-3-22 (03-07) | Tampering | accepting a component of the wrong world and failing later a | medium | mitigate | `instantiate_pre` against the matching world at load time, mapped to `WrongWorld`; ignored wasm tests cover both directions. — verified: loader.rs:152-170 WrongWorld both directions (wasm tests) | closed |
| T-3-23 (03-07) | Information Disclosure | guest-controlled import names and wasmtime text in diagnosti | low | mitigate | Every such string goes through `clean_guest_text` (128 chars per import, 512 for the reason), the import list is capped at 16 entries, and no `#[source_code]` b — verified: loader.rs:25-31,189-200 capped/sanitized (residual IN-01) | closed |
| T-3-SC (03-07) | Tampering | npm/pip/cargo installs | high | mitigate | No new package in this plan. — verified (no dependency change in this plan) | closed |
| T-3-24 (03-08) | Tampering | malformed or oversized plugin items poisoning the Fluvio pro | high | mitigate | Per-item validation in `map_items` (empty or whitespace id and url, `url::Url::parse`, 1_000_000-byte serialized cap under the verified 1_048_576 default); drop — verified: map.rs:84 caps value only; key unbounded (WR-03); sink.rs:82-87 clear_errors skipped on timeout (WR-06) | open |
| T-3-25 (03-08) | Information Disclosure | item content or settings in logs | medium | mitigate | Drop warnings carry only plugin name, item index and a fixed reason label; test asserts a content marker never appears in the captured log. — verified: map.rs:105-110 fixed reason labels | closed |
| T-3-26 (03-08) | Denial of Service | producer hanging for up to 300 s when the cluster is down | medium | mitigate | `FluvioSink::connect` runs `preflight` and fails fast with a named missing topic; every sink call is wrapped in `SINK_TIMEOUT` (60 s) by the scheduler, and a ti — verified: sink.rs:20,56-63; scheduler.rs:222 (residual WR-06) | closed |
| T-3-27 (03-08) | Tampering | social items written to the wrong topic or through the news  | low | mitigate | `connect_social` uses the `TOPIC_SOCIAL_ITEMS` constant; no plugin or registry value selects a topic; live tests use scratch topics only. — verified: sink.rs:66-68 TOPIC_SOCIAL_ITEMS const only | closed |
| T-3-SC (03-08) | Tampering | npm/pip/cargo installs | high | mitigate | No new package in this plan (`url` and `serde_json` are existing dependencies). — verified (no dependency change in this plan) | closed |
| T-3-28 (03-09) | Tampering | a corrupt, truncated, mismatched or newer-version state file | high | mitigate | `read` returns `CursorCorrupt` or `CursorUnreadable` naming plugin and path for bad JSON, bad base64, `plugin` mismatch, unsupported version and io errors; the  — verified: cursor.rs:99-137; scheduler.rs:305-313 (residual IN-03) | closed |
| T-3-29 (03-09) | Tampering | two processes sharing one state directory | medium | mitigate | `CursorStore::open` holds `File::try_lock` on `.lock`; a second open returns `StateDirLocked`; released on drop; test `second_open_of_same_dir_is_locked_and_rel — verified: cursor.rs:61-75 try_lock | closed |
| T-3-30 (03-09) | Tampering | a torn or partial cursor write after a crash | medium | mitigate | Same-directory temp file, `sync_all`, atomic rename, best-effort directory fsync; test `write_leaves_no_temp_files_behind`. — verified: cursor.rs:156-165 tempfile+fsync+rename | closed |
| T-3-31 (03-09) | Information Disclosure | secrets or settings landing in state files | low | mitigate | The file shape is fixed to `version`, `plugin`, `cursor` and a test asserts exactly those keys. The cursor bytes are plugin-defined opaque pagination state by c — verified: cursor.rs:28-33 three-key shape | closed |
| T-3-SC (03-09) | Tampering | npm/pip/cargo installs | high | mitigate | No new package in this plan. — verified (no dependency change in this plan) | closed |
| T-3-32 (03-10) | Denial of Service | a guest that loops forever occupying its own task | medium | accept | Per-poll deadlines and CPU limits are v2 HOST-06. The test proves what is guaranteed instead: other plugins keep polling and shutdown completes within 10 s beca — verified: README.md:264-267; wasm spin test | closed |
| T-3-33 (03-10) | Repudiation | documentation promising guarantees the host does not give | low | mitigate | The README section is written from the code and states at-least-once delivery, no dedupe, no CPU or memory limits and the spinning-guest gap; acceptance criteri — verified: README.md:220-269 (line 220 over-broad, see T-3-13) | closed |
| T-3-34 (03-10) | Tampering | the live test altering real data on the cluster | low | mitigate | The live test creates and deletes its own `filimon-test-host-` scratch topic and never names `filimon-social-items` (grep-checked). — verified: sink.rs:167-182 scratch_topic; README.md:291-293 | closed |
| T-3-SC (03-10) | Tampering | npm/pip/cargo installs | high | mitigate | No new package in this plan. — verified (no dependency change in this plan) | closed |

*Status: open · closed · open — below high threshold (non-blocking)*
*Severity: critical > high > medium > low — only open threats at or above workflow.security_block_on (high) count toward threats_open*
*Disposition: mitigate (implementation required) · accept (documented risk) · transfer (third-party)*

### Open items

- **T-3-24 (high, BLOCKING).** The 1,000,000-byte record cap measures only the serialized value (`map.rs:84`); the guest-controlled key `{platform}:{id}` (`map.rs:70`) is unbounded and not counted, and Fluvio sizes the whole record. An oversized key passes the host gate, fails at the producer, and the scheduler retries forever, re-sending earlier records as duplicates (WR-03). Also `clear_errors` is skipped when the 60 s sink timeout drops `send_batch` (WR-06). **User ruling: block and fix.**
- **T-3-13 (medium, non-blocking).** Accepted at plan time on the premise that failures stay isolated, but no guest memory limit exists (no `StoreLimits`/limiter), so a guest can grow memory until the OS kills the whole process (WR-01), contradicting HOST-05 and README line 220. Items per poll, `next_cursor` size and guest log volume are also uncapped. **User ruling: add a small fixed safety cap now** (HOST-06 keeps configurable per-plugin limits).

### Residuals on closed threats (low, tracked in 03-REVIEW-DISPOSITION.md)

WR-02 epoch-ticker spawn failure swallowed (T-3-12/T-3-32); WR-07 empty `--state-dir` resolves to cwd (T-3-15); IN-01 Unicode line/bidi controls survive `clean_guest_text` (T-3-10/18/23); IN-03 missing `cursor` key reads as first poll (T-3-28); WR-04 URL scheme unrestricted and WR-05 platform impersonation / ambiguous key (recommended hardening on T-3-24). Not checked: RUSTSEC advisory scan of wasmtime 48.0.8 (needs network).

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-3-01 | T-3-01 | Machine-local rustup override removal is user-run and reversible (`rustup override set 1.85.0`); no repo file changed | plan-time disposition (03-01); executed by user | 2026-11-09 |
| AR-3-02 | T-3-02 | fault-source is a test-only fixture: README-labelled, never in an example registry, `.wasm` gitignored | plan-time disposition (03-02) | 2026-11-09 |
| AR-3-03 | T-3-15 | Operator controls the process and `--state-dir`; only non-secret three-key files and a lock go there (residual WR-07 low) | plan-time disposition (03-05) | 2026-11-09 |
| AR-3-04 | T-3-32 | A guest looping forever is never terminated (v2 HOST-06); other plugins keep polling and shutdown returns, test-proven; stated in README Known limits | plan-time disposition (03-10) | 2026-11-09 |

*Accepted risks do not resurface in future audit runs.* T-3-13 is intentionally NOT in this log: its acceptance was withdrawn by the user ruling above.

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-11-09 | 44 | 42 | 2 | gsd-security-auditor via /gsd-secure-phase 3 |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [ ] `threats_open: 0` confirmed
- [ ] `status: verified` set in frontmatter

**Approval:** pending — blocked by T-3-24
