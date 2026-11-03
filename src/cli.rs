use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::config::load_config;

const DEFAULT_TOKENIZER: &str = "models/gliner_small-v2.1/tokenizer.json";
const DEFAULT_MODEL: &str = "models/gliner_small-v2.1/onnx/model.onnx";
const DEFAULT_REGISTRY: &str = "filimon.toml";

/// Where `fili consume` writes its JSON output in batch mode.
pub const DEFAULT_OUTPUT: &str = "output/wise_output.json";

/// Which sources `fili consume` reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum SourceFilter {
    /// News (through the chain modules) and social items, merged.
    Both,
    /// Only news from `filimon-raw-pages`, through the chain modules.
    News,
    /// Only social items from `filimon-social-items`, with no chain modules.
    Social,
}

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

    /// Consume news (through the news chain modules) and social items from Fluvio into one JSON output, with optional NER enrichment.
    Consume {
        /// Consume from the beginning of the topic.
        #[arg(long)]
        from_beginning: bool,

        /// Maximum records across news and social (0 = unlimited streaming).
        #[arg(long, default_value_t = 0)]
        limit: usize,

        /// NER for news and social records, batch mode only.
        #[arg(long)]
        enrich_ner: bool,

        /// Which sources to read: both, news or social.
        #[arg(long, value_enum, default_value_t = SourceFilter::Both)]
        source: SourceFilter,

        /// JSON output file (batch mode).
        #[arg(long, default_value = DEFAULT_OUTPUT)]
        output: PathBuf,
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
        // The default is asserted on argument metadata, so an operator's FILI_REGISTRY cannot skip the check.
        let command = Cli::command();
        let registry = command
            .get_arguments()
            .find(|arg| arg.get_id() == "registry")
            .expect("registry argument exists");
        let defaults = registry.get_default_values();
        assert_eq!(defaults.len(), 1);
        assert_eq!(defaults[0], DEFAULT_REGISTRY);
        assert_eq!(defaults[0], "filimon.toml");
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

    /// Default values of a `fili consume` argument, from argument metadata.
    fn consume_arg_defaults(id: &str) -> Vec<String> {
        let command = Cli::command();
        let consume = command
            .find_subcommand("consume")
            .expect("consume subcommand exists");
        let arg = consume
            .get_arguments()
            .find(|arg| arg.get_id() == id)
            .unwrap_or_else(|| panic!("{id} argument exists"));
        arg.get_default_values()
            .iter()
            .map(|v| v.to_string_lossy().into_owned())
            .collect()
    }

    fn parse_consume(args: &[&str]) -> (SourceFilter, PathBuf, usize) {
        let cli = Cli::try_parse_from(args).expect("parses");
        let Command::Consume {
            source,
            output,
            limit,
            ..
        } = cli.command
        else {
            panic!("expected the consume subcommand");
        };
        (source, output, limit)
    }

    #[test]
    fn consume_flags_default() {
        assert_eq!(consume_arg_defaults("source"), ["both"]);
        assert_eq!(consume_arg_defaults("output"), [DEFAULT_OUTPUT]);
        assert_eq!(DEFAULT_OUTPUT, "output/wise_output.json");

        let (source, output, limit) = parse_consume(&["fili", "consume", "--limit", "3"]);
        assert_eq!(source, SourceFilter::Both);
        assert_eq!(output, PathBuf::from("output/wise_output.json"));
        assert_eq!(limit, 3);
    }

    #[test]
    fn consume_source_and_output_parse() {
        let (source, output, limit) = parse_consume(&[
            "fili", "consume", "--source", "social", "--output", "/tmp/x.json", "--limit", "2",
        ]);
        assert_eq!(source, SourceFilter::Social);
        assert_eq!(output, PathBuf::from("/tmp/x.json"));
        assert_eq!(limit, 2);

        let (source, _, _) = parse_consume(&["fili", "consume", "--source", "news"]);
        assert_eq!(source, SourceFilter::News);
    }

    #[test]
    fn consume_rejects_unknown_source() {
        assert!(Cli::try_parse_from(["fili", "consume", "--source", "video"]).is_err());
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
