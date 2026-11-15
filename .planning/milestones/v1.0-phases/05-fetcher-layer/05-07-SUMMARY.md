---
phase: 05-fetcher-layer
plan: 07
subsystem: fetcher-layer
tags: [crawler, fetcher, wasmtime, registry-config, ssrf, fili-produce]

requires:
  - phase: 05-fetcher-layer
    provides: FetchPolicy/FetchRoute/Egress routing and the http-fetcher component (plans 05-01 to 05-04)
provides:
  - "`[crawler]` registry table (fetcher, timeout_secs, allowlist) with LoadedCrawler and the lenient load_crawler"
  - "CrawlFetcher: a crawler-labelled FetchRoute under the same FetchPolicy sources use"
  - "PageFetcher (built-in client or fetcher component) and the shared USER_AGENT constant"
  - "fili produce selects its fetcher from the registry and runs one injectable crawl loop for both"
affects: [05-08, phase-verification, headless-browser-fetcher]

actuals:
  tokens: 14000
  tasks: 3
  commits: 3
plan_head_before: 6d3f651e1e3fe5900d786cde88fe0ef640d8aea7
plan_head_after: 8cf2474348ede92286f0749383d96730c1f60bad

tech-stack:
  added: []
  patterns:
    - "Crawler is a second consumer of the same routing: consumer label `crawler`, same FetchPolicy type as a source"
    - "Lenient single-purpose registry view (CrawlerView) so unrelated broken entries cannot stop a command"
    - "No silent fallback: the insecure built-in client is chosen only when no [crawler] is configured, and the run says so"

key-files:
  created: []
  modified:
    - src/plugins/config.rs
    - src/plugins/error.rs
    - src/plugins/host/fetch.rs
    - src/crawler.rs
    - src/commands/produce.rs
    - src/main.rs

key-decisions:
  - "Seed page fetched once: the page fetched to rank links is also the first page emitted (the old loop fetched the seed twice); required by the plan's three-GET crawl test"
  - "A registry path is treated as absent only when try_exists() is Ok(false); an unreadable or non-file path goes to load_crawler and errors rather than silently using the built-in client"
  - "Invalid [crawler] tables that fail TOML typing (zero timeout, unknown key) surface as RegistryMalformed with a source snippet, not as a crawler-named variant"

patterns-established:
  - "Dead-code allows used as a scaffold between tracer and wiring tasks, removed when the consumer lands"

requirements-completed: [FETCH-04]

coverage:
  - id: D1
    description: "A [crawler] table makes a page fetch go through the fetcher component with the crawler's User-Agent (tracer)"
    requirement: FETCH-04
    verification:
      - kind: integration
        ref: "src/plugins/host/fetch.rs#wasm_tracer_crawler_page_is_fetched_through_the_component"
        status: pass
    human_judgment: false
  - id: D2
    description: "[crawler] config loading and validation (defaults, unknown/disabled fetcher, zero timeout, unknown key, lenient loader, scoped loads)"
    requirement: FETCH-04
    verification:
      - kind: unit
        ref: "src/plugins/config.rs#crawler_table_* / a_crawler_* / load_crawler_ignores_sources_and_secrets / a_scoped_plugin_load_does_not_report_crawler_problems"
        status: pass
    human_judgment: false
  - id: D3
    description: "fili produce picks its page fetcher from the registry and never falls back to the built-in client from an unusable [crawler]"
    requirement: FETCH-04
    verification:
      - kind: unit
        ref: "src/commands/produce.rs#a_missing_registry_file_selects_the_builtin_client / an_invalid_crawler_table_is_an_error_and_never_a_fallback / a_registry_that_is_not_toml_is_an_error_and_never_a_fallback / a_crawler_whose_fetcher_component_is_missing_is_an_error_naming_it"
        status: pass
    human_judgment: false
  - id: D4
    description: "One crawl loop visits the seed and its ranked links through the component"
    requirement: FETCH-04
    verification:
      - kind: integration
        ref: "src/commands/produce.rs#wasm_crawl_visits_the_seed_and_its_ranked_links_through_the_component"
        status: pass
    human_judgment: false
  - id: D5
    description: "Crawler allowlist (seed hosts plus extras), redirect re-check and timeout hold on real wasm, with the same outcomes as a source route"
    requirement: FETCH-04
    verification:
      - kind: integration
        ref: "src/commands/produce.rs#wasm_crawler_denies_a_redirect_to_a_host_that_is_not_a_seed_host / wasm_crawler_extra_hosts_allow_a_redirect_target / wasm_a_hung_server_is_abandoned_at_the_crawler_timeout_and_the_crawl_moves_on; src/plugins/host/fetch.rs#wasm_source_and_crawler_routes_enforce_the_same_policy"
        status: pass
    human_judgment: false
  - id: D6
    description: "Articles still reach the consumer end to end through Fluvio with a [crawler] configured (roadmap criterion 4)"
    requirement: FETCH-04
    verification: []
    human_judgment: true
    rationale: "Needs a live Fluvio cluster, loaded SmartModules and a live third-party site; the send path is unchanged but the end-to-end run is the operator check in the plan's task 2 human-check"
  - id: D7
    description: "Interrupting fili produce mid-crawl leaves only complete records (backstop statement)"
    verification: []
    human_judgment: true
    rationale: "Plan marks this a backstop: each page is exactly one send and this plan does not change the send; not confirmable without a human"

