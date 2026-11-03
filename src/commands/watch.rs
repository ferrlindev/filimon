// ─────────────────────────────────────────────────────────────────────────────
// commands/watch.rs - Watch mode
//
// Monitors directories for HTML file changes and produces them to Fluvio.
// ─────────────────────────────────────────────────────────────────────────────

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use miette::IntoDiagnostic;
use watchexec::Watchexec;
use watchexec_events::Tag;
use watchexec_signals::Signal;

use crate::check::{ItemError, ValidateWithExt};
use crate::cli::Cli;
use crate::models::RawPage;
use crate::streaming::{PipelineCoordinator, TOPIC_RAW_PAGES};

/// Validate that a path is an existing directory.
fn is_valid_directory(item: &str) -> Result<String, ItemError> {
    if Path::new(item).is_dir() {
        Ok(item.to_string())
    } else {
        Err(ItemError {
            item: item.to_string(),
            message: "No such directory found.".to_string(),
        })
    }
}

/// Watch directories for HTML files and produce them to Fluvio.
pub async fn run(cli: &Cli) -> miette::Result<()> {
    let raw_dirs = cli.resolve_watch_dirs();
    let links: Vec<&str> = raw_dirs.iter().map(String::as_str).collect();
    let result = links.validate_with(is_valid_directory);

    // Report invalid directories
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
        // Handle shutdown signals
        if action
            .signals()
            .any(|s| matches!(s, Signal::Interrupt | Signal::Terminate))
        {
            println!("\n[Watch] Stopping...");
            action.quit();
            return action;
        }

        // Collect unique HTML file paths from events
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

        // Process each detected HTML file
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

            // TODO: We can't easily await here in the sync closure
            // For production, consider using a channel to send to an async task
            println!("  → Would produce to {} (async required)", TOPIC_RAW_PAGES);
        }

        action
    })
    .into_diagnostic()?;

    wx.config.pathset(watch_paths);

    Ok(())
}
