# Phase 6: Host-Managed OAuth - Pattern Map

**Mapped:** 2026-11-11
**Files analyzed:** 16 new/modified
**Analogs found:** 15 / 16

All analog paths are git-tracked under `src/`. Line numbers are from the working tree on branch `fluvio-wise`.

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match |
|---|---|---|---|---|
| `src/plugins/auth/store.rs` | store (file persistence) | file-I/O, atomic write | `src/plugins/host/cursor.rs` | exact |
| `src/plugins/auth/error.rs` | error enum (miette) | request-response | `src/plugins/error.rs` + `src/plugins/host/error.rs` (`CursorCorrupt`) | role-match |
| `src/plugins/auth/pkce.rs` | utility | transform | none in repo (RESEARCH Pattern 3 and the RFC vector) | no analog |
| `src/plugins/auth/callback.rs` | service (loopback listener) | request-response, one-shot | `TestHttpServer` in `src/plugins/host/testing.rs:203-345` (reverse side of parsing) | partial |
| `src/plugins/auth/client.rs` | service (HTTP client) | request-response | `src/plugins/host/fetch.rs` (reqwest builder, no redirects) | role-match |
| `src/plugins/auth/provider.rs` | service (pre-poll decision) | request-response | `src/plugins/host/scheduler.rs` `run_plugin` cursor read block | partial |
| `src/plugins/auth/mod.rs` | module root | n/a | `src/plugins/host/mod.rs` | exact |
| `src/plugins/config.rs` (modify) | config | transform | itself: `HostSection`, `BackoffSection`, `PluginConfig` | exact |
| `src/plugins/error.rs` (modify) | error | n/a | itself: `InvalidValue`, `MissingSecret`, `plugin()` | exact |
| `src/plugins/host/instance.rs` (modify) | trait + wasm adapter | request-response | itself, lines 62-71 and 139-142 | exact |
| `src/plugins/host/scheduler.rs` (modify) | scheduler | event-driven loop | itself: `PluginSpec`, `CycleOutcome`, poll loop | exact |
| `src/plugins/host/once.rs` (modify) | service | request-response | itself, `instance.poll(cursor)` at ~70 | exact |
| `src/plugins/host/testing.rs` (modify) | test support | n/a | itself: `FakeInstance`, `TestHttpServer` | exact |
| `src/commands/auth.rs` | command | request-response | `src/commands/plugins.rs` | exact |
| `src/cli.rs`, `src/main.rs`, `src/commands/mod.rs` (modify) | wiring | n/a | `Command::Plugins`, `shows_banner` | exact |
| `plugins/README.md`, `examples/plugins/*.toml` | docs/config | n/a | existing samples (do NOT edit reddit/youtube TOMLs; RESEARCH Pitfall 10) | n/a |

## Pattern Assignments

### `src/plugins/auth/store.rs` (store, file-I/O)

**Analog:** `src/plugins/host/cursor.rs`

**Header comment style** (lines 1-16): `// ───` rule, file name, then a prose paragraph covering the file layout, atomic writes, and bad-file policy. Section dividers are `// ── Name ───`. Imports are ordered std, external, `super::`/`crate::`.

**Atomic write** (cursor.rs `CursorStore::write`, ~lines 282-306). Copy this directly, adding `<state>/auth` as the dir, and use a token-file struct. `NamedTempFile::new_in` already gives mode 0600.
```rust
let mut temp =
    tempfile::NamedTempFile::new_in(&self.dir).map_err(|e| fail(e.to_string()))?;
serde_json::to_writer(&mut temp, &state).map_err(|e| fail(e.to_string()))?;
temp.as_file().sync_all().map_err(|e| fail(e.to_string()))?;
temp.persist(&path).map_err(|e| fail(e.error.to_string()))?;
// Best effort: make the rename itself durable.
if let Ok(dir) = File::open(&self.dir) {
    let _ = dir.sync_all();
}
```

**Lock-free reader with size cap** (cursor.rs lines 52, 115-140): `MAX_STATUS_BYTES = 64 * 1024`, `file.take(MAX + 1).read_to_end(...)`, reject when larger. Reuse for the token file read. `read_cursor_file(dir, alias)` (line ~158-200) is the template for a free function: no lock, no creation, `Ok(None)` on missing file, `plugin != alias` and `version` newer than known are "corrupt". Add the Unix check `mode & 0o077 != 0` and `symlink_metadata` regular-file check before parsing (D-02, never chmod).

