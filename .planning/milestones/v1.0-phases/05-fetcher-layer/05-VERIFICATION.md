---
phase: 05-fetcher-layer
verified: 2026-11-11T12:00:00Z
status: passed
score: 8/10 must-haves verified
covered_files:
  - ".planning/phases/05-fetcher-layer/05-01-PLAN.md"
  - ".planning/phases/05-fetcher-layer/05-01-SUMMARY.md"
  - ".planning/phases/05-fetcher-layer/05-02-PLAN.md"
  - ".planning/phases/05-fetcher-layer/05-02-SUMMARY.md"
  - ".planning/phases/05-fetcher-layer/05-03-PLAN.md"
  - ".planning/phases/05-fetcher-layer/05-03-SUMMARY.md"
  - ".planning/phases/05-fetcher-layer/05-04-PLAN.md"
  - ".planning/phases/05-fetcher-layer/05-04-SUMMARY.md"
  - ".planning/phases/05-fetcher-layer/05-05-PLAN.md"
  - ".planning/phases/05-fetcher-layer/05-05-SUMMARY.md"
  - ".planning/phases/05-fetcher-layer/05-06-PLAN.md"
  - ".planning/phases/05-fetcher-layer/05-06-SUMMARY.md"
  - ".planning/phases/05-fetcher-layer/05-07-PLAN.md"
  - ".planning/phases/05-fetcher-layer/05-07-SUMMARY.md"
  - ".planning/phases/05-fetcher-layer/05-08-PLAN.md"
  - ".planning/phases/05-fetcher-layer/05-08-SUMMARY.md"
  - "README.md"
  - "examples/plugins/filimon.toml"
  - "plugins/README.md"
  - "plugins/http-fetcher/src/lib.rs"
  - "plugins/stub-fetcher/src/lib.rs"
  - "plugins/stub-source/src/lib.rs"
  - "src/commands/plugins.rs"
  - "src/commands/produce.rs"
  - "src/crawler.rs"
  - "src/main.rs"
  - "src/plugins/config.rs"
  - "src/plugins/error.rs"
  - "src/plugins/host/engine.rs"
  - "src/plugins/host/error.rs"
  - "src/plugins/host/fetch.rs"
  - "src/plugins/host/https_tests.rs"
  - "src/plugins/host/instance.rs"
  - "src/plugins/host/loader.rs"
  - "src/plugins/host/mod.rs"
  - "src/plugins/host/testing.rs"
  - "src/plugins/host/wasm_tests.rs"
  - "wit/host.wit"
  - "wit/types.wit"
covered_digest: "v4:sha256:d237d8f5ac5284afa9186818a1e36f15550c3ec24cddf2ed52b1a2d5a0c0e20d"
behavior_unverified: 2
overrides_applied: 0
behavior_unverified_items:
  - truth: "Roadmap criterion 4 (end-to-end half): with the crawler configured to use a fetcher component, `fili produce <url>` fetches pages through that component and articles still reach the consumer through Fluvio"
    test: "Start a Fluvio cluster, add a `[crawler] fetcher = \"http\"` table to the registry, run `fili produce <url> --depth 1`, then `fili consume` on another terminal"
    expected: "Pages are fetched through the http-fetcher component (the run prints `[Crawl] fetching through the fetcher ...`) and articles arrive at the consumer exactly as with the built-in client"
    why_human: "Needs a live Fluvio cluster and the SmartModule chain. The fetcher half is proven by wasm_crawl_visits_the_seed_and_its_ranked_links_through_the_component against a loopback server. The producer send is a pre-existing path (`SourceItem::news(url, html)`), and no test exercises it against a broker."
  - truth: "Edge FETCH-04/concurrency (backstop): interrupting `fili produce` in the middle of a crawl leaves only complete records in the topic"
    test: "Run `fili produce` through a fetcher component against a live cluster, press Ctrl-C mid-crawl, then read the raw-pages topic"
    expected: "Every record in the topic is a complete `SourceItem` with a url and html; no half record"
    why_human: "Declared `verification: backstop` (non-inferable) in 05-07-PLAN. It depends on Fluvio producer semantics, and no test exercises it. The plan's reasoning (one send per page, `produce` send path unchanged) is plausible but is not evidence."
