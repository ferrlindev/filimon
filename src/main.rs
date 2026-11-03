// ─────────────────────────────────────────────────────────────────────────────
// main.rs  –  filimon
//
// Fluvio-based streaming pipeline for web content extraction.
//
// Modes:
//   fili produce <URL>   Crawl URLs and produce news SourceItems to Fluvio
//   fili file <PATH>     Produce local HTML files as news SourceItems
//   fili consume         Read news through the SmartModule chain and write JSON
//   fili watch           Watch directories and produce HTML to Fluvio
//
// Pipeline Architecture (via SmartModules):
//   filimon-raw-pages → [map-preprocess] → [map-extract] → [filter-relevance] → fili consume
//
// `fili consume` attaches the chain to its own filimon-raw-pages stream and
// writes the unified records to a JSON file. NER is applied on the consumer
// side using GLiNER (too heavy for SmartModules).
// ─────────────────────────────────────────────────────────────────────────────

mod check;
mod cli;
mod commands;
mod config;
mod crawler;
mod models;
mod output;
mod plugins;
mod scorer;
mod streaming;

use clap::Parser;
use cli::{Cli, Command};
use commands::{run_consume, run_produce, run_watch};
use models::SourceItem;
use scorer::NerEngine;
use std::path::{Path, PathBuf};
use streaming::{PipelineCoordinator, TOPIC_RAW_PAGES};

// ── Validation ────────────────────────────────────────────────────────────────
fn criteria(item: &str) -> Result<String, check::ItemError> {
    if Path::new(item).is_dir() {
        Ok(item.to_string())
    } else {
        Err(check::ItemError {
            item: item.to_string(),
            message: "No such directory found.".to_string(),
        })
    }
}

// ── Entry Point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> miette::Result<()> {
    let cli = Cli::parse();

    println!("╔═══════════════════════════════════════════════╗");
    println!("║  filimon · Fluvio streaming news extractor    ║");
    println!("╚═══════════════════════════════════════════════╝\n");

    match cli.command {
        Command::Produce { ref urls, depth } => {
            run_produce(urls, depth).await?;
        }
        Command::Consume {
            from_beginning,
            limit,
            enrich_ner,
            ..
        } => {
            let ner = if enrich_ner {
                load_ner_engine(&cli.tokenizer, &cli.model)
            } else {
                None
            };
            run_consume(from_beginning, limit, ner.as_ref()).await?;
        }
        Command::Watch => {
            run_watch(&cli).await?;
        }
        Command::File { ref paths } => {
            run_file(paths).await?;
        }
    }

    Ok(())
}

fn load_ner_engine(tokenizer: &PathBuf, model: &PathBuf) -> Option<NerEngine> {
    println!("  Loading GLiNER NER engine...");
    println!("    tokenizer: {}", tokenizer.display());
    println!("    model: {}", model.display());

    match NerEngine::new(tokenizer, model) {
        Ok(e) => {
            println!("  NER: ✓ GLiNER loaded\n");
            Some(e)
        }
        Err(e) => {
            eprintln!("  NER: ✗ unavailable ({e})\n");
            None
        }
    }
}

// ── Mode: Produce ─────────────────────────────────────────────────────────────
/*
async fn run_produce(urls: &[String], depth: usize) -> miette::Result<()> {
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
*/
// ── Mode: Consume ─────────────────────────────────────────────────────────────
/*
async fn run_consume(
    from_beginning: bool,
    limit: usize,
    ner: Option<&NerEngine>,
) -> miette::Result<()> {
    println!("[Consume] Connecting to Fluvio cluster...");
    let stream = FluvioStream::connect().await?;

    let offset = if from_beginning {
        Offset::beginning()
    } else {
        Offset::end()
    };

    let consumer: streaming::TypedConsumer<models::ExtractedArticle> =
        stream.consumer(TOPIC_ARTICLES, offset).await?;

    println!("[Consume] Listening on topic: {}", TOPIC_ARTICLES);

    if limit > 0 {
        println!("[Consume] Will consume up to {} articles.\n", limit);
        let articles = consumer.consume_batch(limit).await?;

        for mut article in articles {
            // Optionally enrich with NER
            if let Some(engine) = ner {
                if let Ok(entities) = engine.detect(&article.body_preview) {
                    article.named_entities = entities;
                }
            }
            output::print_summary(&article);
        }

        output::print_run_summary(limit, TOPIC_ARTICLES);
    } else {
        println!("[Consume] Press Ctrl+C to stop.\n");

        consumer
            .process(|article: models::ExtractedArticle| {
                // Can't easily use ner here due to async closure constraints
                // NER enrichment works best with --limit mode
                async move {
                    output::print_summary(&article);
                    Ok(())
                }
            })
            .await?;
    }

    Ok(())
}
*/

