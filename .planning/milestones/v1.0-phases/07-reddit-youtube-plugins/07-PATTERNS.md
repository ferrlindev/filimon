# Phase 7: Reddit & YouTube Plugins - Pattern Map

**Mapped:** 2026-11-11
**Files analyzed:** 27 (new and modified)
**Analogs found:** 25 / 27 (all cited paths verified git-tracked via `git ls-files`)

## File Classification

| New/Modified File | Role | Data Flow | Closest Analog | Match |
|---|---|---|---|---|
| `plugins/{reddit,youtube}/Cargo.toml` | config | build | `plugins/stub-source/Cargo.toml` | exact (+rlib, +deps) |
| `plugins/{reddit,youtube}/rust-toolchain.toml` | config | build | `plugins/stub-source/rust-toolchain.toml` | exact copy |
| `plugins/{reddit,youtube}/.cargo/config.toml` | config | build | `plugins/stub-source/.cargo/config.toml` | exact copy |
| `plugins/{reddit,youtube}/.gitignore` | config | build | `plugins/stub-source/.gitignore` | exact copy |
| `plugins/{reddit,youtube}/Cargo.lock` | config | build | `plugins/stub-source/Cargo.lock` | generated; commit it |
| `plugins/{reddit,youtube}/src/lib.rs` | component glue | module wiring | `plugins/stub-source/src/lib.rs` | role-match (cfg-gated variant, RESEARCH Pattern 1) |
| `.../src/component.rs` (wasm32 only) | component | request-response | `plugins/stub-source/src/lib.rs` lines 22-143 | exact (Guest impl, fetch import) |
| `.../src/settings.rs` | utility | transform | `plugins/stub-source/src/lib.rs` lines 40-53, 62-63 | role-match |
| `.../src/http.rs`, `api.rs`, `cursor.rs`, `poll.rs` | service/core | request-response, transform | no tracked analog; RESEARCH Patterns 2-4 | none |
| `.../tests/replay.rs` + `tests/fixtures/*` | test | batch (fixture replay) | none in plugins; host `TestHttpServer` style only | partial |
| `src/plugins/config.rs` (`OAuthSection`, `OAuthConfig`, `build_oauth`, `oauth_problems`) | config | validation | itself (lines 150-165, 266-280, 1059-1196) | exact |
| `src/plugins/auth/client.rs` (`TokenClient`) | service | request-response | itself (`refresh` 182-205, `post_form` 235-289, UA line 129) | exact |
| `src/plugins/auth/provider.rs` (`TokenProvider`) | service | request-response | itself (lines 47, 98-135) | exact |
| `src/plugins/auth/testing.rs` | test helper | n/a | itself (`oauth_config`, `oauth_config_for`) | exact |
| `src/commands/auth.rs` (login refusal, status row) | controller (CLI) | request-response | itself (`login_command` 146-183; status label ~323) | exact |
| `src/plugins/mod.rs` sample tests | test | batch | itself (lines 29, 100-200) | exact |
| `tests/plugins_cli.rs` | test | CLI | itself (lines 155-170) | exact |
| `examples/plugins/{reddit,youtube}.toml` | config | n/a | `examples/plugins/reddit.toml` | exact |
| `examples/plugins/{reddit-user,youtube-user}.toml`, `filimon.toml`, `.env.example` | config | n/a | `examples/plugins/reddit.toml`, `filimon.toml` | role-match |
| `plugins/README.md` | doc | n/a | itself | exact |

## Pattern Assignments

### Plugin crate skeleton (`plugins/reddit`, `plugins/youtube`)

**Analog:** `plugins/stub-source/` (copy whole layout).

`Cargo.toml` (stub, verbatim) with the Phase 7 deltas called out:
```toml
[package]
name = "stub-source"            # -> reddit-source / youtube-source
version = "0.1.0"
edition = "2024"
publish = false

[lib]
crate-type = ["cdylib"]         # -> ["cdylib", "rlib"] so tests/ can link the core

[dependencies]
wit-bindgen-rt = { version = "0.41", features = ["bitflags"] }
serde = { version = "1", features = ["derive"] }
toml = "1"
# add: serde_json = "1", percent-encoding = "2"; youtube also time = { version = "0.3", default-features = false, features = ["parsing"] }

[package.metadata.component]
package = "filimon:stub-source" # -> filimon:reddit-source / filimon:youtube-source

[package.metadata.component.target]
path = "../../wit"
world = "source"

# Own workspace, so the root filimon workspace does not capture this crate.
[workspace]
```
Copy unchanged: `rust-toolchain.toml` (`channel = "1.96.0"`, `targets = ["wasm32-unknown-unknown"]`), `.cargo/config.toml` (`[build] target = "wasm32-unknown-unknown"`), `.gitignore` (`/target/`, `/src/bindings.rs`).

