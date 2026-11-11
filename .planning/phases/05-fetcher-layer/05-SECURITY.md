---
phase: "5"
slug: "fetcher-layer"
status: verified
# threats_open = count of OPEN threats at or above workflow.security_block_on severity (the blocking gate)
threats_open: 0
asvs_level: 1
created: "2026-11-11"
---

# Phase 5 — Security

> Per-phase security contract: threat register, accepted risks, and audit trail.

---

## Trust Boundaries

| Boundary | Description | Data Crossing |
|----------|-------------|---------------|
| source plugin → host `fetch` import | A guest source asks the host to fetch; the host checks the request before any fetcher runs | request url, headers, body (guest-controlled) |
| host → fetcher component | The assigned fetcher runs in a fresh, resource-limited instance and can reach the network only through the host's `send` | request and response data, possibly credentials in headers |
| fetcher → host `outbound-http.send` | The host re-applies the calling consumer's allowlist, redirect, size and deadline policy on every send | request url and headers (fetcher-controlled) |
| host → network | One shared HTTP client with certificate validation on, redirects followed by the host | TLS sessions, response bodies (untrusted, 8 MiB cap) |
| `filimon.toml` registry → host | Operator-supplied `fetcher` and `[crawler]` assignments | component names, allowlist hosts, timeouts |
| crawler (`fili produce`) → fetcher | Optional route for news-page fetches, same policy type as sources | page urls, HTML text |

---

## Threat Register

