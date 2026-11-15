# Phase 6: Host-Managed OAuth - Research

**Researched:** 2026-11-11
**Domain:** OAuth 2.0 authorization-code + PKCE (RFC 7636) loopback login, host-side token storage and refresh, in a Rust 2024 / tokio / wasmtime-component host
**Confidence:** HIGH on codebase integration points and protocol mechanics; MEDIUM on provider quirks (Reddit/Google, from docs only, nothing run live)

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

#### Token storage (AUTH-02, AUTH-04)
- **D-01:** Tokens are stored as **plain JSON, mode 0600, one file per plugin alias** under the state directory (`<state-dir>/auth/<alias>.json`, default `.fili-state/` per Phase 3 D-06, same `--state-dir` / `FILI_STATE_DIR` override). Written atomically (temp file + rename), like Phase 3 cursor files. — **Reversibility:** costly — the on-disk token file layout is what existing logins depend on; changing it forces every user to log in again.
- **D-02:** A token file that is unreadable, corrupt, or has permissions looser than 0600 **disables only that plugin**, with an error naming the plugin and file path. Other plugins keep polling. Mirrors Phase 3 D-08. The host never silently treats it as logged out and never auto-chmods.
- **D-03:** `fili auth logout <plugin>` always deletes the local token file. If the plugin's `[host.oauth]` declares `revocation_url`, it also attempts a best-effort revoke at the provider and **warns** on failure (logout still succeeds). After logout, later polls run with `access-token = none`.

#### Login flow (AUTH-01)
- **D-04:** The callback listener binds **`127.0.0.1` on a fixed port**, default from `fili` and overridable per plugin via `redirect_port` in `[host.oauth]` (providers like Reddit require a pre-registered exact redirect URI).
- **D-05:** Login **prints the authorize URL and also tries to open the default browser**; `--no-browser` suppresses the launch. Works over SSH/headless.
- **D-06:** The login waits **5 minutes** for the callback, then fails with a clear error and a non-zero exit. The `state` parameter is verified; a mismatched, denied, or error callback is rejected with a named error. The callback page is a plain "you can close this tab" page. Nothing is written on failure. **Codes, tokens, and verifiers are never printed or logged.**

#### Refresh and failure policy (AUTH-02, AUTH-03)
- **D-07:** Refresh is **lazy, just before each poll**: if the access token has less than **60 seconds** left, refresh first. No background timer.
- **D-08:** The host **re-reads the token file at each poll**, so a `fili auth login` in another terminal takes effect without restarting `fili` (this is the "Phase 6 can fix it live" assumption in Phase 3 D-03). Refresh is serialised with **one in-flight refresh per plugin** and the new tokens written atomically, so refresh-token rotation cannot lose the new refresh token.
- **D-09:** On a **permanent** refresh failure (`invalid_grant`, revoked or expired refresh token), the host keeps the file, marks it `needs_login`, and polls with `access-token = none`. The plugin returns `auth-required` and Phase 3 backoff applies (D-02/D-03 there). `fili auth status` shows the `needs_login` state and says to re-run login. **Transient** refresh failures (network, 5xx) retry under the same backoff and do not mark `needs_login`.
- **D-10:** The WIT contract is **unchanged**: the token reaches the plugin as `poll`'s `access-token: option<string>` parameter (Phase 1 D-07), not a new import. The roadmap's "WIT token import" wording is read as this parameter. The refresh token never crosses into the plugin. A recording test component proves it (SC3).

#### Provider config in plugin TOML (AUTH-01)
- **D-11:** OAuth is declared in a **`[host.oauth]` table** in the plugin TOML, beside the other host-owned config (Phase 1 D-11). Keys: `authorize_url`, `token_url`, `scopes = [...]`, `client_id`, optional `client_secret_env`, optional `revocation_url`, optional `redirect_port`. No table means the plugin has no auth. It is not placed under `[settings]`, which is passed raw to the plugin. — **Reversibility:** costly — published plugin TOML shape that authors write against.
- **D-12:** `client_id` is public and lives in the TOML. `client_secret_env` **names a `.env` key** resolved with the existing Phase 1 secret rules (per-plugin only, redacted, never in Debug or logs) and **is never passed to the plugin**.
- **D-13:** `authorize_url`, `token_url`, and `revocation_url` **must be `https://`**. Only the loopback callback may be plain http. Token and revoke calls use the host's own TLS-validating client, **outside the plugin fetch allowlist**, so plugin allowlists need not list login hosts. Config load errors name plugin + field (Phase 1 CONF-03 style).

### Claude's Discretion (not discussed)
- PKCE details: S256 challenge, verifier length, `state` generation (use a CSPRNG).
- Crate choices (HTTP callback server, base64/sha2, browser opener) and sync vs async.
- Token file schema (field names, `expires_at` as absolute time, `needs_login` flag) and where in `src/plugins/host/` the token provider plugs in.
- `fili auth status` output layout (one row per plugin: login state, expiry, no token values).
- Behaviour when a token expires while the host is offline (falls under D-07/D-09: refresh on next poll).
- How `auth-required` from a plugin with no stored token is surfaced to the user (point at `fili auth login <plugin>`).

### Deferred Ideas (OUT OF SCOPE)
- OS keychain / encrypted token storage — revisit if plain 0600 files prove insufficient
- Multiple accounts per plugin — not in requirements
- Background proactive refresh timer — only if long poll intervals cause expiry problems
- Whether Reddit RDDT-01 uses app-only credentials or this PKCE login — Phase 7 research
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| AUTH-01 | `fili auth login <plugin>`: OAuth 2.0 PKCE via local callback, endpoints and scopes from the plugin TOML | PKCE/S256 verified against RFC 7636 App. B vector; loopback rules from RFC 8252 §7.3/§8.3; `[host.oauth]` parse/validation plugs into `config.rs` (§Architecture, Pattern 1, 3); callback listener Pattern 4; Reddit/Google authorize quirks (Pitfall 9, Open Question 1) |
| AUTH-02 | Host stores tokens locally and refreshes before expiry | Token file schema + atomic 0600 write (Pattern 2); lazy refresh with 60 s margin, cross-process lock, refresh-response-without-refresh-token handling (Pattern 5, Pitfalls 2, 3, 4) |
| AUTH-03 | Plugins receive only a current access token, never refresh tokens | Change `PluginInstance::poll` to carry `access_token: Option<&str>` (Pattern 6); client secret kept out of `ResolvedSecrets`; two-level SC3 test (Validation Architecture) |
| AUTH-04 | `fili auth status` / `fili auth logout <plugin>` inspect and revoke | Lock-free status reader like `read_status`; RFC 7009 best-effort revoke; CLI wiring (Architecture, Pattern 7) |
</phase_requirements>

## Project Constraints (from CLAUDE.md)

`.claude/CLAUDE.md` only imports `AGENTS.md` and `WORKFLOW.md`, the "GSD Path" pipeline rules. `.project/STATE.md` does not exist in this repo (checked: `ls .project` fails), so those pipeline rules are inactive; this repo plans through `.planning/`. The directives that still apply to code in this phase come from `.planning/codebase/CONVENTIONS.md` and `Cargo.toml`:

- Clippy `all` and `pedantic` are `warn`; `unsafe_code = "warn"`. Write no `unsafe`. [VERIFIED: Cargo.toml `[lints.clippy] all = "warn" pedantic = "warn"`, `[lints.rust] unsafe_code = "warn"`]
- Rust 2024 edition, `rust-version = "1.95"`, toolchain pinned to `1.96.0`. [VERIFIED: Cargo.toml, rust-toolchain.toml]
- Module header comment block with the `// ───` rule; section dividers `// ── Name ───`; imports ordered std, external, `crate::`. [CITED: .planning/codebase/CONVENTIONS.md]
- Tests live beside code in `#[cfg(test)] mod tests`; test runner is plain `cargo test`. [CITED: .planning/codebase/TESTING.md]
- Secrets never appear in Debug, logs or diagnostics (Phase 1 CONF-02, Phase 3 conventions).
- Never modify the `fluvio/` reference checkout; do not fix the known `fluvio-wise` gaps (STATE.md Blockers).

## Summary

