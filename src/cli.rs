use clap::{Parser, Subcommand};
use std::path::PathBuf;

use crate::config::load_config;

const DEFAULT_TOKENIZER: &str = "models/gliner_small-v2.1/tokenizer.json";
const DEFAULT_MODEL: &str = "models/gliner_small-v2.1/onnx/model.onnx";
const DEFAULT_REGISTRY: &str = "filimon.toml";

/// Where `fili consume` writes its JSON output in batch mode.
pub const DEFAULT_OUTPUT: &str = "output/wise_output.json";

/// A path value that is not empty and not only whitespace. An empty
/// `--state-dir` would otherwise put the lock and state files in the working
/// directory (WR-07).
fn non_blank_path(value: &str) -> Result<PathBuf, String> {
    if value.trim().is_empty() {
        Err("must not be empty or blank".to_string())
    } else {
        Ok(PathBuf::from(value))
    }
}

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

    /// Directory for per-plugin cursor files. Defaults to `.fili-state/` next to the registry file.
    #[arg(long, global = true, env = "FILI_STATE_DIR", value_parser = non_blank_path)]
    pub state_dir: Option<PathBuf>,

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

    /// Inspect, validate and try registered plugins.
    Plugins {
        #[command(subcommand)]
        command: PluginsCommand,
    },

    /// Poll every enabled source plugin on its interval and produce items to Fluvio until stopped.
    Run,

    /// Log in to a plugin's platform, show stored logins, or log out.
    Auth {
        #[command(subcommand)]
        command: AuthCommand,
    },
}

/// The `fili auth` subcommands.
#[derive(Subcommand, Debug)]
pub enum AuthCommand {
    /// Log in to a source plugin's platform through OAuth 2.0 PKCE; --no-browser only prints the URL.
    Login {
        /// The registered source to log in to
        #[arg(value_name = "NAME")]
        plugin: String,

        /// Print the URL but do not try to open a browser (for SSH sessions)
        #[arg(long)]
        no_browser: bool,
    },

    /// Show every stored login and its expiry state; --json prints one JSON array.
    Status {
        /// Print the rows as one JSON array
        #[arg(long)]
        json: bool,
    },

    /// Delete a plugin's stored login and revoke it at the provider when the plugin declares a revocation_url.
    Logout {
        /// The registered source to log out of
        #[arg(value_name = "NAME")]
        plugin: String,
    },
}

/// The `fili plugins` subcommands.
#[derive(Subcommand, Debug)]
pub enum PluginsCommand {
    /// List the registered plugins with their last poll.
    List {
        /// Print the rows as one JSON array
        #[arg(long)]
        json: bool,
    },

    /// Check a plugin's component against its WIT world and validate its config and secrets, without polling
    Check {
        /// The registered plugin to check
        #[arg(value_name = "NAME")]
        name: String,
    },