**Lock (per alias, NOT the state `.lock`)** (cursor.rs `open`, lines ~226-248):
```rust
let lock = OpenOptions::new().create(true).truncate(false).write(true)
    .open(dir.join(LOCK_FILE)).map_err(|e| state_dir_error(&e))?;
match lock.try_lock() {
    Ok(()) => {}
    Err(TryLockError::WouldBlock) => { /* busy */ }
    Err(TryLockError::Error(e)) => return Err(state_dir_error(&e)),
}
```
Use this on `auth/<alias>.lock`, in a `try_lock` + `tokio::time::sleep` loop (RESEARCH Pitfall 5). Never construct `CursorStore` in auth code, because `fili run` holds `.lock` (Pitfall 1).

**Alias safety** (cursor.rs `path_for`, lines ~250-254): `self.dir.join(format!("{alias}.json"))`. The registry limits aliases to `[a-z0-9_-]`, but `fili auth login|logout <plugin>` takes a CLI argument, so run the same name rule first.

**Tests** (cursor.rs `mod tests`, from ~line 320): `TempDir::new()`, write a bad file with `std::fs::write`, assert the error variant with `matches!` and that `to_string()` contains both the plugin alias and the path. Copy this style for corrupt/loose-perm/mismatch cases (use `std::fs::set_permissions` for the 0644 case).

---

### `src/plugins/auth/error.rs` (error, miette)

**Analog:** `src/plugins/error.rs` (`InvalidValue` at lines 63-75) and `src/plugins/host/error.rs` (`CursorCorrupt` / `CursorUnreadable`, which carry `plugin`, `path`, `reason`).
```rust
#[error("plugin `{plugin}` (`{path}`): `{field}`: {message}")]
#[diagnostic(code(fili::config::invalid_value))]
```
Use `#[diagnostic(code(fili::auth::...))]`. Variants carry `plugin` and `path` fields, and `reason` is fixed text, never file content or token text. Provider error text passes through `clean_guest_text` (`src/plugins/host/engine.rs`).

---

### `src/plugins/auth/client.rs` (HTTP client, async)

**Analog:** `src/plugins/host/fetch.rs`. It builds `reqwest::Client::builder().redirect(reqwest::redirect::Policy::none())` with certificate validation left at the default. Do the same, adding `timeout(30s)`, `connect_timeout(10s)` and `user_agent(concat!("fili/", env!("CARGO_PKG_VERSION")))`. Use the **async** client only (`fili` runs under `#[tokio::main]`; `reqwest::blocking` panics). The `json` feature is off, so use `.form(...)` and `serde_json::from_slice`.

**Tests:** `TestHttpServer` + `Reply::ok(...)` + `SeenRequest::header(...)` in `src/plugins/host/testing.rs:203-345` as the fake token/revoke endpoint. Unit tests give the client a `Url` of `http://127.0.0.1:<port>`; config validation (https only) lives in `config.rs`.

---

### `src/plugins/auth/provider.rs` (pre-poll decision)

**Analog:** the cursor-read failure path in `run_plugin` (`scheduler.rs`, the block that calls `record_status_text` then `disable`). A corrupt or loose-perm token file reuses it, returning `Disable { reason }`. Decision table and injectable clock: RESEARCH Pattern 6 and Pitfall 6. Wall-clock `expires_at` does not move under `start_paused`, so inject `Arc<dyn Fn() -> i64 + Send + Sync>`.

`PluginSpec` derives `Debug` with no hand-written impl, so `TokenProvider` needs a redacting `Debug`.

---

### `src/plugins/config.rs` (modify: `[host.oauth]`)

**Analog:** itself.

**Typed table with deny_unknown_fields** (lines 100-123). Add `oauth: Option<OAuthSection>` to `HostSection`, which is `#[serde(deny_unknown_fields)]`, so the table must be declared:
```rust
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostSection {
    poll_interval_secs: NonZeroU64,
    ...
    #[serde(default)]
    secrets: BTreeMap<String, toml::Spanned<String>>,
}
```
Model `OAuthSection` on `BackoffSection` (lines ~125-130): `Option<toml::Spanned<...>>` fields so diagnostics label the exact value. Keys: `authorize_url`, `token_url`, `scopes`, `client_id`, `client_secret_env`, `revocation_url`, `redirect_port`, `authorize_params` (D-14).

**Validation emission** (line ~859): closure building `ConfigError::InvalidValue { plugin, path, field, message, src, span }`. Field strings are `"host.oauth.token_url"` etc. Add checks: https scheme, host present, no userinfo, `redirect_port` non-zero, `client_id` non-empty, and `authorize_params` keys not colliding with `state`, `code_challenge`, `code_challenge_method`, `redirect_uri`, `client_id`, `response_type`, `scope`.