The phase is almost entirely host-side Rust. The WIT contract does not change: `poll` already takes `access-token: option<string>` and the wasmtime call site passes a literal `None` today. The work is (1) a typed `[host.oauth]` table in `config.rs`, (2) a new `auth` module that owns the token file, PKCE, the one-shot loopback callback listener, and the token/refresh/revoke HTTP client, (3) a token provider the scheduler and `run-once` consult immediately before every `poll`, which requires adding an `access_token` parameter to the `PluginInstance::poll` trait (three implementors and two call sites), and (4) a `fili auth login | status | logout` command module.

Three design facts decide most of the risk. First, **the host's state-directory `.lock` is held exclusively by `fili run`**, so `fili auth login/logout` in a second terminal (the whole point of D-08) must never take that lock; they need their own per-alias advisory lock inside `<state>/auth/` plus atomic rename, and the refresh path must re-read the file after acquiring that lock so a concurrent login or a concurrent `run-once` refresh is never overwritten. Second, **`reqwest::blocking` panics inside a tokio runtime** and `fili` runs entirely inside `#[tokio::main]`, so every host-side HTTP call (token, refresh, revoke) must use the async `reqwest::Client` (CONTEXT's "blocking" note must not be followed literally). Third, **`expires_at` is wall-clock time, which tokio's paused clock does not move**, so deterministic expiry tests need an injectable clock rather than `start_paused` alone.

One gap in the locked config shape needs a user decision before planning is final: D-11's key list has no way to add provider-specific authorize parameters. Reddit only issues a refresh token when the authorize request carries `duration=permanent`; without it the host cannot satisfy AUTH-02 for Reddit. An optional `authorize_params` table is an additive, low-risk fix (see Open Question 1).

**Primary recommendation:** Build a new `src/plugins/auth/` module (store, pkce, callback, client, provider) with a hand-written thin OAuth client on the already-present `reqwest` async client plus `sha2`, `getrandom` and `base64`; inject a `TokenProvider` through `PluginSpec`; change `PluginInstance::poll` to take `access_token: Option<&str>`; and prove SC3 twice (a fast fake-instance recording test in the default suite, and an ignored real-wasm recording test through `stub-source`).

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Declare provider endpoints/scopes/client id | Host config (`config.rs`, plugin TOML `[host.oauth]`) | — | Host-owned config sits beside `[host]`; never in `[settings]`, which the plugin parses |
| PKCE, state, authorize URL, code exchange | Host (`auth` module, `fili auth login`) | Browser (user consent only) | The plugin never takes part; it is a one-shot CLI flow |
| Loopback callback receipt | Host CLI process (tokio `TcpListener` on `127.0.0.1`) | Browser (redirect) | Redirect lands on the user's machine; listener is open only during login |
| Token persistence | Host filesystem (`<state>/auth/<alias>.json`, 0600) | — | D-01; tokens never leave the host process except the access token |
| Refresh before expiry | Host scheduler path (lazy, pre-poll) | Provider token endpoint | D-07; no background timer |
| Handing the token to the plugin | Host wasmtime call boundary (`poll` parameter) | WIT contract (unchanged) | Only `access-token: option<string>` can cross; refresh token has no channel |
| Revocation on logout | Host CLI (`fili auth logout`) | Provider revocation endpoint (best effort) | D-03 |
| Status display | Host CLI, lock-free file read | — | Must work while `fili run` holds the state lock |
| Token/revoke TLS | Host `reqwest` client (rustls, validating) | — | D-13; outside the plugin fetch allowlist |

## Standard Stack

### Core

| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| reqwest (already a dep) | 0.12.28 locked, features `rustls-tls` (`blocking` also on) | Async token / refresh / revoke POSTs | Already in the tree; `RequestBuilder::form` and `basic_auth` exist without the `json` feature. [VERIFIED: reqwest-0.12.28/src/async_impl/request.rs lines 260, 414; `json` is `#[cfg(feature = "json")]` at line 445 and is not enabled in Cargo.toml] |
| url (already a dep) | 2.5.8 locked | Build the authorize URL with `query_pairs_mut`, parse callback query | Correct percent-encoding for scopes with spaces. [VERIFIED: Cargo.lock, Cargo.toml `url = "2"`] |
| serde / serde_json (already deps) | 1 | Token file and token-response (de)serialisation | Parse response with `serde_json::from_slice` because reqwest `json` feature is off |
| base64 (already a dep) | 0.22 | `URL_SAFE_NO_PAD` for verifier, challenge, state | Already used by `cursor.rs` with `STANDARD`. [VERIFIED: Cargo.toml `base64 = "0.22"`, cursor.rs imports `base64::engine::general_purpose::STANDARD`] |
| tempfile (already a dep) | 3.23.0 locked | Atomic temp-then-rename token write | `NamedTempFile::new_in` creates the file mode `0o600` on Unix. [VERIFIED: tempfile-3.23.0/src/file/imp/unix.rs line 24 `open_options.mode(permissions.map(\|p\| p.mode()).unwrap_or(0o600));`; Cargo.lock `tempfile` `3.23.0`] |
| chrono (already a dep) | 0.4 | RFC 3339 rendering of expiry in `status` | Already used for `PollStatus::now` |
| tokio (already a dep) | 1, `full`; dev adds `test-util` | `TcpListener`, `timeout`, `spawn_blocking`, tests | Callback server and the 5-minute wait |
| sha2 | `0.10` (0.10.9 already in Cargo.lock; 0.11.1 is latest) | SHA-256 for the S256 challenge | RustCrypto; never hand-roll. Adding `sha2 = "0.10"` adds no new compile unit. [VERIFIED: Cargo.lock; crates.io via `cargo search`] |
| getrandom | `0.3` (0.3.3 in Cargo.lock; 0.4.3 is latest) | CSPRNG bytes for verifier and `state` | Direct OS entropy, `getrandom::fill(&mut [u8]) -> Result<(), Error>`. [VERIFIED: getrandom-0.3.3/src/lib.rs line 66 `pub fn fill(dest: &mut [u8]) -> Result<(), Error>`; same signature in 0.4.3 line 87] |
| open | `5` (5.4.5 on crates.io) | Best-effort default-browser launch | Maintained cross-platform opener; docs say its functions are non-blocking, with `that_detached` for launchers that may block. [CITED: docs.rs/open/latest/open] |

### Supporting

| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| tokio `test-util` (dev, already on) | — | `start_paused` for backoff-timing tests | Scheduler-level tests only; see Pitfall 6 for why not for wall-clock expiry or real sockets |
| `TestHttpServer` (in-repo, `host/testing.rs`) | — | Fake token/revoke endpoint on loopback | Unit tests of the client and provider. [VERIFIED: src/plugins/host/testing.rs lines 203-345 read this session] |

### Alternatives Considered

| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Hand-written thin client (3 form POSTs) | `oauth2` 5.0.0 | Standard crate, but it pulls `rand 0.8`, `getrandom 0.2`, `thiserror 1`, `serde_path_to_error` (duplicates of what the tree already has), its typestate client makes runtime-optional endpoints (`revocation_url`) awkward, and provider quirks (Reddit Basic auth with empty secret, refresh responses without `refresh_token`, `expires_in` as string) need escape hatches anyway. [VERIFIED: crates.io dependency list for oauth2 5.0.0] |
| tokio `TcpListener` callback (about 80 lines) | `tiny_http` 0.12.0 | `tiny_http` is sync and adds a dep; the flow needs exactly one GET with strict size and time caps, and the tokio version composes with `tokio::time::timeout` for D-06's 5 minutes |
| `open` | `webbrowser` 1.2.6, or shelling out to `open`/`xdg-open` | Equivalent; `open` chosen for `that_detached` and its documented Windows non-`cmd` default |
| Task-level `Arc<TokenProvider>` in `PluginSpec` | Extra parameter on `run_plugin`/`supervise` | A field on `PluginSpec` avoids touching every `run_plugin(spec, factory, sink, cursors, shutdown)` test call. `PluginSpec` is built literally in only `testing.rs::spec` and `PluginSpec::from_loaded`. [VERIFIED: grep `PluginSpec {` in src] |

**Installation:**
```bash
cargo add sha2@0.10 getrandom@0.3 open@5
```

