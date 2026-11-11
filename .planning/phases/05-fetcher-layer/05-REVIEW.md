---
phase: 05-fetcher-layer
reviewed: 2026-11-11T00:00:00Z
depth: standard
files_reviewed: 24
files_reviewed_list:
  - README.md
  - examples/plugins/filimon.toml
  - plugins/README.md
  - plugins/http-fetcher/Cargo.toml
  - plugins/http-fetcher/src/lib.rs
  - plugins/stub-fetcher/src/lib.rs
  - plugins/stub-source/src/lib.rs
  - src/commands/plugins.rs
  - src/commands/produce.rs
  - src/crawler.rs
  - src/main.rs
  - src/plugins/config.rs
  - src/plugins/error.rs
  - src/plugins/host/engine.rs
  - src/plugins/host/error.rs
  - src/plugins/host/fetch.rs
  - src/plugins/host/https_tests.rs
  - src/plugins/host/instance.rs
  - src/plugins/host/loader.rs
  - src/plugins/host/mod.rs
  - src/plugins/host/testing.rs
  - src/plugins/host/wasm_tests.rs
  - wit/host.wit
  - wit/types.wit
findings:
  critical: 0
  warning: 3
  info: 4
  total: 7
  necessity: 0
status: issues_found
---

# Phase 5: Code Review Report

**Reviewed:** 2026-11-11
**Depth:** standard
**Files Reviewed:** 24
**Status:** issues_found

## Summary

Reviewed the Phase 5 fetcher layer (`fetch.rs` plus wiring in `engine.rs`, `loader.rs`, `mod.rs`, `produce.rs`, `crawler.rs`, `config.rs`) against `git diff 17f6221 HEAD`. The core enforcement is sound. Policy is checked before any fetcher starts and again on every `send` and every redirect hop. Scheme, size and header rules are enforced. Response bodies are capped while streaming. The one deadline covers redirects and the body read. Transport errors strip the url. Credential stripping on origin change works for the standard headers. I found no critical defect and no bypass of the host allowlist.

The defects are at the edges. A mistyped `--registry` silently selects the built-in client that skips certificate validation. Config validation of allowlist entries disagrees with the runtime allowlist (and the docs). Redirect credential stripping misses common custom auth headers. The remaining items are minor.

The pre-existing `danger_accept_invalid_certs(true)` on the built-in client is out of scope as instructed. WR-01 concerns only the new fallback decision that routes users onto it.

The necessity rubric was read; no necessity findings were raised.

## Warnings

### WR-01: A mistyped explicit `--registry` / `FILI_REGISTRY` path silently selects the certificate-skipping built-in client

**File:** `src/commands/produce.rs:62-68` (rubric text at `src/commands/produce.rs:6-10`, doc at `README.md` "Crawling through a fetcher component")
**Issue:** `select_page_fetcher` treats `registry.try_exists() == Ok(false)` as "no crawler configured" and returns `PageFetcher::BuiltIn`. `cli.registry` has a default (`filimon.toml`, `src/cli.rs:51`), so a missing file cannot be told apart from an operator who passed `--registry crawler.toml` (or set `FILI_REGISTRY`) with a typo or a wrong working directory. That operator asked for the fetcher path and gets the built-in client with `danger_accept_invalid_certs(true)`. The only signal is one `println!` line. This contradicts the module header ("never falls back to the built-in client ... from a `[crawler]` that cannot be used") and the README guarantee that a crawler table that cannot be used is an error and nothing is crawled. The user intent that was expressed (use this registry) is the one that is dropped.
**Fix:** Distinguish explicit from default. For example, give the CLI field no `default_value` and resolve the default in code (`Option<PathBuf>`), or compare against `DEFAULT_REGISTRY` and the env/arg source via `ArgMatches::value_source`. Fall back silently only when the path is the default and absent; return a `RegistryUnreadable` error when the path was explicit:
```rust
if matches!(registry.try_exists(), Ok(false)) && !explicit_registry {
    /* built-in, as today */
}
// explicit path that does not exist falls through to load_crawler, which reports RegistryUnreadable
```

### WR-02: Config validation accepts allowlist entries the runtime allowlist rejects (wildcards), and `[crawler].allowlist` is not validated at all

