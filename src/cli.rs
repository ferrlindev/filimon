use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::config::load_config;

const DEFAULT_TOKENIZER: &str = "models/gliner_small-v2.1/tokenizer.json";
const DEFAULT_MODEL: &str = "models/gliner_small-v2.1/onnx/model.onnx";

#[derive(Parser, Debug)]
#[command(
    name = "fili",
    version = env!("CARGO_PKG_VERSION"),
    about = "filimon -- Fluvio-powered WISE semantic news extractor",
)]
pub struct Cli {
    /// Path to GLiNER tokenizer.json (for NER on consume)
    #[arg(long, env = "TOKENIZER_PATH", default_value = DEFAULT_TOKENIZER)]
    pub tokenizer: PathBuf,

    /// Path to GLiNER onnx/model.onnx (for NER on consume).
    #[arg(long, env = "MODEL_PATH", default_value = DEFAULT_MODEL)]
    pub model: PathBuf,

    /// Relevance threshold for filtering [0.0 - 1.0]
    #[arg(long, default_value_t = 0.05)]
    pub threshold: f64,

    /// Path to JSON config file for watch mode.
    #[arg(long, default_value = "config.json")]
    pub config: String,

    ///  Comma-separated directories to watch (override config files).
    #[arg(long, value_delimiter = ',')]
    pub ls: Option<Vec<String>>,

    #[command(subcommand)]
    pub command: Command,
}

impl Cli {
    pub fn resolve_watch_dirs(&self) -> Vec<String> {
        if let Some(dirs) = &self.ls {
            return dirs.clone();
        }
        load_config(&self.config).unwrap_or_else(|e| {
            eprintln!("  ⚠ Config fead failed ({}); nothing to watch.", e);
            vec![]
        })
    }
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Produce crawled pages to Fluvio for pipeline processing.
    Produce {
        #[arg(required = true, value_name = "URL")]
        urls: Vec<String>,

        /// Maximum pages to fetch per seed after priority ranking.
        #[arg(long, default_value_t = 5)]
        depth: usize,
    },

    /// Consume processed articles from Fluvio (with optional NER enrichment).
    Consume {
        /// Consume from the beginning of the topic.
        #[arg(long)]
        from_beginning: bool,

        /// Maximum number of articles to consume ( 0 = unlimited).
        #[arg(long, default_value_t = 0)]
        limit: usize,

        /// Apply NER enrichment to consumed articles
        #[arg(long)]
        enrich_ner: bool,
    },

    /// Watch directories and produce HTML files to Fluvio.
    Watch,

    /// Produce local HTML files to Fluvio.
    File {
        #[arg(required = true, value_name = "FILE")]
        paths: Vec<PathBuf>,
    },
}
