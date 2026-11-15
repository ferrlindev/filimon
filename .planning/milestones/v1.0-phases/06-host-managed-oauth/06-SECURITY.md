---
phase: "6"
slug: "host-managed-oauth"
status: verified
# threats_open = count of OPEN threats at or above workflow.security_block_on severity (the blocking gate)
threats_open: 0
asvs_level: 1
created: "2026-11-11"
---

# Phase 6 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| `.env`/environment and plugin TOML to host config | The client secret and endpoint/port/scope/parameter values enter here; the secret must stop at the host | client secret, URLs |
| host to third-party plugin guest | Only the access token may cross, as `poll`'s `access-token`; `init` gets no OAuth secret | access token |
| host to the provider (authorize, token, revocation) | The code, verifier, client secret and refresh token leave the host here | credentials |
| browser and any local process to the loopback listener | Anything on the machine can connect to the port while a login waits | authorization code, state |
| host to the user's browser | The host-built authorize URL is handed to an external program | URL |
| token file on disk to host | Content, type and permissions are untrusted; concurrent fili processes share `<state-dir>/auth` | refresh and access tokens |
| CLI argument to registry lookup | The plugin name is user input and becomes a state file name | plugin name |
| token files to terminal output | `status` and `logout` read credential files and print about them | token state |
| documentation and examples to users | Users copy README snippets into real configs | placeholder values |

---

## Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation | Status |
|-----------|----------|-----------|----------|-------------|------------|--------|
| T-06-01 | Information Disclosure | token types, provider Debug, log and error text | high | mitigate | store.rs:80-84,104-108 Debug prints *** for tokens, no Display; provider.rs:86-93; error.rs fixed phrases; store.rs:671-677 | closed |
| T-06-02 | Tampering | alias to path in `TokenStore` | high | mitigate | store.rs:167-174 path_with calls is_valid_name (config.rs:805) before any join; list_registry drops invalid names; store::an_invalid_alias_is_refused_before_any_path_is_built | closed |
| T-06-03 | Information Disclosure | refresh token or client secret reaching the plugin | high | mitigate | instance.rs:70-74 poll takes only (cursor, access_token); scheduler.rs:292-330 auth_for_poll returns only AccessToken; init gets spec.secrets only; wit/ unchanged since 1fd00e7; wasm_auth_* tests | closed |
| T-06-04 | Tampering | endpoint downgrade to plain http | high | mitigate | config.rs:1098-1102 https_endpoint (https, host, no userinfo) applied to all three URLs 1129-1142; oauth_endpoints_must_be_https_naming_plugin_and_field | closed |
| T-06-05 | Denial of Service | one plugin's bad token file stopping the host or other plugins | medium | mitigate | scheduler.rs:189-193,328 TokenFile maps to Action::Disable; sched_auth::a_corrupt_token_file_disables_only_that_plugin (re-run green) | closed |
| T-06-SC (06-01) | Tampering | npm/pip/cargo installs | high | mitigate | No 06-01 commit touches Cargo.toml or Cargo.lock | closed |
| T-06-06 | Information Disclosure | token file readable by other local users | high | mitigate | store.rs:265-278 dir 0700; 307-317 temp file 0600 before first byte; 218-226 refuses mask 0o077, never chmods; write_creates_a_0600_file_in_a_0700_directory | closed |
| T-06-07 | Tampering | login, refresh and logout racing on one file | high | mitigate | store.rs:340-374 per-alias lock; writers under lock (provider.rs:144/224, login.rs:136, auth.rs:680); atomic rename store.rs:320; lock_serialises_two_holders | closed |
| T-06-08 | Denial of Service | oversized, garbage, symlinked or wrong-owner token file | medium | mitigate | store.rs:208-214 symlink_metadata+is_file; 227-239 64 KiB cap; 243-251 version and plugin checks; oversize/symlink/foreign-plugin tests. No OS-uid check (stated in 06-02 plan) | closed |
| T-06-SC (06-02) | Tampering | npm/pip/cargo installs | high | mitigate | No 06-02 commit touches Cargo.toml or Cargo.lock | closed |
| T-06-09 | Information Disclosure | client secret reaching the plugin | high | mitigate | config.rs:731-757 secret only in OAuthConfig.client_secret; scheduler.rs:276-281 init gets spec.secrets only; client_secret_never_appears_in_resolved_secrets (re-run green) | closed |
| T-06-10 | Tampering | `authorize_params` overriding PKCE, state or redirect parameters | high | mitigate | config.rs:41-49,1184-1193 case-insensitive reserved-key collision rejected naming the field; authorize_params_colliding_with_a_standard_key_are_rejected_naming_plugin_and_field | closed |
| T-06-11 | Information Disclosure | secret value in config errors or Debug output | high | mitigate | secrets.rs:19-37 Secret Debug is Secret(***); 341-379 errors carry names and paths only; oauth_secret_value_never_in_debug_or_errors | closed |
| T-06-12 | Tampering | credentials in URLs and plain-http downgrade of any endpoint | high | mitigate | config.rs:1098-1102 https, host, empty userinfo across authorize/token/revocation URLs; oauth_endpoints_reject_userinfo_naming_plugin_and_field | closed |
| T-06-SC (06-03) | Tampering | npm/pip/cargo installs | high | mitigate | No 06-03 commit touches Cargo.toml or Cargo.lock | closed |
| T-06-13 | Information Disclosure | authorization code intercepted by another local process | high | mitigate | pkce.rs:57-63 getrandom 32 bytes (43 chars); 85-87,112 S256 only; verifier in memory only; rfc7636_appendix_b_vector | closed |
| T-06-14 | Spoofing | forged or cross-site callback | high | mitigate | pkce.rs:79-81 CSPRNG state; callback.rs:319-321 StateMismatch before error/code checks; a_state_mismatch_is_a_named_error | closed |
| T-06-15 | Spoofing | port squatting, stray and hostile requests to the listener | medium | mitigate | callback.rs:97 binds 127.0.0.1; 140-176 listener consumed; 5 min cap, 8 KiB and 2 s per connection; 404/400 replies; error.rs:51-57 names config key | closed |
| T-06-16 | Tampering | reflected content on the callback page | medium | mitigate | callback.rs:48-50 constant CALLBACK_PAGE; http_response uses constants only; callback_page_is_static_and_reflects_nothing | closed |
| T-06-17 | Information Disclosure | code, verifier or state in logs, Debug or error text | high | mitigate | callback.rs:73-77 and pkce.rs:50-54 Debug print ***; no println/dbg in auth module; codes_never_appear_in_debug_or_error_text | closed |
| T-06-SC (06-04) | Tampering | npm/pip/cargo installs | high | mitigate | b168bb3 adds sha2 0.10 and getrandom 0.3; Cargo.lock gains only edges, both already resolved; research audit OK | closed |
| T-06-18 | Information Disclosure | credential re-sent to another host on a redirect | high | mitigate | client.rs:126 redirect(Policy::none()); 3xx treated as rejection; a_redirect_is_not_followed_and_the_secret_is_not_resent (re-run green) | closed |
| T-06-19 | Tampering | downgrade to plain http or a man in the middle | high | mitigate | https enforced in config; client.rs:125-133 no danger_accept; rustls defaults (Cargo.toml:20); the crawler's danger_accept_invalid_certs predates Phase 6 and is not used by the token client | closed |
| T-06-20 | Information Disclosure | provider text or request details in errors and logs | high | mitigate | client.rs:373-389 error text through clean_guest_text (160 chars); non-JSON body never echoed; without_url; revoke errors carry status only | closed |
| T-06-21 | Denial of Service | hung or enormous token endpoint | medium | mitigate | client.rs:36-42,127-128 30 s request, 10 s connect, 64 KiB cap; a_hung_endpoint_times_out; an_oversized_body_is_refused | closed |
| T-06-SC (06-05) | Tampering | npm/pip/cargo installs | high | mitigate | No 06-05 commit touches Cargo.toml or Cargo.lock | closed |
| T-06-22 | Tampering | a late refresh overwriting a newer login (lost update) | high | mitigate | provider.rs:143-144 gate then file lock; 151-169 re-read after lock; refresh_re_reads_after_taking_the_lock...; a_login_during_a_refresh_is_not_overwritten | closed |
| T-06-23 | Information Disclosure | losing or leaking the refresh token on an answer that omits or rotates it | high | mitigate | provider.rs:207-231 persist keeps refresh token when omitted, writes before returning; save failure returns no token | closed |
| T-06-24 | Denial of Service | refresh storms and a transient outage ending a working login | medium | mitigate | provider.rs:143 gate; 179-199 needs_login only on Permanent; scheduler.rs:188 backoff; two_concurrent_calls_make_one_refresh_request | closed |
| T-06-25 | Tampering | clock skew or a wrong clock deciding expiry | low | accept | REFRESH_MARGIN_SECS=60 (provider.rs:47), wall clock, injectable clock; documented plugins/README.md:696-698 (accepted) | closed |
| T-06-SC (06-06) | Tampering | npm/pip/cargo installs | high | mitigate | No 06-06 commit touches Cargo.toml or Cargo.lock | closed |
| T-06-26 | Elevation of Privilege | browser opener abuse (a URL treated as a command) | medium | mitigate | auth.rs:178 only open:: call; login.rs:103-106 passes only the host-built https authorize URL; open 5.4.5 direct Command, no shell | closed |
| T-06-27 | Information Disclosure | secrets in what login prints or hands to hooks | high | mitigate | login.rs:20,103-106 hooks get the URL only; auth.rs:195-210,275-287 fixed phrases; nothing_secret_reaches_the_hooks_or_the_summary; nothing_secret_is_printed | closed |
| T-06-28 | Tampering | a failed login clobbering or half-writing an existing login | high | mitigate | login.rs:110-137 writes only after a successful exchange, under lock, atomic rename; login.rs:350,390,413,440 | closed |
| T-06-29 | Denial of Service | a login that hangs or holds the port | medium | mitigate | callback.rs:34 5 min wait; client.rs:36,127 30 s/10 s; listener consumed; login.rs:93 binds first; a_second_login_on_the_same_port_fails_naming_the_key | closed |
| T-06-30 | Tampering | path traversal through the CLI plugin name | high | mitigate | auth.rs:110-137 find_source matches registry names before any path or socket; store.rs:167-174 re-checks; login_refuses_an_unknown_plugin_listing_the_registered_ones | closed |
| T-06-SC (06-07) | Tampering | npm/pip/cargo installs | high | mitigate | 7c8fcfa adds open 5 (+ is-docker, is-wsl), within the plan's limit; recorded in 06-07-SUMMARY.md | closed |
| T-06-31 | Information Disclosure | token values or file content in status or logout output, JSON, warnings or errors | high | mitigate | auth.rs:343-394,519-536 rows from states, times, names, cleaned text; fixed 5-key JSON with no token field; status_lists_each_state_without_token_values; logout_never_prints_the_tokens | closed |
| T-06-32 | Information Disclosure | tokens posted to an unintended endpoint on logout | high | mitigate | auth.rs:694-701 no request without revocation_url; client.rs:208-228 posts only to the https-validated URL via the no-redirect client; failure text is status/transport only | closed |
| T-06-33 | Tampering | a refresh or login racing logout and resurrecting a deleted login | high | mitigate | auth.rs:680-682 delete under lock; provider.rs:151-155 re-read returns NotLoggedIn; a_refresh_never_resurrects_a_logged_out_plugin (re-run green) | closed |
| T-06-34 | Denial of Service | a slow or failing provider keeping the user logged in locally | medium | mitigate | auth.rs:680-683 local delete before revoke; client 30 s timeout; logout_posts_the_refresh_token_to_the_revocation_url_and_deletes_first | closed |
| T-06-SC (06-08) | Tampering | npm/pip/cargo installs | high | mitigate | No 06-08 commit touches Cargo.toml or Cargo.lock | closed |
| T-06-35 | Information Disclosure | refresh token or client secret reaching a plugin | high | mitigate | scheduler.rs:2499 guest inputs lack refresh token, rotated token and secret; wasm_tests.rs:959,981,1008 real wasm via Host::run and poll_once (green after fixture rebuild) | closed |
| T-06-36 | Information Disclosure | credentials in documentation, examples or binary output | medium | mitigate | README.md:560-570 placeholders only; examples/ unchanged since 1fd00e7; tests/auth_cli.rs:364 asserts no tokens on both streams | closed |
| T-06-SC (06-09) | Tampering | npm/pip/cargo installs | high | mitigate | stub-source Cargo files unchanged since 1fd00e7; root Cargo files untouched by 06-09 | closed |