| Threat ID | Category | Component | Severity | Disposition | Mitigation | Status |
|-----------|----------|-----------|----------|-------------|------------|--------|
| T-5-01 | Elevation of Privilege | a source requests a host outside its allowlist (SSRF to internal services) | high | mitigate | fetch.rs:604 policy.check before any instance; Egress::run :341; wasm_denied_host_never_instantiates_the_fetcher, wasm_default_fetcher_is_refused_a_host_outside_the_allowlist | closed |
| T-5-02 | Denial of Service | a slow or hanging fetcher stalls a source poll forever | high | mitigate | timeout_at deadline fetch.rs:612, :328; NonZeroU64 timeouts config.rs:26; egress_deadline_expires_on_a_slow_server, wasm_spinning_custom_fetcher_times_out_and_the_route_survives | closed |
| T-5-03 | Information Disclosure | one source's request data reaching another source through a shared fetcher component | medium | mitigate | fresh store + instantiate_async per call fetch.rs:638-640; wasm_concurrent_fetches_never_share_a_fetcher_instance | closed |
| T-5-04 | Denial of Service | a fetcher guest that traps or exhausts memory takes down the host | medium | mitigate | new_guest_store with store_limits (engine.rs:81-93); failures map to Transport (fetch.rs:642-665); engine.rs:353-395 limit tests | closed |
| T-5-05 | Spoofing | allowlist confusion through case, a trailing dot, userinfo or a look-alike suffix | high | mitigate | Allowlist::new normalizes via url::Host::parse, exact BTreeSet match (fetch.rs:105,126-135); a_url_with_userinfo_is_judged_by_its_real_host | closed |
| T-5-06 | Tampering | non-http schemes such as `file:` used to read local data | medium | mitigate | check_url accepts http/https only (fetch.rs:243-247) and runs on every hop (:382); only_http_and_https_urls_pass_the_policy | closed |
| T-5-07 | Information Disclosure | the default fetcher logging request urls, headers or bodies (query strings and bearer tokens travel there) | low | mitigate | single fixed log line plugins/http-fetcher/src/lib.rs:29; no request.url in the crate | closed |
| T-5-08 | Elevation of Privilege | a custom fetcher calling `send` for a host the source may not reach | high | mitigate | EgressGrant::send -> Egress::execute -> policy.check (fetch.rs:544-551,341); send linked only in the fetcher linker (engine.rs:288-291); wasm_custom_fetcher_cannot_send_outside_the_allowlist | closed |
| T-5-09 | Elevation of Privilege | an open redirect on an allowed host leading to an internal or non-allowlisted host (SSRF) | high | mitigate | redirect::Policy::none() (fetch.rs:304); host follows <= 5 hops with check_url per hop (:39,367,382); egress_denies_a_redirect_to_a_host_outside_the_allowlist | closed |
| T-5-10 | Spoofing | certificate validation disabled, or inherited from the crawler's sandbox client | high | mitigate | no danger_accept/accept_invalid switch on the fetch path; guard the_fetch_path_has_no_invalid_certificate_switch (https_tests.rs:288); behavioural proof is T-5-27 | closed |
| T-5-11 | Denial of Service | an unbounded response body exhausts host or guest memory | medium | mitigate | MAX_RESPONSE_BYTES 8 MiB, read_capped (fetch.rs:36,443-461); egress_rejects_a_response_body_over_the_cap | closed |
| T-5-12 | Information Disclosure | credentials forwarded to another origin by a redirect | medium | mitigate | credentials dropped when a redirect changes origin (fetch.rs:393-395); egress_drops_credentials_when_a_redirect_changes_host | closed |
| T-5-13 | Spoofing | DNS rebinding or a private address behind an allowlisted name | low | accept | accepted: operator-controlled hostnames, no address filtering; documented in plugins/README.md Known limits | closed |
| T-5-14 | Tampering | a guest choosing the virtual host, message framing or an oversized request to defeat the allowlist or exhaust memory | medium | mitigate | FetchPolicy::check size, header and forbidden-header rules (fetch.rs:189-234); six named tests (fetch.rs:1365-1459) | closed |
| T-5-15 | Information Disclosure | bearer tokens, API keys and query-string secrets leaking into logs or into error text returned to the plugin | medium | mitigate | logs carry only consumer, method, host, status, body_len (fetch.rs:419-426); egress_never_logs_headers_bodies_or_query_strings | closed |
| T-5-16 | Information Disclosure | the test harness leaving key material or a listening server behind | low | mitigate | per-test TempDir, no key material committed, binds 127.0.0.1, Drop kills child (https_tests.rs:54,105,155-160) | closed |
| T-5-17 | Tampering | `net_` tests contacting third-party public hosts from a developer machine | low | accept | accepted: net_ tests ignored by default, fixed public hosts, no secret, never gating | closed |
| T-5-18 | Elevation of Privilege | the `forward` and `spin` fixture modes being registered in a real registry | low | accept | accepted: forward/spin modes live only in a test fixture, never in a sample registry; README says host-test only | closed |
| T-5-19 | Tampering | `run-once` running a source without its assigned fetcher, or with another one, and so producing different behaviour from `fili run` | medium | mitigate | load_route_for_source returns FetcherUnavailable naming both plugins (loader.rs:190-224); run_once_names_both_plugins_when_the_assigned_fetcher_cannot_load | closed |
| T-5-20 | Information Disclosure | a fetcher reading every header the source sends, including bearer tokens | medium | accept | accepted: fetchers are operator-registered; sends capped to the allowlist; README says register only trusted fetchers | closed |
| T-5-21 | Elevation of Privilege | the crawler's fetcher being used to reach internal or arbitrary hosts (SSRF) through a seed, a link or a redirect | high | mitigate | CrawlFetcher::load builds seed-host allowlist under the same FetchRoute policy (fetch.rs:776-789); wasm_crawler_denies_a_redirect_to_a_host_that_is_not_a_seed_host | closed |
| T-5-22 | Denial of Service | a hung fetcher or server stalling the whole crawl | medium | mitigate | [crawler] timeout_secs default 15 (config.rs:31); crawl loop continues past a failed page; wasm_a_hung_server_is_abandoned_at_the_crawler_timeout_and_the_crawl_moves_on | closed |
| T-5-23 | Spoofing | a silent fallback to the built-in client, which skips certificate validation, when `[crawler]` is configured but unusable | medium | mitigate | select_page_fetcher falls back only when no registry file or no [crawler] table (produce.rs:79-98); an_invalid_crawler_table_is_an_error_and_never_a_fallback | closed |
| T-5-24 | Tampering | over-broad `[crawler] allowlist` entries written by the operator | low | accept | accepted: extra crawler hosts are an explicit operator choice, normalized and exact-matched | closed |
| T-5-25 | Tampering | fetched page content being hostile input to the pipeline | low | accept | accepted: same HTML text as the built-in client, 8 MiB cap, no new parsing | closed |
| T-5-26 | Repudiation | documentation claiming protections the code does not give (or hiding limits it has), leading an operator to a false sense of safety | low | mitigate | README tables match every constant in fetch.rs; stale interim text removed; wit diff is comments only | closed |
| T-5-27 | Spoofing | certificate validation disabled, broken or silently skipped on the fetch path (plan 05-03 owns the mitigation, see T-5-10) | high | mitigate | tls_ tests: self-signed, wrong-name, default-fetcher rejected, trusted root accepted; always-on guard; with_extra_root is cfg(test) only | closed |
| T-5-SC | Tampering | supply chain, one row in each of plans 05-01..05-08 | high | mitigate | root Cargo.toml and Cargo.lock unchanged since e6cc3f0; plugins/http-fetcher/Cargo.lock has the same packages and checksums as stub-fetcher | closed |