duration: ~45min
completed: 2026-11-11
status: complete
---

# Phase 5 Plan 07: Crawler Fetcher Hook Summary

**`fili produce` can fetch its pages through a fetcher component named by a `[crawler]` registry table, under the host's seed-host allowlist, redirect re-check and timeout, and never silently falls back to the certificate-skipping built-in client.**

## Performance

- **Duration:** ~45 min
- **Completed:** 2026-11-11T03:16:56Z
- **Tasks:** 3
- **Files modified:** 6

## Accomplishments

- `[crawler]` table in `filimon.toml`: required `fetcher`, `timeout_secs` (default 15), optional extra `allowlist`; validated by the strict loader and by the new lenient `load_crawler`, which reads only `[crawler]` and `[fetchers]`.
- `CrawlFetcher` routes each page through a `crawler`-labelled `FetchRoute` with the same `FetchPolicy` type sources use; its allowlist is the seed urls' hosts plus the operator's extras.
- `fili produce` picks its fetcher before connecting to Fluvio: built-in client only with no registry file or no `[crawler]` (and it prints which); every other problem is an error naming the cause.
- One injectable `crawl` loop serves both fetchers; the per-page send (`SourceItem::news`, `TOPIC_RAW_PAGES`, key = page url) is unchanged.
- Real-wasm tests prove a redirect to a non-seed host is denied and never contacted, a listed host is followed, a hung server is abandoned at the crawler timeout with the crawl moving on, and source and crawler routes give identical `Denied`/`Timeout` outcomes.

## Task Commits

1. **Task 1: tracer, a page is fetched through the component named by `[crawler]`** - `296d3e0` (feat)
2. **Task 2: `fili produce` picks its page fetcher from the registry, one crawl loop** - `f2d2f2b` (feat)
3. **Task 3: allowlist, redirect and timeout rules on real wasm, one policy for sources and crawler** - `8cf2474` (test; no production change was needed)

**Plan metadata:** committed separately with this SUMMARY (docs).

## Files Created/Modified

- `src/plugins/config.rs` - `CrawlerEntry`, `CrawlerView`, `LoadedCrawler`, `LoadedRegistry.crawler`, `crawler_problem`, `loaded_crawler`, `load_crawler`, shared `read_toml`, nine config tests
- `src/plugins/error.rs` - `UnknownCrawlerFetcher` / `DisabledCrawlerFetcher` (`plugin()` returns `crawler`)
- `src/plugins/host/fetch.rs` - `seed_hosts`, `page_request`, `page_from_response`, `fetch_error_text`, `CrawlFetcher`; pure tests, tracer, invariant test
- `src/crawler.rs` - `USER_AGENT` constant (sole edit to `build_client`), `PageFetcher`
- `src/commands/produce.rs` - `run` with registry path, `select_page_fetcher`, `crawl`, selection and crawl tests
- `src/main.rs` - `run_produce(urls, depth, &cli.registry)`

## Decisions Made

- The seed page is fetched once and reused as the first emitted page. The previous loop fetched the seed twice; the plan's `wasm_crawl_visits...` test requires exactly three GETs for seed plus two links.
- A registry path counts as "no registry" only when `try_exists()` is `Ok(false)`. A directory or an unreadable path reaches `load_crawler` and errors, so it cannot silently select the built-in client.
- `ConfigError::plugin()` returns `Some("crawler")` for the two new variants (as the plan specifies), so scoped plugin loads drop them.

