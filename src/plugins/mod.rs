// ─────────────────────────────────────────────────────────────────────────────
// plugins/mod.rs - WebAssembly plugin configuration
//
// Loads the `filimon.toml` plugin registry, each plugin's own TOML file and the
// `.env` secrets next to the registry into a typed `LoadedRegistry`. The plugin
// host and the CLI consume it; secrets are filtered per plugin here, before
// they can ever cross into a guest's `init`.
// ─────────────────────────────────────────────────────────────────────────────

pub mod config;
pub mod error;
pub mod host;
pub mod secrets;

// ── Sample Registry Tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use tempfile::TempDir;

    use super::config::{LoadedRegistry, PluginConfig, load_registry_with_env};
    use super::secrets::read_dotenv;

    const SAMPLE_TOMLS: [&str; 4] = ["filimon.toml", "stub.toml", "reddit.toml", "youtube.toml"];

    fn sample_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/plugins")
    }

    /// Copies the committed sample TOMLs into a fresh TempDir and writes a
    /// `.env` there. The repo's `examples/plugins/` never receives a `.env`.
    fn sample_copy(dotenv: &str) -> TempDir {
        let dir = TempDir::new().expect("tempdir");
        for name in SAMPLE_TOMLS {
            std::fs::copy(sample_dir().join(name), dir.path().join(name))
                .unwrap_or_else(|e| panic!("copy sample {name}: {e}"));
        }
        std::fs::write(dir.path().join(".env"), dotenv).expect("write .env");
        dir
    }

    fn plugin<'a>(registry: &'a LoadedRegistry, name: &str) -> &'a PluginConfig {
        registry.sources[name]
            .plugin
            .as_ref()
            .unwrap_or_else(|| panic!("{name} is enabled and loaded"))
    }

    fn secret_keys(plugin: &PluginConfig) -> Vec<&str> {
        plugin.secrets.keys().map(String::as_str).collect()
    }

    fn settings(plugin: &PluginConfig) -> toml::Table {
        toml::from_str(&plugin.settings_toml).expect("settings_toml is TOML")
    }

    #[test]
    fn loads_sample_registry() {
        let dir = sample_copy(
            "REDDIT_CLIENT_ID=rid\n\
             REDDIT_CLIENT_SECRET=rsecret\n\
             YOUTUBE_API_KEY=ykey\n\
             UNRELATED_TOKEN=nope\n",
        );

        let registry = load_registry_with_env(&dir.path().join("filimon.toml"), &|_| None)
            .expect("sample registry loads");

        let sources: Vec<&str> = registry.sources.keys().map(String::as_str).collect();
        assert_eq!(sources, vec!["reddit", "stub", "youtube"]);
        let fetchers: Vec<&str> = registry.fetchers.keys().map(String::as_str).collect();
        assert_eq!(fetchers, vec!["http", "stub-fetcher"]);

        assert_eq!(registry.sources["reddit"].fetcher.as_deref(), Some("http"));
        assert_eq!(
            registry.sources["stub"].fetcher.as_deref(),
            Some("stub-fetcher")
        );

        // reddit: host values, two secrets, subreddits.
        let reddit = plugin(&registry, "reddit");
        assert_eq!(reddit.host.poll_interval, Duration::from_secs(300));
        assert_eq!(reddit.host.timeout, Duration::from_secs(30));
        assert_eq!(
            reddit.host.allowlist,
            vec!["oauth.reddit.com".to_string(), "www.reddit.com".to_string()]
        );
        assert_eq!(reddit.host.backoff.initial, Duration::from_secs(60));
        assert_eq!(reddit.host.backoff.max, Duration::from_secs(3600));
        assert!((reddit.host.backoff.multiplier - 2.0).abs() < f64::EPSILON);
        assert_eq!(secret_keys(reddit), vec!["client_id", "client_secret"]);
        assert_eq!(reddit.secrets["client_id"].expose(), "rid");
        assert_eq!(reddit.secrets["client_secret"].expose(), "rsecret");
        let reddit_settings = settings(reddit);
        let subreddits: Vec<&str> = reddit_settings["subreddits"]
            .as_array()
            .expect("subreddits is an array")
            .iter()
            .map(|v| v.as_str().expect("subreddit is a string"))
            .collect();
        assert_eq!(subreddits, vec!["rust", "programming"]);

        // youtube: interval, one secret, channels.
        let youtube = plugin(&registry, "youtube");
        assert_eq!(youtube.host.poll_interval, Duration::from_secs(900));
        assert_eq!(secret_keys(youtube), vec!["api_key"]);
        assert_eq!(youtube.secrets["api_key"].expose(), "ykey");
        assert!(settings(youtube).contains_key("channels"));

        // stub: no secrets, greeting only.
        let stub = plugin(&registry, "stub");
        assert!(stub.secrets.is_empty());
        assert!(settings(stub).contains_key("greeting"));

        // No plugin receives the unrelated .env key, by alias or by value.
        for (name, source) in &registry.sources {
            let loaded = source.plugin.as_ref().expect("every sample source loads");
            assert!(
                !loaded.secrets.contains_key("UNRELATED_TOKEN"),
                "{name} received UNRELATED_TOKEN"
            );
            assert!(
                loaded.secrets.values().all(|s| s.expose() != "nope"),
                "{name} received the UNRELATED_TOKEN value"
            );
        }

        // Every component path resolves against the registry's directory.
        let components = registry
            .sources
            .values()
            .map(|s| &s.component)
            .chain(registry.fetchers.values().map(|f| &f.component));
        for component in components {
            assert!(
                component.starts_with(dir.path()),
                "{} is not registry-relative",
                component.display()
            );
        }
    }

    #[test]
    fn sample_env_example_lists_every_declared_secret() {
        let mut declared = BTreeSet::new();
        for name in SAMPLE_TOMLS {
            let text = std::fs::read_to_string(sample_dir().join(name))
                .unwrap_or_else(|e| panic!("read sample {name}: {e}"));
            let table: toml::Table =
                toml::from_str(&text).unwrap_or_else(|e| panic!("parse sample {name}: {e}"));
            let secrets = table
                .get("host")
                .and_then(|host| host.get("secrets"))
                .and_then(toml::Value::as_table);
            for env_var in secrets.into_iter().flat_map(toml::Table::values) {
                let env_var = env_var.as_str().expect("secret env var name is a string");
                declared.insert(env_var.to_string());
            }
        }

        let listed: BTreeSet<String> = read_dotenv(&sample_dir().join(".env.example"))
            .expect(".env.example parses")
            .into_keys()
            .collect();

        assert_eq!(listed, declared);
        assert_eq!(
            listed,
            BTreeSet::from([
                "REDDIT_CLIENT_ID".to_string(),
                "REDDIT_CLIENT_SECRET".to_string(),
                "YOUTUBE_API_KEY".to_string(),
            ])
        );
    }
}