Gotcha: because `.cargo/config.toml` forces wasm, every verify uses `cargo test --target "$(rustc -vV | sed -n 's/^host: //p')"`.

### `src/lib.rs` / `src/component.rs` (component, request-response)

**Analog:** `plugins/stub-source/src/lib.rs`. The stub has `#[allow(warnings)] mod bindings;` ungated (line 22-23). Phase 7 gates it under `#[cfg(target_arch = "wasm32")]` and moves the Guest impl to `component.rs`.

**Imports** (stub lines 25-28):
```rust
use bindings::Guest;
use bindings::filimon::plugin::fetch::fetch;
use bindings::filimon::plugin::log::{Level, log};
use bindings::filimon::plugin::types::{Item, Method, PollError, PollResponse, Request};
```

**Guest impl / export** (stub lines 57-62, 143):
```rust
struct Component;
impl Guest for Component {
    fn init(settings: String, secrets: Vec<(String, String)>) -> Result<(), PollError> { ... }
    fn poll(cursor: Option<Vec<u8>>, access_token: Option<String>) -> Result<PollResponse, PollError> { ... }
}
bindings::export!(Component with_types_in bindings);
```

**Per-instance state** (stub lines 33-36): `thread_local! { static X: RefCell<Option<_>> = const { RefCell::new(None) }; }`. Use for parsed settings between `init` and `poll`.

**Settings parsing** (stub lines 40-43, 62-63): `#[derive(Deserialize)] #[serde(deny_unknown_fields)] struct Settings {..}` then `toml::from_str(&settings).map_err(|e| PollError::Config(e.to_string()))?`. Extend with field-naming config errors (user_agent_contact, app_id, subreddits, channels).

**Fetch request shape** (stub lines 86-96): `Request { method: Method::Get, url, headers: Vec<(String,String)>, body: None }`; `fetch(&request)` returns `Result<Response, FetchError>`. Adapter converts to/from the core `Http` trait types.

**Logging** (stub lines 71-81): log alias/feed names only, never values: `log(Level::Info, &format!(...))`.

**Item shape** (stub lines 124-134): `Item { id, url, title: Some(..), text, author: Some(..), published: Some(epoch_ms), platform, raw: Some(String) }`; `PollResponse { items, next_cursor: Some(Vec<u8>) }`.

### Core modules (`http.rs`, `api.rs`, `cursor.rs`, `poll.rs`): No tracked analog

Use RESEARCH.md Pattern 2 (Http trait), Pattern 3 (per-feed marks), Pattern 4 (error mapping table), and Code Examples (cursor encode with 4096 cap, Reddit `rate_limit_hit`, `user_agent`, `classify_youtube`). Apply D-19 (saved fullname set), D-20 (videoPublishedAt), D-22 (app_id UA).

### `src/plugins/config.rs` (D-03 `grant = "client_credentials"`)

**Analog:** itself. Add to `OAuthSection` (line 150, `#[serde(deny_unknown_fields)]`, fields are `toml::Spanned<_>`):
```rust
struct OAuthSection {
    authorize_url: toml::Spanned<String>,   // becomes Option; required only for authorization_code
    token_url: toml::Spanned<String>,
    client_id: toml::Spanned<String>,
    client_secret_env: Option<toml::Spanned<String>>,
    revocation_url: Option<toml::Spanned<String>>,
    redirect_port: Option<toml::Spanned<u16>>,
    authorize_params: BTreeMap<String, toml::Spanned<String>>,
    // add: grant (default authorization_code), user_agent: Option<Spanned<String>>
}
```
`OAuthConfig` (lines 266-280): add `grant` enum and `user_agent: Option<String>`. Update `build_oauth` (1059-1099) and `testing.rs::oauth_config` struct literals.

**Error pattern** (`oauth_problems`, lines 1114-1196): closure `invalid(field, message, span)` yields `ConfigError::InvalidValue { plugin, path, field, message, src: NamedSource::new(path, text.to_string()), span: Some(span) }`. Fields are named like `"host.oauth.client_secret_env"`; messages state the rule and never echo the value. For `client_credentials`, push problems for: missing `client_secret_env`, and `authorize_url`/`redirect_port`/`authorize_params` present. Validate `user_agent` has no CR/LF/control chars.

