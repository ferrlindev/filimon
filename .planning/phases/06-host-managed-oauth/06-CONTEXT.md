# Phase 6: Host-Managed OAuth - Context

**Gathered:** 2026-11-11
**Status:** Ready for planning

<domain>
## Phase Boundary

Users log in to a plugin's platform once through OAuth 2.0 PKCE (`fili auth login <plugin>`). The host stores tokens, refreshes access tokens before expiry, and hands plugins only a current access token. `fili auth status` and `fili auth logout <plugin>` inspect and revoke logins.

Not in this phase: the Reddit/YouTube plugins and their API calls (Phase 7); Reddit app-only vs PKCE decision is Phase 7 research (STATE.md concern); multiple accounts per plugin; OS keychain storage.

Requirements: AUTH-01..AUTH-04.

</domain>

<decisions>
## Implementation Decisions

### Token storage (AUTH-02, AUTH-04)
- **D-01:** Tokens are stored as **plain JSON, mode 0600, one file per plugin alias** under the state directory (`<state-dir>/auth/<alias>.json`, default `.fili-state/` per Phase 3 D-06, same `--state-dir` / `FILI_STATE_DIR` override). Written atomically (temp file + rename), like Phase 3 cursor files. — **Reversibility:** costly — the on-disk token file layout is what existing logins depend on; changing it forces every user to log in again.
- **D-02:** A token file that is unreadable, corrupt, or has permissions looser than 0600 **disables only that plugin**, with an error naming the plugin and file path. Other plugins keep polling. Mirrors Phase 3 D-08. The host never silently treats it as logged out and never auto-chmods.
- **D-03:** `fili auth logout <plugin>` always deletes the local token file. If the plugin's `[host.oauth]` declares `revocation_url`, it also attempts a best-effort revoke at the provider and **warns** on failure (logout still succeeds). After logout, later polls run with `access-token = none`.

### Login flow (AUTH-01)
- **D-04:** The callback listener binds **`127.0.0.1` on a fixed port**, default from `fili` and overridable per plugin via `redirect_port` in `[host.oauth]` (providers like Reddit require a pre-registered exact redirect URI).
- **D-05:** Login **prints the authorize URL and also tries to open the default browser**; `--no-browser` suppresses the launch. Works over SSH/headless.
- **D-06:** The login waits **5 minutes** for the callback, then fails with a clear error and a non-zero exit. The `state` parameter is verified; a mismatched, denied, or error callback is rejected with a named error. The callback page is a plain "you can close this tab" page. Nothing is written on failure. **Codes, tokens, and verifiers are never printed or logged.**

### Refresh and failure policy (AUTH-02, AUTH-03)
- **D-07:** Refresh is **lazy, just before each poll**: if the access token has less than **60 seconds** left, refresh first. No background timer.
- **D-08:** The host **re-reads the token file at each poll**, so a `fili auth login` in another terminal takes effect without restarting `fili` (this is the "Phase 6 can fix it live" assumption in Phase 3 D-03). Refresh is serialised with **one in-flight refresh per plugin** and the new tokens written atomically, so refresh-token rotation cannot lose the new refresh token.
- **D-09:** On a **permanent** refresh failure (`invalid_grant`, revoked or expired refresh token), the host keeps the file, marks it `needs_login`, and polls with `access-token = none`. The plugin returns `auth-required` and Phase 3 backoff applies (D-02/D-03 there). `fili auth status` shows the `needs_login` state and says to re-run login. **Transient** refresh failures (network, 5xx) retry under the same backoff and do not mark `needs_login`.
- **D-10:** The WIT contract is **unchanged**: the token reaches the plugin as `poll`'s `access-token: option<string>` parameter (Phase 1 D-07), not a new import. The roadmap's "WIT token import" wording is read as this parameter. The refresh token never crosses into the plugin. A recording test component proves it (SC3).

### Provider config in plugin TOML (AUTH-01)
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

