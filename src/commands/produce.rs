// ─────────────────────────────────────────────────────────────────────────────
// commands/produce.rs - Produce mode
//
// Crawls seed URLs, discovers and priority-ranks child links, and produces
// raw HTML pages to the Fluvio pipeline.
// ─────────────────────────────────────────────────────────────────────────────

use crate::crawler;
use crate::models::CrawlTarget;
use crate::streaming::{PipelineCoordinator, TOPIC_RAW_PAGES};

/// Maximum links to extract per seed URL for priority ranking.
const LINKS_PER_SEED: usize = 30;

/// Crawl URLs and produce raw pages to Fluvio.
///
/// # Arguments
/// * `urls` - Seed URLs to crawl
/// * `depth` - Maximum pages to fetch per seed after priority ranking
pub async fn run(urls: &[String], depth: usize) -> miette::Result<()> {
    println!("[Produce] Connecting to Fluvio cluster...");
    let coordinator = PipelineCoordinator::new().await?;
    let producer = coordinator.raw_pages_producer();

    let mut total_produced = 0;

    for seed_url in urls {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("[Crawl] Seed: {seed_url}");

        let seed = CrawlTarget {
            url: seed_url.clone(),
            priority: 1.0,
            depth: 0,
        };

        let seed_page = match crawler::fetch_page(&seed) {
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
            targets.first().map(|t| t.priority).unwrap_or(0.0)
        );
        targets.insert(0, seed);

        for (i, target) in targets.iter().enumerate().take(depth + 1) {
            println!(
                "\n  [{}/{}] p={:.2}  {}",
                i + 1,
                depth + 1,
                target.priority,
                target.url
            );

            let page = match crawler::fetch_page(target) {
                Ok(p) => p,
                Err(e) => {
                    println!("    ✗ {e}");
                    continue;
                }
            };

            // Produce to Fluvio
            let locked = producer.lock().await;
            locked.send(&target.url, &page).await?;
            println!("    → Produced to {}", TOPIC_RAW_PAGES);
            total_produced += 1;
        }
    }

    // Flush all pending records
    let locked = producer.lock().await;
    locked.flush().await?;

    println!("\n╔══════════════════════════════════════════╗");
    println!("║  Produce complete                         ║");
    println!("╠══════════════════════════════════════════╣");
    println!("║  Pages produced: {:<24}║", total_produced);
    println!("║  Topic: {:<32}║", TOPIC_RAW_PAGES);
    println!("╚══════════════════════════════════════════╝");

    Ok(())
}