**URL validation** (lines 1100-1105): reuse `https_endpoint(text)`.

**Tests** (config.rs ~1947-1990): copy the `oauth_plugin(authorize_url, token_url)` helper and `oauth_table_loads_into_the_plugin_config` style, using `alpha_with_plugin(&dir, ...)` and `load(&dir)`; the invalid-field extraction helper at ~1940 collects `InvalidValue.field`.

### `src/plugins/auth/client.rs` (TokenClient: client_credentials request, user_agent)

**Analog:** itself.

**UA to change** (line 129): `.user_agent(concat!("fili/", env!("CARGO_PKG_VERSION")))` becomes `oauth.user_agent.as_deref()` fallback to that default.

**New grant method, copy `refresh`** (lines 182-205):
```rust
let form = vec![("grant_type", "client_credentials")];
let (status, body) = match self.post_form(&self.oauth.token_url, form).await {
    Ok(answer) => answer,
    Err(SendFailure::Transport(text)) => return Err(RefreshFailure::Transient(text)),
    Err(SendFailure::TooLarge) => return Err(RefreshFailure::Transient(too_large_text())),
};
if status.is_success() {
    return parse_token_set(&body).map_err(|_| RefreshFailure::Transient("answer has no usable access token".to_string()));
}
Err(classify_refresh_error(status, &body))
```
`post_form` (235-289) already sends HTTP Basic when `client_secret` is set and keeps the secret out of the form. Reuse `parse_token_set` (327-345).

**Tests:** copy the `refresh_against`/`failure_of`/`oauth_error` helpers (client.rs ~794-817) and `exchange_with_a_secret_uses_basic_auth_and_no_body_secret` (486) plus `the_request_carries_accept_json_and_a_fili_user_agent` (621) for the Basic/UA assertions; use `serve(reply)`, `form_pairs`, `has_pair`, `basic`, `assert_no_credentials`.

### `src/plugins/auth/provider.rs` (TokenProvider: in-memory app token)

**Analog:** itself. Existing flow (lines 119-135):
```rust
pub async fn access_token(&self) -> TokenOutcome {
    let tokens = match self.store.read() { Ok(Some(t)) => t, Ok(None) => return TokenOutcome::NotLoggedIn, Err(e) => return TokenOutcome::FileProblem(e) };
    if tokens.needs_login { return TokenOutcome::NeedsLogin; }
    let now = (self.clock)();
    if tokens.expires_at.saturating_sub(now) >= REFRESH_MARGIN_SECS { return TokenOutcome::Token(tokens.access_token); }
    self.renew(tokens, now).await
}
```
Fields to mirror: `gate: tokio::sync::Mutex<()>` (single-flight, see test `two_concurrent_calls_make_one_refresh_request` ~789), `client: OnceLock<..>` via `client()` (246), injected `clock` via `with_clock` (112). For `client_credentials`: branch at the top of `access_token`; keep `Mutex<Option<(AccessToken, expires_at)>>` in memory; reuse `REFRESH_MARGIN_SECS = 60` (line 47) and the default-3600 lifetime when `expires_in` is absent (test at 555). Map failure to the existing `TokenOutcome` refresh-failure variants (`cleaned()` at 255 strips text). No token file, no `needs_login`.

**Tests:** copy the harness in provider.rs ~405-470 (`new(replies)`, `provider()`, `good(access, refresh, expires_in)`, `token_text`, `requests()`) and tests at 477-486 for the 60-s margin boundary.

### `src/commands/auth.rs` (login refusal and status row)

**Analog:** itself. `login_command` (146-183) fetches `oauth` then errors with `miette::miette!("plugin `{}` declares no [host.oauth] table, so there is nothing to log in to", clean_guest_text(name, NAME_ECHO_CLIP))`. Add a sibling refusal after `oauth` is resolved: if grant is `client_credentials`, `miette::miette!("plugin `{}` uses client_credentials; there is nothing to log in to", ...)`. Status labels (match around line 323, `Self::NotLoggedIn => "not-logged-in"`): add an `AppOnly => "app-only"` variant; `status_cells` (432) and tests at ~1166-1250 (`status_lists_each_state_without_token_values`) show the assertion style. Update the README state list.

### Sample registry (`examples/plugins/*.toml`, `.env.example`, host tests)