// ── Mode: Watch ───────────────────────────────────────────────────────────────
/*
async fn run_watch(cli: &Cli) -> miette::Result<()> {
    let raw_dirs = cli.resolve_watch_dirs();
    let links: Vec<&str> = raw_dirs.iter().map(String::as_str).collect();
    let result = links.validate_with(criteria);

    for err in &result.invalid {
        eprintln!("  ✗ {}: {}", err.item, err.message);
    }
    if result.valid.is_empty() {
        eprintln!("  No valid directories to watch. Exiting.");
        return Ok(());
    }

    let watch_paths: Vec<PathBuf> = result.valid.iter().map(PathBuf::from).collect();
    println!("[Watch] Connecting to Fluvio cluster...");
    let coordinator = Arc::new(PipelineCoordinator::new().await?);
    let _producer = coordinator.raw_pages_producer();

    println!("[Watch] Watching: {:?}", watch_paths);
    println!("[Watch] Press Ctrl+C to stop.\n");

    let wx = Watchexec::new(move |mut action| {
        if action
            .signals()
            .any(|s| matches!(s, Signal::Interrupt | Signal::Terminate))
        {
            println!("\n[Watch] Stopping...");
            action.quit();
            return action;
        }

        let html_paths: Vec<PathBuf> = action
            .events
            .iter()
            .flat_map(|event| &event.tags)
            .filter_map(|tag| match tag {
                Tag::Path { path, .. }
                    if path.is_file()
                        && path
                            .extension()
                            .map(|e| e.eq_ignore_ascii_case("html"))
                            .unwrap_or(false) =>
                {
                    Some(path.clone())
                }
                _ => None,
            })
            .collect::<HashSet<_>>()
            .into_iter()
            .collect();

        if html_paths.is_empty() {
            return action;
        }

        for path in &html_paths {
            println!("[Watch] Detected: {}", path.display());

            let html = match std::fs::read_to_string(path) {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("  ✗ Read error: {e}");
                    continue;
                }
            };

            let url = format!("file://{}", path.display());
            let _page = RawPage {
                url: url.clone(),
                html,
            };

            // We can't easily await here in the sync closure
            // For production, consider using a channel to send to an async task
            println!("  → Would produce to {} (async required)", TOPIC_RAW_PAGES);
        }

        action
    })
    .into_diagnostic()?;

    wx.config.pathset(watch_paths);

    Ok(())
}
*/
// ── Mode: File ────────────────────────────────────────────────────────────────
async fn run_file(paths: &[PathBuf]) -> miette::Result<()> {
    println!("[File] Connecting to Fluvio cluster...");
    let coordinator = PipelineCoordinator::new().await?;
    let producer = coordinator.raw_pages_producer();

    let mut total_produced = 0;

    for path in paths {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("[File] {}", path.display());

        let html = match std::fs::read_to_string(path) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("  ✗ {e}");
                continue;
            }
        };

        let url = format!("file://{}", path.display());
        let item = SourceItem::news(url.clone(), html);

        let locked = producer.lock().await;
        locked.send(&url, &item).await?;
        println!("  → Produced to {}", TOPIC_RAW_PAGES);
        total_produced += 1;
    }

    let locked = producer.lock().await;
    locked.flush().await?;

    println!(
        "\n[File] Produced {} files to {}",
        total_produced, TOPIC_RAW_PAGES
    );
    Ok(())
}
