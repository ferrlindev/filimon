use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::config::load_config;

const DEFAULT_TOKENIZER: &str = "models/gliner_small-v2.1/tokenizer.json";
const DEFAULT_MODEL: &str = "models/gliner_small-v2.1/onnx/model.onnx";
const DEFAULT_REGISTRY: &str = "filimon.toml";

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

    /// Path to the plugin registry (filimon.toml); component and config paths inside it resolve relative to its directory.
    #[arg(long, global = true, env = "FILI_REGISTRY", default_value = DEFAULT_REGISTRY)]
    pub registry: PathBuf,

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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use std::ffi::OsStr;
    use tempfile::TempDir;

    #[test]
    fn registry_flag_defaults_to_filimon_toml() {
        // An operator's FILI_REGISTRY would legitimately override the default;
        // read it only, never write it, so the process env is left untouched.
        if std::env::var_os("FILI_REGISTRY").is_some() {
            return;
        }
        let cli = Cli::try_parse_from(["fili", "watch"]).expect("parses");
        assert_eq!(cli.registry, PathBuf::from("filimon.toml"));
    }

    #[test]
    fn registry_flag_is_global_with_env() {
        let command = Cli::command();
        let registry = command
            .get_arguments()
            .find(|arg| arg.get_id() == "registry")
            .expect("registry argument exists");
        assert_eq!(registry.get_env(), Some(OsStr::new("FILI_REGISTRY")));
        assert!(registry.is_global_set());
    }

    #[test]
    fn registry_flag_after_subcommand() {
        let cli = Cli::try_parse_from(["fili", "watch", "--registry", "conf/filimon.toml"])
            .expect("parses");
        assert_eq!(cli.registry, PathBuf::from("conf/filimon.toml"));
    }

    #[test]
    fn registry_flag_feeds_loader() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::write(
            dir.path().join("filimon.toml"),
            "[sources.alpha]\n\
             component = \"alpha.wasm\"\n\
             config = \"alpha.toml\"\n\
             fetcher = \"beta\"\n\
             \n\
             [fetchers.beta]\n\
             component = \"beta.wasm\"\n",
        )
        .expect("write registry");
        std::fs::write(
            dir.path().join("alpha.toml"),
            "[host]\npoll_interval_secs = 60\n",
        )
        .expect("write plugin");
        let registry_path = dir.path().join("filimon.toml");

        let cli = Cli::try_parse_from([
            OsStr::new("fili"),
            OsStr::new("--registry"),
            registry_path.as_os_str(),
            OsStr::new("watch"),
        ])
        .expect("parses");
        assert_eq!(cli.registry, registry_path);

        let loaded = crate::plugins::config::load_registry_with_env(&cli.registry, &|_| None)
            .expect("registry loads");
        let names: Vec<&str> = loaded.sources.keys().map(String::as_str).collect();
        assert_eq!(names, vec!["alpha"]);
    }
}