## Deviations from Plan

### Auto-fixed Issues

**1. [Rule 3 - Blocking] Temporary dead-code allows between Task 1 and Task 2**
- **Found during:** Task 1 (clippy acceptance)
- **Issue:** Task 1 adds `CrawlFetcher`, `PageFetcher`, `load_crawler` and helpers that only Task 2 consumes, so non-test clippy reported never-used warnings that broke Task 1's own acceptance thresholds (`fetch.rs` zero, `config.rs` at most 3, `crawler.rs` at most 5).
- **Fix:** Added `#[allow(dead_code, reason = ...)]` scaffolding in Task 1 and removed every one in Task 2 once `produce` used the items.
- **Files modified:** src/plugins/host/fetch.rs, src/plugins/config.rs, src/crawler.rs
- **Verification:** clippy counts at Task 1 and again after Task 2 match the baselines; no allow remains.
- **Committed in:** 296d3e0 (added), f2d2f2b (removed)

**2. [Rule 1 - Bug] Test expectation for the zero-timeout table**
- **Found during:** Task 2
- **Issue:** `an_invalid_crawler_table_is_an_error_and_never_a_fallback` first asserted that every invalid table renders text naming `crawler` and `nope`. A zero `timeout_secs` is a TOML typing error whose rendered text shows the offending line, not the table name.
- **Fix:** The unknown and disabled fetcher cases assert both names; the zero-timeout case asserts only that it is an error (never a fallback), which is the behavior the plan protects.
- **Files modified:** src/commands/produce.rs
- **Committed in:** f2d2f2b

**3. [Rule 1 - Consistency] Seed fetched once instead of twice**
- **Found during:** Task 2
- **Issue:** The original loop fetched the seed to rank links and then again as the first target; the plan's crawl test expects three GETs for seed plus two links.
- **Fix:** The seed page fetched for ranking is emitted as the first page. Emitted pages are identical; one request is saved per seed.
- **Files modified:** src/commands/produce.rs
- **Committed in:** f2d2f2b

---

**Total deviations:** 3 auto-fixed (1 blocking, 2 consistency/test)
**Impact on plan:** None on scope. Task 3 needed no production change: all four enforcement and invariant tests passed against `CrawlFetcher::load` as written.

## Issues Encountered

None beyond the deviations above. The worktree started without a `target/` directory, so the first build compiled all dependencies; the gitignored `.wasm` fixtures were copied from the main checkout.

## Verification Results

- `cargo test -p filimon`: 433 passed, 0 failed (plan floor: more than 420; baseline 374 plus earlier plans).
- `cargo test -p filimon -- --ignored wasm_`: 41 + 3 passed, 0 failed.
- `cargo clippy -p filimon --all-targets`: no warnings in `error.rs`, `host/fetch.rs` or `produce.rs`; `config.rs` three (baseline), `crawler.rs` five (baseline), `main.rs` three (baseline).
- `git diff -- Cargo.toml Cargo.lock src/streaming.rs src/models.rs` empty. `build_client`'s diff changes only the User-Agent literal; the certificate-validation bypass line is untouched and not copied.
- Human check (live Fluvio, harvested at phase end): not run here.

## TDD Gate Compliance

The plan is `type: execute` (not `type: tdd`), so the RED/GREEN gate sequence is not enforced. Task 2 carries `tdd="true"` but, as a Rust change with tests in the same file, was committed as one `feat` commit with its tests; Task 3 is tests-only (`test` commit).

## Known Stubs

None.

## Threat Flags

None. The only new network surface is the crawler route, covered by T-5-21 to T-5-23 and enforced by the Task 3 tests.

## User Setup Required

None - no external service configuration required.

## Next Phase Readiness

- `[crawler]` is wired end to end through `fili produce`; the end-to-end Fluvio check (roadmap criterion 4) remains the operator human-check at phase verification.
- Plan 05-08 can build on `CrawlFetcher`, `PageFetcher` and `load_crawler`.

---
*Phase: 05-fetcher-layer*
*Completed: 2026-11-11*

## Self-Check: PASSED