human_verification:
  - test: "Live crawl through a fetcher component (roadmap criterion 4)"
    expected: "With a Fluvio cluster up and `[crawler] fetcher = \"http\"`, `fili produce <url>` fetches via the component and the consumer receives the articles"
    why_human: "Live Fluvio cluster required; not run by the verifier"
  - test: "Ctrl-C during a fetcher-component crawl"
    expected: "Only complete records in the topic"
    why_human: "Backstop truth; live cluster required"
  - test: "Prohibition review: 05-07 'MUST NOT fall back to the built-in client when a `[crawler]` cannot be used' (flagged: unverified-prohibition, human review recommended)"
    expected: "Decide whether a missing registry path that the operator passed explicitly (`--registry typo.toml` or `FILI_REGISTRY`) must be an error. Today `select_page_fetcher` treats any non-existent registry path as 'no crawler' and uses the built-in client, which has `danger_accept_invalid_certs(true)`. This is review finding WR-01. It matches the plan prohibition's literal wording ('no registry file') but not the README and module-header claim."
    why_human: "Judgment-tier prohibition. A verdict on intent versus literal wording is the developer's call. Recorded as a non-authoritative LLM judgment: literal text satisfied, spirit partly unmet for the explicit-path case."
  - test: "Prohibition review: 05-04 'MUST NOT log request headers, bodies, or url path/query' and 05-01 'MUST NOT silently substitute another fetcher' (flagged: unverified-prohibition, human review recommended)"
    expected: "Reviewer confirms the evidence below is enough. Non-authoritative verdict: both hold."
    why_human: "Judgment-tier prohibitions; the verifier's verdict is not authoritative"
  - test: "Optional: `cargo test -- --ignored net_` with internet access"
    expected: "The public HTTPS site succeeds with validation on; badssl self-signed, expired, wrong-host and untrusted-root hosts are rejected"
    why_human: "Needs internet. Not required for the goal: offline `tls_` tests already prove rejection of a self-signed certificate and of a wrong name, and acceptance of a trusted root, through the production client and through the real default fetcher component."
---

# Phase 5: Fetcher Layer Verification Report

**Phase Goal:** All outbound HTTP from source plugins, and optionally from the news crawler, goes through a swappable fetcher component. The host enforces allowlists and timeouts whichever fetcher handles the request.
**Verified:** 2026-11-11
**Status:** human_needed
**Re-verification:** No, initial verification

The code achieves the goal. No truth is FAILED and no blocker was found. The status is `human_needed` because two truths need a live Fluvio cluster, and three prohibitions are judgment-tier. The three open code-review warnings (WR-01 to WR-03) are advisory and are recorded below.

## Goal Achievement

### Observable Truths

