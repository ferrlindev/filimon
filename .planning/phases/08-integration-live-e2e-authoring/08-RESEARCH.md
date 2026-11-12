# Phase 8: Integration, Live E2E & Authoring - Research

**Researched:** 2026-11-12
**Domain:** Rust integration testing of a wasmtime component host against a live Fluvio cluster; WASM component authoring docs; human-run live E2E evidence
**Confidence:** HIGH (codebase facts and the WIT-03 feasibility spike were verified this session); MEDIUM on external-service access (Reddit approval, WIT tag distribution)

<user_constraints>
## User Constraints (from CONTEXT.md)

### Locked Decisions

**Replay & Fluvio harness (TEST-02)**
- **D-01:** Fixtures are replayed by a **replay fetcher plugin** (a `fetcher`-world component serving fixtures by URL). The Reddit/YouTube source instances are assigned to it via the Phase 5 fetcher assignment. This exercises the real host fetch path, allowlist and fetcher assignment with no network. — **Reversibility:** reversible — test infrastructure.
- **D-02:** Integration tests need a **local Fluvio cluster** and **skip cleanly with a clear message** if none is reachable. They create a unique topic per run and assert the expected `SourceItem`s land in it. The harness does not start a cluster, and a missing cluster is not a failure.
- **D-03:** The test helper builds `plugins/reddit` and `plugins/youtube` (and the replay fetcher) with `cargo component build --release` **when the `.wasm` is missing or stale**, so `cargo test` is self-sufficient. No built `.wasm` is committed.

**Live mode gating (TEST-03)**
- **D-04:** Live tests use the **same test binary** and **skip at runtime** when the plugin's required `.env` keys are absent, printing `skipped: missing <KEY>`. No extra opt-in flag, no `#[ignore]`.
- **D-05:** Live tests cover the **app-only instances only** (`reddit`: client_id/secret; `youtube`: API key). `reddit-user` / `youtube-user` (PKCE, interactive login) are excluded from automated live runs.
- **D-06:** **One test body, two fetch backends.** Replay uses the replay fetcher and asserts exact items; live uses the `http` fetcher and asserts structure only (non-empty, `platform`, ids, `published` range).

**Live E2E run & evidence (TEST-04)**
- **D-07:** TEST-04 is a **human-run checkpoint with scripted steps**: a runbook/script (start Fluvio, news crawl, run `fili` over Reddit + YouTube, consume enriched output) executed by the user with real keys. Not an automated test.
- **D-08:** Evidence committed under the phase directory: a **sanitised, trimmed enriched JSON sample** (social + news, tokens/PII stripped) plus the **run transcript**, referenced from VERIFICATION.md.
- **D-09:** SC3 requires **both Reddit and YouTube**. If Reddit credentials cannot be obtained, **stop and ask the user** whether to waive; never silently drop Reddit. (Replay-only Reddit coverage with synthetic fixtures per 07 D-21 does not satisfy TEST-04.)

