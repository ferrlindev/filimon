---
phase: 06-host-managed-oauth
reviewed: 2026-11-11T00:00:00Z
depth: standard
files_reviewed: 29
files_reviewed_list:
  - Cargo.lock
  - Cargo.toml
  - plugins/README.md
  - plugins/stub-source/src/lib.rs
  - src/cli.rs
  - src/commands/auth.rs
  - src/commands/mod.rs
  - src/commands/plugins.rs
  - src/main.rs
  - src/plugins/auth/callback.rs
  - src/plugins/auth/client.rs
  - src/plugins/auth/error.rs
  - src/plugins/auth/login.rs
  - src/plugins/auth/mod.rs
  - src/plugins/auth/pkce.rs
  - src/plugins/auth/provider.rs
  - src/plugins/auth/store.rs
  - src/plugins/auth/testing.rs
  - src/plugins/config.rs
  - src/plugins/error.rs
  - src/plugins/host/instance.rs
  - src/plugins/host/mod.rs
  - src/plugins/host/once.rs
  - src/plugins/host/scheduler.rs
  - src/plugins/host/testing.rs
  - src/plugins/host/wasm_tests.rs
  - src/plugins/mod.rs
  - src/plugins/secrets.rs
  - tests/auth_cli.rs
findings:
  critical: 0
  warning: 4
  info: 4
  total: 8
  necessity: 0
status: issues_found
---

# Phase 06: Code Review Report

**Reviewed:** 2026-11-11
**Depth:** standard
**Files Reviewed:** 29
**Status:** issues_found

## Summary

Reviewed the host-managed OAuth phase: PKCE and loopback callback, token client,
token store and provider, `fili auth` commands, scheduler and `run-once`
wiring, `[host.oauth]` config and secret resolution, the stub fixture and tests.

The design is sound and carefully hardened: S256 only, CSPRNG verifier and
state, https-only endpoints with no user-info, redirects disabled, capped
responses, redacting token types, 0600/0700 files with a regular-file and mode
check, atomic writes, and a per-alias lock. No hardcoded secrets, injection or
path-traversal paths were found: the alias is name-validated before any path is
built. No Critical issues. The weaknesses are in the callback listener's
handling of unauthenticated requests, a read-before-lock race in logout, the
Basic-auth credential encoding, and refresh/lock timing.

Necessity (`necessity-review.md` read): no findings. The guards found sit on
trust boundaries (files, network, environment) or are test seams.

No structural findings (fallow) were provided. Two injection-scan hits on
`?code=` in `callback.rs` and `login.rs` are false positives (a doc comment and
test request strings).

## Warnings

### WR-01: Any request to `/callback` with a wrong state aborts the login

**File:** `src/plugins/auth/callback.rs:319-321` (decision returned at `163-166`)
**Issue:** `judge` turns a missing or wrong `state` into
`Reply::Callback(Err(StateMismatch))`, and `wait_for_code` returns the first
decision it receives. So one stray request to
`http://127.0.0.1:17420/callback` ends the login with an error. Any web page
open in the user's browser can trigger it with a no-cors GET to a fixed,
documented port, and so can a stale tab or redirect from a previous attempt
against the same port, or a browser prefetch. The attacker cannot obtain a code
(the state check holds), but a forged request is enough to deny the login. The
`error=` branch is behind the state check, so only a state-bearing request can
deny. Only the state-mismatch path is a problem.
**Fix:** Treat a state mismatch as a rejected request, not a decision. Answer
400, send nothing to the decision channel, and keep waiting until the timeout:

```rust
if first("state").as_deref() != Some(expected_state) {
    return Reply::BadRequest; // not a decision; the wait continues
}
```

Keep `AuthError::StateMismatch` for the timeout path if the wait saw
mismatches, so the user still gets a useful diagnostic.

### WR-02: Logout reads the stored tokens before taking the lock, so it can revoke a stale token

**File:** `src/commands/auth.rs:663-683`
**Issue:** `perform_logout` calls `store.read()` and only then `store.lock()`.
A refresh in `fili run` (or a re-login) that lands between the two rotates the
refresh token. Logout then deletes the new file but sends the old, already
superseded refresh token to the revocation endpoint. The valid refresh token
stays live at the provider while the user was told "The login was revoked at the
provider." The module comment promises this operation is serialized by the
per-alias lock (D-03), but the read is outside it.
**Fix:** Take the lock first, read under it, delete, then release:

```rust
let lock = store.lock().await.map_err(miette::Report::new)?;
let tokens = match store.read() { /* existing Ok(None)/Err handling */ };
store.delete(&lock).map_err(miette::Report::new)?;
drop(lock);
```

The `Ok(None)` early return then happens with the lock held and dropped on
return, which is fine.

### WR-03: HTTP Basic client credentials are not form-urlencoded as RFC 6749 section 2.3.1 requires

