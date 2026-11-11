// ─────────────────────────────────────────────────────────────────────────────
// commands/produce.rs - Produce mode
//
// Crawls seed URLs, discovers and priority-ranks child links, and produces
// raw HTML pages to the Fluvio pipeline.
//
// Pages are fetched by the built-in HTTP client or, when the registry's
// `[crawler]` table names one, through a fetcher component. The choice is made
// once, before anything connects, and never falls back to
// the built-in client (which skips certificate validation) from a `[crawler]`
// that cannot be used. One crawl loop serves both fetchers.
// ─────────────────────────────────────────────────────────────────────────────

use std::future::Future;
use std::path::Path;

use crate::crawler::{self, PageFetcher};
use crate::models::{CrawlTarget, RawPage, SourceItem};
use crate::plugins::config::load_crawler;
use crate::plugins::host::fetch::CrawlFetcher;
use crate::streaming::{PipelineCoordinator, TOPIC_RAW_PAGES};

/// Maximum links to extract per seed URL for priority ranking.
const LINKS_PER_SEED: usize = 30;

/// Crawl URLs and produce raw pages to Fluvio.
///
/// # Arguments
/// * `urls` - Seed URLs to crawl
/// * `depth` - Maximum pages to fetch per seed after priority ranking
/// * `registry` - The plugin registry whose `[crawler]` table, when present,
///   names the fetcher the pages are fetched through
pub async fn run(urls: &[String], depth: usize, registry: &Path) -> miette::Result<()> {
    // A bad `[crawler]` stops the command before it connects to Fluvio.
    let fetcher = select_page_fetcher(registry, urls)?;

    println!("[Produce] Connecting to Fluvio cluster...");
    let coordinator = PipelineCoordinator::new().await?;
    let producer = coordinator.raw_pages_producer();

    let total_produced = crawl(&fetcher, urls, depth, |page| {
        let producer = &producer;
        async move {
            // Produce to Fluvio
            let key = page.url.clone();
            let item = SourceItem::news(page.url, page.html);
            let locked = producer.lock().await;
            locked.send(&key, &item).await?;
            println!("    → Produced to {TOPIC_RAW_PAGES}");
            Ok::<(), miette::Report>(())
        }
    })
    .await?;

    // Flush all pending records
    let locked = producer.lock().await;
    locked.flush().await?;

    println!("\n╔══════════════════════════════════════════╗");
    println!("║  Produce complete                         ║");
    println!("╠══════════════════════════════════════════╣");
    println!("║  Pages produced: {total_produced:<24}║");
    println!("║  Topic: {TOPIC_RAW_PAGES:<32}║");
    println!("╚══════════════════════════════════════════╝");

    Ok(())
}

/// Chooses how pages are fetched.
///
/// The built-in client is used only when no `[crawler]` is configured: the
/// registry file does not exist, or it has no `[crawler]` table. Either way
/// one line says so. Every other problem is an error: a registry that cannot
/// be read or parsed, a `[crawler]` table that is invalid, names an unknown or
/// disabled fetcher, or whose fetcher component cannot be loaded.
///
/// # Errors
/// The configuration or load problem, rendered by `miette`.
pub(crate) fn select_page_fetcher(registry: &Path, urls: &[String]) -> miette::Result<PageFetcher> {
    if matches!(registry.try_exists(), Ok(false)) {
        println!(
            "[Crawl] No plugin registry at {}; using the built-in HTTP client",
            registry.display()
        );
        return Ok(PageFetcher::BuiltIn);
    }
    let Some(crawler) = load_crawler(registry).map_err(miette::Report::new)? else {
        println!(
            "[Crawl] {} has no [crawler] table; using the built-in HTTP client",
            registry.display()
        );
        return Ok(PageFetcher::BuiltIn);
    };
    let fetcher = CrawlFetcher::load(&crawler, urls).map_err(miette::Report::new)?;
    println!("[Crawl] fetching through the {}", fetcher.describe());
    Ok(PageFetcher::Component(Box::new(fetcher)))
}

