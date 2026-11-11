---
phase: 07-reddit-youtube-plugins
verified: 2026-11-11T16:00:00Z
status: human_needed
score: 8/9 must-haves verified
covered_files:
  - ".planning/phases/07-reddit-youtube-plugins/07-01-PLAN.md"
  - ".planning/phases/07-reddit-youtube-plugins/07-01-SUMMARY.md"
  - ".planning/phases/07-reddit-youtube-plugins/07-02-PLAN.md"
  - ".planning/phases/07-reddit-youtube-plugins/07-02-SUMMARY.md"
  - ".planning/phases/07-reddit-youtube-plugins/07-03-PLAN.md"
  - ".planning/phases/07-reddit-youtube-plugins/07-03-SUMMARY.md"
  - ".planning/phases/07-reddit-youtube-plugins/07-04-PLAN.md"
  - ".planning/phases/07-reddit-youtube-plugins/07-04-SUMMARY.md"
  - ".planning/phases/07-reddit-youtube-plugins/07-05-PLAN.md"
  - ".planning/phases/07-reddit-youtube-plugins/07-05-SUMMARY.md"
  - ".planning/phases/07-reddit-youtube-plugins/07-06-PLAN.md"
  - ".planning/phases/07-reddit-youtube-plugins/07-06-SUMMARY.md"
  - ".planning/phases/07-reddit-youtube-plugins/07-07-PLAN.md"
  - ".planning/phases/07-reddit-youtube-plugins/07-07-SUMMARY.md"
  - ".planning/phases/07-reddit-youtube-plugins/07-08-PLAN.md"
  - ".planning/phases/07-reddit-youtube-plugins/07-08-SUMMARY.md"
  - ".planning/phases/07-reddit-youtube-plugins/07-09-PLAN.md"
  - ".planning/phases/07-reddit-youtube-plugins/07-09-SUMMARY.md"
  - ".planning/phases/07-reddit-youtube-plugins/07-10-PLAN.md"
  - ".planning/phases/07-reddit-youtube-plugins/07-10-SUMMARY.md"
  - ".planning/phases/07-reddit-youtube-plugins/07-11-PLAN.md"
  - ".planning/phases/07-reddit-youtube-plugins/07-11-SUMMARY.md"
  - ".planning/phases/07-reddit-youtube-plugins/07-12-PLAN.md"
  - ".planning/phases/07-reddit-youtube-plugins/07-12-SUMMARY.md"
  - "examples/plugins/filimon.toml"
  - "plugins/reddit/src/api.rs"
  - "plugins/reddit/src/component.rs"
  - "plugins/reddit/src/cursor.rs"
  - "plugins/reddit/src/poll.rs"
  - "plugins/reddit/src/settings.rs"
  - "plugins/youtube/src/api.rs"
  - "plugins/youtube/src/component.rs"
  - "plugins/youtube/src/cursor.rs"
  - "plugins/youtube/src/poll.rs"
  - "plugins/youtube/src/settings.rs"
  - "src/plugins/auth/client.rs"
  - "src/plugins/auth/provider.rs"
  - "src/plugins/config.rs"
covered_digest: "v4:sha256:1ce73907a4a02b349924e93c0f8eac65660f8a7ebfe414cfdbe18a429ad3b71c"
behavior_unverified: 0
overrides_applied: 0
deferred:
  - truth: "Live confirmation that the compiled plugins stream real Reddit and YouTube content from the real APIs (research assumptions A1-A4, A11, A13)"
    addressed_in: "Phase 8"
    evidence: "Phase 8 SC2: 'The same integration tests run in live mode against the real APIs only when the required credentials are in .env' (TEST-03); SC3: 'A live end-to-end run works: fili loads the Reddit and YouTube plugins, polls the real APIs' (TEST-04)"