| #  | Truth | Status | Evidence |
|----|-------|--------|----------|
| 1  | SC1: A source's `fetch` is served by the fetcher the registry assigns it. Changing the assignment changes which component serves it, with no rebuild. | VERIFIED | `engine.rs:196-209` `fetch::Host::fetch` awaits `route.fetch` and returns `NotConfigured` only when no route is assigned. `mod.rs:142-165` builds one `FetchRoute` per source from `registry.sources[..].fetcher`. `fetch.rs:627-652` `call_fetcher` creates a fresh store and instance per call. Tests (ran green under `--ignored wasm_`): `wasm_changing_the_assigned_fetcher_changes_who_serves_the_request`, `wasm_tracer_fetch_is_served_by_the_assigned_fetcher`, `wasm_host_run_routes_a_source_through_the_registry_assigned_fetcher`, `wasm_concurrent_fetches_never_share_a_fetcher_instance`. |
| 2  | SC2: The host rejects with a clear error a request to a domain outside the allowlist and a request that exceeds its timeout, for the default fetcher and a custom fetcher. | VERIFIED | `FetchPolicy::check` / `check_url` (`fetch.rs:189-262`) run before any fetcher starts. `EgressGrant::send` runs the same policy on the fetcher's own `send`. `run()` re-checks every redirect hop (`fetch.rs:382`). A single deadline is applied with `timeout_at` in `FetchRoute::fetch` (`fetch.rs:612`) and `Egress::execute` (`fetch.rs:328`). Tests: `wasm_default_fetcher_is_refused_a_host_outside_the_allowlist`, `wasm_default_fetcher_is_denied_a_redirect_outside_the_allowlist`, `wasm_custom_fetcher_cannot_send_outside_the_allowlist`, `wasm_spinning_custom_fetcher_times_out_and_the_route_survives`, `egress_deadline_expires_on_a_slow_server`, and the exact-host matrix. The fetcher world imports only `log` and `outbound-http` (`wit/worlds.wit`), so a fetcher has no other way to reach the network. |
| 3  | SC3: The bundled default fetcher makes real HTTPS requests with certificate validation on. An invalid certificate fails. The crawler's disabled validation is not carried over. | VERIFIED | `plugins/http-fetcher/src/lib.rs` only forwards to `outbound-http.send`. The host client is `reqwest::Client::builder().redirect(none)` with no invalid-certs switch (`fetch.rs:300-314`). The only `danger_accept_invalid_certs` in the tree is the pre-existing built-in crawler client (`crawler.rs:68`) plus a test-only use and a guard string in `https_tests.rs`. `the_fetch_path_has_no_invalid_certificate_switch` enforces the absence. I ran `--ignored tls_`: `tls_a_self_signed_certificate_is_rejected`, `tls_a_certificate_for_another_name_is_rejected`, `tls_a_certificate_trusted_as_a_root_is_accepted`, `tls_default_fetcher_component_rejects_a_self_signed_certificate` all passed. |
| 4  | SC4: With the crawler configured to use a fetcher component, `fili produce <url>` fetches pages through that component and articles still reach the consumer. | PRESENT_BEHAVIOR_UNVERIFIED | Fetcher half VERIFIED. `produce.rs:select_page_fetcher` loads `[crawler]`, builds `CrawlFetcher` (`fetch.rs:768-806`) and `PageFetcher::Component` (`crawler.rs:92-111`). The one crawl loop emits `SourceItem::news(url, html)` exactly as before. Tests: `wasm_crawl_visits_the_seed_and_its_ranked_links_through_the_component`, `wasm_tracer_crawler_page_is_fetched_through_the_component`. Fluvio half: not run, see Human Verification 1. |
| 5  | A source whose assigned fetcher cannot load is not loaded or polled, and is reported naming both. There is no silent substitution of another fetcher or direct network access. | VERIFIED | `mod.rs:149-160` pushes a `FetcherUnavailable` load failure and `continue`s. A source with no `fetcher` key gets `NotConfigured`. Tests: `wasm_host_load_skips_a_source_whose_fetcher_cannot_load`, `run_once_names_both_plugins_when_the_assigned_fetcher_cannot_load`. |
| 6  | The crawler's fetcher reaches only the seed urls' hosts plus `[crawler] allowlist`. A redirect elsewhere is denied. A hung fetch is abandoned at `timeout_secs`. | VERIFIED | `CrawlFetcher::load` builds `Allowlist::new(seed_hosts + extra_hosts)` and a policy labelled `crawler` (`fetch.rs:787-796`). Tests: `wasm_crawler_denies_a_redirect_to_a_host_that_is_not_a_seed_host`, `wasm_crawler_extra_hosts_allow_a_redirect_target`, `wasm_a_hung_server_is_abandoned_at_the_crawler_timeout_and_the_crawl_moves_on`, `wasm_source_and_crawler_routes_enforce_the_same_policy`. |
| 7  | Request rules, redirect rules and response cap: forbidden and malformed headers, size limits, at most five redirects with credentials stripped across origins, an 8 MiB response cap, and a 429 passed through. | VERIFIED | `fetch.rs:36-70` (constants), `check` (`fetch.rs:189-234`), `run` (`fetch.rs:336-432`), `read_capped`. The full non-ignored suite passed (433 unit + 12 integration, 0 failed). |
| 8  | The built-in crawler client's code path is not edited or copied. Only its User-Agent literal became a shared constant. | VERIFIED | `git diff 17f6221 HEAD -- src/crawler.rs` shows only the `USER_AGENT` constant, the `PageFetcher` enum and an import. The `danger_accept_invalid_certs(true)` line is unchanged and not copied into `fetch.rs`. |
| 9  | The docs, WIT comments and sample registry describe the fetcher layer as built (plan 05-08), and the default fetcher is a registry entry, not part of the binary. | VERIFIED | `examples/plugins/filimon.toml` registers `[fetchers.http]` and a commented `[crawler]`. `plugins/README.md` has `## Fetcher layer`, `### Assigning a fetcher`, `### What the host enforces` and `### The crawler` (README:415 is the allowlist row). Sample-registry tests pass. One doc inaccuracy is recorded as WR-02. |
| 10 | Edge FETCH-04 backstop: interrupting `fili produce` mid-crawl leaves only complete records. | PRESENT_BEHAVIOR_UNVERIFIED | Declared `verification: backstop`. No test exercises it, and it needs a live cluster. See Human Verification 2. |

