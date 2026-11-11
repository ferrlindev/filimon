// ─────────────────────────────────────────────────────────────────────────────
// plugins/mod.rs - WebAssembly plugin configuration
//
// Loads the `filimon.toml` plugin registry, each plugin's own TOML file and the
// `.env` secrets next to the registry into a typed `LoadedRegistry`. The plugin
// host and the CLI consume it; secrets are filtered per plugin here, before
// they can ever cross into a guest's `init`.
// ─────────────────────────────────────────────────────────────────────────────

pub mod auth;
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

    use super::config::{LoadedRegistry, OAuthGrant, PluginConfig, load_registry_with_env};
    use super::secrets::read_dotenv;

    const SAMPLE_TOMLS: [&str; 6] = [
        "filimon.toml",
        "stub.toml",
        "reddit.toml",
        "reddit-user.toml",
        "youtube.toml",
        "youtube-user.toml",
    ];

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
            "REDDIT_CLIENT_SECRET=rsecret\n\
             YOUTUBE_API_KEY=ykey\n\
             YOUTUBE_OAUTH_CLIENT_SECRET=ysecret\n\
             UNRELATED_TOKEN=nope\n",
        );

        let registry = load_registry_with_env(&dir.path().join("filimon.toml"), &|_| None)
            .expect("sample registry loads");

        let sources: Vec<&str> = registry.sources.keys().map(String::as_str).collect();
        assert_eq!(
            sources,
            vec!["reddit", "reddit-user", "stub", "youtube", "youtube-user"]
        );
        let fetchers: Vec<&str> = registry.fetchers.keys().map(String::as_str).collect();
        assert_eq!(fetchers, vec!["http", "stub-fetcher"]);

        assert_eq!(registry.sources["reddit"].fetcher.as_deref(), Some("http"));
        assert_eq!(
            registry.sources["stub"].fetcher.as_deref(),
            Some("stub-fetcher")
        );

        // reddit (app-only): host values, no secrets, a client-credentials grant.
        let reddit = plugin(&registry, "reddit");
        assert_eq!(reddit.host.poll_interval, Duration::from_secs(300));
        assert_eq!(reddit.host.timeout, Duration::from_secs(30));
        assert_eq!(reddit.host.allowlist, vec!["oauth.reddit.com".to_string()]);
        assert_eq!(reddit.host.backoff.initial, Duration::from_secs(60));
        assert_eq!(reddit.host.backoff.max, Duration::from_secs(3600));
        assert!((reddit.host.backoff.multiplier - 2.0).abs() < f64::EPSILON);
        assert!(secret_keys(reddit).is_empty(), "the plugin gets no secret");
        let oauth = reddit.oauth.as_ref().expect("reddit has [host.oauth]");
        assert_eq!(oauth.grant, OAuthGrant::ClientCredentials);
        assert_eq!(
            oauth.client_secret.as_ref().map(|s| s.expose()),
            Some("rsecret")
        );
        assert!(oauth.user_agent.is_some());
        let reddit_settings = settings(reddit);
        let subreddits: Vec<&str> = reddit_settings["subreddits"]
            .as_array()
            .expect("subreddits is an array")
            .iter()
            .map(|v| v.as_str().expect("subreddit is a string"))
            .collect();
        assert_eq!(subreddits, vec!["rust", "programming"]);

        // reddit-user: a login instance with the permanent-grant parameter.
        let reddit_user = plugin(&registry, "reddit-user");
        let oauth = reddit_user.oauth.as_ref().expect("reddit-user has oauth");
        assert_eq!(oauth.grant, OAuthGrant::AuthorizationCode);
        assert_eq!(
            oauth.scopes,
            vec!["read".to_string(), "history".to_string()]
        );
        assert!(
            oauth
                .authorize_params
                .contains(&("duration".to_string(), "permanent".to_string()))
        );
        assert!(secret_keys(reddit_user).is_empty());
        assert_eq!(settings(reddit_user)["feed"].as_str(), Some("home"));

        // youtube: interval, one secret, channels.
        let youtube = plugin(&registry, "youtube");
        assert_eq!(youtube.host.poll_interval, Duration::from_secs(900));
        assert_eq!(secret_keys(youtube), vec!["api_key"]);
        assert_eq!(youtube.secrets["api_key"].expose(), "ykey");
        assert!(settings(youtube).contains_key("channels"));

        // youtube-user: a login instance, no secret, one read-only scope.
        let youtube_user = plugin(&registry, "youtube-user");
        assert!(secret_keys(youtube_user).is_empty());
        let oauth = youtube_user.oauth.as_ref().expect("youtube-user has oauth");
        assert_eq!(
            oauth.scopes,
            vec!["https://www.googleapis.com/auth/youtube.readonly".to_string()]
        );
        assert_eq!(
            settings(youtube_user)["subscriptions"].as_bool(),
            Some(true)
        );

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
            let oauth_secret = table
                .get("host")
                .and_then(|host| host.get("oauth"))
                .and_then(|oauth| oauth.get("client_secret_env"))
                .and_then(toml::Value::as_str);
            declared.extend(oauth_secret.map(str::to_string));
        }

        let listed: BTreeSet<String> = read_dotenv(&sample_dir().join(".env.example"))
            .expect(".env.example parses")
            .into_keys()
            .collect();

        assert_eq!(listed, declared);
        assert_eq!(
            listed,
            BTreeSet::from([
                "REDDIT_CLIENT_SECRET".to_string(),
                "YOUTUBE_API_KEY".to_string(),
                "YOUTUBE_OAUTH_CLIENT_SECRET".to_string(),
            ])
        );

        // Every value is empty: no real credential is ever committed.
        let values = read_dotenv(&sample_dir().join(".env.example")).expect(".env.example parses");
        assert!(values.values().all(String::is_empty), "{values:?}");
    }

    fn sample_registry() -> (TempDir, LoadedRegistry) {
        let dir = sample_copy(
            "REDDIT_CLIENT_SECRET=a\nYOUTUBE_API_KEY=b\nYOUTUBE_OAUTH_CLIENT_SECRET=c\n",
        );
        let registry = load_registry_with_env(&dir.path().join("filimon.toml"), &|_| None)
            .expect("sample registry loads");
        (dir, registry)
    }

    #[test]
    fn sample_instances_of_one_component_share_it() {
        let (_dir, registry) = sample_registry();
        let component = |name: &str| registry.sources[name].component.clone();

        assert_eq!(component("reddit"), component("reddit-user"));
        assert_eq!(component("youtube"), component("youtube-user"));
        assert_ne!(component("reddit"), component("youtube"));
        assert!(component("reddit").ends_with("reddit_source.wasm"));
        assert!(component("youtube").ends_with("youtube_source.wasm"));
    }

    #[test]
    fn sample_user_instances_request_read_only_scopes_and_official_hosts() {
        let (_dir, registry) = sample_registry();

        let reddit_user = plugin(&registry, "reddit-user");
        let scopes = &reddit_user.oauth.as_ref().expect("oauth").scopes;
        assert_eq!(scopes, &vec!["read".to_string(), "history".to_string()]);

        let youtube_user = plugin(&registry, "youtube-user");
        let scopes = &youtube_user.oauth.as_ref().expect("oauth").scopes;
        assert_eq!(
            scopes,
            &vec!["https://www.googleapis.com/auth/youtube.readonly".to_string()]
        );

        for name in ["reddit", "reddit-user"] {
            assert_eq!(
                plugin(&registry, name).host.allowlist,
                vec!["oauth.reddit.com".to_string()],
                "{name}"
            );
        }
        for name in ["youtube", "youtube-user"] {
            assert_eq!(
                plugin(&registry, name).host.allowlist,
                vec!["www.googleapis.com".to_string()],
                "{name}"
            );
        }
    }

    #[test]
    fn sample_settings_use_only_keys_the_plugins_accept() {
        let (_dir, registry) = sample_registry();
        let reddit_keys = [
            "subreddits",
            "listings",
            "feed",
            "username",
            "user_agent_contact",
            "app_id",
            "first_poll_limit",
        ];
        let youtube_keys = ["channels", "subscriptions", "subscriptions_limit"];

        for name in ["reddit", "reddit-user"] {
            let table = settings(plugin(&registry, name));
            for key in table.keys() {
                assert!(reddit_keys.contains(&key.as_str()), "{name}: {key}");
            }
            assert!(table.contains_key("user_agent_contact"), "{name}");
        }
        for name in ["youtube", "youtube-user"] {
            let table = settings(plugin(&registry, name));
            for key in table.keys() {
                assert!(youtube_keys.contains(&key.as_str()), "{name}: {key}");
            }
        }
    }
}