*Status: open · closed · open — below high threshold (non-blocking)*
*Severity: critical > high > medium > low — only open threats at or above workflow.security_block_on count toward threats_open*
*Disposition: mitigate (implementation required) · accept (documented risk) · transfer (third-party)*
*Each T-06-SC row is one plan's supply-chain row; the auditor checked per-plan commit scope because a plain `git diff --quiet -- Cargo.toml Cargo.lock` now fails (06-04 and 06-07 legitimately changed them).*

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-6-01 | T-06-25 | Expiry uses wall-clock time against a fixed 60 s margin, so clock skew can refresh early or late; a refused token routes to `needs_login`; documented at plugins/README.md:696-698 | plan author (planned accept; verified by gsd-security-auditor) | 2026-11-11 |

*Accepted risks do not resurface in future audit runs.*

### Auditor notes (none changes a verdict)

- T-06-08: "wrong owner" is a plugin-name check; there is no OS uid check (stated in the 06-02 plan).
- T-06-20/27: a failed login prints the provider's cleaned `error` and `error_description` (up to 160 chars); a provider that echoes a code there would reach the terminal. This is the declared design.
- T-06-06: an existing `auth` directory is never tightened (0700 applies at creation); files are 0600 and checked on every read. Mode checks are skipped on non-Unix.
- T-06-14/15: one `/callback` hit with a wrong or missing state ends the login (fail-closed; see review finding WR-01). State is compared with `!=`, negligible over loopback.
- T-06-10: only `authorize_params` is screened for standard keys; a query already on `authorize_url` is not (the plugin author controls that URL anyway).
- T-06-35: the real-wasm tests show the guest never saw the refresh token or secret; the "host used the secret" proof is the fake-level scheduler test.
- Outside scope: `src/crawler.rs:68` has `danger_accept_invalid_certs(true)`; it predates Phase 6 and the token client does not use it.

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-11-11 | 45 (36 numbered + 9 T-06-SC rows) | 45 | 0 | gsd-security-auditor (ASVS L1, block_on high); 6 targeted tests re-run green by the auditor; full suite 632 + 10 + 12 and wasm group 44 + 3 green per 06-VALIDATION.md |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-11-11