**Score:** 8/10 truths verified (2 present, behavior-unverified, both needing a live Fluvio cluster)

### Prohibitions (must-NOT)

All three are judgment-tier (no `verification: test` key). Each carries a non-authoritative verdict and the flag `unverified-prohibition - human review recommended`.

| Prohibition (plan) | Non-authoritative verdict | Evidence |
|--------------------|---------------------------|----------|
| No silent substitution of another fetcher or direct network access (05-01) | Holds | `mod.rs:149-160` skips the source. The `fetch` import with no route returns `NotConfigured`. The fetcher world exposes no other network import. |
| No headers, bodies, url path or query in logs or in error text returned to a plugin (05-04) | Holds | Every `tracing::` call in `fetch.rs` (lines 150, 256, 331, 396, 419, 606, 615, 663) logs only the consumer, host, method, status, body length or fixed text. Transport errors use `without_url()`. `check` returns fixed strings. Test `egress_never_logs_headers_bodies_or_query_strings` exists and passed in the full run. The only text outside host control is the guest's own error text, which is cleaned and clipped. |
| No fallback to the built-in client when a `[crawler]` cannot be used; built-in only when none is configured, and the run says which (05-07) | Literal text holds. Spirit partly unmet. | An invalid table, unknown or disabled fetcher, unparsable registry or unloadable component is an error (`produce.rs` `select_page_fetcher`; test `an_invalid_crawler_table_is_an_error_and_never_a_fallback`). The built-in client is announced with a printed line. A non-existent registry path also selects it, even when the operator passed that path explicitly (WR-01). |
| No allow-all for the crawler's fetcher (05-07) | Holds | The allowlist is the seed hosts plus `[crawler] allowlist` only (`fetch.rs:787`). Covered by the redirect tests above. |

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `src/plugins/host/fetch.rs` | Allowlist, FetchPolicy, Egress, EgressGrant, FetchRoute, CrawlFetcher | VERIFIED | 1967 lines, substantive, wired from `engine.rs`, `mod.rs`, `loader.rs` and `crawler.rs`. |
| `src/plugins/host/engine.rs` | HostState route and grant, `new_guest_store`, `fetch` and `send` imports | VERIFIED | Both imports delegate to the route and the grant. No stub. |
| `src/plugins/host/mod.rs`, `loader.rs`, `instance.rs`, `error.rs` | Route building, `load_fetcher`, `load_route_for_source`, `FetcherUnavailable` | VERIFIED | `FetchRoute::new` and `FetchPolicy::for_source` are used at `mod.rs:157` and `loader.rs:218`. |
| `plugins/http-fetcher/src/lib.rs` | Default fetcher forwarding to `send` | VERIFIED | Built component exists under `plugins/http-fetcher/target`. Loaded and run by the `--ignored wasm_` and `tls_` tests. |
| `src/plugins/host/testing.rs`, `https_tests.rs`, `wasm_tests.rs` | Test harness | VERIFIED | All named tests exist (checked with `--list`). |
| `src/plugins/config.rs`, `error.rs` | `[crawler]` table, `load_crawler`, diagnostics | VERIFIED | `load_crawler` reads only `[crawler]` and `[fetchers]`. |
| `src/commands/produce.rs`, `src/crawler.rs`, `src/main.rs` | `select_page_fetcher`, `PageFetcher`, registry passed in | VERIFIED | `main.rs:104` passes `&cli.registry`. The commented-out `run_produce` left in `main.rs` is dead text and pre-existing. |
| `examples/plugins/filimon.toml`, `plugins/README.md`, `README.md`, `wit/*.wit` | Docs and sample registry | VERIFIED | WIT changes are comments only. |