**File:** `src/plugins/auth/client.rs:244-246`
**Issue:** `request.basic_auth(&client_id, Some(secret.expose()))` base64-encodes
`id:secret` as is. RFC 6749 section 2.3.1 requires the id and secret to be
`application/x-www-form-urlencoded` first. A secret containing `:`, `%`, `+`,
`&` or non-ASCII characters is sent differently from what a compliant provider
decodes, so confidential-client logins and refreshes fail with
`invalid_client`. The failure is hard to diagnose because the error shows only
the provider's code. Some providers (Reddit) take raw values, which is why this
is easy to miss in manual testing.
**Fix:** Encode both parts, since for unreserved characters the result is
unchanged:

```rust
let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
request = request.basic_auth(enc(&self.oauth.client_id), Some(enc(secret.expose())));
```

Document the choice, or add a `client_auth` option if a provider needs raw values.

### WR-04: A token refresh cannot be interrupted by shutdown, and it holds the shared lock for as long as the lock wait

**File:** `src/plugins/host/scheduler.rs:523-536`; `src/plugins/auth/provider.rs:143-176`; `src/plugins/auth/store.rs:47`; `src/plugins/auth/client.rs:36`
**Issue:** Two timing problems.

1. `auth_for_poll` is awaited outside the `select!` on `shutdown`. The comment
   explains why (do not cut a refresh between the answer and the file write),
   but the cost is unbounded by the shutdown signal: a refresh can wait up to
   `LOCK_WAIT` (30 s) for the lock and then run a request of up to 30 s. A
   Ctrl-C during that window is ignored until it ends, per plugin, unless the
   caller aborts tasks (not verified here).
2. `renew` holds the per-alias lock across the whole network call.
   `REQUEST_TIMEOUT` (30 s) equals `LOCK_WAIT` (30 s), so a slow provider makes
   a concurrent `fili auth logout` or `login` time out with "another fili
   process is updating this login" at the same moment the refresh times out.

**Fix:** Race only the cancellable part against shutdown. For example, `select!`
the lock acquisition and the HTTP call against `shutdown.cancelled()`, and keep
the read, persist and write section uninterruptible. Make `LOCK_WAIT` clearly
longer than `REQUEST_TIMEOUT + write time` (for example 45 to 60 s) so the lock
timeout is not racing the request timeout.

## Info

### IN-01: The callback page says "The login finished" for denied and mismatched logins

**File:** `src/plugins/auth/callback.rs:46-50, 213-217`
**Issue:** Every `/callback` reply, including `error=access_denied`, a missing
code, a state mismatch, and an exchange that later fails, gets 200 and "The
login finished. You can close this tab." A user who denied access, or whose
exchange fails afterwards, is told it finished and must look at the terminal.
The page is fixed on purpose so that no request value is echoed, but there is
room for two fixed pages.
**Fix:** Use a neutral constant such as "Return to the terminal to see the
result of the login.", or choose between two fixed pages by decision outcome.

### IN-02: `client_secret_env` can name any environment variable, and its value is sent to the plugin's `token_url`

**File:** `src/plugins/config.rs:1168-1176, 733-745`; `src/plugins/secrets.rs:341-379`
**Issue:** The key is only checked for non-empty. A plugin TOML (often shipped
with a plugin) can set `client_secret_env = "AWS_SECRET_ACCESS_KEY"` with a
`token_url` it controls, and `fili auth login` sends that value as the Basic
password to that host. This matches the existing `[host.secrets]` trust model,
where the operator reviews the plugin TOML, but this path sends the secret from
the host's own client, outside the plugin's fetch allowlist. The README does not
warn about it.
**Fix:** Add one sentence to `plugins/README.md` ("Declaring OAuth") that the
secret is sent to `token_url` and `revocation_url`, so review those with the
variable name. Optionally require a naming convention such as a prefix, or print
the env var name and token host on `fili auth login`.

### IN-03: A rejected or transient refresh retries and warns on every poll for the last 60 seconds of a token

**File:** `src/plugins/auth/provider.rs:188-198`
**Issue:** When a refresh fails but the old token still has life left, the
provider returns the old token and logs a warning. The token stays under
`REFRESH_MARGIN_SECS`, so the next poll starts a new refresh and logs again. A
permanent non-`invalid_grant` refusal (for example `invalid_client` after a
rotated client secret) therefore produces a request and a warning on every poll
until expiry. No state is wrong, but a short poll interval makes it noisy and
sends repeated failing requests to the provider.
**Fix:** Keep the last failure in provider state (an `Instant` or a Unix time)
and skip the refresh for a short cool-down, or log only on a state change as the
login hint already does.

### IN-04: The stub fixture's `record_inputs` copies the access token into an item that is produced to Fluvio

**File:** `plugins/stub-source/src/lib.rs:108-124`
**Issue:** With `record_inputs = true`, `poll` writes `access_token={access_token:?}`
and the init secrets into `Item.raw`, which `fili run` sends to the sink. The
setting defaults off and is documented as a test fixture, and the tests cover
it. If a developer enables it against a real cluster, a live token and the
secrets go to a topic.
**Fix:** Keep as is for the tests, and add a warning to the fixture header and to
`plugins/README.md` that `record_inputs` must never be enabled with a real sink
or a real login.

---

_Reviewed: 2026-11-11_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