### Carried forward
- `poll(cursor, access-token)`; `auth-required` is a `poll-error` variant treated as backoff (Phase 1 D-07, Phase 3 D-03).
- Secrets come from `.env` per plugin alias only and are redacted (Phase 1 D-11/CONF-02).
- State dir convention and atomic write pattern (Phase 3 D-05, D-06).
- Corrupt state disables only that plugin and names plugin and path (Phase 3 D-08).
- Commands live in `src/commands/*.rs`; global `--registry` / `FILI_REGISTRY` already exist (Phase 4/5).

</decisions>

<canonical_refs>
## Canonical References

**Downstream agents MUST read these before planning or implementing.**

### Contract and config
- `wit/worlds.wit` — `source` world: `poll(cursor, access-token: option<string>)` (Phase 1 D-07)
- `wit/types.wit` — `poll-error::auth-required`
- `src/plugins/config.rs` — `LoadedSource`, `HostConfig`, secrets loading and redaction; where `[host.oauth]` parsing and validation goes
- `src/plugins/secrets.rs`, `src/plugins/error.rs` — secret handling and diagnostics patterns
- `examples/plugins/filimon.toml`, `examples/plugins/reddit.toml` — registry and plugin TOML shape to extend

### Host and CLI
- `src/plugins/host/` — scheduler and poll loop where the token provider supplies `access-token` (currently `none`)
- `src/commands/plugins.rs`, `src/commands/mod.rs`, `src/cli.rs`, `src/main.rs` — command wiring pattern for the new `fili auth` subcommands

### Prior decisions
- `.planning/phases/01-plugin-contract-configuration/01-CONTEXT.md` — D-07 (access-token param), D-11 (`[host]` table, secrets)
- `.planning/phases/03-component-host-runtime/03-CONTEXT.md` — D-02/D-03 (backoff, auth-required), D-05..D-08 (state dir, atomic files, corrupt-file policy)
- `.planning/phases/05-fetcher-layer/05-CONTEXT.md` — host TLS client rules (host-side HTTP must validate certificates)

### Requirements and roadmap
- `.planning/REQUIREMENTS.md` — AUTH-01..AUTH-04
- `.planning/ROADMAP.md` §Phase 6 — success criteria SC1-SC4
- `.planning/PROJECT.md` — constraints (refresh tokens never cross into the plugin; secrets in `.env`)
- `.planning/STATE.md` §Blockers/Concerns — Phase 7 Reddit OAuth question

</canonical_refs>

<code_context>
## Existing Code Insights

### Reusable Assets
- `LoadedSource` / `HostConfig` already carry per-plugin host config and redacting `Secret` values; `[host.oauth]` extends this.
- Phase 3 cursor-file code gives the atomic temp+rename write and state-dir resolution to reuse for token files.
- `reqwest` (rustls, blocking) is already a dependency for host-side token and revoke calls.
- Phase 3 scheduler already treats `auth-required` as backoff and passes `access-token = none`; this phase replaces the `none` with a real value.

### Established Patterns
- `miette` diagnostics that name plugin and field/key; secrets never in Debug or logs.
- Commands in `src/commands/*.rs`, dispatched from `main.rs`.
- Corrupt per-plugin state disables only that plugin.

### Integration Points
- New token provider sits between the host's poll loop and the plugin's `poll` call.
- New `fili auth login | status | logout` command module beside `plugins.rs`.
- Config loader gains `[host.oauth]` validation (https endpoints, `client_secret_env` resolution).

</code_context>

<specifics>
## Specific Ideas

- SC3 test: a recording test component that captures everything passed to `init`/`poll`, asserting it sees the access token but never the refresh token or client secret.
- Test refresh and expiry deterministically (tokio paused time and a fake token endpoint on loopback).
- Reddit's exact-redirect-URI requirement is why the callback port is fixed and configurable.

</specifics>

<deferred>
## Deferred Ideas

- OS keychain / encrypted token storage — revisit if plain 0600 files prove insufficient
- Multiple accounts per plugin — not in requirements
- Background proactive refresh timer — only if long poll intervals cause expiry problems
- Whether Reddit RDDT-01 uses app-only credentials or this PKCE login — Phase 7 research

</deferred>

---

*Phase: 6-Host-Managed OAuth*
*Context gathered: 2026-11-11*