### Key Link Verification

| From | To | Via | Status |
|------|----|-----|--------|
| `engine.rs` fetch import | `fetch.rs` | `route.fetch(` (`engine.rs:208`) | WIRED |
| `engine.rs` send import | `fetch.rs` | `grant.send(` (`engine.rs:223`) | WIRED |
| `mod.rs` `Host::load` | `fetch.rs` | `FetchRoute::new(` and `host_engine.egress()` (`mod.rs:157-162`) | WIRED |
| `fetch.rs` | `engine.rs` | `HostState::for_fetcher` and `new_guest_store` (`fetch.rs:638-639`) | WIRED |
| `commands/plugins.rs` | `loader.rs` | `load_route_for_source` | WIRED |
| `produce.rs` | `config.rs` | `load_crawler(registry)` | WIRED |
| `crawler.rs` | `fetch.rs` | `PageFetcher::Component` calls `get_page` | WIRED |
| `examples/plugins/filimon.toml` | `http_fetcher.wasm` | `[fetchers.http]` | WIRED |

### Data-Flow Trace (Level 4)

| Artifact | Data | Source | Real data | Status |
|----------|------|--------|-----------|--------|
| `crawl` emit to producer | `RawPage` from `PageFetcher` | `CrawlFetcher::get_page` to `FetchRoute::fetch` to http-fetcher to `Egress` (reqwest) to the network | Yes (loopback server in tests) | FLOWING up to the producer. The producer to Fluvio leg is unchanged and not run. |
| Source `fetch` result | `types::Response` | `FetchRoute::fetch` to fetcher instance to `Egress` | Yes | FLOWING |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Full non-ignored suite | `cargo test --offline` | 433 unit + 12 integration pass, 0 failed (final integration line seen: `12 passed; 0 failed; 3 ignored`) | PASS |
| Real-wasm and local-TLS groups | `cargo test --offline -- --ignored wasm_ tls_` | 46 passed, 0 failed (unit); 3 passed, 0 failed (`plugins_cli`) | PASS |
| `net_` group (internet) | not run | n/a | SKIP (human item 5; not required) |
| Live Fluvio produce and consume | not run | n/a | SKIP (human items 1 and 2) |

### Probe Execution

No `probe-*.sh` scripts are declared by this phase. Skipped.

### Requirements Coverage

Every plan declares `requirements` in its frontmatter. All four IDs are accounted for in REQUIREMENTS.md, and no ID is mapped to Phase 5 without a plan.

