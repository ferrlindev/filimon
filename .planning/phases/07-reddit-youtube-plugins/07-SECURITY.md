---
phase: "07"
slug: "reddit-youtube-plugins"
status: verified
threats_open: 0
asvs_level: 1
created: "2026-11-12"
---

# Phase 07 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| Config → plugin | `subreddits`, `channels`, `username`, `user_agent_contact` interpolated into API requests | User-supplied identifiers (validated, percent-encoded) |
| Host ↔ OAuth token endpoint | client-credentials and code-flow token exchange | Client secret, access tokens (never logged; memory-only for app grant) |
| Plugin guest ↔ Reddit / YouTube APIs | WASM guest fetches via host `filimon:plugin/fetch` allowlist | Bearer tokens, `x-goog-api-key`, untrusted JSON |
| Samples / fixtures → repo | example TOMLs, `.env.example`, test fixtures | Placeholders and synthetic data only |

---

## Threat Register

63 threats: 51 unique IDs T-07-01..T-07-51 plus one T-07-SC (supply chain) row per plan (12). All disposition `mitigate`, all CLOSED with code/test evidence (auditor verification, 2026-11-12).

| Threat ID | Category | Component | Severity | Disposition | Mitigation | Status |
|-----------|----------|-----------|----------|-------------|------------|--------|
| T-07-01 | Tampering | subreddits / user_agent_contact interpolation | high | mitigate | settings.rs charset validation + percent-encoding (api.rs) | closed |
| T-07-02 | Info Disclosure | token/request details in errors/logs | high | mitigate | poll.rs fixed-phrase errors; counts-only warnings | closed |
| T-07-03 | DoS | hostile JSON / guest panic | medium | mitigate | `.get()`/`as_*` parsing; 8 MiB host cap | closed |
| T-07-04 | DoS | oversize cursor / batch | medium | mitigate | cursor 4096 cap, MAX_FEEDS, page_cap (see WR-04 caveat) | closed |
| T-07-05 | Elev. of Privilege | WASI / host imports | medium | mitigate | wasm-tools targets check; zero wasi imports | closed |
| T-07-06 | Info Disclosure | client secret to plugin/logs | high | mitigate | secret only in oauth.client_secret; `Secret(***)` Debug | closed |
| T-07-07 | Tampering | user_agent header injection | high | mitigate | `user_agent_is_valid` printable ASCII 1-200 | closed |
| T-07-08 | Tampering | plaintext/userinfo endpoints | high | mitigate | `https_endpoint` on token/authorize/revocation URLs | closed |
| T-07-09 | Elev. of Privilege | grant confusion | medium | mitigate | grant_problems; login refused before listener bind | closed |
| T-07-10 | Info Disclosure | secret/token/body in logs/Debug | high | mitigate | clean_guest_text, redacted Debug | closed |
| T-07-11 | Info Disclosure | secret re-sent via redirect | high | mitigate | `Policy::none()` on token client | closed |
| T-07-12 | DoS | token endpoint hang | medium | mitigate | 30 s timeout, 64 KiB cap, single-flight mutex | closed |
| T-07-13 | Info Disclosure | app token on disk | medium | mitigate | app token memory-only | closed |
| T-07-14 | Spoofing | stale login file as app token | low | mitigate | ClientCredentials never reads store | closed |
| T-07-15 | DoS | hostile listing JSON | high | mitigate | Unreadable on missing children; skip counting | closed |
| T-07-16 | DoS | huge post inflating record | medium | mitigate | RAW_MAX_BYTES 256 KiB | closed |
| T-07-17 | Tampering | corrupt/stale cursor | low | mitigate | decode fallback to first poll; retain_feeds | closed |
| T-07-18 | Info Disclosure | content/error echo | medium | mitigate | fixed phrases only | closed |
| T-07-19 | Repudiation | silently skipped posts | low | mitigate | skipped count warning | closed |
| T-07-20 | Tampering | channels into playlistId | high | mitigate | `valid_channel_id` + enc | closed |
| T-07-21 | Info Disclosure | API key in URL/log/error | high | mitigate | `x-goog-api-key` header only (see WR-06 note) | closed |
| T-07-22 | DoS | hostile JSON (YouTube) | high | mitigate | no unwrap/indexing; Unreadable on missing items | closed |
| T-07-23 | DoS | oversize cursor/batch (YouTube) | medium | mitigate | MAX_FEEDS, cursor cap, ID bounds | closed |
| T-07-24 | Elev. of Privilege | host capabilities | medium | mitigate | zero WASI imports | closed |
| T-07-25 | DoS | hammering after limit | high | mitigate | RateLimited aborts poll, no retry | closed |
| T-07-26 | Info Disclosure | bodies/headers echoed | medium | mitigate | fixed phrases | closed |
| T-07-27 | Spoofing | missing UA / unofficial host | medium | mitigate | fixed API_BASE, UA always set | closed |
| T-07-28 | Tampering | hostile x-ratelimit-reset | low | mitigate | clamped/saturated; host cap | closed |
| T-07-29 | Tampering | username into saved path | high | mitigate | 3-20 char validation + enc | closed |
| T-07-30 | Info Disclosure | private titles/username in logs | medium | mitigate | counts-only warnings | closed |
| T-07-31 | Elev. of Privilege | missing scope / app token on user feeds | low | mitigate | AuthRequired, named 403 | closed |
| T-07-32 | Tampering | wrong-shape cursor | low | mitigate | variant mismatch → None, cleared | closed |
| T-07-33 | DoS | quota exhaustion | high | mitigate | one request per playlist, abort on error | closed |
| T-07-34 | Info Disclosure | bodies/credentials in errors | high | mitigate | `error_reason` allowlist | closed |
| T-07-35 | Info Disclosure | key in URL / two credentials | high | mitigate | single Auth; StrictHttp test | closed |
| T-07-36 | Repudiation | failure looks like empty poll | medium | mitigate | typed error on non-2xx | closed |
| T-07-37 | DoS | subscription fan-out | high | mitigate | subscriptions_limit, quota_units_per_poll | closed |
| T-07-38 | DoS | endless nextPageToken | medium | mitigate | MAX_SUBSCRIPTION_PAGES=10 | closed |
| T-07-39 | Info Disclosure | subscription list in logs | medium | mitigate | counts only | closed |
| T-07-40 | Tampering | API channel IDs into playlistId | high | mitigate | re-validated, skipped+counted | closed |
| T-07-41 | Elev. of Privilege | key used for subscriptions | medium | mitigate | bearer-only subscriptions request | closed |
| T-07-42 | Info Disclosure | token/secret printed (app-only) | high | mitigate | AppOnly row never reads store | closed |
| T-07-43 | Elev. of Privilege | login listener for no-login grant | medium | mitigate | refused before bind | closed |
| T-07-44 | Info Disclosure | README/examples with credentials | low | mitigate | placeholders only | closed |
| T-07-45 | Elev. of Privilege | write scopes in samples | high | mitigate | read-only scopes; sample test | closed |
| T-07-46 | Info Disclosure | real credentials in samples | high | mitigate | empty `.env.example`, placeholders | closed |
| T-07-47 | Spoofing | sample allowlist to unofficial host | medium | mitigate | exact official hosts | closed |
| T-07-48 | Tampering | sample settings drift | low | mitigate | drift test | closed |
| T-07-49 | Info Disclosure | tokens/PII in fixtures | high | mitigate | synthetic fixtures; regex sweep clean | closed |
| T-07-50 | Tampering | tests altered to hide bugs | medium | mitigate | no tests removed since 75b078e | closed |
| T-07-51 | Info Disclosure | stub secrets / .env in repo | low | mitigate | no .env tracked | closed |
| T-07-SC (×12, one per plan 01–12) | Tampering | cargo deps | high | mitigate | root Cargo.toml/lock/wit unchanged vs 75b078e; plugin deps registry-only and in Package Legitimacy Audit | closed |

*Status: open · closed · open — below high threshold (non-blocking)*

---

## Non-blocking Observations (not in register)

- **WR-06 (warning):** host `fetch.rs:393-395` drops only `authorization`/`cookie`/`proxy-authorization` on cross-origin redirect; `x-goog-api-key` could be forwarded on an https→http redirect to an allowlisted host. Phase 5 host code, outside T-07-21's declared scope. Follow-up: add `x-goog-api-key`/`x-api-key` to the drop list or refuse https→http redirects. Tracked open in `07-REVIEW-DISPOSITION.md`.
- **WR-04 (caveat on T-07-04/T-07-17):** Reddit `parse_post` accepts any non-empty `name` with no length bound; an abnormally long fullname could make `cursor::encode` fail → Fatal → plugin disabled. Medium, non-blocking.
- WR-01/02/03/05, IN-03, IN-04: robustness/hygiene findings already open in the code review.

---

## Accepted Risks Log

No accepted risks.

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-11-12 | 63 | 63 | 0 | gsd-security-auditor |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-11-12