    /// Poll one source once and print the items; nothing is produced unless --produce is given
    RunOnce {
        /// The registered source to poll
        #[arg(value_name = "NAME")]
        name: String,

        /// Send the items to Fluvio and save the cursor (without it the poll is a read-only look)
        #[arg(long)]
        produce: bool,

        /// Print one JSON item per line on stdout; notes go to stderr
        #[arg(long)]
        json: bool,
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

    #[test]
    fn state_dir_flag_is_global_with_env() {
        let command = Cli::command();
        let state_dir = command
            .get_arguments()
            .find(|arg| arg.get_id() == "state_dir")
            .expect("state_dir argument exists");
        assert_eq!(state_dir.get_env(), Some(OsStr::new("FILI_STATE_DIR")));
        assert!(state_dir.is_global_set());
    }

    #[test]
    fn state_dir_has_no_default() {
        // Asserted on argument metadata, so an operator's FILI_STATE_DIR cannot skip the check.
        let command = Cli::command();
        let state_dir = command
            .get_arguments()
            .find(|arg| arg.get_id() == "state_dir")
            .expect("state_dir argument exists");
        assert!(state_dir.get_default_values().is_empty());
    }

    #[test]
    fn state_dir_flag_parses_before_and_after_subcommand() {
        let before = Cli::try_parse_from(["fili", "--state-dir", "/tmp/s", "watch"])
            .expect("parses before the subcommand");
        assert_eq!(before.state_dir, Some(PathBuf::from("/tmp/s")));

        let after = Cli::try_parse_from(["fili", "watch", "--state-dir", "/tmp/s"])
            .expect("parses after the subcommand");
        assert_eq!(after.state_dir, Some(PathBuf::from("/tmp/s")));
    }

    #[test]
    fn state_dir_empty_value_is_rejected() {
        let error = Cli::try_parse_from(["fili", "--state-dir", "", "plugins", "list"])
            .expect_err("an empty --state-dir is a usage error");
        assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn state_dir_blank_value_is_rejected() {
        let error = Cli::try_parse_from(["fili", "--state-dir", "   ", "plugins", "list"])
            .expect_err("a blank --state-dir is a usage error");
        assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation);
    }

    #[test]
    fn auth_requires_a_subcommand() {
        assert!(Cli::try_parse_from(["fili", "auth"]).is_err());
    }

    #[test]
    fn auth_login_parses() {
        let cli = Cli::try_parse_from(["fili", "auth", "login", "reddit"]).expect("parses");
        assert!(matches!(
            cli.command,
            Command::Auth {
                command: AuthCommand::Login { ref plugin, no_browser: false }
            } if plugin == "reddit"
        ));
    }

    #[test]
    fn auth_login_no_browser_flag_parses() {
        let cli = Cli::try_parse_from(["fili", "auth", "login", "reddit", "--no-browser"])
            .expect("parses");
        assert!(matches!(
            cli.command,
            Command::Auth {
                command: AuthCommand::Login { ref plugin, no_browser: true }
            } if plugin == "reddit"
        ));
    }

    #[test]
    fn auth_login_requires_a_plugin_name() {
        assert!(Cli::try_parse_from(["fili", "auth", "login"]).is_err());
    }

    #[test]
    fn auth_status_parses() {
        let cli = Cli::try_parse_from(["fili", "auth", "status"]).expect("parses");
        assert!(matches!(
            cli.command,
            Command::Auth {
                command: AuthCommand::Status { json: false }
            }
        ));
    }

    #[test]
    fn auth_logout_parses() {
        let cli = Cli::try_parse_from(["fili", "auth", "logout", "reddit"]).expect("parses");
        assert!(matches!(
            cli.command,
            Command::Auth {
                command: AuthCommand::Logout { ref plugin }
            } if plugin == "reddit"
        ));
    }

    #[test]
    fn auth_logout_requires_a_plugin_name() {
        assert!(Cli::try_parse_from(["fili", "auth", "logout"]).is_err());
    }

    #[test]
    fn auth_status_json_flag_parses() {
        let cli = Cli::try_parse_from(["fili", "auth", "status", "--json"]).expect("parses");
        assert!(matches!(
            cli.command,
            Command::Auth {
                command: AuthCommand::Status { json: true }
            }
        ));
    }

    #[test]
    fn plugins_requires_a_subcommand() {
        assert!(Cli::try_parse_from(["fili", "plugins"]).is_err());
    }

    #[test]
    fn plugins_list_parses() {
        let cli = Cli::try_parse_from(["fili", "plugins", "list"]).expect("parses");
        assert!(matches!(
            cli.command,
            Command::Plugins {
                command: PluginsCommand::List { json: false }
            }
        ));
    }

    #[test]
    fn plugins_list_json_flag_parses() {
        let cli = Cli::try_parse_from(["fili", "plugins", "list", "--json"]).expect("parses");
        assert!(matches!(
            cli.command,
            Command::Plugins {
                command: PluginsCommand::List { json: true }
            }
        ));
        let plain = Cli::try_parse_from(["fili", "plugins", "list"]).expect("parses");
        assert!(matches!(
            plain.command,
            Command::Plugins {
                command: PluginsCommand::List { json: false }
            }
        ));
    }

    #[test]
    fn plugins_check_parses() {
        let cli = Cli::try_parse_from(["fili", "plugins", "check", "stub"]).expect("parses");
        assert!(matches!(
            cli.command,
            Command::Plugins {
                command: PluginsCommand::Check { ref name }
            } if name == "stub"
        ));
        assert!(Cli::try_parse_from(["fili", "plugins", "check"]).is_err());
    }

    #[test]
    fn plugins_run_once_parses() {
        let cli = Cli::try_parse_from(["fili", "plugins", "run-once", "stub"]).expect("parses");
        assert!(matches!(
            cli.command,
            Command::Plugins {
                command: PluginsCommand::RunOnce { ref name, produce: false, json: false }
            } if name == "stub"
        ));

        let both = Cli::try_parse_from(["fili", "plugins", "run-once", "stub", "--produce", "--json"])
            .expect("parses");
        assert!(matches!(
            both.command,
            Command::Plugins {
                command: PluginsCommand::RunOnce { produce: true, json: true, .. }
            }
        ));
    }

    #[test]
    fn plugins_run_once_requires_a_name() {
        assert!(Cli::try_parse_from(["fili", "plugins", "run-once"]).is_err());
        assert!(Cli::try_parse_from(["fili", "plugins", "run-once", "--produce"]).is_err());
    }

    #[test]
    fn run_parses() {
        let cli = Cli::try_parse_from(["fili", "run"]).expect("parses");
        assert!(matches!(cli.command, Command::Run));
        assert!(Cli::try_parse_from(["fili", "run", "extra"]).is_err());
    }

    #[test]
    fn run_takes_the_global_flags() {
        let before = Cli::try_parse_from([
            "fili",
            "--registry",
            "conf/filimon.toml",
            "--state-dir",
            "/tmp/s",
            "run",
        ])
        .expect("parses before the subcommand");
        assert_eq!(before.registry, PathBuf::from("conf/filimon.toml"));
        assert_eq!(before.state_dir, Some(PathBuf::from("/tmp/s")));
        assert!(matches!(before.command, Command::Run));

        let after = Cli::try_parse_from([
            "fili",
            "run",
            "--registry",
            "conf/filimon.toml",
            "--state-dir",
            "/tmp/s",
        ])
        .expect("parses after the subcommand");
        assert_eq!(after.registry, PathBuf::from("conf/filimon.toml"));
        assert_eq!(after.state_dir, Some(PathBuf::from("/tmp/s")));
        assert!(matches!(after.command, Command::Run));
    }

    #[test]
    fn run_rejects_an_empty_state_dir() {
        for value in ["", "   "] {
            let error = Cli::try_parse_from(["fili", "--state-dir", value, "run"])
                .expect_err("an empty or blank --state-dir is a usage error");
            assert_eq!(error.kind(), clap::error::ErrorKind::ValueValidation, "{value:?}");
        }
    }

    #[test]
    fn state_dir_and_registry_flags_work_after_plugins_list() {
        let cli = Cli::try_parse_from([
            "fili",
            "plugins",
            "list",
            "--registry",
            "conf/filimon.toml",
            "--state-dir",
            "/tmp/s",
        ])
        .expect("parses");
        assert_eq!(cli.registry, PathBuf::from("conf/filimon.toml"));
        assert_eq!(cli.state_dir, Some(PathBuf::from("/tmp/s")));
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