human_verification:
  - test: "With a real Reddit web-app client id and secret, run `fili --registry <registry> plugins run-once reddit` (app-only instance) against the real API"
    expected: "Real `new` posts from the configured subreddits are returned as items; no 401/403/404; the request carries the compliant `filimon:reddit:<ver> (by /u/<name>)` User-Agent. Confirms A3 (app-only access to `/r/{sub}/new` on oauth.reddit.com), A4 (error shapes) and the listing shape."
    why_human: "Every Reddit fixture is synthetic (D-21 ruling `synthetic`). No offline check can show that the real API answers these requests with this shape."
  - test: "After `fili auth login reddit-user`, run `plugins run-once reddit-user` once with `feed = \"home\"` and once with `feed = \"saved\"` and `username` set"
    expected: "The user's real home feed (endpoint `GET oauth.reddit.com/new`) and saved items (`/user/<name>/saved`, `history` scope, mixed t3/t1 children) return items."
    why_human: "Assumptions A1 (home endpoint and shape) and A2 (saved path, scope, kinds) are unconfirmed by any live capture. Needs a real Reddit login."
  - test: "With a real `YOUTUBE_API_KEY`, run `plugins run-once youtube` for a known channel; then, after `fili auth login youtube-user`, run `plugins run-once youtube-user`"
    expected: "Real recent uploads come back from the uploads playlist (`UC` to `UU`). The subscription run returns uploads for subscribed channels. The key is accepted in the `x-goog-api-key` header (A11) and Google error JSON has the assumed nesting (A13)."
    why_human: "All YouTube fixtures are synthetic. The header form of the key and the error JSON nesting are research assumptions that only a live call can confirm."
  - test: "Trigger or simulate a real YouTube `quotaExceeded` answer (or inspect a real 403 body) and watch the host log"
    expected: "The named message `YouTube quota exceeded (quotaExceeded): daily Data API quota exhausted; resets at midnight Pacific` appears and the poll backs off; it is not a silent empty poll."
    why_human: "The classification keys on `error.errors[0].reason` (A13), which was never seen in a real response."
---

# Phase 7: Reddit & YouTube Plugins Verification Report

**Phase Goal:** Two reference `.wasm` source components, built separately from `fili`, stream real Reddit and YouTube content through the host using only official APIs, and each is proven by offline fixture tests
**Verified:** 2026-11-11
**Status:** human_needed
**Re-verification:** No, initial verification

## Summary

Everything that can be proven without network access is proven. Both plugins exist as separately built components, load and check in the host, use only the official APIs, and pass their offline suites. The word "real" in the goal is the one thing that is not shown. By the user's D-21 ruling every fixture is synthetic. Six research assumptions that the plugins depend on (A1-A4, A11, A13) have never met a live API. The code is consistent with the documented API behavior, but it has not been observed to work against the real services. Live confirmation is scheduled in Phase 8 (TEST-03, TEST-04). It is routed here as human verification and not counted as passed.

No must-have truth failed.

## Goal Achievement

### Observable Truths