**Analog:** `examples/plugins/reddit.toml`:
```toml
[host]
poll_interval_secs = 300
timeout_secs = 30
allowlist = ["oauth.reddit.com", "www.reddit.com"]
[host.backoff]
initial_secs = 60
max_secs = 3600
multiplier = 2.0
[host.secrets]
client_id = "REDDIT_CLIENT_ID"
client_secret = "REDDIT_CLIENT_SECRET"
[settings]
subreddits = ["rust", "programming"]
listing = "new"
```
Rewrite per RESEARCH "`[host.oauth]` additions": drop `[host.secrets]` and `listing`; add `[host.oauth] grant = "client_credentials"`, `settings.listings`, `user_agent_contact`; allowlist `["oauth.reddit.com"]`. New `reddit-user.toml` and `youtube-user.toml` follow the same `[host]`/`[host.backoff]` header.

**Host tests to update together (RESEARCH Pitfall 9):**
- `src/plugins/mod.rs:29`: `const SAMPLE_TOMLS: [&str; 4] = ["filimon.toml", "stub.toml", "reddit.toml", "youtube.toml"];` add the two new files.
- `loads_sample_registry` (mod.rs ~60-130): expected sources and secret keys (`secret_keys(youtube) == ["api_key"]`; reddit secrets assertions); the test `.env` must define `REDDIT_CLIENT_SECRET` since `client_secret_env` is resolved strictly.
- `sample_env_example_lists_every_declared_secret` (mod.rs ~149-185): collect `table["host"]["secrets"]` values; extend to also collect `host.oauth.client_secret_env`, and update the `BTreeSet::from([...])` expected set.
- `tests/plugins_cli.rs:164`: `["reddit", "stub", "youtube", "http", "stub-fetcher"]` gains `reddit-user`, `youtube-user` in registry order.
- `examples/plugins/filimon.toml`: add the two `[[sources]]` entries pointing at the same `.wasm` components (registry permits duplicates, config.rs:834-845 only rejects source/fetcher name clashes).

## Shared Patterns

### Error handling in plugins
**Source:** `plugins/stub-source/src/lib.rs` line 63 (`PollError::Config(e.to_string())`); typed table in RESEARCH Pattern 4. Apply to all plugin `init`/`poll` paths; never log or echo URLs, headers, tokens, bodies.

### Host diagnostics (miette, field-naming, no value echo)
**Source:** `src/plugins/config.rs` `oauth_problems` closure `invalid(...)` (lines 1120-1128). Apply to all new `[host.oauth]` validations.

### Secret handling
**Source:** `src/plugins/config.rs` (client secret stays in `OAuthConfig::client_secret`, never `PluginConfig::secrets`; ~line 731). Add a host test that a `client_credentials` plugin's `init` receives no client secret (stub `record_inputs` setting in `plugins/stub-source/src/lib.rs` lines 12-16, 116-122 is the existing proof fixture).

### Host lint level
Root `Cargo.toml` `[lints.clippy] all/pedantic = "warn"`: new host code in `src/plugins/` must be pedantic-clean (use `# Errors` doc sections like `client.rs` 109-112, `#[must_use]` on constructors like `provider.rs` 97).

### Build and verify commands
```bash
(cd plugins/reddit && cargo test --target "$(rustc -vV | sed -n 's/^host: //p')")
(cd plugins/reddit && cargo component build --release && wasm-tools component targets ../../wit -w source target/wasm32-unknown-unknown/release/reddit_source.wasm)
cargo test -p filimon plugins::config plugins::auth
cargo test -p filimon --test auth_cli --test plugins_cli
```

## No Analog Found

| File | Role | Data Flow | Reason |
|---|---|---|---|
| `plugins/*/src/{http,api,cursor,poll}.rs` | core logic | request-response / transform | Stubs have no Http-trait seam or JSON API parsing; use RESEARCH Patterns 2-4 and Code Examples |
| `plugins/*/tests/replay.rs`, `tests/fixtures/*`, fixtures `README.md` | test | fixture replay | No plugin crate has native tests; the host's `TestHttpServer` (`src/plugins/host/testing.rs`) is a loose style reference only. Fixtures are human-action capture or `synthetic` labelled (D-21) |

## Metadata

**Analog search scope:** `plugins/`, `src/plugins/`, `src/commands/`, `examples/plugins/`, `tests/`
**Files scanned:** about 60 listed, 10 read in part
**Pattern extraction date:** 2026-11-11
