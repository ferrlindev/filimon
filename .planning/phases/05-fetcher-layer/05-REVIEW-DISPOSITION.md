---
phase: 05-fetcher-layer
review: 05-REVIEW.md
status: all-open
---

# Phase 5 code review disposition

Advisory review (standard depth, 24 files): 0 Critical, 3 Warning, 4 Info. Nothing was fixed during execution; every finding is open. Triage with `/gsd-code-review 5 --fix` or in a gap plan.

| # | Severity | Summary | Disposition |
|---|----------|---------|-------------|
| 1 | Warning | WR-01: select_page_fetcher (produce.rs) treats a missing registry file as "no crawler", so a mistyped explicit `--registry` or `FILI_REGISTRY` path silently selects the built-in client, which skips certificate validation; only a println! signals it, contradicting the "never falls back" claim | open |
| 2 | Warning | WR-02: config host_problems accepts wildcard allowlist entries (e.g. `*.reddit.com`) that Allowlist::new rejects, so config loads clean and every request is denied; `[crawler].allowlist` is not validated; plugins/README.md wrongly calls it a registry error | open |
| 3 | Warning | WR-03: a cross-origin redirect strips only authorization, cookie and proxy-authorization; custom credential headers such as x-api-key and x-auth-token are forwarded to the redirect target (only matters with several allowlisted hosts) | open |
| 4 | Info | IN-01: non-2xx page error wording differs between the component path and the built-in client (both carry a double space) although documented as the same | open |
| 5 | Info | IN-02: the shared reqwest client honours proxy environment variables, which the README does not mention | open |
| 6 | Info | IN-03: stale `allow(dead_code)` on with_extra_root and planning ids (WD-, FETCH-) left in source comments | open |
| 7 | Info | IN-04: start_with_port duplicated across test modules; one wasm test has an absolute 800 ms bound that can flake under load | open |