| # | Truth | Status | Evidence |
|---|-------|--------|----------|
| 1 | SC1: Reddit plugin returns new/hot posts for the subreddits in its TOML via the official API, with a compliant User-Agent, and backs off by rate-limit headers (mechanics, offline) | VERIFIED (offline) | `plugins/reddit/src/api.rs` builds `GET https://oauth.reddit.com/r/{sub}/{new\|hot}` with bearer token and the enforced UA. `settings.rs:316-337` builds `filimon:<app>:<ver> (by /u/<name>)` and rejects a missing or malformed contact. `poll.rs:76-112` maps 429 or `x-ratelimit-remaining < 1` to `RateLimited(reset secs)`. The host scheduler turns that into backoff (`scheduler.rs:182,215`; tests at 696-723). Native suite: 34 + 2 + 59 pass. Test `only_the_official_oauth_host_is_ever_requested` and `every_request_is_official_authenticated_and_carries_the_enforced_user_agent` pass. The UA header is not in the host's forbidden-header list (`fetch.rs:61`). |
| 2 | SC2: After login, the Reddit plugin can poll the user's home feed or saved posts | VERIFIED (offline), realised as the `reddit-user` instance | `poll.rs:170-225` implements `FeedKind::Home` (`/new`) and `FeedKind::Saved` (`/user/<name>/saved`, seen-set state because saved is ordered by save time). `examples/plugins/reddit-user.toml` uses the authorization-code flow with `read` and `history`. See the note on the instance split below. |
| 3 | SC3a: YouTube plugin returns new uploads for its TOML channels from their uploads playlists | VERIFIED (offline) | `api.rs` `playlist_request` calls `playlistItems` with the `UC`-to-`UU` playlist id and the key in `x-goog-api-key` (never in the URL). `poll.rs:175-279` filters strictly newer than a per-playlist mark. 22 + 1 + 52 tests pass. |
| 4 | SC3b: After login, YouTube returns new uploads from the user's subscriptions | VERIFIED (offline), realised as the `youtube-user` instance | `api.rs` `subscriptions_request` (`mine=true`, bearer token only) feeds `poll.rs:150-160`. Scope `youtube.readonly` in `youtube-user.toml`. |
| 5 | SC4a: YouTube uses only low-cost endpoints (no 100-unit search) | VERIFIED | Only `playlistItems` and `subscriptions` are requested in `plugins/youtube/src`. `grep` for `search` in non-generated sources returns nothing. Replay test sweeps every request URL. |
| 6 | SC4b: A quota-exceeded response is a clear named error, not an empty poll | VERIFIED (offline) | `api.rs` `classify_youtube`: 403 with reason `quotaExceeded` or `dailyLimitExceeded` returns `Transient(QUOTA_EXCEEDED_TEXT)`, the named D-15 text. Every non-2xx is a typed error. The real error body shape (A13) is unconfirmed: see human item 4. |
| 7 | SC5: Each plugin's unit tests pass against recorded fixtures with no network and no keys | VERIFIED, with the caveat that the fixtures are synthetic | I ran both suites natively: Reddit 34 + 2 + 59 passed, YouTube 22 + 1 + 52 passed, 0 failed. Tests use a mock `Http` seam and `tests/fixtures/*.json`. Each fixture folder has a README labelling every file `synthetic`; a `fixtures_manifest.rs` test enforces the listing. The fixtures are hand-authored, so "recorded" is not literally true. They are the accepted D-21 fallback. |
| 8 | Goal: two reference `.wasm` components, built separately from `fili`, valid for the `source` world | VERIFIED | `plugins/reddit` and `plugins/youtube` each have their own `[workspace]`, `Cargo.lock` and `rust-toolchain.toml`; root `Cargo.toml` has `exclude = ["plugins"]`. `wasm-tools component targets ../wit -w source` passes on both built `.wasm` files, and neither imports any `wasi:` interface. `fili plugins check` passed for `reddit`, `reddit-user`, `youtube` and `youtube-user`. `fili auth login reddit` is refused with exit 1. `git diff 75b078e..HEAD -- wit Cargo.toml` is empty, so the WIT is unchanged (D-02). |
| 9 | Goal: they stream **real** Reddit and YouTube content through the host | UNCERTAIN (human needed) | No live call has ever been made. All fixtures are synthetic by D-21. A1 (home endpoint), A2 (saved path and scope), A3 (app-only listing access and shape), A4 (error shapes), A11 (`x-goog-api-key` header form), A13 (Google error nesting) are unconfirmed (07-12-SUMMARY, fixture READMEs). No test runs the compiled `.wasm` in the host against even a replayed server: that is Phase 8 TEST-02. |

**Score:** 8/9 truths verified. 0 present-but-behavior-unverified. Truth 9 is UNCERTAIN and carried as human verification.

### Note on SC2 and SC3 wording

The roadmap says "after `fili auth login reddit`" and "after `fili auth login youtube`". The shipped design splits auth by plugin instance (`reddit` app-only; `reddit-user` with login; same for YouTube), so the commands are `fili auth login reddit-user` and `fili auth login youtube-user`, and `fili auth login reddit` is refused. This is the user-ruled D-01, D-02, D-07 in `07-CONTEXT.md`, which also records that SC2 and SC3 were checked against the instance names. I treat it as an intended realisation of the criteria, not a gap. The ROADMAP text is now slightly out of line with the shipped command names. Consider updating it.

### Deferred Items

| # | Item | Addressed In | Evidence |
|---|------|-------------|----------|
| 1 | Live confirmation against the real Reddit and YouTube APIs | Phase 8 | SC2 (live mode against real APIs when credentials are in `.env`, TEST-03) and SC3 (live end-to-end run, TEST-04) |

Deferral does not change the status: Phase 7's own goal says "real" content, so the human items above remain open for this phase.

### Required Artifacts

| Artifact | Expected | Status | Details |
|----------|----------|--------|---------|
| `plugins/reddit/` (src: api, cursor, http, poll, settings, component) | Reddit source crate, own workspace | VERIFIED | Substantive (`poll.rs` 331 lines, `api.rs` 443, `settings.rs` 654), wired through `component.rs` to the generated `Guest` and `fetch` import. |
| `plugins/youtube/` (same layout) | YouTube source crate, own workspace | VERIFIED | Substantive and wired the same way. |
| `reddit_source.wasm`, `youtube_source.wasm` | Valid source-world components | VERIFIED | Built, and pass `wasm-tools component targets` and `fili plugins check`. |
| `plugins/*/tests/fixtures/*` and `replay.rs`, `fixtures_manifest.rs` | Offline fixtures and replay suites | VERIFIED (synthetic) | All files labelled `synthetic`. |
| `examples/plugins/{filimon,reddit,reddit-user,youtube,youtube-user}.toml` | Sample registry for four instances | VERIFIED | All four load and pass `plugins check`. |
| `src/plugins/config.rs`, `auth/client.rs`, `auth/provider.rs`, `auth/login.rs`, `commands/auth.rs` | Host `client_credentials` grant, token cache, login refusal | VERIFIED | 13 host tests matching `client_credentials` pass, plus the CLI test `login_on_a_client_credentials_plugin_is_refused_with_exit_1_and_creates_nothing`. |