**Public output** (`PluginConfig`, lines 209-216, built literally in `finish_plugin` ~590 and a `secrets.rs` test ~637): add `oauth: Option<OAuthConfig>` here. NOT in `HostConfig` (built in four other places) and NOT in `ResolvedSecrets` (which `expose_secrets` hands to the guest).

**Trap (pass 2, line 357):** `.env` is read only `if parsed.values().any(|plugin| !plugin.aliases.is_empty())`. Extend it with `|| plugin.oauth_client_secret_env.is_some()`, or a plugin whose only secret is `client_secret_env` never reads `.env`.

---

### `src/plugins/error.rs` (modify)

Add `MissingOAuthSecret` / `EmptyOAuthSecret` modeled on `MissingSecret` (lines 77-90, with `code(fili::config::missing_secret)` and `help("add `{env_var}=...` to {dotenv}")`) but naming `host.oauth.client_secret_env`. **Add them to the `ConfigError::plugin()` match** (lines 207-227) or a scoped `load_plugin` drops them.

---

### `src/plugins/host/instance.rs` (modify)

**Analog:** itself. Trait (lines 67-70) and wasm call (139-142) change together:
```rust
fn poll(&mut self, cursor: Option<&[u8]>)
    -> impl Future<Output = CallOutcome<PollResponse>> + Send;
...
async fn poll(&mut self, cursor: Option<&[u8]>) -> CallOutcome<PollResponse> {
    // The access token is always none until Phase 6.
    classify(self.source.call_poll(&mut self.store, cursor, None).await)
}
```
New signature adds `access_token: Option<&str>`, passed straight in place of `None`. Remove the "until Phase 6" comment. Implementors and callers that must change in the same commit: `FakeInstance` (`testing.rs` ~559), `ProbeInstance` (`commands/plugins.rs` ~2338), `scheduler.rs` (`instance.poll(cursor.as_deref())`, ~433), `once.rs` (`instance.poll(cursor)`, ~70).

---

### `src/plugins/host/scheduler.rs` (modify)

**Analog:** itself.

`PluginSpec` (lines 37-42): add `pub auth: Option<Arc<TokenProvider>>`. Both construction sites are `PluginSpec::from_loaded` (lines 47-59, which copies `plugin.host/settings_toml/secrets`) and `testing.rs::spec` (`auth: None`).

`CycleOutcome` (lines ~80-92): add `TokenRefresh(String)`. `next_action` matches exhaustively, so treat it like `Transient`: `(step, false)` and keep the instance. `PollError::Transient(_) | PollError::AuthRequired => (step, discard)` is at line 162, and the auth-required status text is at line 194.

Poll loop (lines ~430-433): the provider call goes immediately before `instance.poll(...)`, inside the same `tokio::select!` with `biased; () = shutdown.cancelled() => return report`:
```rust
let polled = tokio::select! {
    biased;
    () = shutdown.cancelled() => return report,
    polled = instance.poll(cursor.as_deref()) => polled,
};
```

---

### `src/plugins/host/once.rs` (modify, D-15)

The call is `instance.poll(cursor).await` at ~line 70 inside `poll_once(...)`, after `init(&spec.settings_toml, &expose_secrets(spec))`. Obtain the token from `spec.auth` before it. Provider failure maps to `Err(CycleOutcome::TokenRefresh(..))` like the other `Err(CycleOutcome::...)` returns there.

---

### `src/plugins/host/testing.rs` (modify, SC3)

`FakeInstance` (lines ~548-596) holds `script` and `events` and pushes `FakeEvent::Poll { cursor, started, ended }`. Extend `init` to record `(settings, secrets)` and `poll` to record the `access_token` into the same events or a recorder. The SC3 test dumps everything recorded into one string and asserts it contains the access token but not the refresh-token or client-secret sentinels (RESEARCH "SC3 fake-level recording test"). An ignored real-wasm variant goes through `stub-source` (`wasm_tests.rs` is the harness).

---

### `src/commands/auth.rs` (command)

**Analog:** `src/commands/plugins.rs`

**Header block** (lines 1-27): module comment stating what each subcommand does and does not touch (lock, state dir creation, secrets). Imports (lines 29-40): `miette::IntoDiagnostic`, `crate::cli::...Command`, `crate::output::sanitize_for_terminal`, `crate::plugins::config::{load_plugin, ...}`.