/// The crawl loop, the same for both fetchers: fetch each seed, rank its
/// links, then hand the seed page and up to `depth` ranked links, in priority
/// order, to `emit`. A seed or page that fails to fetch is reported and
/// skipped; an `emit` error stops the crawl. Returns the number of pages
/// emitted.
///
/// The seed page is fetched once: the page fetched to rank its links is also
/// the first page emitted.
///
/// # Errors
/// The first error `emit` returns.
pub(crate) async fn crawl<F, Fut>(
    fetcher: &PageFetcher,
    urls: &[String],
    depth: usize,
    mut emit: F,
) -> miette::Result<usize>
where
    F: FnMut(RawPage) -> Fut,
    Fut: Future<Output = miette::Result<()>>,
{
    let mut emitted = 0;

    for seed_url in urls {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("[Crawl] Seed: {seed_url}");

        let seed = CrawlTarget {
            url: seed_url.clone(),
            priority: 1.0,
            depth: 0,
        };

        let seed_page = match fetcher.fetch_page(&seed).await {
            Ok(p) => p,
            Err(e) => {
                eprintln!("  ✗ {e}");
                continue;
            }
        };

        let mut targets = crawler::extract_links(&seed_page, LINKS_PER_SEED);
        println!(
            "  Discovered {} links (top priority: {:.2})",
            targets.len(),
            targets.first().map_or(0.0, |t| t.priority)
        );
        targets.insert(0, seed);

        let mut seed_page = Some(seed_page);
        for (i, target) in targets.iter().enumerate().take(depth + 1) {
            println!(
                "\n  [{}/{}] p={:.2}  {}",
                i + 1,
                depth + 1,
                target.priority,
                target.url
            );

            let page = match seed_page.take().filter(|_| i == 0) {
                Some(page) => page,
                None => match fetcher.fetch_page(target).await {
                    Ok(p) => p,
                    Err(e) => {
                        println!("    ✗ {e}");
                        continue;
                    }
                },
            };

            emit(page).await?;
            emitted += 1;
        }
    }

    Ok(emitted)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use std::cell::RefCell;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU16, Ordering};
    use std::time::{Duration, Instant};

    use tempfile::TempDir;

    use crate::plugins::config::LoadedCrawler;
    use crate::plugins::host::testing::{Reply, SeenRequest, TestHttpServer, fixture_wasm};

    fn write(dir: &TempDir, name: &str, text: &str) -> std::path::PathBuf {
        let path = dir.path().join(name);
        std::fs::write(&path, text).expect("write fixture");
        path
    }

    /// The miette report as a terminal would show it, without colour.
    fn rendered(report: &miette::Report) -> String {
        let mut out = String::new();
        miette::GraphicalReportHandler::new_themed(miette::GraphicalTheme::unicode_nocolor())
            .with_width(400)
            .render_report(&mut out, report.as_ref())
            .expect("render report");
        out
    }

    fn seeds(text: &[&str]) -> Vec<String> {
        text.iter().map(|url| (*url).to_string()).collect()
    }

    #[test]
    fn a_missing_registry_file_selects_the_builtin_client() {
        let dir = TempDir::new().expect("tempdir");

        let fetcher = select_page_fetcher(&dir.path().join("filimon.toml"), &seeds(&[]))
            .expect("no registry is no crawler");

        assert!(matches!(fetcher, PageFetcher::BuiltIn));
    }

    #[test]
    fn a_registry_without_a_crawler_table_selects_the_builtin_client() {
        let dir = TempDir::new().expect("tempdir");
        let registry = write(
            &dir,
            "filimon.toml",
            "[sources.alpha]\ncomponent = \"alpha.wasm\"\nconfig = \"alpha.toml\"\n",
        );

        let fetcher = select_page_fetcher(&registry, &seeds(&[])).expect("no crawler table");

        assert!(matches!(fetcher, PageFetcher::BuiltIn));
    }

    #[test]
    fn an_invalid_crawler_table_is_an_error_and_never_a_fallback() {
        let dir = TempDir::new().expect("tempdir");
        let unknown = write(
            &dir,
            "unknown.toml",
            "[fetchers.http]\ncomponent = \"http.wasm\"\n\n[crawler]\nfetcher = \"nope\"\n",
        );
        let disabled = write(
            &dir,
            "disabled.toml",
            "[fetchers.nope]\ncomponent = \"http.wasm\"\nenabled = false\n\n[crawler]\nfetcher = \"nope\"\n",
        );
        let invalid = write(
            &dir,
            "invalid.toml",
            "[fetchers.nope]\ncomponent = \"http.wasm\"\n\n[crawler]\nfetcher = \"nope\"\ntimeout_secs = 0\n",
        );

        for registry in [&unknown, &disabled] {
            let Err(report) = select_page_fetcher(registry, &seeds(&[])) else {
                panic!("{} fell back to the built-in client", registry.display());
            };
            let text = rendered(&report);
            assert!(text.contains("crawler"), "{text}");
            assert!(text.contains("nope"), "{text}");
        }
        // A table that does not parse (a zero timeout) is an error as well.
        assert!(
            select_page_fetcher(&invalid, &seeds(&[])).is_err(),
            "an invalid [crawler] fell back to the built-in client"
        );
    }

    #[test]
    fn a_registry_that_is_not_toml_is_an_error_and_never_a_fallback() {
        let dir = TempDir::new().expect("tempdir");
        let registry = write(&dir, "filimon.toml", "this is = not [toml");

        let result = select_page_fetcher(&registry, &seeds(&[]));

        assert!(result.is_err(), "a broken registry fell back to the built-in client");
    }

    #[test]
    fn a_crawler_whose_fetcher_component_is_missing_is_an_error_naming_it() {
        let dir = TempDir::new().expect("tempdir");
        let registry = write(
            &dir,
            "filimon.toml",
            "[fetchers.http]\ncomponent = \"missing.wasm\"\n\n[crawler]\nfetcher = \"http\"\n",
        );

        let Err(report) = select_page_fetcher(&registry, &seeds(&["http://127.0.0.1/"])) else {
            panic!("a missing component fell back to the built-in client");
        };

        let text = rendered(&report);
        assert!(text.contains("http"), "{text}");
        assert!(text.contains("missing.wasm"), "{text}");
    }

    // ── Crawl loop over a real fetcher component ──────────────────────────────

    fn component_fetcher(seed_urls: &[String], timeout: Duration, extra: &[&str]) -> PageFetcher {
        let crawler = LoadedCrawler {
            fetcher: "http".to_string(),
            fetcher_component: fixture_wasm("http-fetcher", "http_fetcher.wasm"),
            timeout,
            extra_hosts: extra.iter().map(|host| (*host).to_string()).collect(),
        };
        let fetcher = CrawlFetcher::load(&crawler, seed_urls).expect("the fetcher loads");
        PageFetcher::Component(Box::new(fetcher))
    }

    #[tokio::test]
    #[ignore = "needs plugin fixtures: see plugins/README.md"]
    async fn wasm_crawl_visits_the_seed_and_its_ranked_links_through_the_component() {
        // `/news/2024/story` outranks `/world/report`, which outranks
        // `/misc/page` (a noise word); the external link is not followed.
        let seed_page = r#"<html><body>
            <a href="/misc/page">link</a>
            <a href="/world/report">link</a>
            <a href="/news/2024/story">link</a>
            <a href="http://elsewhere.invalid/news/story">link</a>
        </body></html>"#;
        let server = TestHttpServer::start(move |seen| match seen.target.as_str() {
            "/" => Reply::ok(seed_page),
            other => Reply::ok(format!("<html><body>{other}</body></html>")),
        })
        .await;
        let urls = vec![server.url("/")];
        let fetcher = component_fetcher(&urls, Duration::from_secs(5), &[]);
        let pages = RefCell::new(Vec::new());

        let count = crawl(&fetcher, &urls, 2, |page| {
            pages.borrow_mut().push(page);
            async { Ok(()) }
        })
        .await
        .expect("the crawl completes");

        assert_eq!(count, 3);
        let pages = pages.into_inner();
        let emitted: Vec<&str> = pages.iter().map(|page| page.url.as_str()).collect();
        assert_eq!(
            emitted,
            [
                server.url("/"),
                server.url("/news/2024/story"),
                server.url("/world/report"),
            ]
        );
        assert!(pages[1].html.contains("/news/2024/story"), "{}", pages[1].html);
        let requests = server.seen();
        let targets: Vec<&str> = requests.iter().map(|r| r.target.as_str()).collect();
        assert_eq!(targets, ["/", "/news/2024/story", "/world/report"]);
        for request in &requests {
            assert_eq!(
                request.header("user-agent"),
                Some("WISECrawler/1.0 (academic research)")
            );
        }
    }

    // ── Allowlist, redirect and timeout on real wasm ──────────────────────────

    /// A server whose handler also learns the server's own port, so a reply
    /// can point at `localhost:<port>`.
    async fn start_with_port(
        handler: impl Fn(&SeenRequest, u16) -> Reply + Send + Sync + 'static,
    ) -> TestHttpServer {
        let port = Arc::new(AtomicU16::new(0));
        let shared = Arc::clone(&port);
        let server =
            TestHttpServer::start(move |seen| handler(seen, shared.load(Ordering::SeqCst))).await;
        port.store(server.port(), Ordering::SeqCst);
        server
    }

    fn redirecting_server() -> impl std::future::Future<Output = TestHttpServer> {
        start_with_port(|seen, port| match seen.target.as_str() {
            "/r" => Reply::redirect(302, &format!("http://localhost:{port}/x")),
            _ => Reply::ok("reached"),
        })
    }

    fn target(url: String) -> CrawlTarget {
        CrawlTarget {
            url,
            priority: 1.0,
            depth: 0,
        }
    }

    #[tokio::test]
    #[ignore = "needs plugin fixtures: see plugins/README.md"]
    async fn wasm_crawler_denies_a_redirect_to_a_host_that_is_not_a_seed_host() {
        let server = redirecting_server().await;
        let urls = vec![server.url("/r")];
        let fetcher = component_fetcher(&urls, Duration::from_secs(5), &[]);

        let result = fetcher.fetch_page(&target(server.url("/r"))).await;

        let Err(text) = result else {
            panic!("a redirect to another host was followed: {result:?}");
        };
        assert!(text.contains("denied"), "{text}");
        assert!(text.contains("localhost"), "{text}");
        let requests: Vec<String> = server.seen().into_iter().map(|r| r.target).collect();
        assert_eq!(requests, ["/r"], "the second host was contacted");
    }

    #[tokio::test]
    #[ignore = "needs plugin fixtures: see plugins/README.md"]
    async fn wasm_crawler_extra_hosts_allow_a_redirect_target() {
        let server = redirecting_server().await;
        let urls = vec![server.url("/r")];
        let fetcher = component_fetcher(&urls, Duration::from_secs(5), &["localhost"]);

        let page = fetcher
            .fetch_page(&target(server.url("/r")))
            .await
            .expect("the listed host is reachable through the redirect");

        assert_eq!(page.html, "reached");
        let requests: Vec<String> = server.seen().into_iter().map(|r| r.target).collect();
        assert_eq!(requests, ["/r", "/x"]);
    }

    #[tokio::test]
    #[ignore = "needs plugin fixtures: see plugins/README.md"]
    async fn wasm_a_hung_server_is_abandoned_at_the_crawler_timeout_and_the_crawl_moves_on() {
        let server = TestHttpServer::start(|seen| match seen.target.as_str() {
            "/slow" => Reply::ok("too late").delayed(Duration::from_secs(5)),
            _ => Reply::ok("<html><body>ok</body></html>"),
        })
        .await;
        let urls = vec![server.url("/slow"), server.url("/ok")];
        let fetcher = component_fetcher(&urls, Duration::from_secs(1), &[]);
        let pages = RefCell::new(Vec::new());
        let started = Instant::now();

        let count = crawl(&fetcher, &urls, 0, |page| {
            pages.borrow_mut().push(page.url);
            async { Ok(()) }
        })
        .await
        .expect("a hung seed does not stop the crawl");

        assert_eq!(count, 1);
        assert_eq!(pages.into_inner(), [server.url("/ok")]);
        let elapsed = started.elapsed();
        assert!(elapsed < Duration::from_secs(4), "the crawl took {elapsed:?}");
        assert!(elapsed >= Duration::from_secs(1), "the timeout was not applied: {elapsed:?}");
    }
}