**Authoring: template, WIT distribution, guide (WIT-03, DOCS-01, DOCS-02)**
- **D-10:** The template is a **`plugins/template` crate, copy-and-rename**, derived from `plugins/stub-source`, minimal and commented. It must build as-is in CI/tests.
- **D-11:** An outside author obtains the WIT by **copying `wit/` from a tagged release or a git dependency** (git archive / sparse checkout into the plugin's `wit/`). No WIT registry publishing (wkg/warg) this phase. — **Reversibility:** costly — this is the published authoring contract third parties follow.
- **D-12:** The guide lives at **`docs/plugin-guide.md`** (new `docs/` dir). A **script builds the template in a temp dir outside the repo against only the WIT**, registers it in a `filimon.toml`, and runs `fili plugins run-once` with the **unmodified `fili` binary**; the user also runs it once as the SC4 proof.
- **D-13:** The guide's fetcher walkthrough (DOCS-02) **reuses the replay fetcher as the worked example**, so the documented steps are exercised by the integration tests. A built-by-following-the-guide fetcher must serve a source's requests.

### Claude's Discretion
- Exact test file layout, helper names, Fluvio reachability probe, topic naming, and staleness check for the cached build.
- Runbook/script format (shell vs `just`) and location; sanitisation tooling for the evidence sample.
- How the replay fetcher maps URL to fixture file and how Reddit/YouTube base URLs are routed to it.
- Guide structure and section order; pinning the WIT tag name used in examples.

### Carried forward
- WIT unchanged: one-token `poll(cursor, access-token)` (Phase 1 D-07, Phase 6 D-10, Phase 7 D-02).
- Plugins target `wasm32-unknown-unknown`, built with `cargo component`, own `[workspace]` and `Cargo.lock` (Phase 1 D-16, Phase 7 D-16).
- Fixtures under each crate's `tests/fixtures/`; synthetic ones labelled per Phase 7 D-21.
- Instance pattern: `reddit`, `reddit-user`, `youtube`, `youtube-user`, one `.wasm` per platform (Phase 7 D-01, D-07).
- Host does TLS, allowlist, timeout and size limits; `http-fetcher` is a normal registry entry (Phase 5).
- Items pass to `filimon-social-items` unchanged; NER is consumer-side (Phase 2).
- Existing `fluvio-wise` gaps remain out of scope (PROJECT.md).

### Deferred Ideas (OUT OF SCOPE)
- Publishing the WIT to a registry (wkg/warg) — future work once third-party adoption warrants it.
- Automated live E2E run including news crawling — deferred in favour of a human-run checkpoint.
- Live tests for PKCE instances (`reddit-user`, `youtube-user`).
- Adding CI (`.github/workflows`) — not requested; tests are written to be CI-safe.
</user_constraints>

<phase_requirements>
## Phase Requirements

| ID | Description | Research Support |
|----|-------------|------------------|
| TEST-02 | Compiled Reddit and YouTube `.wasm` run in the host against replayed fixtures; expected `SourceItem`s reach a local Fluvio topic | In-crate test module (the crate is bin-only and the production sink is hard-wired to one topic, so `tests/` cannot do this); replay-fetcher crate; reuse of `live_support` scratch-topic helpers; stored-token trick for the Reddit token (Pattern 2, 3) |
| TEST-03 | Same tests have a live mode that runs only when credentials are in `.env` | Same test body parameterised by fetch backend; runtime skip with `skipped: missing <KEY>`; env closure instead of secret files (Pattern 4) |
| TEST-04 | Live E2E with real Reddit and YouTube data produces enriched JSON alongside news | Human runbook script; GLiNER model is NOT installed locally (verified); sanitise-and-scan evidence step (Pattern 7) |
| WIT-03 | A plugin built outside the repo against only the published WIT runs in an unmodified `fili` | Feasibility spike PASSED this session (Pattern 5); WIT distribution by sparse checkout; repo has no git tag yet (Open Question 1) |
| DOCS-01 | Template plus guide: build a source plugin, write its TOML, register it | `plugins/template` + `docs/plugin-guide.md` + verification script (Pattern 5, 6) |
| DOCS-02 | Guide covers writing a fetcher and assigning it to a source | Replay fetcher split into `lib.rs` + `fixtures.rs` so the guide can show a copy-and-edit path (Pattern 6) |
</phase_requirements>

## Summary

Everything this phase needs already exists in some form: the host (`Host::load`/`Host::run`), a Fluvio sink with a unique-scratch-topic helper (`live_support`), a registry writer, stub/fault/http fetchers and the built Reddit and YouTube components. The work is assembly, plus four new artifacts: a `replay-fetcher` component, a `template` source crate, an in-crate integration test module, and the guide plus scripts. The load-bearing constraint is architectural: `filimon` is a bin-only crate (`[[bin]] name = "fili"`, no `[lib]`) and the production `FluvioSink` is only ever built for `filimon-social-items`, so TEST-02/03 cannot live in `tests/` and drive the binary without polluting the production topic. They must be `#[cfg(test)]` modules inside the crate, like the existing `wasm_*`/`live_*` host tests, but with runtime skips instead of `#[ignore]` (D-02, D-04).

Two non-obvious facts shape the replay design. First, the replay fetcher needs no URL rewriting: the Reddit and YouTube plugins hard-code `https://oauth.reddit.com` and `https://www.googleapis.com/youtube/v3`, the host checks the allowlist on that original URL, then hands the request to the assigned fetcher, which can answer from a compiled-in table. Second, the Reddit plugin returns `auth-required` before making any request when it has no token, and the app-only `client_credentials` grant makes the host call the real `www.reddit.com` token endpoint (https only). For a no-network replay the test must therefore declare an `authorization_code` `[host.oauth]` and pre-write a stored token file with the existing `write_tokens` test helper. That still exercises the real host token-delivery path. A fetcher instance is fresh for every call, so a replay fetcher is stateless: replay can only cover a first poll.

WIT-03 feasibility was proven with a spike: copying `stub-source` plus `git archive HEAD wit` into a temp dir, changing two `Cargo.toml` lines, running `cargo component build --release` there, and registering the result in a standalone `filimon.toml` produced a component that the existing `target/debug/fili` binary accepted (`plugins check`) and polled (`plugins run-once`), with the binary's hash unchanged. Remaining risks are not technical: the repository has no git tag, so D-11's "tagged release" does not exist yet; the local GLiNER model files are absent, which blocks the NER half of TEST-04; and Reddit now gates new API access behind an approval process, which is exactly the D-09 stop-and-ask scenario.

**Primary recommendation:** Put TEST-02/03 in a new `src/plugins/host/integration_tests.rs` that reuses `live_support`, run replay and live through one parameterised body, and build the replay fetcher, template and authoring script around a one-file `fixtures.rs` seam so the guide's fetcher walkthrough is exercised by the same code the tests use.

## Architectural Responsibility Map

| Capability | Primary Tier | Secondary Tier | Rationale |
|------------|-------------|----------------|-----------|
| Serving canned API responses for replay | Plugin (fetcher component) | Host (allowlist, deadline) | D-01: replay goes through the real fetcher path; the host still judges the source's call |
| Allowlist / timeout / TLS on live runs | Host | `http-fetcher` (forwards only) | Carried forward from Phase 5: the host enforces, fetchers cannot weaken |
| Token delivery to Reddit plugin | Host (`TokenProvider`) | Test harness (pre-written token file) | Plugin only ever receives `access-token`; replay stores a token instead of calling the network |
| Item mapping and producing to Fluvio | Host (`map_items`, `FluvioSink`) | Fluvio cluster | Existing HOST-04 path under test |
| Reading back and asserting records | Test harness | Fluvio cluster | Scratch topic per run, deleted afterwards |
| NER enrichment of social + news into one JSON | CLI consumer (`fili consume --enrich-ner`) | GLiNER model files | PIPE-04; consumer-side, batch (`--limit`) mode only |
| Skipping when Fluvio or credentials are absent | Test harness | — | Runtime decision, same binary (D-02, D-04) |
| Building `.wasm` for tests | Test harness (calls `cargo component`) | Plugin crates' own toolchain pins | D-03 |
| Template, guide, authoring proof | Docs + scripts | `fili plugins check/run-once` | WIT-03/DOCS-01/02 acceptance path |
| Live E2E evidence | Human operator + script | `jq` sanitiser | D-07/D-08 |

## Standard Stack

### Core
No new libraries are needed in the root crate. All pieces below already exist in the repo or on this machine.

| Library / Tool | Version | Purpose | Why Standard |
|----------------|---------|---------|--------------|
| `cargo-component` | 0.21.1 [VERIFIED: `cargo component --version` run this session prints `cargo-component-component 0.21.1`] | Build plugin components | Pinned by `plugins/README.md` prerequisites |
| `wasm-tools` | 1.263.0 [VERIFIED: `wasm-tools --version` run this session] | Component/world conformance checks | Pinned by `plugins/README.md` |
| `wit-bindgen-rt` | 0.41.0 [VERIFIED: `plugins/http-fetcher/Cargo.lock` contains `name = "wit-bindgen-rt"` / `version = "0.41.0"`] | Guest bindings runtime for new crates (replay-fetcher, template) | Same dependency every existing plugin uses; no new package |
| `fluvio` client | `"0.50"` [VERIFIED: `Cargo.toml` `fluvio = "0.50"`] | Scratch topics, read-back | Already used by `sink.rs::live_support` |
| `tokio`, `tempfile`, `serde_json` | existing root deps | Test harness | Already in `Cargo.toml` and `[dev-dependencies]` |
| Rust toolchain | 1.96.0 [VERIFIED: `cargo --version` prints `cargo 1.96.0`; plugin `rust-toolchain.toml` pins `channel = "1.96.0"`] | All builds | Pinned |

### Supporting
| Tool | Version | Purpose | When to Use |
|------|---------|---------|-------------|
| `jq` | present at `/usr/bin/jq` [VERIFIED: `which jq`] | Sanitise the TEST-04 evidence sample | Runbook script |
| `git` sparse checkout | git on PATH | Fetch `wit/` at a tag for the guide | Guide step; script default uses local `git archive HEAD wit` |
| `bash` | macOS system | Scripts (`scripts/*.sh`) | `just` is NOT installed [VERIFIED: `which just` printed `just not found`] |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| In-crate tests | A `tests/` crate driving `fili` | Rejected: bin-only crate and fixed production topic (see Summary) |
| `just` | Plain bash scripts | `just` is not installed; bash needs nothing |
| Symlink `plugins/template/wit` -> `../../wit` | One-line path edit in the guide | Symlink breaks on Windows and `cp -R`; the path edit is explicit and teaches the reader where WIT lives |
| wkg/warg registry | Copy `wit/` at a tag | Locked out of scope (D-11) |

**Installation:** none (no new packages). New plugin crates reuse `wit-bindgen-rt = { version = "0.41", features = ["bitflags"] }` exactly as `plugins/stub-source/Cargo.toml` does.

**Version verification:** cargo-component and wasm-tools versions were read from the installed binaries this session. `cargo-component` has not released since 2025-04 per `plugins/README.md` (fallback documented there). [CITED: plugins/README.md "If cargo-component 0.21.1 stops building"]

## Package Legitimacy Audit

This phase installs no new external packages. The two new plugin crates depend only on `wit-bindgen-rt 0.41.0`, which is already locked in four sibling `Cargo.lock` files, and the optional template extras (`serde`, `toml`) are already used by `plugins/stub-source`. cargo-component and wasm-tools are pre-installed tools, not new installs. `gsd-tools query package-legitimacy check` was not run because no package is being newly introduced; if the planner adds any dependency, it must be audited before installation.

**Packages removed due to [SLOP] verdict:** none
**Packages flagged as suspicious [SUS]:** none

## Architecture Patterns

### System Architecture Diagram

```
 REPLAY MODE (CI-safe, needs only a local Fluvio)               LIVE MODE (credential-gated)
 ------------------------------------------------               -----------------------------
 cargo test e2e_replay_*                                        cargo test e2e_credentialed_*
        |                                                              |
        v                                                              v
 ensure_built(reddit, youtube, replay-fetcher)  <-- D-03        ensure_built(reddit, youtube, http-fetcher)
        |                                                              |
        v                                                              v
 probe Fluvio (5 s timeout) --no--> print "skipped: ..." return        same probe, then env keys present?
        | yes                                                          | no --> "skipped: missing <KEY>"
        v                                                              v yes
 temp dir: filimon.toml + reddit.toml + youtube.toml            temp dir: same TOML, real client_id,
   fetcher = "replay"   (allowlist oauth.reddit.com,              fetcher = "http", client_credentials
   www.googleapis.com)                                            grant, secrets via env closure
 write stored Reddit token (write_tokens)                              |
        |                                                              |
        +------------------------+-------------------------------------+
                                 v
                  load_registry_with_env(closure) -> Host::load -> Host::run(FluvioSink, scratch topic)
                                 |
       source `fetch` import --> host checks allowlist on ORIGINAL url --> assigned fetcher (fresh instance per call)
                                 |                                  |
                     replay-fetcher: table lookup      http-fetcher: outbound-http.send (host TLS)
                       -> compiled-in fixture                       -> real API
                                 |
                  poll-response -> map_items -> key "<platform>:<id>" -> FluvioSink -> scratch topic
                                 v
                  read topic from beginning -> assert (replay: exact items; live: structure)
                                 v
                  delete scratch topic (also on panic)

 HUMAN LIVE E2E (TEST-04)                       AUTHORING PROOF (WIT-03)
 ------------------------                       -------------------------
 scripts/live-e2e.sh                            scripts/verify-authoring.sh
  fluvio up -> fili produce <news url>           mktemp -d (outside repo) -> copy plugins/template
  -> fili run (reddit+youtube+http only)         -> copy wit/ (local git archive, or WIT_REF tag)
  -> wait until `plugins list --json` ok         -> edit Cargo.toml wit path + package name
  -> SIGINT -> fili consume --enrich-ner        -> cargo component build --release
     --limit N --output evidence.json            -> write filimon.toml + plugin TOML
  -> jq sanitise -> secret-value scan            -> fili plugins check / run-once (hash of fili before == after)
  -> transcript + sample into phase dir          -> fetcher leg: build replay-fetcher copy, assign, run-once
```

### Recommended Project Structure
```
src/plugins/host/
├── integration_tests.rs     # NEW (#[cfg(test)]): replay + credentialed bodies, build helper, Fluvio probe
├── testing.rs               # EXTEND: richer registry writer (fetcher, allowlist, secrets, oauth)
└── sink.rs                  # live_support reused as-is
plugins/
├── replay-fetcher/          # NEW fetcher-world crate (own [workspace], Cargo.lock, rust-toolchain, .cargo/config.toml)
│   ├── src/lib.rs           #   generic component glue + matcher (never edited by authors)
│   └── src/fixtures.rs      #   the ONE file that maps url -> include_bytes!(fixture)
└── template/                # NEW source-world crate derived from stub-source
docs/
└── plugin-guide.md          # NEW
scripts/
├── verify-authoring.sh      # NEW: WIT-03 / DOCS proof (D-12)
└── live-e2e.sh              # NEW: TEST-04 runbook (D-07/D-08)
examples/plugins/.env.example  # EXTEND: REDDIT_CLIENT_ID (+ optional contact) for the credentialed tests
.planning/phases/08-.../evidence/  # sanitised sample + transcript (D-08)
```
`plugins/*` is excluded from the root workspace by `exclude = ["plugins"]` in `Cargo.toml` [VERIFIED: Cargo.toml lines 56-63], so new crates there need no workspace edit but must carry their own empty `[workspace]` table, as `plugins/stub-source/Cargo.toml` does.

### Pattern 1: In-crate integration module with runtime skips
**What:** A `#[cfg(test)] mod integration_tests;` in `src/plugins/host/mod.rs`; tests are normal `#[tokio::test]` (real clock, NOT `start_paused`) that return early with a printed skip line.
**When to use:** Every TEST-02/03 test.
**Why:** `Cargo.toml` declares only `[[bin]] name = "fili" path = "src/main.rs"` (no `[lib]`), and `sink.rs` documents "The production sink is only ever built for `filimon-social-items`: no plugin or registry value selects a topic." A `tests/` file can only run `fili` and would write to the real topic.
**Constraints from existing code:** `sink.rs` notes real-clock tests are required for Fluvio I/O ("Real clock: the Fluvio client's I/O and the host's sink timeout must not see an auto-advanced clock").
**Naming:** use `e2e_replay_*` and `e2e_credentialed_*`. Existing groups use `wasm_`, `live_`, `tls_`, `net_` with `--ignored` filters [CITED: plugins/README.md "Running the host tests"]; new names avoid a filter collision. Add a fifth group to that README section.
**Pitfall note:** libtest reports a skipped test as `ok`, and a `eprintln!` shows only with `--nocapture`. Print the skip line unconditionally to stderr and document `cargo test -p filimon e2e_ -- --nocapture`.

### Pattern 2: Replay without touching the network (routing, token, and instances)
- **Routing:** none needed. Reddit sends `https://oauth.reddit.com/r/{sub}/{listing}?limit={limit}&raw_json=1` and YouTube sends `https://www.googleapis.com/youtube/v3/playlistItems?part=snippet%2CcontentDetails&playlistId=<id>&maxResults=<n>` [VERIFIED: plugins/reddit/src/api.rs and plugins/youtube/src/api.rs, `API_BASE` constants and `listing_request`/`playlist_request`]. The host checks the allowlist against that URL, then the assigned fetcher answers it. The replay fetcher never calls `outbound-http.send`.
- **Fixture selection (recommended):** substring needles on host + path + one identifying query value, ignoring `limit`/`maxResults`, e.g. `("oauth.reddit.com/r/rust/new", "listing_new.json")` and `("playlistId=UU_x5XG1OV2P6uZZ5FSM9Ttw", "playlist_page.json")`. Unmatched URL: answer a `404` with a fixed body and log host plus path only, so a drifted URL fails the exact-items assertion loudly.
- **Reddit token:** the plugin returns `AuthRequired` before any request when the token is missing or empty [VERIFIED: plugins/reddit/src/poll.rs, the `poll` function head (about lines 126-132): "D-04: a missing token is auth-required before any request is made", `_ => return Err(PollError::AuthRequired)`]. `grant = "client_credentials"` makes the host call `https://www.reddit.com/api/v1/access_token` itself, so for replay declare an `authorization_code` `[host.oauth]` (any https `authorize_url`/`token_url`, a `client_id`, `scopes`) and pre-write the token file with `crate::plugins::auth::testing::write_tokens(state_dir, "reddit", "replay-token", unix_now() + 86_400)`. `PluginSpec::from_loaded` builds a `TokenProvider` over `<state_dir>/auth` for any plugin with `[host.oauth]` [VERIFIED: src/plugins/host/scheduler.rs `from_loaded`], and `auth_for_poll` reads the stored token with no network. `write_tokens` is `pub(crate)` in a `cfg(test)` module, reachable from an in-crate test only.
- **YouTube:** the `youtube` instance uses `[host.secrets] api_key = "YOUTUBE_API_KEY"`; supply a fake value through the `env` closure of `load_registry_with_env` [VERIFIED: signature `load_registry_with_env(path, env: &dyn Fn(&str) -> Option<String>)`], no `.env` file.
- **Statelessness:** every fetch runs in a fresh fetcher instance [CITED: plugins/README.md "Every call runs in a fresh fetcher instance"]. A replay fetcher cannot count calls, so replay covers a first poll only. Use a long `poll_interval_secs` (e.g. 3600) and cancel after the expected record count arrives. Later-poll behaviour (cursor, "strictly newer") stays covered by the native crate tests (TEST-01).
- **Instances:** register `reddit` and `youtube`, both `fetcher = "replay"`. Expected first-poll content: 3 Reddit posts (`t3_1kx9aa1`, `t3_1kx9ab2`, `t3_1kx9ac3` in `plugins/reddit/tests/fixtures/listing_new.json`, newest first in the file, emitted oldest first) and 3 YouTube videos (`vidNew00001`, `vidMid00002`, `vidOld00003` in `playlist_page.json`). Order across plugins is unspecified [CITED: plugins/README.md "One plugin's batches reach the sink in poll order; the order across plugins is not specified"]; assert per-platform order and the set overall.

### Pattern 3: One test body, two backends (D-06)
Write `async fn drive(backend: Backend) -> miette::Result<Observed>` that builds the temp registry, runs the host into a scratch topic until N records arrive (30 s cap, copy `run_host_until` from `sink.rs`), reads the topic from the beginning and returns the `SourceItem`s. `Backend::Replay` supplies `replay_fetcher.wasm`, the stored token and fake key, then asserts exact ids/urls/titles/published/key (`reddit:t3_...`, `youtube:vid...`). `Backend::Live` supplies `http_fetcher.wasm`, real credentials, then asserts structure only: non-empty, `platform` equals `reddit` or `youtube`, id prefix `t3_` for Reddit, non-empty id and `https` URL for YouTube, `published` parses and lies between a lower bound (2005-06-23, Reddit founding is a safe floor; use a generous constant) and now plus clock-skew slack.

### Pattern 4: Runtime skip helper
```rust
// Source: pattern derived from src/plugins/host/sink.rs live_support (scratch_topic/delete_topic) - new code
async fn fluvio_or_skip() -> Option<()> {
    match tokio::time::timeout(Duration::from_secs(5), fluvio::Fluvio::connect()).await {
        Ok(Ok(_)) => Some(()),
        _ => {
            eprintln!("skipped: no reachable Fluvio cluster (start one with `fluvio cluster start`)");
            None
        }
    }
}
fn missing_key<'a>(keys: &[&'a str], env: &dyn Fn(&str) -> Option<String>) -> Option<&'a str> {
    keys.iter().copied().find(|k| env(k).map_or(true, |v| v.is_empty()))
}
// caller: if let Some(k) = missing_key(..) { eprintln!("skipped: missing {k}"); return Ok(()); }
```
Skip only on a failed connect. A failure after a successful connect (create topic, send, read) is a test failure. Credential source: process environment first, then `crate::plugins::secrets::read_dotenv(<repo root>/.env)`; build one `HashMap` and pass `&|k| map.get(k).cloned()` to the loader so no secret is ever written to the temp dir. Keys: Reddit needs `REDDIT_CLIENT_ID` and `REDDIT_CLIENT_SECRET`; YouTube needs `YOUTUBE_API_KEY`. `client_id` is public and lives in the TOML in the examples, and `.env.example` has no `REDDIT_CLIENT_ID` [VERIFIED: examples/plugins/.env.example lists `REDDIT_CLIENT_SECRET=`, `YOUTUBE_API_KEY=`, `YOUTUBE_OAUTH_CLIENT_SECRET=`], so the harness must introduce `REDDIT_CLIENT_ID` (written into the temp TOML) and add it to `.env.example`. A contact for the User-Agent is required by the plugin (`user_agent_contact`); use optional `REDDIT_USER_AGENT_CONTACT` with a default.

### Pattern 5: Authoring proof script (WIT-03), spike-verified
Steps proven in a throwaway temp dir this session (the temp dir was deleted afterwards):
1. Copy the plugin source excluding `target/` and `src/bindings.rs` (`tar --exclude`), keep dotfiles (`.cargo/config.toml`, `.gitignore`).
2. Copy the WIT: `git archive HEAD wit | tar xf - -C <dir>`.
3. Edit `Cargo.toml`: `path = "../../wit"` -> `path = "wit"`; set `package = "<ns>:<name>"` and the crate `name`.
4. `cargo component build --release` -> `target/wasm32-unknown-unknown/release/<name>.wasm` (built in about 10 s with warm crate cache).
5. Registry beside it: `[sources.<name>] component = "<dir>/target/wasm32-unknown-unknown/release/<name>.wasm"`, `config = "<name>.toml"`.
6. `fili --registry filimon.toml plugins check <name>` printed `component: ok (compiles, imports only filimon:plugin/*, fits the `source` world)` and exit 0; `plugins run-once <name> --json` printed one `SourceItem` JSON line, `polled 1 item (0 dropped)`, `dry run: nothing was produced and the cursor was not saved`, exit 0. No Fluvio is needed (`--produce` is the only path that touches it).
7. "Unmodified binary": record `shasum -a 256` of the `fili` binary before and after; assert equal. The script must use an already built `target/debug/fili` or `target/release/fili` and never invoke `cargo build` for the root crate after the plugin exists.
The proof must run from a directory outside the repo (the spike used `mktemp -d` under `/tmp`). Components built here target `wasm32-unknown-unknown` via the copied `.cargo/config.toml` and `rust-toolchain.toml`; an author who forgets those files gets a `wasm32-wasip1` component that the host rejects with an import error [CITED: plugins/README.md "Why wasm32-unknown-unknown", "Loading"].

### Pattern 6: Template and fetcher authoring surface
- **Template** (`plugins/template`, D-10): copy `stub-source` and keep the same manifest shape. Verbatim from `plugins/stub-source/Cargo.toml` lines 17-22: `[package.metadata.component]` / `package = "filimon:stub-source"` / `[package.metadata.component.target]` / `path = "../../wit"` / `world = "source"`. Keep `path = "../../wit"` so it builds in-repo; the guide's WIT step ends with the one-line edit to `path = "wit"`. Make `poll` demonstrate the `fetch` import: when a `fetch_url` setting is present, GET it through `fetch` and put the response status and a bounded excerpt of the body in the item. That makes DOCS-02 observable (the item changes when a different fetcher serves the request). Without a `fetcher` key the host answers `not-configured` [CITED: plugins/README.md "Imports"], so the template must degrade to a hardcoded item or a clear config error. Ship `Cargo.lock`, `rust-toolchain.toml`, `.cargo/config.toml` and `.gitignore` (`/target/`, `/src/bindings.rs`).
- **Replay fetcher** (D-13): split into `src/lib.rs` (component glue, generic) and `src/fixtures.rs` (the only editable file: a `&[(&str, u16, &[u8])]` table of needle, status and `include_bytes!` body). In-repo the table includes `../../reddit/tests/fixtures/...` and `../../youtube/tests/fixtures/...` via `include_bytes!`. The guide's fetcher walkthrough tells the reader to copy `plugins/replay-fetcher` and replace `fixtures.rs`; the authoring script does exactly that outside the repo with a one-entry table, builds it, registers `[fetchers.<name>]`, sets `fetcher = "<name>"` on the template source and shows the item reflecting the canned body. Existing minimal shape to follow: `plugins/stub-fetcher/src/lib.rs` (`impl Guest for Component { fn fetch(request: Request) -> Result<Response, FetchError> ... }`, `bindings::export!(Component with_types_in bindings);`).
- **Do not log** the full URL, headers or body in the replay fetcher (log host and path at most). The sample YouTube key travels in a header, and the host's own rules log only host/method/status [CITED: plugins/README.md "Logs" row].

### Pattern 7: TEST-04 runbook and evidence (D-07, D-08, D-09)
Script `scripts/live-e2e.sh` (bash) that prints each step and tees a transcript:
1. Preconditions with named failures: `fluvio cluster status`, topics `filimon-raw-pages` and `filimon-social-items` present, the three SmartModules loaded, `.env` keys present, model files present. Local state verified this session: topics `filimon-articles`, `filimon-raw-pages`, `filimon-social-items` exist and `filimon/map-extract@0.1.0`, `filimon/map-preprocess@0.1.0`, `filimon/filter-relevance@0.1.0` are loaded.
2. Work directory outside the tracked tree (gitignored, e.g. `.live-e2e/`) holding a registry with ONLY `[sources.reddit]`, `[sources.youtube]` and `[fetchers.http]`. Do not copy the sample registry blindly: it enables `stub`, `reddit-user`, `youtube-user`, and component paths are relative `../../plugins/...` [VERIFIED: examples/plugins/filimon.toml lines 3-34]; use absolute component paths (a joined absolute path replaces the base).
3. News: `fili produce <seed url>` (optionally through `[crawler] fetcher = "http"`).
4. Social: start `fili run` in the background, poll `fili plugins list --json` until both sources show `last_poll.state == "ok"` (`list` works while `run` is up [CITED: plugins/README.md `fili plugins list`]), then send SIGINT. macOS has no `timeout`; use background pid plus a wait loop.
5. Consume: `fili consume --from-beginning --limit <N> --enrich-ner --output <file>`; NER runs only in `--limit` mode [CITED: README.md consume flags table; STATE.md Blockers "Consumer NER only runs in batch/`--limit` mode"]. `--limit` counts news and social together and the production topics may hold earlier records, so choose N deliberately and check the result contains both `source_type` values and at least one social record with non-empty `entities`.
6. Sanitise with `jq`: drop or null `raw`, replace `author`, truncate `text`, keep a handful of records; then run a secret scan that `grep -F`s every non-empty value from `.env` against the evidence files and fails the script on a hit. Commit only the sanitised sample and transcript.
7. D-09 gate: if Reddit credentials or approval are missing, the script exits with a message telling the operator to ask the user; the plan must contain an explicit `checkpoint:human-action` for obtaining them and a `checkpoint:decision` for waiving.

### Anti-Patterns to Avoid
- **Driving `fili run` from `tests/` for TEST-02:** writes to the production topic. Use in-crate tests with `FluvioSink::connect(<scratch>)`.
- **`client_credentials` in replay:** triggers a real call to `www.reddit.com`. Use the stored-token trick.
- **`#[tokio::test(start_paused = true)]` with Fluvio:** the paused clock auto-advances during real network I/O and fires the sink timeout [CITED: sink.rs doc comments]. Use the real clock.
- **Committing built `.wasm` or `bindings.rs`:** both are gitignored/generated; D-03 builds on demand.
- **Writing secrets into the temp registry directory:** pass them through the env closure.
- **Using the sample `filimon.toml` for the live run as is:** enables PKCE instances that would warn or fail, and its paths are relative.

## Don't Hand-Roll

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Scratch topic lifecycle | New create/delete code | `live_support::{scratch_topic, delete_topic, ready_sink}` in `src/plugins/host/sink.rs` | Unique name (`filimon-test-<kind>-<nanos>-<pid>-<n>`), never touches real topics; `ready_sink` retries topic provisioning (up to 20 x 250 ms) |
| Host run until N records, then cancel | New loop | Copy `run_host_until` + `CountingSink` from `sink.rs` tests | Already handles the 30 s deadline, cancellation and `disabled` check |
| Registry/TOML writing | String concatenation in every test | Extend `testing.rs::write_registry` (it only supports `[host]` interval, backoff and `[settings]` today) with fetcher, allowlist, secrets and oauth | One place to keep TOML shape in sync with the loader |
| Reddit token in tests | A fake HTTPS token server | `auth::testing::write_tokens` | Real host token path, no network |
| `.env` parsing | New parser | `crate::plugins::secrets::read_dotenv` | Hand-written strict parser (Phase 1); values are never expanded |
| Building components | Raw `rustc`/`cargo build --target` | `cargo component build --release` from the crate dir | Generates bindings and componentizes; toolchain pin and target come from the crate's own files |
| Conformance check | Hand-written import scan | `fili plugins check <name>` (and `wasm-tools component targets`) | The host's own preflight is the acceptance test |
| Fetching `wit/` | Custom downloader | `git clone --depth 1 --branch <tag> --filter=blob:none --sparse` + `git sparse-checkout set wit` | Standard git; no registry (D-11) |
| JSON sanitising | A Rust tool | `jq` filter + `grep -F` secret scan | Tiny, auditable, already installed |

**Key insight:** the phase is assembly. Every piece of risky logic (host, auth, fetch policy, sink) is already tested in isolation; the new code should be thin glue and docs, and anything that looks like new host logic is a sign the plan has drifted.

## Runtime State Inventory

Not a rename/refactor/migration phase. Omitted. (Note for execution: tests write only to a per-run scratch topic and a temp state dir; the runbook writes to the production topics `filimon-raw-pages` and `filimon-social-items`, which have 7-day retention.)

## Common Pitfalls

### Pitfall 1: `cargo component` inside `cargo test` inherits the wrong toolchain
**What goes wrong:** the child build uses the root toolchain or the parent's target dir instead of the plugin's pin.
**Why:** `cargo test` (via the rustup proxy) exports toolchain-selecting and cargo env vars (`RUSTUP_TOOLCHAIN`, `CARGO_TARGET_DIR` and similar) to children; an env toolchain overrides the crate's `rust-toolchain.toml`. [ASSUMED: rustup proxy env behaviour; here root and plugin both pin 1.96.0 so it would not bite today]
**How to avoid:** spawn with `current_dir(<plugin dir>)` and `env_remove("RUSTUP_TOOLCHAIN")`, `env_remove("CARGO_TARGET_DIR")`, `env_remove("RUSTFLAGS")`, `env_remove("CARGO_ENCODED_RUSTFLAGS")`. README already warns the build "must run from the crate directory, which the subshells handle" [CITED: plugins/README.md "Build"].
**Warning signs:** a wasm in an unexpected target dir, or `wasm32-wasip1` output.

### Pitfall 2: Staleness check that never settles
**What goes wrong:** an mtime check that includes `src/bindings.rs` always says "stale".
**Why:** `cargo component build` rewrites `src/bindings.rs` on every build. [VERIFIED: ran it in `plugins/http-fetcher` twice; `src/bindings.rs` mtime moved from 1794366186 to 1794466553 and the crate recompiled in 1.96 s]
**How to avoid:** compare the `.wasm` mtime against the newest of `src/**` except `bindings.rs`, `Cargo.toml`, `Cargo.lock`, `../../wit/*`, plus (replay fetcher) every fixture it `include_bytes!`s. Serialise per crate with a process-wide `OnceLock` so parallel tests do not start concurrent builds of one crate.

### Pitfall 3: A missing build tool looks like a test failure or, worse, a silent pass
**What goes wrong:** `cargo-component` absent and no `.wasm` -> confusing error, or a skip that hides a broken environment.
**How to avoid:** D-03 says the helper builds; recommend failing loudly with the install command from `plugins/README.md` rather than skipping. "CI-safe" in CONTEXT means no network and no keys, not no toolchain. (See Open Question 2.)

### Pitfall 4: Reddit replay fails with `auth-required`
**What goes wrong:** the replay `reddit` instance polls with no token and the plugin returns `AuthRequired` before any fetch.
**How to avoid:** Pattern 2 (stored token). Also set the token file mode correctly by using `write_tokens`; the host refuses a token file with group/other permission bits [CITED: plugins/README.md "Where tokens live"].

### Pitfall 5: Replay assertions drift from plugin behaviour
**What goes wrong:** expected items hand-computed in the test diverge from the plugin's mapping rules.
**How to avoid:** assert fields derivable from the fixtures (id = fullname, url = `https://www.reddit.com` + permalink, `published` = `created_utc * 1000` ms then host-formatted RFC 3339 UTC with millis, per the existing native test in `plugins/reddit/tests/replay.rs`), compute expectations from the fixture JSON at test time, and pin key format `<platform>:<id>`.

### Pitfall 6: A skipped test counts as passed
**What goes wrong:** `cargo test` is green on a machine with no Fluvio, and nobody notices TEST-02 never ran.
**How to avoid:** print the skip line to stderr, document `-- --nocapture`, and have VERIFICATION.md record the run on a machine with a cluster (this machine has one on `127.0.0.1:9003`, verified). Optionally honour an extra env guard such as `FILI_TEST_REQUIRE_FLUVIO=1` that turns the Fluvio skip into a failure; this is not an opt-in for running tests, so it does not contradict D-04.

### Pitfall 7: NER cannot run because the model is missing
**What goes wrong:** TEST-04 needs NER-enriched social records, but `models/gliner_small-v2.1/` here holds only `gliner_config.json` and `README.md` [VERIFIED: `ls models/gliner_small-v2.1`]; `tokenizer.json` and `onnx/model.onnx` are absent. Without a model `entities` stay empty and the README says "filimon runs without a model" [CITED: README.md "GLiNER NER Setup"].
**How to avoid:** the runbook precondition step checks both files and prints the download recipe (README section) before anything else; the evidence check requires at least one non-empty `entities` on a social record. This is a human prerequisite and belongs in a `checkpoint:human-action`.

### Pitfall 8: Reddit access may not be obtainable
**What goes wrong:** D-05/D-09 need real Reddit app credentials. Reddit announced that self-service access to its public data API is being closed and new access requires approval; developers report rejections. [CITED: r/redditdev "Introducing the Responsible Builder Policy" thread (third-party mirror of the Nov 2025 announcement; anecdotal on rejections)] Existing docs already warn "Reddit may restrict new API access" [CITED: plugins/README.md "Provider notes"].
**How to avoid:** plan the credential step first as a human-action checkpoint with an explicit decision gate for waiving, exactly as D-09 requires. Never substitute synthetic replay data for TEST-04.

### Pitfall 9: The sample registry paths and instances
Covered in Pattern 7 step 2. Also: `fili run` loads the registry strictly, so one broken enabled plugin TOML stops the command [CITED: plugins/README.md `fili run`]; keep the live registry to the three entries it needs.

### Pitfall 10: Tag does not exist
**What goes wrong:** the guide tells an author to `git clone --branch <tag>` and the tag is not there. `git tag` printed nothing and `git describe` found no names [VERIFIED: commands run this session]; the remote is `https://github.com/ferrlindev/filimon.git` and `wit/host.wit`, `wit/types.wit`, `wit/worlds.wit` are tracked [VERIFIED: `git ls-files wit`].
**How to avoid:** the plan needs a human step to create and push a WIT tag (name is Claude's discretion, e.g. `wit-v0.1.0`), and the guide pins that name. The verification script defaults to the local `git archive HEAD wit` so it does not depend on the network or the tag, and accepts `WIT_REF` to run the remote path. Because D-11 is a costly-to-reverse published contract, confirm the tag name with the user.

### Pitfall 11: Test duration and parallel builds in default `cargo test`
`cargo test -p filimon` will now build three components when missing; first run is minutes, later runs seconds. Per-crate serialisation (Pitfall 2) avoids lock contention. Mention the cost in the README test section.

## Code Examples

### Replay fetcher glue (shape follows the existing stub-fetcher)
```rust
// Source: plugins/stub-fetcher/src/lib.rs (existing shape); fixtures.rs is new
#[allow(warnings)]
mod bindings;
mod fixtures;

use bindings::exports::filimon::plugin::fetch::Guest;
use bindings::filimon::plugin::types::{FetchError, Request, Response};

struct Component;

impl Guest for Component {
    fn fetch(request: Request) -> Result<Response, FetchError> {
        for (needle, status, body) in fixtures::TABLE {
            if request.url.contains(needle) {
                return Ok(Response {
                    status: *status,
                    headers: vec![("content-type".to_string(), "application/json".to_string())],
                    body: body.to_vec(),
                });
            }
        }
        Ok(Response { status: 404, headers: vec![], body: br#"{"replay":"no fixture"}"#.to_vec() })
    }
}

bindings::export!(Component with_types_in bindings);
```
```rust
// src/fixtures.rs - the only file an author edits (new)
pub const TABLE: &[(&str, u16, &[u8])] = &[
    ("oauth.reddit.com/r/rust/new", 200, include_bytes!("../../reddit/tests/fixtures/listing_new.json")),
    ("playlistId=UU_x5XG1OV2P6uZZ5FSM9Ttw", 200, include_bytes!("../../youtube/tests/fixtures/playlist_page.json")),
];
```

### Registry for the replay run (shape of existing registry entries)
```toml
# Source: examples/plugins/filimon.toml lines 11-15 and 36-40 (verbatim keys); replay-fetcher entry is new
[sources.reddit]
component = "<abs>/plugins/reddit/target/wasm32-unknown-unknown/release/reddit_source.wasm"
config = "reddit.toml"
enabled = true
fetcher = "replay"

[fetchers.replay]
component = "<abs>/plugins/replay-fetcher/target/wasm32-unknown-unknown/release/replay_fetcher.wasm"
```
Replay `reddit.toml` (new): `[host] poll_interval_secs = 3600`, `allowlist = ["oauth.reddit.com"]`; `[host.oauth]` with `authorize_url`, `token_url`, `scopes = ["read"]`, `client_id = "replay"`; `[settings] subreddits = ["rust"]`, `listings = ["new"]`, `user_agent_contact = "u/tester"` (the last three keys are valid per `examples/plugins/reddit.toml` `[settings]` and plugins/README.md "Reddit settings").

### Build helper (new)
```rust
fn ensure_built(crate_dir: &str, wasm: &str) -> PathBuf {
    static BUILT: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    let dir = repo_root().join("plugins").join(crate_dir);
    let out = dir.join("target/wasm32-unknown-unknown/release").join(wasm);
    let mut done = BUILT.get_or_init(Default::default).lock().unwrap(); // serialises builds
    if done.contains(crate_dir) { return out; }
    if is_stale(&dir, &out) {
        let status = Command::new("cargo")
            .args(["component", "build", "--release"])
            .current_dir(&dir)
            .env_remove("RUSTUP_TOOLCHAIN").env_remove("CARGO_TARGET_DIR")
            .status()
            .unwrap_or_else(|e| panic!("cargo component not runnable ({e}); install per plugins/README.md"));
        assert!(status.success(), "cargo component build failed in {}", dir.display());
    }
    done.insert(crate_dir.to_string());
    out
}
```

### WIT distribution lines for the guide (D-11)
```sh
git clone --depth 1 --branch <TAG> --filter=blob:none --sparse https://github.com/ferrlindev/filimon.git filimon-wit
git -C filimon-wit sparse-checkout set wit
cp -R filimon-wit/wit ./wit
```
[ASSUMED: sparse-checkout flags work on git >= 2.25 against GitHub; verify in the script's `WIT_REF` path before the guide ships. `git archive --remote` is not offered by GitHub, which is why it is not the guide's primary path.]

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `wasm32-wasip1` components (cargo-component default) | `wasm32-unknown-unknown` with no WASI imports | Phase 1/3 decision | Template must ship the `.cargo/config.toml` and `rust-toolchain.toml` or the host rejects the build |
| Self-service Reddit API apps | Approval-gated new access (Responsible Builder Policy) | Announced Nov 2025 [CITED: r/redditdev thread, mirror] | Real Reddit credentials may be unobtainable; D-09 gate applies |
| `cargo-component` active development | Marked experimental, last release 2025-04; fallback `cargo build --target wasm32-wasip2` | per plugins/README.md | Keep 0.21.1 pinned; fallback needs user sign-off |

**Deprecated/outdated:** none in scope beyond the above.

## Assumptions Log

| # | Claim | Section | Risk if Wrong |
|---|-------|---------|---------------|
| A1 | The rustup proxy exports `RUSTUP_TOOLCHAIN` to `cargo test` children so it can override a plugin's `rust-toolchain.toml` | Pitfall 1 | Low today (both pin 1.96.0); stripping env vars is harmless either way |
| A2 | `git clone --filter=blob:none --sparse` plus `sparse-checkout set wit` works against GitHub for a tag | Code Examples (WIT distribution) | Guide's primary fetch command fails; fall back to GitHub tarball. Verify when a tag exists |
| A3 | GitHub does not support `git archive --remote` | Pitfall 10 / Code Examples | Minor: would only widen the options |
| A4 | Reddit credentials contact default `u/filimon-integration-test` is acceptable to Reddit's UA rules; `REDDIT_CLIENT_ID` / `REDDIT_USER_AGENT_CONTACT` are the right new `.env` key names | Pattern 4 | Live test fails or key names need renaming; user may prefer different names |
| A5 | A lower bound of 2005-06-23 is a safe `published` floor for live assertions (Reddit founding) and a few hours of clock slack is enough on the upper bound | Pattern 3 | Spurious live failure; widen the range |
| A6 | Template `poll` fetching `fetch_url` and echoing a bounded body excerpt is acceptable as "minimal" for D-10 | Pattern 6 | If the user wants the template to stay a pure hardcoded item, DOCS-02's observable proof needs a separate worked source |
| A7 | Failing loudly (not skipping) when `cargo-component` is missing matches the user's intent for "CI-safe" | Pitfall 3 | A CI box without the toolchain would fail rather than skip |

## Open Questions

1. **WIT tag name and publication (D-11, costly to reverse)**
   - Known: no tags exist; `wit/` is tracked; remote is `ferrlindev/filimon`; the work is on branch `fluvio-wise` and push state is unknown.
   - Unclear: tag name, and whether the commit containing the current `wit/` is on the remote.
   - Recommendation: plan a human-action checkpoint to create and push a tag (suggest `wit-v0.1.0` matching `package filimon:plugin@0.1.0`); the script defaults to local `git archive HEAD wit` so verification never blocks on it, with `WIT_REF` exercising the remote route once the tag exists.

2. **Missing `cargo-component` when the `.wasm` is absent**
   - Recommendation: fail with the install command (A7). Confirm if the user would rather skip.

3. **Where do the live-test credentials come from?**
   - Known: `.env.example` lacks `REDDIT_CLIENT_ID`; the sample TOML hard-codes a placeholder `client_id`.
   - Recommendation: repo-root `.env` plus process environment (A4); add `REDDIT_CLIENT_ID` and `REDDIT_USER_AGENT_CONTACT` to `examples/plugins/.env.example`; document in README.

4. **Reddit approval status (D-09)**
   - Unknown whether the user already holds an approved app. Plan the credentials checkpoint first in the TEST-04 plan and gate the waiver as a user decision.

5. **GLiNER model (TEST-04)**
   - Missing locally; human download step required before the run. Whether the user wants Docker (`docker compose --profile setup up model-downloader`) or the Python recipe is theirs to choose; Docker 29.8.2 is installed.

6. **Guide's fetcher walkthrough shape (D-13)**
   - Recommendation in Pattern 6 (copy `replay-fetcher`, replace `fixtures.rs`). Confirm it satisfies "reuses the replay fetcher as the worked example" since the outside-repo copy cannot keep cross-crate `include_bytes!` paths.

## Environment Availability

| Dependency | Required By | Available | Version | Fallback |
|------------|------------|-----------|---------|----------|
| Rust / cargo | all builds | yes | 1.96.0 (active) | none needed |
| `cargo-component` | D-03 builds, template proof | yes | 0.21.1 | fallback in plugins/README.md needs sign-off |
| `wasm-tools` | conformance checks | yes | 1.263.0 | none needed |
| Fluvio CLI + local cluster | TEST-02/03, TEST-04 | yes | cluster reachable on 127.0.0.1:9003 [VERIFIED: `nc -z 127.0.0.1 9003`; `fluvio cluster status` ran] | tests skip cleanly (D-02) |
| Topics + SmartModules | TEST-04 | yes | topics `filimon-raw-pages`, `filimon-social-items`, `filimon-articles`; SmartModules `map-extract`, `map-preprocess`, `filter-relevance` at 0.1.0 [VERIFIED: `fluvio topic list`, `fluvio smartmodule list`] | — |
| Built plugin `.wasm` | tests | yes (6 present) [VERIFIED: `ls plugins/*/target/.../*.wasm`] | — | built on demand; `replay-fetcher` and `template` do not exist yet |
| `fili` binary | WIT-03 proof | yes | `target/debug/fili` present (no `target/release/fili`) | script builds it before, never after, the plugin |
| `jq`, `curl`, `tar`, `git`, `python3`, `openssl` | scripts | yes | system | — |
| `just` | — | no | — | bash scripts (chosen) |
| Docker | optional model download | yes | 29.8.2 | Python `huggingface_hub` recipe |
| GLiNER `tokenizer.json` + `onnx/model.onnx` | TEST-04 NER | NO | — | none: human download required (blocking for SC3's "NER-enriched") |
| Reddit app credentials, YouTube API key | live tests, TEST-04 | unknown (not inspected; `.env` is gitignored and was not read) | — | D-09: stop and ask |
| Outbound network for crates.io | authoring proof | yes in spike | — | cached crates may allow `--offline` (not verified) |

**Missing dependencies with no fallback:** GLiNER model files; Reddit credentials/approval (human).
**Missing dependencies with fallback:** none blocking.

## Validation Architecture

### Test Framework
| Property | Value |
|----------|-------|
| Framework | Rust built-in harness (`cargo test`), `#[tokio::test]` |
| Config file | none; `[dev-dependencies] tokio` with `test-util` already in `Cargo.toml` |
| Quick run command | `cargo test -p filimon e2e_replay_ -- --nocapture` |
| Full suite command | `cargo test -p filimon` (plus the `--ignored` groups in plugins/README.md as before) |

### Phase Requirements -> Test Map
| Req ID | Behavior | Test Type | Automated Command | File Exists? |
|--------|----------|-----------|-------------------|-------------|
| TEST-02 | Reddit + YouTube replay lands exact `SourceItem`s in a scratch Fluvio topic | integration (needs cluster, skips otherwise) | `cargo test -p filimon e2e_replay_ -- --nocapture` | Wave 0 (`src/plugins/host/integration_tests.rs`) |
| TEST-02 | Missing cluster skips with a clear message | integration | same, run with Fluvio stopped (manual) | Wave 0 |
| TEST-03 | Same body against real APIs with credentials; skips with `skipped: missing <KEY>` | integration | `cargo test -p filimon e2e_credentialed_ -- --nocapture` | Wave 0 |
| TEST-04 | Enriched social + news JSON from a real run | manual-only (human checkpoint, D-07) | `scripts/live-e2e.sh` | Wave 0; evidence in phase dir |
| WIT-03 | Out-of-repo plugin builds against only the WIT and runs in an unmodified `fili` | script | `scripts/verify-authoring.sh` | Wave 0 |
| DOCS-01 | Template builds in-repo as-is and via guide steps | unit-ish build | `(cd plugins/template && cargo component build --release)` and the script | Wave 0 |
| DOCS-02 | Fetcher built by following the guide serves a source's requests | script | `scripts/verify-authoring.sh` (fetcher leg) | Wave 0 |

### Sampling Rate
- **Per task commit:** the relevant quick command above (each is under 30 s with a warm build cache and a running cluster).
- **Per wave merge:** `cargo test -p filimon` and `scripts/verify-authoring.sh`.
- **Phase gate:** full suite green with a reachable cluster (skips are not a pass: record "not run" if no cluster), authoring script run by the user once (SC4), TEST-04 evidence committed and referenced from VERIFICATION.md.

### Wave 0 Gaps
- [ ] `plugins/replay-fetcher/` crate (and its built `.wasm` via helper)
- [ ] `plugins/template/` crate
- [ ] `src/plugins/host/integration_tests.rs` plus `mod` line in `src/plugins/host/mod.rs`
- [ ] Extended registry writer in `src/plugins/host/testing.rs`
- [ ] `scripts/verify-authoring.sh`, `scripts/live-e2e.sh`
- [ ] `docs/plugin-guide.md`
- [ ] `.env.example` additions, `plugins/README.md` test-group and plugin-list updates, `.gitignore` entry for the live-E2E work dir
- [ ] Framework install: none

## Security Domain

`security_enforcement` is enabled (absent/true in `.planning/config.json`; `security_asvs_level: 1`).

### Applicable ASVS Categories

| ASVS Category | Applies | Standard Control |
|---------------|---------|-----------------|
| V2 Authentication | partial | Host-managed tokens stay in the host; tests use a stored token file with mode 0600 via `TokenStore`; never print tokens |
| V3 Session Management | no | No sessions introduced |
| V4 Access Control | yes (plugin boundary) | Host allowlist and fetcher assignment exercised by the replay path; replay fetcher must never be registered in a real registry |
| V5 Input Validation | yes | Replay fetcher treats the request as untrusted (substring match only); guide scripts quote paths and never `eval` downloaded content |
| V6 Cryptography | no | TLS stays in the host |
| V7 Error Handling and Logging | yes | No secrets, tokens, URLs with query, headers or bodies in test output, replay-fetcher logs or the transcript |
| V8 Data Protection | yes | Evidence sample sanitised (authors, `raw`, long text) and secret-scanned before commit |
| V14 Configuration | yes | `.env` stays gitignored (`.env`, `.env.*` ignored, `!.env.example`); new `.env.example` keys have empty values |

### Known Threat Patterns for this stack

| Pattern | STRIDE | Standard Mitigation |
|---------|--------|---------------------|
| Real credentials leak into committed evidence or transcript | Information disclosure | `jq` redaction + `grep -F` scan of every non-empty `.env` value against evidence files, failing the script; review diff before commit |
| Credentials written to temp registry files by tests | Information disclosure | Pass secrets through the `load_registry_with_env` closure; temp dir holds only non-secret TOML |
| Live test failure message echoes a request header or key | Information disclosure | Assert on structure and print fixed text; never `{:?}` a request or response |
| Replay fetcher copied into a real registry serves stale data | Tampering | Doc comment plus README note (same wording as the stub-fetcher entry: "never register it for a real source"); keep it out of `examples/plugins/filimon.toml` |
| Guide tells authors to run a downloaded script or untrusted tarball | Tampering / supply chain | Guide uses `git clone` of a pinned tag and plain file copies; scripts take paths as arguments and build in a fresh empty temp dir |
| A malicious or buggy third-party fetcher sees all headers (incl. `authorization`) | Information disclosure | Already a documented limit [CITED: plugins/README.md "Known limits"]; the guide's fetcher section must repeat it |
| Test pollutes the production topic | Tampering | Scratch topics only; the replay/live tests never call `FluvioSink::connect_social()` |
| Live runs consume real API quota or rate-limit the user's Reddit app | Denial of service | Single poll per test, long interval; YouTube costs 1 unit per call [CITED: plugins/README.md "Rate limits and quota"] |

## Project Constraints (from CLAUDE.md)

`.claude/CLAUDE.md` imports `AGENTS.md` and `WORKFLOW.md` (the GSD Path pipeline rules). Actionable directives that touch this research:
- Authority order: user in chat, CHARTER, INTENT, SYNTHESIS, then phase brief. This repo uses the `.planning/` (GSD) layout, and `.project/` does not exist in the working tree; no conflict was found, but if the planner introduces `.project/` artifacts, the AGENTS.md rules (verified Verify commands that name a path from the task's `files`; a task Verify must not copy the project command) apply.
- Evidence and honesty: never report a command as passing unless it ran; report partial results plainly. In particular a skipped integration test must be reported as skipped, not passed.
- Ask the user for choices with a recommended option listed first and a one-line reason; present Outcome / Review / Next at checkpoints. The Open Questions above map to such choices (tag name, Reddit waiver).
- Files are the only memory: plans and verification must be on disk, not in conversation.
- Plain-prompt mutation guard: only relevant if `.project/STATE.md` exists; it does not.
- Project-level constraints from STATE/CONTEXT also bind: never modify the `fluvio/` reference checkout; do not fix existing `fluvio-wise` gaps.
- `.claude/skills/` does not exist; `.agents/skills/` holds GSD skills only (no project plugin-authoring rules found).

## Sources

### Primary (HIGH confidence) - read or executed this session
- `/Users/jferrolino/Devs/portfolio/filimon/.planning/phases/08-integration-live-e2e-authoring/08-CONTEXT.md`, `.planning/REQUIREMENTS.md`, `.planning/STATE.md`
- `wit/types.wit`, `wit/worlds.wit`, `wit/host.wit` (worlds `source` and `fetcher`; `fetch`/`outbound-http` interfaces)
- `plugins/README.md` (whole file: build, host behaviour, fetcher layer, OAuth, Reddit/YouTube sections, host test groups)
- `src/plugins/host/{mod.rs,sink.rs,testing.rs,wasm_tests.rs,once.rs,scheduler.rs}`, `src/plugins/config.rs`, `src/plugins/auth/testing.rs`, `src/main.rs`, `src/cli.rs`, `Cargo.toml`
- `plugins/{stub-source,stub-fetcher,http-fetcher,reddit,youtube}` sources, manifests, fixtures and READMEs; `examples/plugins/*`
- `tests/plugins_cli.rs`, `.planning/codebase/TESTING.md`
- Executed: `cargo component --version`, `wasm-tools --version`, `fluvio topic list`, `fluvio smartmodule list`, `nc -z 127.0.0.1 9003`, `git tag`, `git ls-files wit`, double `cargo component build --release` in `plugins/http-fetcher`, and the WIT-03 spike (build outside repo, `plugins check`, `plugins run-once --json`, binary hash before/after)

### Secondary (MEDIUM confidence)
- r/redditdev "Introducing the Responsible Builder Policy" thread (mirror `r.datuan.dev`, Nov 2025): approval required for new API access; anecdotal rejections. Not an official Reddit page.

### Tertiary (LOW confidence)
- Search for cargo-component 0.21 path-dependency/target documentation returned nothing authoritative; behaviour relied on here is what the repo's existing crates and the spike demonstrated.

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - no new packages; versions read from installed tools and lockfiles.
- Architecture: HIGH - bin-only constraint, fixed topic, replay routing and Reddit token behaviour all read in source; WIT-03 flow executed end to end.
- Pitfalls: MEDIUM-HIGH - most verified by running commands; env-var inheritance (A1), sparse-checkout (A2) and Reddit access are not.

**Research date:** 2026-11-12
**Valid until:** 2026-12-12 (stable; re-check Reddit access policy and cargo-component release status if planning slips)