### Key Link Verification

| From | To | Via | Status | Details |
|------|----|-----|--------|---------|
| Plugin `poll` | host `fetch` import | `component.rs` `FetchHttp` adapter | WIRED | Maps every `FetchError` variant to `HttpError`. |
| Plugin `PollError::RateLimited` | host backoff | `to_wit_error`, `scheduler.rs:182,215` | WIRED | Reset seconds reach the scheduler; scheduler tests cover `Some`, `None` and clamping. |
| `[host.oauth] grant = "client_credentials"` | `TokenProvider::app_token` | `provider.rs:150-216` | WIRED | In-memory, single-flight, 60 s refresh margin, never persisted. |
| Provider token | plugin `poll(cursor, access-token)` | scheduler token step (`scheduler.rs:303-314`) | WIRED | A plugin with no `[host.oauth]` gets none. |
| Plugin User-Agent header | outbound request | host `Egress` | WIRED | `user-agent` is not in `FORBIDDEN_REQUEST_HEADERS`. |
| Registry instances | one `.wasm` | `examples/plugins/filimon.toml` | WIRED | Two sources per component, distinct cursor and token keys. |

### Data-Flow Trace (Level 4)

| Artifact | Data Variable | Source | Produces Real Data | Status |
|----------|---------------|--------|--------------------|--------|
| `reddit/poll.rs` | `response.body` to `items` | `Http::get` via host `fetch` | Yes in production wiring; tests substitute synthetic fixtures | FLOWING (code path), live data unconfirmed |
| `youtube/poll.rs` | `response.body` to `items` | same | Same | FLOWING (code path), live data unconfirmed |

### Behavioral Spot-Checks

| Behavior | Command | Result | Status |
|----------|---------|--------|--------|
| Reddit native suite | `cd plugins/reddit && cargo test --target x86_64-apple-darwin --offline` | 34 + 2 + 59 passed, 0 failed | PASS |
| YouTube native suite | `cd plugins/youtube && cargo test --target x86_64-apple-darwin --offline` | 22 + 1 + 52 passed, 0 failed | PASS |
| Host app-grant, token and login-refusal tests | `cargo test --offline client_credentials` | 13 + 1 passed | PASS |
| Components are valid `source` worlds | `wasm-tools component targets wit -w source <wasm>` | ok for both, 0 `wasi:` imports | PASS |
| Four instances load and check | `fili --registry examples/plugins/filimon.toml plugins check <name>` | "check passed" for all four | PASS |
| Login on app-grant plugin refused | `fili auth login reddit` | error "nothing to log in to", non-zero | PASS |

I did not re-run the full root suite (`cargo test`); the orchestrator reports it green after the final merge, and I ran only targeted host tests.

### Probe Execution

SKIPPED: no `probe-*.sh` scripts are declared by the plans or exist under `scripts/`.

### Requirements Coverage

All seven IDs appear in plan frontmatter and in REQUIREMENTS.md (marked `[x]` and mapped to Phase 7). No orphaned Phase 7 IDs: REQUIREMENTS.md maps only these seven to Phase 7. TEST-02, TEST-03, TEST-04 are Phase 8 and correctly pending.

