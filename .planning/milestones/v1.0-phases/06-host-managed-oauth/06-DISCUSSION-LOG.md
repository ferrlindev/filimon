# Phase 6: Host-Managed OAuth - Discussion Log

> **Audit trail only.** Do not use as input to planning, research, or execution agents.
> Decisions are captured in CONTEXT.md — this log preserves the alternatives considered.

**Date:** 2026-11-11
**Phase:** 6-Host-Managed OAuth
**Areas discussed:** Token storage, Login flow UX, Refresh & failure policy, Provider TOML shape

---

## Token storage

| Question | Options | Selected |
|----------|---------|----------|
| Where to store tokens | Plain JSON 0600 in state dir / per-user config dir / OS keychain | Plain JSON 0600 in state dir |
| Unreadable/corrupt/loose-permission file | Disable that plugin / treat as logged out / auto-fix perms | Disable that plugin, name file |
| Logout behaviour | Local delete + best-effort revoke / local only / require revoke | Local delete + best-effort revoke |

## Login flow UX

| Question | Options | Selected |
|----------|---------|----------|
| Callback address/port | 127.0.0.1 fixed / random / random with fallback | 127.0.0.1 fixed port |
| Reaching authorize URL | Print + open browser / print only / open only | Print URL + try to open browser |
| Timeout and failures | 5 min, clear error / configurable / none | 5 min timeout, clear error, non-zero exit |

## Refresh & failure policy

| Question | Options | Selected |
|----------|---------|----------|
| When to refresh | Before each poll if <60s / background timer / after 401 | Just before each poll if <60s left |
| Permanent refresh failure | Mark logged-out, poll with none / disable until restart / delete tokens | Mark logged-out, poll with none |
| Live re-login and concurrency | Re-read each poll + per-plugin lock / cache until restart / fs watcher | Re-read file each poll; per-plugin lock |

## Provider TOML shape

| Question | Options | Selected |
|----------|---------|----------|
| Where OAuth is declared | `[host.oauth]` / top-level `[oauth]` / inside `[settings]` | `[host.oauth]` table |
| client_id / client_secret | id in TOML + secret via .env / both .env / both TOML | client_id in TOML, secret via .env alias |
| Endpoint restrictions | HTTPS, host-side outside allowlist / must be in allowlist / none | HTTPS required; host-side only |

## Claude's Discretion

PKCE internals, crate choices, token file schema, `auth status` layout, offline-expiry behaviour.

## Deferred Ideas

OS keychain, multiple accounts, background refresh timer, Reddit app-only vs PKCE (Phase 7).
