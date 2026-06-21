// ─────────────────────────────────────────────────────────────────────────────
// main.rs  –  filimon
//
// Three modes, all sharing one loaded NerEngine:
//
//   (default)         Watch dirs → detect HTML changes → WISE pipeline
//   fili crawl <URL>  Crawl live URLs → WISE pipeline
//   fili file  <FILE> Process local HTML files → WISE pipeline
//
// Original filimon flow (Args → produce_links → validate_with → watchexec)
// is preserved exactly; WISE processing is added inside the event handler.
// ─────────────────────────────────────────────────────────────────────────────

mod check; // existing: ValidateWithExt, ItemError, ProcessingResult
mod crawler; // WISE Stage 1
mod extractor; // WISE Stage 3
mod models; // shared structs
mod output; // WISE Stage 4
mod preprocessor; // WISE Stage 2
mod scorer; // relevance scoring + NerEngine

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use clap::{Parser, Subcommand};
use miette::IntoDiagnostic;
use serde::Deserialize;
use watchexec::Watchexec;
use watchexec_events::Tag;
use watchexec_signals::Signal;

use check::ValidateWithExt;
use models::CrawlTarget;
use scorer::NerEngine;

// ── Model path defaults (overridden by env vars or CLI flags) ─────────────────

const DEFAULT_TOKENIZER: &str = "models/gliner_small-v2.1/tokenizer.json";
const DEFAULT_MODEL: &str = "models/gliner_small-v2.1/onnx/model.onnx";
const LINKS_PER_SEED: usize = 30;

// ── Config file (existing filimon schema) ─────────────────────────────────────

#[derive(Deserialize)]
struct Config {
    ls: Vec<String>,
}

fn load_config(path: &str) -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let raw = std::fs::read_to_string(path)?;
    let config: Config = serde_json::from_str(&raw)?;
    Ok(config.ls)
}

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(
    name    = "fili",
    version = env!("CARGO_PKG_VERSION"),
    about   = "filimon — directory watcher + WISE semantic news extractor",
)]
struct Cli {
    // ── Original filimon args (backward-compatible) ───────────────────────────
    /// Comma-separated list of directories to watch.
    /// Falls back to `ls` in the config file when not provided.
    #[arg(long, value_delimiter = ',')]
    ls: Option<Vec<String>>,

    /// Path to JSON config file (default: config.json).
    #[arg(long, default_value = "config.json")]
    config: String,

    // ── WISE pipeline args (all readable from env vars set by docker-compose) ─
    /// Path to GLiNER tokenizer.json.
    #[arg(long, env = "TOKENIZER_PATH", default_value = DEFAULT_TOKENIZER)]
    tokenizer: PathBuf,

    /// Path to GLiNER onnx/model.onnx.
    #[arg(long, env = "MODEL_PATH", default_value = DEFAULT_MODEL)]
    model: PathBuf,

    /// Where to write extracted JSON results.
    #[arg(long, env = "OUTPUT_PATH", default_value = "wise_output.json")]
    output: PathBuf,

    /// Discard pages with relevance score below this value [0.0–1.0].
    #[arg(long, default_value_t = 0.05)]
    threshold: f64,

    /// WISE mode subcommand.
    /// Omit to run in default Watch mode (original filimon behaviour).
    #[command(subcommand)]
    command: Option<Command>,
}

impl Cli {
    /// Reproduce the original `produce_links` logic:
    /// CLI arg takes priority over config file.
    fn resolve_watch_dirs(&self) -> Vec<String> {
        if let Some(dirs) = &self.ls {
            return dirs.clone();
        }
        load_config(&self.config).unwrap_or_else(|e| {
            eprintln!("  ⚠ Config read failed ({}); nothing to watch.", e);
            vec![]
        })
    }
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Crawl one or more live seed URLs through the WISE pipeline.
    Crawl {
        #[arg(required = true, value_name = "URL")]
        urls: Vec<String>,

        /// Maximum pages to fetch per seed after priority ranking.
        #[arg(long, default_value_t = 5)]
        depth: usize,
    },