| Requirement | Source Plans | Description | Status | Evidence |
|-------------|-------------|-------------|--------|----------|
| RDDT-01 | 07-01, 02, 03, 04, 10, 11, 12 | Poll new/hot from TOML subreddits via official API | SATISFIED (offline); live unconfirmed | Truth 1; host app grant in 07-02/03/10 |
| RDDT-02 | 07-07, 11, 12 | Logged-in user's home feed / saved posts | SATISFIED (offline); A1, A2 unconfirmed | Truth 2 |
| RDDT-03 | 07-01, 02, 03, 06, 10, 11, 12 | Honour rate-limit headers, compliant UA | SATISFIED | Truth 1 |
| YTUB-01 | 07-05, 11, 12 | New uploads from TOML channels via uploads playlists | SATISFIED (offline); A11 unconfirmed | Truth 3 |
| YTUB-02 | 07-09, 11, 12 | New uploads from user's subscriptions | SATISFIED (offline) | Truth 4 |
| YTUB-03 | 07-08, 09, 11, 12 | Low-cost endpoints, clear quota errors | SATISFIED (offline); A13 unconfirmed | Truths 5, 6 |
| TEST-01 | 07-01, 04, 05, 06, 07, 08, 09, 11, 12 | Per-plugin offline fixture tests | SATISFIED, with synthetic fixtures | Truth 7 |

REQUIREMENTS.md marks RDDT-01..03, YTUB-01..03 and TEST-01 as Complete. Given that no requirement has been confirmed against a live API, "Complete" there means "implemented and offline-proven".

### Anti-Patterns Found

No `TBD`, `FIXME`, `XXX`, `TODO`, `todo!` or `unimplemented!` in the phase's source, tests or example files (generated `bindings.rs` excluded). No stubs, placeholder returns or hollow props found on the data path.

### Advisory Code Review (07-REVIEW.md: 0 critical, 6 warning, 6 info, all dispositions open)

None of the findings makes a must-have FAIL, because none breaks a stated success criterion on its normal path. I confirmed WR-01, WR-02 and WR-04 against the code. They are real robustness defects and are worth fixing before relying on these plugins unattended:

| Finding | Confirmed | Effect on must-haves |
|---------|-----------|----------------------|
| WR-01 rate-limit on a 2xx discards the page and can starve later feeds | Yes (`poll.rs:83-88`, abort at `:186`) | SC1 still holds (backs off by headers). Only large feed counts are affected. Matches the recorded D-13 design. |
| WR-02 one 404/403 feed returns `Config`/`Fatal` and the host disables the plugin until restart | Yes (`poll.rs:92-101`; `scheduler.rs` disables on config/fatal) | A single banned subreddit or bad channel stops all other feeds. A resilience gap, not a criterion failure. |
| WR-03 `order=relevance` makes the subscription window unstable, so videos can be re-emitted | Plausible, not run | Delivery is at-least-once by design. This produces duplicates. |
| WR-04 Reddit `name` is not length-checked when used as a time mark (`api.rs:164-176`; `valid_fullname` exists only for the saved path) | Yes | A hostile or odd response can overflow the 4096-byte cursor, giving `Fatal` and a disabled plugin. Low probability against the real API. |
| WR-05 cached app token not dropped after an API 401 | Yes by inspection (`provider.rs:173-216`) | A revoked or rotated app secret keeps failing for up to ~24 h. |
| WR-06 `x-goog-api-key` not stripped on cross-origin redirect | Not re-checked (host code outside the phase diff) | The key is only ever sent to `www.googleapis.com`. Low likelihood. |

These dispositions should be resolved (fix or accept) before Phase 8 builds live tests on top of the plugins. WR-02 and WR-04 are the ones most likely to surface during a live run.

### Other deviations noted

- 07-12 reported two acceptance-criterion regex discrepancies (a test function name matching the `access_token` secret-scan pattern, and 3 pre-existing clippy `duration_suboptimal_units` warnings in `src/plugins/config.rs` from phase 6). Neither is a defect in this phase.
- `.planning/phases/06-host-managed-oauth/06-VERIFICATION.md` and `.planning/config.json` show as modified in the working tree. They are outside this phase and I did not touch them.

### Human Verification Required

See the `human_verification` list in the frontmatter. In short:

1. Reddit app-only live run (`plugins run-once reddit`): confirms A3 and A4.
2. Reddit user live run, home and saved (`reddit-user`): confirms A1 and A2.
3. YouTube API-key and subscription live runs: confirms A11 and the uploads/subscription shapes.
4. A real YouTube quota-exceeded body: confirms A13 and the named error.

If the user accepts that these live confirmations are exactly what Phase 8 (TEST-03, TEST-04) will do, they can proceed to Phase 8 with this status noted as accepted. The alternative is a short manual run now.

### Gaps Summary

No gaps. The one open item is evidence, not implementation: the offline proof is complete and honest (fixtures labelled synthetic), but the "real content" half of the goal has never been exercised against the live APIs.

---

_Verified: 2026-11-11_
_Verifier: Claude (gsd-verifier)_