**Version verification:** `cargo search` (2026-11-11) returned `open = "5.4.5"`, `sha2 = "0.11.1"`, `getrandom = "0.4.3"`, `oauth2 = "5.0.0"`, `tiny_http = "0.12.0"`, `webbrowser = "1.2.6"`. `sha2 0.10.9` and `getrandom 0.3.3` are the versions already resolved in `Cargo.lock`. Choosing the locked majors is a convenience, not a requirement; the RFC 7636 vector test (below) validates whichever is chosen.

## Package Legitimacy Audit

| Package | Registry | Age | Downloads | Source Repo | Verdict | Disposition |
|---------|----------|-----|-----------|-------------|---------|-------------|
| open | crates.io | since 2015-02 | ~1.86M/wk | github.com/Byron/open-rs | OK | Approved (new dep) |
| sha2 | crates.io | since 2016-05 | ~28.8M/wk | github.com/RustCrypto/hashes | OK | Approved (already transitive) |
| getrandom | crates.io | long-standing | seam run covered it | github.com/rust-random/getrandom | OK (see note) | Approved (already transitive) |
| oauth2 | crates.io | since 2014-12 | ~1.48M/wk | github.com/ramosbugs/oauth2-rs | OK | Not used (alternative only) |
| tiny_http | crates.io | since 2015-05 | ~1.45M/wk | github.com/tiny-http/tiny-http | OK | Not used |
| webbrowser | crates.io | since 2015-12 | ~1.49M/wk | github.com/amodm/webbrowser-rs | OK | Not used |

Source: `gsd-tools query package-legitimacy check --ecosystem crates oauth2 open sha2 rand tiny_http webbrowser getrandom url base64 secrecy zeroize`, run this session; every package returned `OK` (output for `getrandom`, `url`, `base64`, `secrecy`, `zeroize` was cut off by `head`; `getrandom`, `url`, `base64` are already in `Cargo.lock`). `[VERIFIED: gsd-tools package-legitimacy + cargo search]` for existence and legitimacy; the choice of `open` as the browser opener rests on training knowledge plus its docs.rs page, so the planner need not add a human-verify checkpoint (verdict is `OK`, not `SUS`) but should run `cargo add` and confirm `Cargo.lock` gains only `open` and its own small dependency set.