    /// Process local HTML files offline (no network).
    File {
        #[arg(required = true, value_name = "FILE")]
        paths: Vec<PathBuf>,
    },
}

// ── Validation (original filimon logic, unchanged) ────────────────────────────

/// Accepts a directory path if it exists on disk.
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

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> miette::Result<()> {
    let cli = Cli::parse();

    println!("╔═══════════════════════════════════════════════╗");
    println!("║  filimon  ·  watcher + WISE semantic extractor ║");
    println!("╚═══════════════════════════════════════════════╝");
    println!("  tokenizer : {}", cli.tokenizer.display());
    println!("  model     : {}", cli.model.display());
    println!("  output    : {}", cli.output.display());
    println!("  threshold : {:.2}\n", cli.threshold);

    // Load GLiNER once — expensive (~1–2 s) but amortised over all pages.
    let ner_engine: Arc<Option<NerEngine>> =
        Arc::new(match NerEngine::new(&cli.tokenizer, &cli.model) {
            Ok(e) => {
                println!("  NER : ✓ GLiNER loaded\n");
                Some(e)
            }
            Err(e) => {
                eprintln!("  NER : ✗ unavailable ({e})\n");
                None
            }
        });

    match &cli.command {
        // watch directories + process HTML through WISE
        None => {
            run_watch(cli, ner_engine).await?;
        }
        Some(Command::Crawl { urls, depth }) => {
            let articles = run_crawl(urls, *depth, cli.threshold, ner_engine.as_ref().as_ref());
            persist(&articles, &cli.output);
        }
        Some(Command::File { paths }) => {
            let articles = run_file(paths, cli.threshold, ner_engine.as_ref().as_ref());
            persist(&articles, &cli.output);
        }
    }

    Ok(())
}

// ── Mode: Watch ───────────────────────────────────────────────────────────────
//
// Extends the original filimon watchexec handler with WISE processing:
// when an .html file is created or modified inside a watched directory,
// run it through all four WISE stages and append the result to the output file.

async fn run_watch(cli: Cli, ner: Arc<Option<NerEngine>>) -> miette::Result<()> {
    // ── Step 1: resolve + validate directories (original filimon flow) ────────
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
    println!("  Watching : {:?}", watch_paths);
    println!("  Press Ctrl+C to stop.\n");

    // ── Step 2: shared state for the event handler ────────────────────────────
    // Arc<Mutex<_>> lets the closure (which may be called concurrently by
    // watchexec) safely accumulate articles and flush to disk.
    let articles: Arc<Mutex<Vec<models::ExtractedArticle>>> = Arc::new(Mutex::new(Vec::new()));

    let threshold = cli.threshold;
    let output = cli.output.clone();
    let ner_c = Arc::clone(&ner);
    let articles_c = Arc::clone(&articles);
    let output_c = output.clone();

    // ── Step 3: build watchexec with the WISE-aware handler ───────────────────
    let wx = Watchexec::new(move |mut action| {
        // Quit gracefully on Ctrl+C / SIGTERM, flushing any buffered results.
        if action
            .signals()
            .any(|s| matches!(s, Signal::Interrupt | Signal::Terminate))
        {
            println!("\n[fili] Stopping — flushing results...");
            let locked = articles_c.lock().unwrap();
            if let Err(e) = output::save_json(&locked, &output_c.to_string_lossy()) {
                eprintln!("  ✗ Flush error: {e}");
            }
            action.quit();
            return action;
        }

        // Collect unique HTML file paths from this event batch.
        // watchexec may batch several filesystem events together.
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
            .collect::<HashSet<_>>() // deduplicate: one write event can fire twice
            .into_iter()
            .collect();

        if html_paths.is_empty() {
            return action; // non-HTML event (dir create, metadata, etc.) — ignore
        }

        // ── WISE pipeline for each detected HTML file ─────────────────────────
        let ner_ref = ner_c.as_ref().as_ref();

        for path in &html_paths {
            println!("[Stage 1] Detected: {}", path.display());

            let html = match std::fs::read_to_string(path) {
                Ok(h) => h,
                Err(e) => {
                    eprintln!("  ✗ Read error: {e}");
                    continue;
                }
            };

            // Stage 2 – preprocess
            let url = format!("file://{}", path.display());
            let page = models::RawPage {
                url: url.clone(),
                html,
            };
            let tokens = preprocessor::preprocess(&url, &page.html);
            println!(
                "  Stage 2 → {} tokens ({} unique)",
                tokens.tokens.len(),
                tokens.frequencies.len()
            );

            // Stage 3 – extract + score
            let article = extractor::extract_article(&page, &tokens, ner_ref);
            println!(
                "  Stage 3 → relevance {:.3}  category: {}",
                article.relevance_score, article.inferred_category
            );

            if article.relevance_score < threshold {
                println!("  ✗ Below threshold ({threshold:.2}) — discarded");
                continue;
            }

            // Stage 4 – persist (append + overwrite output file)
            output::print_summary(&article);
            let mut locked = articles_c.lock().unwrap();
            locked.push(article);
            if let Err(e) = output::save_json(&locked, &output_c.to_string_lossy()) {
                eprintln!("  ✗ Save error: {e}");
            } else {
                println!("  Stage 4 → written to {}", output_c.display());
            }
        }

        action
    })
    .into_diagnostic()?;
    wx.config.pathset(watch_paths);

    Ok(())
}