| Requirement | Source plans | Description | Status | Evidence |
|-------------|--------------|-------------|--------|----------|
| FETCH-01 | 05-01, 05-06, 05-08 | `fetch` calls are routed to the fetcher configured per source in `filimon.toml` | SATISFIED | Truths 1 and 5 |
| FETCH-02 | 05-01, 05-03, 05-04, 05-06, 05-08 | The host enforces each source's allowlist and timeout whichever fetcher handles the request | SATISFIED | Truths 2 and 7 |
| FETCH-03 | 05-02, 05-03, 05-05, 05-08 | A default HTTP fetcher component ships, with TLS certificate validation on | SATISFIED | Truth 3 |
| FETCH-04 | 05-07, 05-08 | The news crawler can fetch pages through a fetcher component instead of its built-in client | SATISFIED for the fetcher half. The Fluvio hand-off is a human item. | Truths 4, 6 and 8 |

Orphaned requirements: none. REQUIREMENTS.md still shows these four as `[ ]` / `Pending`. The tracking update is left to the orchestrator.

### Anti-Patterns Found

No TBD, FIXME or XXX marker, and no TODO, in the phase's implementation files. No stub returns. The open review findings were checked against the code and are accurate.

| File | Line | Pattern | Severity | Impact |
|------|------|---------|----------|--------|
| `src/commands/produce.rs` | 62-68 | A non-existent explicit `--registry` path selects the built-in client (WR-01) | Warning | Only a `println!` signals a fallback to the client that skips certificate validation. It contradicts the README and module header. The plan prohibition's literal wording permits it. |
| `src/plugins/config.rs` | 872 | `Host::parse` accepts `*.x.com`, while `Allowlist::new` rejects it (WR-02). `[crawler].allowlist` is not validated, and `plugins/README.md:415` calls it a registry error. | Warning | An operator's wildcard entry loads cleanly and every request is denied. This fails closed. |
| `src/plugins/host/fetch.rs` | 393-395 | Cross-origin redirects strip only `authorization`, `cookie` and `proxy-authorization` (WR-03) | Warning | Custom credential headers such as `x-api-key` go to another allowlisted host. Only matters when a consumer allowlists several hosts. |
| `fetch.rs` | 291, 303-304, 715-733 | IN-01 to IN-04 (stale `allow`, proxy env, `HTTP  ` wording, duplicated helpers) | Info | Cosmetic or documentation. |

None of these is a blocker for the phase goal: host enforcement of the allowlist and timeouts holds for every fetcher, and no bypass was found. WR-01 is the only one that touches a plan prohibition, and it is raised as a human decision above.

### Human Verification Required

1. **Live crawl through a fetcher component (roadmap criterion 4).** With a Fluvio cluster up and `[crawler] fetcher = "http"` in the registry, run `fili produce <url> --depth 1` and `fili consume`. Expected: pages are fetched through the component and the consumer receives articles. Why human: a live cluster is needed.
2. **Ctrl-C mid-crawl.** Interrupt a fetcher-component crawl and read the topic. Expected: only complete records. Why human: backstop truth, live cluster.
3. **WR-01 decision.** Decide whether an explicitly passed, non-existent registry path should be an error rather than a silent built-in-client fallback. Recommended fix is in `05-REVIEW.md`. Why human: a judgment-tier prohibition.
4. **Confirm the other prohibition verdicts** (no secrets in logs, no fetcher substitution). Non-authoritative verdicts above: both hold.
5. **Optional: `cargo test -- --ignored net_`** with internet. The offline `tls_` proof already covers the criterion.

### Gaps Summary

There are no gaps. Every roadmap success criterion is backed by the code and by passing tests, apart from the live-cluster half of criterion 4 and the Ctrl-C backstop. Both need a human with a running Fluvio cluster. The three open review warnings are advisory. Fixing WR-01 and WR-02 is recommended before relying on the fetcher path for the crawler in production.

---

_Verified: 2026-11-11_
_Verifier: Claude (gsd-verifier)_