**File:** `src/plugins/config.rs:871-881` (validation), `src/plugins/host/fetch.rs:101-112` (runtime), `plugins/README.md` "What the host enforces" table (claims such an entry is a registry error)
**Issue:** `host_problems` validates each `[host] allowlist` entry with only `url::Host::parse(entry).is_err()`. That accepts `*.reddit.com` (verified: `url::Host::parse("*.reddit.com")` returns `Ok(Domain("*.reddit.com"))`) and `reddit.com.`. `Allowlist::new` then rejects any entry containing `*`, so the entry can never match and the only feedback is a `tracing::warn!` at load time. The README states that an entry that is not a bare host name "is a registry error", which is false for wildcards. An operator who writes `*.reddit.com` expecting subdomain matching gets a registry that loads cleanly and a source whose every request is denied. `[crawler].allowlist` (`CrawlerEntry.allowlist`, `config.rs:67`, `loaded_crawler` `config.rs:761`) gets no validation whatsoever; `fetch.rs` only logs a warning.
**Fix:** Make one function the single definition of "bare host entry" and call it from both places. For example, expose `Allowlist::entry_is_valid(&str) -> bool` (trim, strip one trailing dot, reject empty and `*`, then `Host::parse`) and use it in `host_problems` and for `CrawlerEntry.allowlist` (a new `InvalidValue` problem with the entry span). Add a config test for `*.example.com`.

### WR-03: Redirect credential stripping leaves custom auth headers in place across origins

**File:** `src/plugins/host/fetch.rs:393-395`
**Issue:** On a cross-origin redirect only `authorization`, `cookie` and `proxy-authorization` are removed. Plugins commonly authenticate with `x-api-key`, `x-auth-token`, `x-csrf-token` and similar. Those headers are forwarded verbatim to the redirect target. The target must be on the same allowlist, so this is limited to a source or crawler that lists several hosts, but that is exactly the case the cross-origin stripping exists for, and the WIT doc and README describe the behaviour as "removes credentials when a redirect changes origin". The comment/doc promise is stronger than the implementation.
**Fix:** Either invert the rule (on origin change keep only an allowlist of safe, non-credential headers such as `accept*`, `user-agent`, `content-type`, `range`), or document in `wit/host.wit`, `plugins/README.md` and the code comment that only the three standard credential headers are stripped and that custom credential headers are the plugin's responsibility.

## Info

### IN-01: Error text for a non-2xx page differs from the built-in client but is documented as the same wording

**File:** `src/plugins/host/fetch.rs:715-733`
**Issue:** The doc says `HTTP  <status>` is "the built-in client's own wording". The built-in path prints `format!("HTTP  {}", resp.status())` where `StatusCode`'s `Display` is `404 Not Found`; the component path prints `HTTP  404`. Both also carry a double space that looks like a typo copied from `crawler.rs:79`. The test pins `starts_with("HTTP  404")`, which hides the difference.
**Fix:** Keep one formatter. Either drop the doc claim, or format with the canonical reason (`http::StatusCode::from_u16(status).map(|s| s.to_string())`) and fix the double space in both places.

### IN-02: The shared client honours proxy environment variables, which the documented guarantees do not mention

**File:** `src/plugins/host/fetch.rs:303-304`
**Issue:** `reqwest::Client::builder()` keeps reqwest's default system-proxy lookup (`HTTP_PROXY`, `HTTPS_PROXY`, `ALL_PROXY`, `NO_PROXY`). The allowlist is judged on the url's host name, but traffic for `http://` urls then goes to whatever proxy the environment names. The README documents the DNS-rebinding limit and the bundled-roots limit but not this. It is an operator-controlled environment, so it is not a bypass, but it changes the effective network path of an "allowlisted" request.
**Fix:** Either call `.no_proxy()` for deterministic egress, or add one line to the README trust notes that environment proxy settings apply.

### IN-03: Stale annotation and planning identifiers left in source

**File:** `src/plugins/host/fetch.rs:291` and throughout (`WD-11`, `WD-13`, `WD-14`, `FETCH-01`, "plan 05-05")
**Issue:** `with_extra_root` still carries `#[allow(dead_code, reason = "used by the tls_ tests in plan 05-05")]`, but `https_tests.rs` now uses it, so the allow is dead and its reason is a planning-document reference that will rot. The `WD-` and `FETCH-` ids point at phase artifacts that are archived at ship.
**Fix:** Remove the `allow`, and keep comments self-contained (state the rule, not the planning id).

### IN-04: Duplicated test helpers and a timing-sensitive assertion

**File:** `src/plugins/host/fetch.rs:1010-1019`, `src/plugins/host/wasm_tests.rs` (`start_with_port`, `get`), `src/commands/produce.rs` (`start_with_port`); `src/plugins/host/wasm_tests.rs` `wasm_a_slow_call_does_not_time_out_a_fast_concurrent_call`
**Issue:** `start_with_port` is copy-pasted three times and `get`/`request` helpers twice. Move it into `testing.rs` next to `TestHttpServer`. Separately, the concurrency test asserts `fast_done < 800 ms` against a 1 s timeout; that figure includes fresh wasm instantiation and will flake on a loaded CI machine. The test already proves the property with `fast_done < slow_done`.
**Fix:** Move the helper to `testing.rs` and widen or drop the absolute 800 ms bound.

---

_Reviewed: 2026-11-11_
_Reviewer: Claude (gsd-code-reviewer)_
_Depth: standard_