// ── Mode: Crawl ───────────────────────────────────────────────────────────────

fn run_crawl(
    urls: &[String],
    depth: usize,
    threshold: f64,
    ner: Option<&NerEngine>,
) -> Vec<models::ExtractedArticle> {
    let mut all = Vec::new();

    for seed_url in urls {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("[Stage 1] Seed: {seed_url}");

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

            let tokens = preprocessor::preprocess(&page.url, &page.html);
            let article = extractor::extract_article(&page, &tokens, ner);

            println!(
                "    relevance={:.3}  category={}",
                article.relevance_score, article.inferred_category
            );

            if article.relevance_score < threshold {
                println!("    ✗ Below threshold — discarded");
                continue;
            }

            output::print_summary(&article);
            all.push(article);
        }
    }

    all
}

// ── Mode: File ────────────────────────────────────────────────────────────────

fn run_file(
    paths: &[PathBuf],
    threshold: f64,
    ner: Option<&NerEngine>,
) -> Vec<models::ExtractedArticle> {
    let mut all = Vec::new();

    for path in paths {
        println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
        println!("[Stage 1] File: {}", path.display());

        let html = match std::fs::read_to_string(path) {
            Ok(h) => h,
            Err(e) => {
                eprintln!("  ✗ {e}");
                continue;
            }
        };

        let url = format!("file://{}", path.display());
        let page = models::RawPage {
            url: url.clone(),
            html,
        };
        let tokens = preprocessor::preprocess(&url, &page.html);
        let art = extractor::extract_article(&page, &tokens, ner);

        println!(
            "  relevance={:.3}  category={}",
            art.relevance_score, art.inferred_category
        );

        if art.relevance_score < threshold {
            println!("  ✗ Below threshold — discarded");
            continue;
        }

        output::print_summary(&art);
        all.push(art);
    }

    all
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn persist(articles: &[models::ExtractedArticle], path: &PathBuf) {
    println!(
        "\n[Stage 4] Writing {} article(s) → {}",
        articles.len(),
        path.display()
    );
    match output::save_json(articles, &path.to_string_lossy()) {
        Ok(()) => println!("  ✓ Saved."),
        Err(e) => eprintln!("  ✗ {e}"),
    }
    output::print_run_summary(articles.len(), &path.to_string_lossy());
}