*Status: open · closed · open — below high threshold (non-blocking)*
*Severity: critical > high > medium > low — only open threats at or above workflow.security_block_on count toward threats_open*
*Disposition: mitigate (implementation required) · accept (documented risk) · transfer (third-party)*

T-5-10 and T-5-27 describe one property (certificate validation stays on). 05-03 owns the mitigation (T-5-10) and 05-05 owns its behavioural proof; the 05-05 row was renumbered from a duplicate T-5-10 to T-5-27 before execution.

---

## Accepted Risks Log

| Risk ID | Threat Ref | Rationale | Accepted By | Date |
|---------|------------|-----------|-------------|------|
| AR-5-01 | T-5-13 | allowlist is operator-controlled hostnames; the host does not resolve or filter addresses, so a name that resolves privately is the operator's choice (plugins/README.md Known limits) | plan author (planned accept; verified by gsd-security-auditor) | 2026-11-11 |
| AR-5-02 | T-5-17 | net_ tests are ignored by default, use fixed well-known hosts, send one GET each, carry no secret and are never part of a gating command | plan author (planned accept; verified by gsd-security-auditor) | 2026-11-11 |
| AR-5-03 | T-5-18 | forward/spin modes exist only in a test fixture never listed in a sample registry; README states the fixtures are host-test only | plan author (planned accept; verified by gsd-security-auditor) | 2026-11-11 |
| AR-5-04 | T-5-20 | a fetcher is operator-registered and sees request headers by necessity; its sends are capped to the source allowlist and the README says to register only trusted fetchers | plan author (planned accept; verified by gsd-security-auditor) | 2026-11-11 |
| AR-5-05 | T-5-24 | extra [crawler] allowlist hosts are an explicit operator choice, normalized and exact-matched; default is the seed hosts only | plan author (planned accept; verified by gsd-security-auditor) | 2026-11-11 |
| AR-5-06 | T-5-25 | the fetcher path returns the same HTML text as the built-in client, capped at 8 MiB, with no new parsing | plan author (planned accept; verified by gsd-security-auditor) | 2026-11-11 |

*Accepted risks do not resurface in future audit runs.*

Informational (not a finding): `src/crawler.rs:68` keeps the pre-existing `danger_accept_invalid_certs(true)` on the built-in blocking client. It is unchanged by this phase, off the fetch path, selected only when no registry file or no `[crawler]` table exists, and documented in README.md and plugins/README.md.

---

## Security Audit Trail

| Audit Date | Threats Total | Closed | Open | Run By |
|------------|---------------|--------|------|--------|
| 2026-11-11 | 35 (27 numbered + 8 T-5-SC rows) | 35 | 0 | gsd-security-auditor (ASVS L1, block_on high); ignored wasm_/tls_ groups re-run green by the orchestrator (46 + 3 passed) |

---

## Sign-Off

- [x] All threats have a disposition (mitigate / accept / transfer)
- [x] Accepted risks documented in Accepted Risks Log
- [x] `threats_open: 0` confirmed
- [x] `status: verified` set in frontmatter

**Approval:** verified 2026-11-11
