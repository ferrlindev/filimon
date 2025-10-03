mod args;
mod check;

use args::Args;
use check::{ItemError, validate_with};
use clap::Parser;
use std::path::{Path, PathBuf};

use miette::{IntoDiagnostic, Result};
use watchexec::Watchexec;
use watchexec_signals::Signal;

fn criteria(item: &str) -> Result<String, ItemError> {
    if is_valid_directory(item) {
        Ok(item.to_string())
    } else {
        Err(ItemError {
            item: item.to_string(),
            message: "No such directory found.".to_string(),
        })
    }
}

fn is_valid_directory(path: &str) -> bool {
    return Path::new(path).is_dir();
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = Args::parse();
    let links = args.produce_links();
    let result = validate_with(links, criteria);

    // for debugging purposes only.
    println!("Valid items {:?}", result.valid);
    println!("Invalid items:");
    for error in result.invalid {
        println!(" - {}", error);
    }

    let paths: Vec<PathBuf> = result
        .valid
        .clone()
        .into_iter()
        .map(PathBuf::from)
        .collect();

    let wx = Watchexec::new(move |mut action| {
        for event in action.events.iter() {
            println!("Event: {:?}", event);
        }

        if action.signals().any(|sig| sig == Signal::Interrupt) {
            action.quit();
        }

        action
    })?;

    wx.config.pathset(paths);
    println!("Watching for changes in {:?}", result.valid);
    let _ = wx.main().await.into_diagnostic()?;

    Ok(())
}