**Packages removed due to [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

## Architecture Patterns

### System Architecture Diagram

```
 LOGIN (one-shot CLI, own process; never takes the state-dir .lock)

 fili auth login <plugin>
   |  load_plugin(registry, name)  -> [host.oauth] + client secret (Secret, redacted)
   v
 gen verifier(32 random bytes) -> challenge = b64url(SHA256(verifier)); gen state
   |
   v
 bind 127.0.0.1:<redirect_port>  (fail early: port-in-use error names key `redirect_port`)
   |
   +--> print authorize URL ----------------------------> user's browser --> provider consent
   +--> open::that_detached(url)  (skipped with --no-browser; failure = warning only)
   |                                                                  |
   |            GET http://127.0.0.1:<port>/callback?code=..&state=..  |
   v  <---------------------------------------------------------------+
 accept loop (<= 5 min, D-06): non-/callback paths -> 404, keep waiting
   | /callback: error= -> Denied;  state != expected -> StateMismatch;  code -> serve static page
   v
 POST token_url  (grant_type=authorization_code, code, redirect_uri, code_verifier,
                  client_id [+ Basic auth if secret])   async reqwest, TLS, timeout, no redirects
   v
 lock auth/<alias>.lock -> atomic write auth/<alias>.json (0600, needs_login=false)

 POLL PATH (inside fili run / plugins run-once; per plugin task, sequential)

 scheduler loop: before every instance.poll(...)
   |
   v
 TokenProvider::access_token(plugin)  -- no [host.oauth]? -> None
   |  re-read auth/<alias>.json (D-08)
   |    missing                     -> None            (plugin answers auth-required -> Phase 3 backoff, hint to log in)
   |    unreadable/corrupt/perm>0600-> Disable plugin  (D-02, names plugin + path)
   |    needs_login                 -> None
   |    >= 60 s left                -> Some(access_token)
   |    < 60 s left or expired      -> lock, RE-READ, still stale? POST refresh_token grant
   |         ok          -> write atomically (keep old refresh_token if response omits it) -> Some(new)
   |         permanent   -> set needs_login=true, keep file -> None            (D-09)
   |         transient   -> token still valid? Some(old) : backoff outcome      (D-09)
   v
 instance.poll(cursor, access_token.as_deref())  --> wasm guest sees ONLY the access token
```

### Recommended Project Structure

```
src/plugins/
├── config.rs            # + OAuthSection (serde), OAuthConfig (public), validation, client-secret resolve
├── error.rs             # + ConfigError variants for oauth secret problems (see Pattern 1)
├── auth/                # NEW
│   ├── mod.rs           # module docs, pub re-exports, DEFAULT_REDIRECT_PORT
│   ├── error.rs         # AuthError (miette Diagnostic, codes fili::auth::*)
│   ├── store.rs         # token file schema, read/write/mark_needs_login/delete, 0600 check, per-alias lock
│   ├── pkce.rs          # verifier, challenge, state; RFC 7636 vector test
│   ├── callback.rs      # one-shot loopback listener (tokio), request-line parse, static page
│   ├── client.rs        # exchange_code / refresh / revoke, response + error parsing, classification
│   └── provider.rs      # TokenProvider: the pre-poll decision table, injectable clock
├── host/
│   ├── instance.rs      # PluginInstance::poll gains access_token; WasmtimeInstance passes it
│   ├── scheduler.rs     # PluginSpec gains `auth`; loop consults provider; CycleOutcome::TokenRefresh
│   ├── once.rs          # poll_once uses the provider too
│   └── testing.rs       # FakeInstance records (settings, secrets, cursor, token)
└── ...
src/commands/auth.rs     # NEW: login | status | logout (dispatch like plugins.rs)
src/cli.rs               # Command::Auth { command: AuthCommand }
src/main.rs              # dispatch + shows_banner must also exclude Command::Auth
plugins/stub-source/     # optional `record_inputs` setting for the real-wasm SC3 test
plugins/README.md        # "OAuth" section documenting [host.oauth]
```

### Pattern 1: `[host.oauth]` in the existing typed `[host]` table

**What:** Add `oauth: Option<OAuthSection>` to `HostSection` (which is `#[serde(deny_unknown_fields)]`, so the table must be declared or it is rejected). Keep spans (`toml::Spanned`) on `authorize_url`, `token_url`, `revocation_url`, and `client_secret_env` so diagnostics label the exact value. Range/URL checks go in `host_problems`-style code and emit the existing `ConfigError::InvalidValue` with `field: "host.oauth.token_url"` etc. Expose a public `OAuthConfig` (resolved URLs as `url::Url`, `scopes: Vec<String>`, `client_id: String`, `client_secret: Option<Secret>`, `revocation_url: Option<Url>`, `redirect_port: u16`) on `PluginConfig` (a new field `oauth: Option<OAuthConfig>`), **not** inside `HostConfig` (whose literal is built in four other places) and **not** inside `ResolvedSecrets` (which `expose_secrets` hands to the guest).

Verified existing shapes to extend:
- `HostSection` is declared `#[derive(Debug, Deserialize)] #[serde(deny_unknown_fields)] struct HostSection { poll_interval_secs: NonZeroU64, ... secrets: BTreeMap<String, toml::Spanned<String>>, }` [VERIFIED: src/plugins/config.rs:109-123].
- `ConfigError::InvalidValue { plugin, path, field, message, src: NamedSource<String>, span: Option<SourceSpan> }` displays as ``plugin `{plugin}` (`{path}`): `{field}`: {message}`` [VERIFIED: src/plugins/error.rs `InvalidValue` variant].
- `PluginConfig` has exactly the fields `host`, `settings_toml`, `secrets`, and is built literally at `config.rs:590` (`finish_plugin`) and in a `secrets.rs` test (~line 637) [VERIFIED: grep `PluginConfig {`].

**Two non-obvious traps in the existing loader:**
1. Pass 2 reads `.env` only `if parsed.values().any(|plugin| !plugin.aliases.is_empty())` [VERIFIED: config.rs pass-2 block]. A plugin whose only secret is `client_secret_env` has an empty `aliases` map, so `.env` would never be read and the secret would look missing. Extend the condition to include `oauth.client_secret_env.is_some()`.
2. `resolve_secrets` produces `MissingSecret`/`EmptySecret` whose text is ``"`host.secrets.{alias}` needs env var ..."`` [VERIFIED: error.rs]. Feeding a synthetic alias would print a wrong field path. Add dedicated variants (for example `MissingOAuthSecret`, `EmptyOAuthSecret`, naming `host.oauth.client_secret_env`) and add them to `ConfigError::plugin()` so a scoped `load_plugin` keeps the correct plugin. Reuse the same lookup order (process env wins even when empty, then `.env`; D-12 of Phase 1).

**Validation rules (D-13):** parse each URL with `url::Url`; require scheme `https` (a case-insensitive `http` is rejected); require a host; reject userinfo. Reject an empty `scopes` list? Not mandated; leave allowed (some providers need none) but reject empty strings. `redirect_port` is `u16`, non-zero. `client_id` non-empty.

### Pattern 2: Token file (`<state>/auth/<alias>.json`)

**What:** Mirror `StateFile` in `cursor.rs`: `version`, `plugin` (verified equals the alias on read), tolerant of unknown fields. Absolute `expires_at` as Unix seconds. Recommended schema:

```json
{"version":1,"plugin":"reddit","access_token":"...","refresh_token":"..."|null,
 "expires_at":1789000000,"token_type":"Bearer","scope":"read identity","needs_login":false,
 "obtained_at":1788996400}
```

- Define `AccessToken`/`RefreshToken` as a small redacting newtype (`Debug` prints `***`, no `Display`) with `Serialize`/`Deserialize`; the existing `Secret` has no `Serialize` by design [VERIFIED: secrets.rs `Secret` doc "It has no `Display` and no `Serialize`"] so do not reuse it for file I/O.
- Write path: `NamedTempFile::new_in(<state>/auth)` (mode 0600 already), `serde_json::to_writer`, `sync_all`, `persist`, then best-effort directory sync, exactly as `CursorStore::write` does [VERIFIED: cursor.rs `write`]. Create `<state>/auth` with `DirBuilder::mode(0o700)` on Unix.
- Read path checks, in order: `symlink_metadata` is a regular file (refuse symlinks); on Unix `mode & 0o077 != 0` -> permission error naming the file and the fix (`chmod 600 <path>`) but never chmod it (D-02); size cap (for example 64 KiB, like `MAX_STATUS_BYTES` [VERIFIED: cursor.rs `MAX_STATUS_BYTES: u64 = 64 * 1024`]); JSON parse; `version <= 1`; `plugin == alias`. All failures map to one `AuthError::TokenFile { plugin, path, reason }` (reason fixed text, never file content).
- Alias safety: aliases come only from the loaded registry and are limited to `[a-z0-9_-]` [VERIFIED: cursor.rs `path_for` doc: "Aliases come only from the loaded registry, which limits them to `[a-z0-9_-]`"], so `auth/<alias>.json` cannot traverse. Still call the same `is_valid_name` rule in `fili auth login <plugin>` before building a path, because `status`/`logout` take a free-form CLI argument.

### Pattern 3: PKCE + state

**What:** Verifier = 32 bytes from `getrandom::fill`, `URL_SAFE_NO_PAD` (43 chars, inside RFC 7636's 43-128 range). Challenge = `URL_SAFE_NO_PAD(Sha256::digest(verifier.as_bytes()))`, method `S256`. State = 32 more random bytes the same way. Authorize URL via `Url::query_pairs_mut()`: `response_type=code`, `client_id`, `redirect_uri=http://127.0.0.1:<port>/callback`, `scope` (space-joined), `state`, `code_challenge`, `code_challenge_method=S256`, then any provider extras (Open Question 1).

The RFC 7636 Appendix B vector is the unit test: verifier `dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk` must give challenge `E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM`. [VERIFIED: computed in this session with `printf '<verifier>' | openssl dgst -sha256 -binary | openssl base64 -A | tr '+/' '-_' | tr -d '='` which printed `E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM`]

### Pattern 4: One-shot loopback callback (tokio, no HTTP framework)

**What:** `TcpListener::bind(("127.0.0.1", port))` (never `localhost`, never `0.0.0.0`; RFC 8252 §8.3 says `localhost` is NOT RECOMMENDED and to listen only on loopback and only for the duration of the request) [CITED: datatracker.ietf.org/doc/html/rfc8252 §7.3, §8.3]. Bind **before** printing the URL so a port conflict fails fast with an error that names `host.oauth.redirect_port`. Loop on `accept` inside `tokio::time::timeout(LOGIN_TIMEOUT)` (5 min). Per connection: read at most 8 KiB or until `\r\n\r\n`, with a short per-connection read timeout (a silent client must not stall the loop); parse only the request line; require `GET`; split target at `?`; `Url::parse("http://127.0.0.1")?.join(target)` or `form_urlencoded::parse` for the query. Rules:
- path != `/callback` (favicon, probes): reply `404`, keep waiting.
- `/callback` with `error=...`: reply the static page, return `AuthError::Denied(<cleaned error>)` (`access_denied` etc.).
- `/callback` with `state` absent or != expected: reply the static page, return `AuthError::StateMismatch` (D-06 says mismatch is rejected with a named error).
- `/callback` with matching `state` and `code`: reply the static page, return the code.
- The page is a fixed byte string; it must not echo any query parameter (no reflected XSS). `Connection: close`, `Cache-Control: no-store`.
- Response write failure must not fail the login once the code is in hand.

### Pattern 5: Token client and refresh classification

**What:** One shared `reqwest::Client` built once per login/provider: `timeout(30s)`, `connect_timeout(10s)`, `redirect(Policy::none())` (a token endpoint that 307-redirects a POST must not get the client secret re-sent), `user_agent(concat!("fili/", env!("CARGO_PKG_VERSION")))` (Reddit requires a unique User-Agent; its token docs page does not say so but its API rules do, see Open Question 3), rustls default roots, TLS validation left at its default [VERIFIED: fetch.rs builds `reqwest::Client::builder().redirect(reqwest::redirect::Policy::none())` with validation default, `Egress` doc "certificate validation is the client's default (on)"]. Always send `Accept: application/json`.

Client authentication: confidential client (secret present) uses HTTP Basic (`basic_auth(client_id, Some(secret))`); public client sends `client_id` in the form body. Reddit's token endpoint requires Basic with the client id as username and, for installed apps, an empty password [CITED: github.com/reddit-archive/reddit/wiki/OAuth2], so a Reddit installed-app config (no `client_secret_env`) would need Basic with an empty password, while sending Basic with an empty password could break providers that expect a body `client_id` from public clients. Recommendation (Open Question 2): default to body `client_id` for public clients and add an optional `token_auth = "basic" | "body"` only if a Phase 7 provider needs the other mode.

Response parsing (do not use `.json()`):
- `access_token` required, non-empty. `token_type` optional, not validated. `expires_in` accepts a JSON number or numeric string; missing -> assume 3600 s and log once (assumption, A3). `scope` optional. `refresh_token` optional.
- On a refresh response with **no** `refresh_token`, keep the stored one. Reddit's docs state the refresh response omits `refresh_token` and Google's samples show the same [CITED: reddit wiki; developers.google.com/identity/protocols/oauth2/native-app]. On a refresh response **with** a new one (rotation), replace it, and write before returning the token.
- Cap the body read (for example 64 KiB) before parsing.

Classification (the D-09 split):
| Condition | Class |
|-----------|-------|
| HTTP 400/401 with JSON `error = "invalid_grant"` | **Permanent** -> `needs_login` |
| connect/DNS/TLS/timeout error, HTTP 5xx, 408, 429, `error = "temporarily_unavailable"` or `"server_error"`, body not parseable on a 5xx | **Transient** -> backoff, never `needs_login` |
| other 4xx OAuth errors (`invalid_client`, `unauthorized_client`, `invalid_request`, `unsupported_grant_type`, `invalid_scope`) | **Rejected**: not a user-login problem; treat like transient (backoff) but carry the OAuth `error` code in the message so the operator sees it; do not set `needs_login` (re-running login will not fix a bad client id). This is a recommendation, flagged A4. |

Error text shown or logged is the OAuth `error` code plus `error_description` passed through `clean_guest_text` (it is attacker-influenced text from the network: control characters and terminal escapes). Never log or print the response body wholesale.

### Pattern 6: Provider seam and the `PluginInstance::poll` signature change

**What (verified current shapes):**
- Trait today: `fn poll(&mut self, cursor: Option<&[u8]>) -> impl Future<Output = CallOutcome<PollResponse>> + Send;` [VERIFIED: src/plugins/host/instance.rs:67-70].
- Real call: `classify(self.source.call_poll(&mut self.store, cursor, None).await)` with the comment `// The access token is always none until Phase 6.` [VERIFIED: instance.rs:139-142].
- Other implementors/callers that must change in the same commit: `FakeInstance` (`host/testing.rs:559`), `ProbeInstance` (`commands/plugins.rs:2338`), call sites `scheduler.rs:433` (`instance.poll(cursor.as_deref())`) and `once.rs:70` (`instance.poll(cursor)`) [VERIFIED: grep `impl PluginInstance` / `.poll(`].
- Change the trait to `poll(&mut self, cursor: Option<&[u8]>, access_token: Option<&str>)`; `WasmtimeInstance::poll` passes `access_token` straight into `call_poll`. The generated bindgen method takes the same `Option<&str>` shape the call site already supplies as `None`.

`PluginSpec` currently holds `name`, `host`, `settings_toml`, `secrets` and derives `Debug` [VERIFIED: scheduler.rs:37-42]. Add `auth: Option<Arc<TokenProvider>>`; `TokenProvider` must have a redacting `Debug` (or `#[derive(Debug)]` over fields that are all redacting). `PluginSpec::from_loaded` builds it when `plugin.oauth.is_some()`. `testing.rs::spec` sets `auth: None`.

Add a `CycleOutcome` variant for a transient pre-poll refresh failure (for example `TokenRefresh(String)`), treated by `next_action` exactly like `Transient`: `(step, false)`, keep the instance (the instance is fine; the failure is host-side). Current `CycleOutcome` variants are `PollOk`, `Guest { phase, error }`, `InstanceFatal(String)`, `BuildFailed(String)`, `SinkFailed(String)` [VERIFIED: scheduler.rs:80-92] and `next_action` matches them exhaustively, so the compiler will force every site. A corrupt/loose-permission token file returns `Disable { reason }` (reuse the cursor-read-failure path in `run_plugin`, which calls `record_status_text` then `disable`) [VERIFIED: scheduler.rs `run_plugin` cursor read block].

Provider decision table (returns a small enum, never the refresh token):
`NoAuthConfigured | NotLoggedIn | NeedsLogin | Token(AccessToken) | RefreshFailedTransient{valid_token: Option<AccessToken>, text} | FileProblem(AuthError)`.

Use a transient-refresh-failure shortcut: if the refresh fails transiently but the access token still has time left, hand the existing token to the plugin rather than skipping the poll.

When the plugin answers `auth-required` and the provider reported `NotLoggedIn`/`NeedsLogin`, log one `warn!` per transition that names the plugin and says `run: fili auth login <plugin>` (the Discretion item about surfacing `auth-required`). `PollError::AuthRequired` is the variant [VERIFIED: scheduler.rs `PollError::Transient(_) | PollError::AuthRequired => (step, discard)`].

### Pattern 7: CLI wiring

Follow `plugins.rs`: `Command::Auth { command: AuthCommand }` in `cli.rs` [VERIFIED: `Command::Plugins { #[command(subcommand)] command: PluginsCommand }` at cli.rs:131-134], `commands::auth::dispatch(&command, &cli.registry, cli.state_dir.as_deref())` in `main.rs`, and add `Command::Auth { .. }` to `shows_banner`'s `matches!` so stdout stays clean [VERIFIED: main.rs `!matches!(command, Command::Plugins { .. } | Command::Run)`]. Reuse `state_dir_for(&listing.base_dir, flag)` (`pub(crate)` in `commands/plugins.rs:248`) and `HostOptions::resolve_state_dir`. Subcommands:
- `login <plugin> [--no-browser]`: `load_plugin(registry, name)` (scoped; resolves only that plugin's secrets, same as `run-once`), requires `[host.oauth]` (else a clear "plugin has no [host.oauth]" error), then the Login flow above.
- `status [--json]`: one row per registered enabled source: `plugin`, `login` (`logged-in`, `needs-login`, `not-logged-in`, `no-auth`, `unreadable`), `expires` (relative and RFC 3339), never a token value. Reads lock-free; works while `fili run` is up. Do not require `.env`/secrets to be resolvable for `status` (a scoped load per plugin that tolerates failure as a row, or a light `[host.oauth]` presence probe).
- `logout <plugin>`: delete the file first (local logout always succeeds, D-03), then if `revocation_url` is set, best-effort `POST token=<refresh or access>&token_type_hint=...` (RFC 7009), warn on failure. Revoke the refresh token when present (Reddit and Google both revoke related access tokens with it) [CITED: reddit wiki; Google native-app doc]. Logout of a plugin with no file: succeed with a note.

### Anti-Patterns to Avoid
- **`reqwest::blocking` anywhere on these paths:** `fili` runs under `#[tokio::main]` [VERIFIED: main.rs `#[tokio::main] async fn main()`]; the blocking client panics when used inside a runtime. CONTEXT's "reqwest (rustls, blocking)" describes the Cargo feature set, not what to call.
- **Taking `CursorStore::open` (the `.lock`) in any `auth` subcommand:** `fili run` holds it for its whole life, so login/logout/status would fail with `StateDirLocked`. [VERIFIED: cursor.rs `CursorStore::open` takes `try_lock` on `.lock` and returns `StateDirLocked`]
- **Putting the client secret in `ResolvedSecrets`:** `expose_secrets` hands every entry to `init`; it would reach the guest. Keep it in `OAuthConfig`.
- **Logging a `reqwest::Error` for the authorize/callback step or printing `Url` values that contain `code`/`state`:** only the authorize URL (no secret) is printed; the callback URL and token request bodies are never logged.
- **Auto-chmod or auto-reset on a bad token file** (D-02 forbids it).
- **Holding the per-alias lock across the whole poll:** hold it only around read-check-refresh-write.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| SHA-256 | Own digest | `sha2` | Crypto primitive |
| Random verifier/state | `SystemTime`-seeded or `rand` `thread_rng` from memory | `getrandom::fill` | Must be a CSPRNG (RFC 7636 §7.1 entropy requirement) |
| URL-safe base64 | Manual `+/` replace | `base64::engine::general_purpose::URL_SAFE_NO_PAD` | Padding and alphabet bugs |
| URL building / query parsing | `format!` with manual escapes | `url::Url`, `query_pairs_mut`, `form_urlencoded` | Scope spaces, `&`, `=` in values |
| Atomic file write with 0600 | `File::create` then `chmod` | `tempfile::NamedTempFile::new_in` + `persist` | Mode is 0600 from creation (no window of looser perms); rename is atomic |
| Cross-process mutual exclusion | PID files, sleep loops on existence | `std::fs::File::try_lock` (advisory lock, already used by `CursorStore::open`) | Released automatically on process death |
| Terminal-safe error text | `{}` of provider strings | `clean_guest_text` (`host/engine.rs`) | Strips control characters/escapes, clips length. [VERIFIED: engine.rs `clean_guest_text` doc "terminal control characters are removed ... clipped to `max_chars`"] |
| Redacting secret type | Plain `String` | A newtype with `Debug` = `***`, no `Display` | Existing convention (`Secret`) |
| TLS validation | Custom verifier or `danger_accept_invalid_certs` | reqwest default rustls | D-13, FETCH-03 convention |

**Key insight:** the only genuinely custom protocol code is request/response shaping for three form POSTs and a 60-line callback parser; every security-relevant primitive (hash, RNG, URL encoding, TLS, atomic write, locking) already exists in the tree.

## Common Pitfalls

### Pitfall 1: `fili auth` colliding with `fili run`'s state lock
**What goes wrong:** `fili auth login` in a second terminal fails with "state directory ... is in use by another fili process".
**Why it happens:** `CursorStore::open` takes an exclusive lock on `<state>/.lock` for the lifetime of `fili run` and `run-once --produce`.
**How to avoid:** Auth code touches only `<state>/auth/`, guarded by per-alias `auth/<alias>.lock` and atomic rename; it never constructs `CursorStore`. The poll path reads the token file with no state lock held beyond what the host already has.
**Warning signs:** an `auth` test that opens a `CursorStore` first; any `StateDirLocked` in an auth integration test.

### Pitfall 2: Login and refresh overwriting each other
**What goes wrong:** the host starts a refresh with the old refresh token; meanwhile the user finishes `fili auth login`; the host's late write replaces the fresh login with the refreshed-old tokens (or, with rotation, invalidates the new login).
**How to avoid:** under the per-alias lock, **re-read the file after acquiring it** and re-evaluate (another process may have already refreshed or re-logged-in); login writes under the same lock. Optionally compare the refresh token that the refresh started from with the file's current one before persisting and discard on mismatch. Same lock covers `needs_login` marking and `logout` deletion.

### Pitfall 3: Losing the refresh token on a refresh response that omits it
**What goes wrong:** the file is rewritten with `refresh_token: null`, and the next refresh is impossible.
**How to avoid:** `new.refresh_token.or(old.refresh_token)`. Unit-test both branches (omitted, rotated) with the fake endpoint.

### Pitfall 4: Rotation and crash ordering
**What goes wrong:** with a provider that rotates, a crash between the network response and the file write loses the only valid refresh token.
**How to avoid:** write the new tokens atomically before using them or returning them; nothing may be done with the new access token until `persist` returned. There is no way to make network-then-disk fully transactional; the residual risk is an inherent property of rotating refresh tokens and should simply be documented.

### Pitfall 5: Blocking the async runtime
**What goes wrong:** `File::lock()` (blocking) or `reqwest::blocking` inside an async task stalls a tokio worker or panics.
**How to avoid:** acquire the lock with a `try_lock` + short `tokio::time::sleep` loop (or `spawn_blocking`), use async `reqwest::Client`. File reads/writes of a tiny JSON file are acceptable inline, matching `CursorStore::write` which is already called from async code.

### Pitfall 6: Test clocks (paused time does not move wall time; paused time + real sockets)
**What goes wrong:** (a) CONTEXT suggests "tokio paused time" for refresh/expiry tests, but `expires_at` is a Unix timestamp compared with `SystemTime`/`chrono::Utc::now()`, which `start_paused` does not advance; a token that "should have expired after `advance(1h)`" is still fresh. (b) With `start_paused = true`, tokio auto-advances the mocked clock whenever the runtime is otherwise idle, including while it waits on a real loopback socket, so a 5-minute login `timeout` can fire before the test's real HTTP request is served.
**How to avoid:** inject a clock (`Arc<dyn Fn() -> i64 + Send + Sync>` or a `Clock` trait) into `TokenProvider`; tests build `now = base + (tokio::time::Instant::now() - start).as_secs()` for scheduler-level tests, or a plain `AtomicI64` for pure provider tests. Test the callback's 5-minute timeout with a constructor-injected short `Duration` under real time (for example 100 ms), not paused time. Existing tests do combine `start_paused` with `TestHttpServer`, so keep paused time to tests whose only waits are backoff sleeps.

### Pitfall 7: `.env` not read for a client-secret-only plugin
See Pattern 1 trap 1. Add a test: a plugin with `[host.oauth] client_secret_env = "X"` and no `[host.secrets]`, with `X` in `.env`, loads and resolves.

### Pitfall 8: Backoff delays the effect of a fresh login
**What goes wrong:** `auth-required` advances the Phase 3 backoff (up to `max_secs`, default 3600 s), so after the user logs in from another terminal the running `fili` may not re-poll for up to an hour.
**Why it happens:** D-08 re-reads the file each poll, but there is no wake-up signal across processes.
**How to avoid:** accept and document it (the locked D-09/D-03 say backoff applies); state the expected delay in `fili auth login`'s success message ("a running `fili run` picks this up on its next poll"). Do not add cross-process signalling in this phase.

### Pitfall 9: Provider-specific authorize parameters
**What goes wrong:** Reddit issues a refresh token only if the authorize request includes `duration=permanent`; without it the login "works" but the token dies after about an hour and AUTH-02 silently fails. Google desktop flows are documented as returning refresh tokens for installed apps without extra params, but other providers (and Google web clients) need `access_type=offline`/`prompt=consent`. [CITED: reddit wiki; developers.google.com native-app]
**How to avoid:** see Open Question 1; also warn at login when the token response has no `refresh_token` ("this login will expire at <time> and cannot be refreshed").

### Pitfall 10: The example TOMLs and `.env.example` are test-pinned
**What goes wrong:** editing `examples/plugins/reddit.toml` or `youtube.toml` to add `[host.oauth]` breaks `sample_env_example_lists_every_declared_secret`, which asserts the `.env.example` key set equals the set of `[host.secrets]` env-var names, and `loads_sample_registry`, which asserts exact secret keys. [VERIFIED: src/plugins/mod.rs tests read this session; reddit.toml declares `client_id`/`client_secret` as `[host.secrets]` for the plugin]
**How to avoid:** do not change those samples in Phase 6 (Phase 7 owns the Reddit/YouTube configs and the app-only vs PKCE choice). Add a commented-out `[host.oauth]` example block and document the table in `plugins/README.md`; exercise the feature with temp-dir fixtures.

### Pitfall 11: Binary-level tests cannot reach a fake https token endpoint
D-13 forbids `http://` token URLs, so `tests/plugins_cli.rs`-style tests through the compiled binary cannot point at a plain-http loopback fake. Keep the validation in config loading only and give the client/provider constructors a `Url` that unit tests may set to `http://127.0.0.1:<port>`. Binary-level tests cover argument parsing, "plugin has no `[host.oauth]`", `status` output on prepared token files, and `logout` file deletion (with no `revocation_url`).

### Pitfall 12: Banner on stdout
`shows_banner` prints a decorative banner to stdout for every command not matched by `Plugins | Run`; `auth status --json` would be corrupted. Add `Command::Auth` to the exclusion.

## Code Examples

### PKCE and state (verified API shapes only)
```rust
// Source: RFC 7636 §4.1-4.2; getrandom 0.3 `fill`; base64 0.22 URL_SAFE_NO_PAD; sha2 0.10
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

fn random_token() -> Result<String, getrandom::Error> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes)) // 43 chars
}

fn s256_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

#[test]
fn rfc7636_appendix_b_vector() {
    assert_eq!(
        s256_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
}
```

### Authorize URL
```rust
// Source: url 2.x `Url::parse` + `query_pairs_mut`
let mut url = oauth.authorize_url.clone();
url.query_pairs_mut()
    .append_pair("response_type", "code")
    .append_pair("client_id", &oauth.client_id)
    .append_pair("redirect_uri", &redirect_uri) // "http://127.0.0.1:{port}/callback"
    .append_pair("scope", &oauth.scopes.join(" "))
    .append_pair("state", &state)
    .append_pair("code_challenge", &challenge)
    .append_pair("code_challenge_method", "S256");
```

### Token exchange (async, no `json` feature)
```rust
// Source: reqwest 0.12 RequestBuilder::form / basic_auth (both available without `json`)
let mut request = client
    .post(oauth.token_url.clone())
    .header(reqwest::header::ACCEPT, "application/json")
    .form(&[
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", &redirect_uri),
        ("code_verifier", &verifier),
    ]);
request = match &oauth.client_secret {
    Some(secret) => request.basic_auth(&oauth.client_id, Some(secret.expose())),
    None => request.form(&[("client_id", oauth.client_id.as_str())]), // see Open Question 2 for the exact shape
};
let response = request.send().await?;
let bytes = read_capped(response).await?;          // cap the body
let tokens: TokenResponse = serde_json::from_slice(&bytes)?;
```
(When both a form body and an extra form pair are needed, build one `Vec<(&str, &str)>` first; chaining `.form` twice overwrites the body.)

### Scheduler-side extension points (current code, quoted)
```rust
// src/plugins/host/instance.rs:67-70 (trait today)
fn poll(
    &mut self,
    cursor: Option<&[u8]>,
) -> impl Future<Output = CallOutcome<PollResponse>> + Send;

// src/plugins/host/instance.rs:139-142 (call site today)
async fn poll(&mut self, cursor: Option<&[u8]>) -> CallOutcome<PollResponse> {
    // The access token is always none until Phase 6.
    classify(self.source.call_poll(&mut self.store, cursor, None).await)
}
```

### SC3 fake-level recording test (default suite)
```rust
// FakeInstance (host/testing.rs) records every argument it receives.
// Sentinels are distinctive strings placed in .env / the token file:
//   refresh token  "RT-SENTINEL-7f3a", client secret "CS-SENTINEL-91bc"
// After running the plugin task for several polls (including one refresh):
let all = recorder.dump();   // settings, secrets, cursors, access tokens, as one String
assert!(all.contains("AT-fresh"));                 // the access token DID arrive
assert!(!all.contains("RT-SENTINEL-7f3a"));        // refresh token never arrived
assert!(!all.contains("CS-SENTINEL-91bc"));        // client secret never arrived
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Native apps embed a client secret / use embedded webviews | System browser + loopback redirect + PKCE (RFC 8252 + RFC 7636), S256 not `plain` | RFC 8252, 2017 | Matches this phase; Google: "Always use PKCE", S256 recommended, `plain` is default if method omitted so always send `code_challenge_method=S256` [CITED: developers.google.com native-app] |
| `localhost` redirect host | `127.0.0.1` literal | RFC 8252 §8.3 | `localhost` NOT RECOMMENDED [CITED: RFC 8252] |
| `oauth2` crate typestate for everything | thin client where provider quirks matter | — | Rationale for the hand-written client |

**Deprecated/outdated:** nothing in scope. Note Reddit's own docs do not mention PKCE; Reddit is expected to ignore an unknown `code_challenge`, which is harmless (A5, Phase 7 settles whether Reddit uses this flow at all).

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | A fixed default callback port below 32768 and off common dev-server ports (for example 17420) avoids OS ephemeral-range collisions; the exact number is a free choice | Pattern 4 / CLI | Low: a bind conflict is reported clearly and `redirect_port` overrides it |
| A2 | `open::that_detached` returns an error (or silently does nothing) on headless Linux instead of blocking or crashing | Pattern 4 | Low: the URL is always printed and the launch error is a warning |
| A3 | A token response with no `expires_in` should be treated as valid for 3600 s | Pattern 5 | Medium: a provider with shorter lifetimes and no `expires_in` would give the plugin an expired token once; the plugin returns `auth-required`/401 and the user re-logs in |
| A4 | Other 4xx OAuth errors (`invalid_client` and friends) should back off without marking `needs_login` | Pattern 5 | Low-medium: user-facing wording only; alternative is `needs_login` which would mislead for a bad client id |
| A5 | Reddit ignores an unknown `code_challenge` parameter at authorize and token time | Pitfall 9 / Open Q | Low for Phase 6 (generic flow); Phase 7 research verifies Reddit behaviour live |
| A6 | Google Desktop-type OAuth clients still require a `client_secret` in practice even though the docs list it as optional | Open Question 2 | Low: `client_secret_env` is optional in the schema, so both are supported |
| A7 | `fili plugins run-once` should use the token provider and may refresh and persist tokens (D-08's "re-read each poll" applies to it); CONTEXT does not mention `run-once` | Architecture | Medium: if run-once stayed token-less, Phase 7's `fili plugins run-once reddit` success criterion could not be met after login |
| A8 | Treating a symlinked token file as unreadable (refuse symlinks) is acceptable hardening beyond D-02 | Pattern 2 | Low |

## Open Questions

1. **Provider-specific authorize parameters are missing from D-11.**
   - What we know: Reddit returns a refresh token only when the authorize request has `duration=permanent` [CITED: reddit wiki OAuth2]. D-11's key list (`authorize_url`, `token_url`, `scopes`, `client_id`, `client_secret_env`, `revocation_url`, `redirect_port`) has nowhere to put it; `deny_unknown_fields` will reject any extra key.
   - What's unclear: whether the user accepts one more optional key. It is additive, but the table shape is marked costly to change.
   - Recommendation: add an optional `authorize_params = { duration = "permanent" }` (table of string to string, appended after the standard params; keys that collide with the standard ones such as `state`, `code_challenge`, `redirect_uri`, `client_id`, `response_type` are a config error). Confirm with the user in plan-phase before locking the shape; without it AUTH-02 cannot work for Reddit in Phase 7.

2. **Client authentication mode for the token endpoint.**
   - What we know: Reddit requires HTTP Basic with the client id as username and an empty password for installed apps [CITED: reddit wiki]. Google lists `client_secret` as optional for native apps [CITED: Google native-app doc]; RFC 6749 requires servers to support Basic.
   - What's unclear: whether a public client (no `client_secret_env`) should send `client_id` in the body (RFC 7636 public-client norm, Google) or Basic-with-empty-password (Reddit).
   - Recommendation: v1 default = secret present -> Basic; no secret -> `client_id` in body. If Reddit needs the other mode, Phase 7 adds an optional `token_auth` key (backward-compatible).

3. **User-Agent for host-side token calls.** Reddit's API rules require a descriptive unique User-Agent. Recommendation: `fili/<version>` set on the shared client; Phase 7 may need a per-plugin override.

4. **Default redirect port value.** Pure discretion (A1); pick one in plan-phase and write it in `plugins/README.md`.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust toolchain | build, tests | yes | rustc/cargo 1.96.0 (matches `rust-toolchain.toml`) | — |
| `wasm32-unknown-unknown` target | rebuilding stub-source for the real-wasm SC3 test | yes | installed | — |
| `cargo-component` | rebuilding `plugins/stub-source` after adding `record_inputs` | yes | `~/.cargo/bin/cargo-component` | — |
| `wasm-tools` | component validation | yes | `~/.cargo/bin/wasm-tools` | — |
| prebuilt `stub_source.wasm` | existing `wasm_*` ignored tests | yes | `plugins/stub-source/target/wasm32-unknown-unknown/release/stub_source.wasm` | rebuild per `plugins/README.md` |
| `openssl` CLI | existing `tls_` tests; PKCE vector cross-check | yes | OpenSSL 4.1.0 | — |
| Default browser / `xdg-open` | manual UAT of `fili auth login` | n/a (interactive) | — | `--no-browser` and copy the URL |
| A real OAuth provider app registration | manual UAT only | not available here | — | all automated tests use a fake loopback endpoint; manual UAT with Google or Reddit is a human-verify item |
| Fluvio cluster | not needed for this phase's tests | — | — | `VecSink` fakes |

**Missing dependencies with no fallback:** none for automated work.
**Missing dependencies with fallback:** live provider login (manual UAT only).

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in harness; `tokio` `test-util` for paused time; in-repo `TestHttpServer`, `FakeFactory`/`FakeInstance` |
| Config file | none (Cargo.toml `[dev-dependencies] tokio = { ..., features = ["test-util"] }`) |
| Quick run command | `cargo test -p filimon plugins::auth` |
| Full suite command | `cargo test -p filimon` (add `-- --ignored wasm_` for fixture-backed tests per plugins/README.md) |
| Lint gate | `cargo clippy --all-targets` (pedantic is `warn`; keep it clean) |

### Phase Requirements to Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| AUTH-01 | RFC 7636 vector, authorize URL contents (S256, state, redirect_uri, scopes, extras) | unit | `cargo test -p filimon plugins::auth::pkce` | Wave 0 |
| AUTH-01 | Callback: success, `error=`, state mismatch, wrong path 404 then success, oversize/garbage request, timeout (real-time 100 ms) | unit (real loopback) | `cargo test -p filimon plugins::auth::callback` | Wave 0 |
| AUTH-01 | Code exchange against fake token endpoint: form fields incl. `code_verifier`, Basic vs body client auth, tokens stored 0600 | unit | `cargo test -p filimon plugins::auth::client` | Wave 0 |
| AUTH-01 | `[host.oauth]` parse, https-only (`http://` rejected with plugin + field), missing/empty `client_secret_env`, `.env` read for secret-only plugin, unknown key rejected | unit | `cargo test -p filimon plugins::config` | extends existing tests |
| AUTH-02 | Refresh at < 60 s left, not at >= 60 s; omitted `refresh_token` keeps old; rotated one replaces; transient failure with valid token still polls; transient + expired backs off; `invalid_grant` -> `needs_login` kept on disk | unit (injected clock) | `cargo test -p filimon plugins::auth::provider` | Wave 0 |
| AUTH-02 | Re-read each poll: token file replaced between polls takes effect; concurrent refresh vs login does not clobber (two tasks, lock) | unit | `cargo test -p filimon plugins::auth::store` | Wave 0 |
| AUTH-02 | Scheduler: `CycleOutcome` for refresh failure maps to transient backoff; corrupt/0644 file disables only that plugin, other plugin keeps polling | unit (paused time) | `cargo test -p filimon plugins::host::scheduler` | extends existing |
| AUTH-03 | Fake-instance recording: access token present, refresh token and client secret sentinels absent from settings/secrets/cursor/token | unit | `cargo test -p filimon plugins::host::scheduler::sched_auth` | Wave 0 |
| AUTH-03 | Real wasm: `stub-source` with `record_inputs` returns everything it received in the item `raw`; sentinel assertions | integration (ignored) | `cargo test -p filimon -- --ignored wasm_auth` | Wave 0 (needs rebuilt fixture) |
| AUTH-04 | `status` output per state (logged-in, expired-with-refresh, needs-login, not-logged-in, unreadable, no-auth) without token values, works while state lock is held | unit + binary | `cargo test -p filimon commands::auth` / `cargo test --test auth_cli` | Wave 0 |
| AUTH-04 | `logout` deletes file; with `revocation_url` posts `token`; revoke failure warns but exits 0; later poll uses `none` | unit (fake endpoint) | `cargo test -p filimon commands::auth` | Wave 0 |
| CLI | `fili auth` parses; banner not printed for `auth` | unit | `cargo test -p filimon cli::tests` | extends existing |

### Sampling Rate
- **Per task commit:** `cargo test -p filimon plugins::auth` plus the touched module's tests.
- **Per wave merge:** `cargo test -p filimon` and `cargo clippy --all-targets`.
- **Phase gate:** full suite green, then `cargo test -p filimon -- --ignored wasm_` with the rebuilt `stub-source` fixture, before `/gsd-verify-work`; manual UAT of a real `fili auth login` against a provider the user has an app for.

### Wave 0 Gaps
- [ ] `src/plugins/auth/` (all files above) with their `#[cfg(test)]` modules.
- [ ] `FakeInstance` in `host/testing.rs`: record `(settings, secrets, cursor, access_token)` and take the new `poll` argument; update `ProbeInstance` in `commands/plugins.rs`.
- [ ] A small fake OAuth server helper on top of `TestHttpServer` (scripted replies per path: token, refresh, revoke, with request capture for form-field assertions).
- [ ] `plugins/stub-source`: optional `record_inputs = true` setting that puts `{settings, secrets, cursor, access_token}` into the returned item's `raw`; rebuild with `cargo component build --release` per `plugins/README.md`. The WIT does not change, so `bindings.rs` is regenerated unchanged.
- [ ] `tests/auth_cli.rs` (binary-level: arg parsing, no-`[host.oauth]` error, status on prepared files, logout without revocation).
- Framework install: none.

## Security Domain

`security_enforcement` is enabled (absent/true in `.planning/config.json`; `security_asvs_level: 1`, `security_block_on: high`).

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | yes (delegated authn via OAuth) | Authorization code + PKCE S256, `state` verification, loopback `127.0.0.1` redirect, no embedded secret for public clients |
| V3 Session Management | yes (token lifecycle) | Short-lived access token, refresh only host-side, revocation on logout, `needs_login` on permanent failure |
| V4 Access Control | yes | Token files per plugin alias with 0600 and 0700 dir; a plugin receives only its own access token; `[host.oauth]` not in `[settings]` |
| V5 Input Validation | yes | `url::Url` https-only validation at config load; strict callback request parsing with size/time caps; OAuth response parsing with body cap; alias rule before path building |
| V6 Cryptography | yes | `getrandom` CSPRNG, `sha2`; never hand-roll; TLS via rustls validation on |
| V7 Error handling and logging | yes | No codes/tokens/verifiers/secrets in logs, Debug or errors; provider error text through `clean_guest_text` |
| V9 Communications | yes | HTTPS enforced for authorize/token/revoke; plain http only on loopback callback |

### Known Threat Patterns for this stack

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Authorization-code interception by another local process | Information disclosure | PKCE S256; verifier never leaves memory/never logged (RFC 8252 §8.1 [CITED]) |
| CSRF / forged callback | Spoofing | 32-byte random `state`, verified, mismatch aborts login (RFC 8252 §8.9 [CITED]) |
| Local port squatting / stray requests to the listener | Spoofing, DoS | Bind `127.0.0.1` only, open only during login, 5-minute cap, only `/callback` is meaningful, per-connection read timeout and 8 KiB cap, serve a static page |
| Reflected content on the callback page | Tampering (XSS) | Fixed page, never echo query parameters |
| Token theft from disk | Information disclosure | 0600 file created that way (no window), 0700 dir, refuse looser perms on read (D-02), refuse symlinks, no auto-chmod |
| Token or secret leakage via logs/Debug/errors | Information disclosure | Redacting newtypes, no `Display`, never log request bodies or callback URL, `clean_guest_text` on provider strings |
| Refresh token reaching the guest | Information disclosure | Only `access-token` param exists in WIT; client secret kept out of `ResolvedSecrets`; recording-test with sentinels |
| Credential re-send on redirect from token endpoint | Information disclosure | `redirect(Policy::none())` on the auth client |
| Downgrade to plain http / MITM | Tampering | https-only config validation; TLS validation default on; no `danger_accept_invalid_certs` |
| Race between login/refresh/logout corrupting or resurrecting tokens | Tampering | Per-alias advisory lock, re-read after lock, atomic rename |
| Terminal escape injection from provider `error_description` | Tampering | `clean_guest_text` before printing or logging |
| Path traversal via alias on `status`/`logout` CLI arg | Tampering | Validate against the registry name rule before building `auth/<alias>.json` |
| Browser opener abuse (URL as command) | Elevation | URL is host-constructed from validated https config; `open` crate used with its secure Windows default (no `insecure` feature) [CITED: docs.rs/open] |

## Sources

### Primary (HIGH confidence)
- In-repo files read this session: `wit/worlds.wit`, `wit/types.wit`, `wit/host.wit`, `src/plugins/config.rs`, `src/plugins/secrets.rs`, `src/plugins/error.rs`, `src/plugins/mod.rs`, `src/plugins/host/{instance,scheduler,once,cursor,mod,engine,error,testing,fetch,https_tests,wasm_tests}.rs`, `src/commands/{plugins,run}.rs`, `src/cli.rs`, `src/main.rs`, `Cargo.toml`, `Cargo.lock`, `examples/plugins/*`, `plugins/stub-source/src/lib.rs`, `plugins/README.md`
- Local crate sources: `reqwest-0.12.28` (`form`, `basic_auth`, `json` feature gate), `tempfile-3.23.0/3.27.0` (0600 creation), `getrandom-0.3.3` / `0.4.3` (`fill`)
- RFC 7636 Appendix B vector, recomputed with `openssl dgst` this session
- crates.io dependency list for `oauth2 5.0.0` (API query this session)

### Secondary (MEDIUM confidence)
- https://datatracker.ietf.org/doc/html/rfc8252 — §7.3, §8.1, §8.3, §8.9 (loopback redirect, PKCE, `state`), fetched this session
- https://developers.google.com/identity/protocols/oauth2/native-app — endpoints, loopback, PKCE, refresh/revoke behaviour, fetched this session
- https://github.com/reddit-archive/reddit/wiki/OAuth2 — authorize `duration`, Basic auth, refresh omits `refresh_token`, revoke endpoint (archive of Reddit's docs; Reddit's current docs may differ)
- https://docs.rs/open/latest/open/ — non-blocking behaviour, `that_detached`, Windows `insecure` feature note

### Tertiary (LOW confidence)
- Training knowledge: Reddit User-Agent requirement and Google desktop clients needing a `client_secret` in practice (A5, A6, Open Question 3); `open` 5.x API beyond what docs.rs summary showed

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH, every recommended crate is already in `Cargo.lock` except `open`, and API shapes were read from source.
- Architecture: HIGH, extension points (trait, `PluginSpec`, `CycleOutcome`, config passes, CLI wiring) were read in the code; the state-lock collision and blocking-reqwest hazards are verified facts of the current code.
- Pitfalls: HIGH for locking, clocks, `.env` pass-2 and sample-test pinning (all read in code); MEDIUM for provider quirks (documentation only, no live run).

**Research date:** 2026-11-11
**Valid until:** 2026-12-11 (stable protocol; provider docs and crate versions are the moving parts)