**State dir** (line 248):
```rust
pub(crate) fn state_dir_for(base_dir: &Path, flag: Option<&Path>) -> miette::Result<PathBuf> {
    let dir = HostOptions::resolve_state_dir(base_dir, flag);
    require_state_dir(&dir)?;
    Ok(dir)
}
```
**Dispatch** (line 838):
```rust
pub async fn dispatch(command: &PluginsCommand, registry: &Path, state_dir_flag: Option<&Path>) -> miette::Result<()> {
    match command {
        PluginsCommand::List { json } => {
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            ...
            list(registry, state_dir_flag, *json, &mut out, &mut err)
```
Subcommand functions take `out: &mut impl Write` so tests capture output. `status` uses `list_registry` (reads `filimon.toml` only, no secrets) and a lock-free token reader, the way `list` reads `*.status.json`. `login` and `logout` use scoped `load_plugin`, like `run-once`. Everything printed from files or the network goes through `sanitize_for_terminal`.

---

### `src/cli.rs`, `src/main.rs`, `src/commands/mod.rs` (wiring)

- `cli.rs` lines 131-134: `Plugins { #[command(subcommand)] command: PluginsCommand }`, with the enum at ~line 138 (`#[derive(Subcommand, Debug)]`, `List { #[arg(long)] json: bool }`). Add `Auth { #[command(subcommand)] command: AuthCommand }` and `AuthCommand::{Login { plugin, #[arg(long)] no_browser }, Status { #[arg(long)] json }, Logout { plugin }}`.
- `main.rs` line 87-88: `!matches!(command, Command::Plugins { .. } | Command::Run)` becomes `... | Command::Auth { .. }`. The test at ~lines 462-470 asserts banner behaviour per command; add an `auth` case.
- `main.rs` line 127: `Command::Plugins { ref command } => { ... }` dispatch arm. Add the sibling.
- `commands/mod.rs`: add `pub mod auth;` and `pub use auth::dispatch as run_auth;` alongside `run_plugins`.

---

## Shared Patterns

### Secrets never leak
**Source:** `src/plugins/secrets.rs` (`Secret`: no `Display`, no `Serialize`, redacted `Debug`).
**Apply to:** the client secret (`OAuthConfig.client_secret: Option<Secret>`) and every token type. Token types need a new redacting newtype WITH `Serialize`/`Deserialize` (`Debug` prints `***`), because `Secret` cannot be serialised. Never log codes, verifiers, state, or response bodies (D-06).

### Cleaned untrusted text
**Source:** `clean_guest_text` in `src/plugins/host/engine.rs`; `sanitize_for_terminal` in `src/output`.
**Apply to:** provider `error` / `error_description`, and anything printed from files.

### Corrupt per-plugin state disables only that plugin
**Source:** `read_cursor_file` / `CursorCorrupt` (`cursor.rs`) and the `run_plugin` disable path (`scheduler.rs`).
**Apply to:** token file problems (D-02): the error names plugin and path, with no auto-reset or auto-chmod.

### Test layout
**Source:** `cursor.rs` tests; `testing.rs` (`TestHttpServer`, `FakeInstance`, `FakeFactory`). Tests sit in `#[cfg(test)] mod tests` beside the code. Use `tempfile::TempDir`, and assert error `to_string()` contains plugin and path.

### Lints
Clippy `all` + `pedantic` warn, `unsafe_code` warn; write no `unsafe`. Rustdoc `# Errors` sections on every `pub fn` returning `Result` (as in `cursor.rs`).

## No Analog Found

| File | Role | Data Flow | Reason |
|---|---|---|---|
| `src/plugins/auth/pkce.rs` | utility | transform | No PKCE/crypto code exists. Use RESEARCH "PKCE and state" snippet (`getrandom::fill`, `URL_SAFE_NO_PAD`, `Sha256`) plus the RFC 7636 App. B vector test. `base64` is already used in `cursor.rs` (`STANDARD`); this needs `URL_SAFE_NO_PAD`. |
| `src/plugins/auth/callback.rs` | service | one-shot loopback server | No production HTTP server exists. Use RESEARCH Pattern 4 (tokio `TcpListener` on `127.0.0.1`, 8 KiB cap, per-connection read timeout, static page). `TestHttpServer` (`testing.rs:203-345`) shows raw-socket request parsing you can invert. |

## Dependencies to add (RESEARCH)

`cargo add sha2@0.10 getrandom@0.3 open@5`. `reqwest`, `url`, `base64`, `tempfile`, `chrono`, `serde_json` and `tokio` are already present.

## Metadata

**Analog search scope:** `src/plugins/**`, `src/commands/**`, `src/cli.rs`, `src/main.rs`
**Files read:** cursor.rs, instance.rs, scheduler.rs, once.rs, testing.rs, config.rs, error.rs, commands/plugins.rs, cli.rs, main.rs (targeted ranges)
**Pattern extraction date:** 2026-11-11
