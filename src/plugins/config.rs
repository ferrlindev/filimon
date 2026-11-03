// ─────────────────────────────────────────────────────────────────────────────
// plugins/config.rs - Plugin registry and per-plugin TOML loading
//
// `filimon.toml` names every plugin under `[sources.<name>]` and
// `[fetchers.<name>]`. Each enabled source points at its own TOML file with a
// typed `[host]` table and an opaque `[settings]` table. All paths resolve
// relative to the registry file's directory.
//
// Loading never stops at the first problem: every problem is collected into
// one `ConfigErrors`, and a partial `LoadedRegistry` is never returned.
// ─────────────────────────────────────────────────────────────────────────────

use std::collections::BTreeMap;
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;

use super::error::ConfigErrors;
use super::secrets::ResolvedSecrets;

// ── Defaults ──────────────────────────────────────────────────────────────────

const DEFAULT_TIMEOUT_SECS: NonZeroU64 = NonZeroU64::new(30).unwrap();
const DEFAULT_BACKOFF_INITIAL_SECS: NonZeroU64 = NonZeroU64::new(60).unwrap();
const DEFAULT_BACKOFF_MAX_SECS: NonZeroU64 = NonZeroU64::new(3600).unwrap();
const DEFAULT_BACKOFF_MULTIPLIER: f64 = 2.0;

fn default_true() -> bool {
    true
}

fn default_timeout_secs() -> NonZeroU64 {
    DEFAULT_TIMEOUT_SECS
}

fn default_backoff_initial_secs() -> NonZeroU64 {
    DEFAULT_BACKOFF_INITIAL_SECS
}

fn default_backoff_max_secs() -> NonZeroU64 {
    DEFAULT_BACKOFF_MAX_SECS
}

fn default_backoff_multiplier() -> f64 {
    DEFAULT_BACKOFF_MULTIPLIER
}

// ── File Shapes (private) ─────────────────────────────────────────────────────

/// `filimon.toml`: every plugin, by kind and name.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Registry {
    #[serde(default)]
    sources: BTreeMap<String, SourceEntry>,
    #[serde(default)]
    fetchers: BTreeMap<String, FetcherEntry>,
}

/// One `[sources.<name>]` table.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceEntry {
    component: toml::Spanned<PathBuf>,
    config: toml::Spanned<PathBuf>,
    #[serde(default = "default_true")]
    enabled: bool,
    fetcher: Option<toml::Spanned<String>>,
}

/// One `[fetchers.<name>]` table.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct FetcherEntry {
    component: toml::Spanned<PathBuf>,
    #[serde(default = "default_true")]
    enabled: bool,
}

/// A source plugin's own TOML file.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PluginFile {
    host: HostSection,
    #[serde(default)]
    settings: toml::Table,
}

/// The `[host]` table: values the host enforces on the plugin.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct HostSection {
    poll_interval_secs: NonZeroU64,
    #[serde(default = "default_timeout_secs")]
    timeout_secs: NonZeroU64,
    #[serde(default)]
    allowlist: Vec<toml::Spanned<String>>,
    #[serde(default)]
    backoff: BackoffSection,
    /// alias -> env var name
    #[serde(default)]
    secrets: BTreeMap<String, toml::Spanned<String>>,
}

/// The `[host.backoff]` table.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct BackoffSection {
    #[serde(default = "default_backoff_initial_secs")]
    initial_secs: NonZeroU64,
    #[serde(default = "default_backoff_max_secs")]
    max_secs: NonZeroU64,
    #[serde(default = "default_backoff_multiplier")]
    multiplier: f64,
}

impl Default for BackoffSection {
    fn default() -> Self {
        Self {
            initial_secs: DEFAULT_BACKOFF_INITIAL_SECS,
            max_secs: DEFAULT_BACKOFF_MAX_SECS,
            multiplier: DEFAULT_BACKOFF_MULTIPLIER,
        }
    }
}

// ── Loaded Configuration (public) ─────────────────────────────────────────────

/// The whole plugin configuration, validated and typed.
#[derive(Debug, Clone)]
pub struct LoadedRegistry {
    /// The registry file as given.
    pub path: PathBuf,
    /// The registry file's directory; every relative path resolves against it.
    pub base_dir: PathBuf,
    pub sources: BTreeMap<String, LoadedSource>,
    pub fetchers: BTreeMap<String, LoadedFetcher>,
}

/// One source plugin. `plugin` is `Some` exactly when the source is enabled.
#[derive(Debug, Clone)]
pub struct LoadedSource {
    pub name: String,
    pub component: PathBuf,
    pub config_path: PathBuf,
    pub enabled: bool,
    pub fetcher: Option<String>,
    pub plugin: Option<PluginConfig>,
}

/// An enabled source's own configuration.
#[derive(Debug, Clone)]
pub struct PluginConfig {
    pub host: HostConfig,
    /// `[settings]` re-serialised as TOML; `""` when absent. Key order and
    /// comments are not preserved.
    pub settings_toml: String,
    /// Only the aliases this plugin's `[host.secrets]` names.
    pub secrets: ResolvedSecrets,
}

/// Host-enforced values for one plugin.
#[derive(Debug, Clone, PartialEq)]
pub struct HostConfig {
    pub poll_interval: Duration,
    pub timeout: Duration,
    /// Hosts the plugin may fetch from. Empty means deny all.
    pub allowlist: Vec<String>,
    pub backoff: BackoffConfig,
}

/// Retry backoff for one plugin.
#[derive(Debug, Clone, PartialEq)]
pub struct BackoffConfig {
    pub initial: Duration,
    pub max: Duration,
    pub multiplier: f64,
}

/// One fetcher plugin.
#[derive(Debug, Clone)]
pub struct LoadedFetcher {
    pub name: String,
    pub component: PathBuf,
    pub enabled: bool,
}

// ── Loading ───────────────────────────────────────────────────────────────────

/// Loads the registry at `path`, reading secrets from the process environment
/// and the `.env` next to the registry.
///
/// # Errors
/// Returns every problem found in the registry, plugin files and secrets.
pub fn load_registry(path: &Path) -> Result<LoadedRegistry, ConfigErrors> {
    load_registry_with_env(path, &|key| std::env::var(key).ok())
}

/// Loads the registry at `path`, looking env vars up through `env` before
/// falling back to the `.env` next to the registry.
///
/// # Errors
/// Returns every problem found in the registry, plugin files and secrets.
pub fn load_registry_with_env(
    _path: &Path,
    _env: &dyn Fn(&str) -> Option<String>,
) -> Result<LoadedRegistry, ConfigErrors> {
    Err(ConfigErrors {
        problems: Vec::new(),
    })
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn write(dir: &TempDir, name: &str, text: &str) {
        std::fs::write(dir.path().join(name), text).expect("write fixture");
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn load(dir: &TempDir) -> Result<LoadedRegistry, ConfigErrors> {
        load_registry_with_env(&dir.path().join("filimon.toml"), &no_env)
    }

    #[test]
    fn loads_minimal_registry() {
        let dir = TempDir::new().expect("tempdir");
        write(
            &dir,
            "filimon.toml",
            r#"
[sources.alpha]
component = "alpha.wasm"
config = "alpha.toml"
fetcher = "beta"

[fetchers.beta]
component = "beta.wasm"
"#,
        );
        write(
            &dir,
            "alpha.toml",
            r#"
[host]
poll_interval_secs = 120
allowlist = ["api.example.com"]

[host.secrets]
token = "ALPHA_TOKEN"

[settings]
greeting = "hi"
"#,
        );
        write(&dir, ".env", "ALPHA_TOKEN=a1\nOTHER_KEY=x\n");

        let registry = load(&dir).expect("registry loads");

        let alpha = &registry.sources["alpha"];
        assert_eq!(alpha.name, "alpha");
        assert_eq!(alpha.component, dir.path().join("alpha.wasm"));
        assert_eq!(alpha.config_path, dir.path().join("alpha.toml"));
        assert!(alpha.enabled);
        assert_eq!(alpha.fetcher.as_deref(), Some("beta"));

        let plugin = alpha.plugin.as_ref().expect("enabled source is loaded");
        assert_eq!(plugin.host.poll_interval, Duration::from_secs(120));
        assert_eq!(plugin.host.timeout, Duration::from_secs(30));
        assert_eq!(plugin.host.backoff.initial, Duration::from_secs(60));
        assert_eq!(plugin.host.backoff.max, Duration::from_secs(3600));
        assert!((plugin.host.backoff.multiplier - 2.0).abs() < f64::EPSILON);
        assert_eq!(plugin.host.allowlist, vec!["api.example.com".to_string()]);

        let settings: toml::Table =
            toml::from_str(&plugin.settings_toml).expect("settings_toml is TOML");
        assert_eq!(
            settings.get("greeting").and_then(toml::Value::as_str),
            Some("hi")
        );

        let keys: Vec<&str> = plugin.secrets.keys().map(String::as_str).collect();
        assert_eq!(keys, vec!["token"]);
        assert_eq!(plugin.secrets["token"].expose(), "a1");

        let beta = &registry.fetchers["beta"];
        assert_eq!(beta.name, "beta");
        assert_eq!(beta.component, dir.path().join("beta.wasm"));
        assert!(beta.enabled);
    }

    #[test]
    fn empty_registry_loads() {
        let dir = TempDir::new().expect("tempdir");
        write(&dir, "filimon.toml", "");

        let registry = load(&dir).expect("empty registry loads");

        assert!(registry.sources.is_empty());
        assert!(registry.fetchers.is_empty());
    }

    #[test]
    fn absent_settings_becomes_empty_string() {
        let dir = TempDir::new().expect("tempdir");
        write(
            &dir,
            "filimon.toml",
            "[sources.alpha]\ncomponent = \"alpha.wasm\"\nconfig = \"alpha.toml\"\n",
        );
        write(&dir, "alpha.toml", "[host]\npoll_interval_secs = 60\n");

        let registry = load(&dir).expect("registry loads");

        let plugin = registry.sources["alpha"]
            .plugin
            .as_ref()
            .expect("enabled source is loaded");
        assert_eq!(plugin.settings_toml, "");
        assert!(plugin.secrets.is_empty());
    }

    #[test]
    fn disabled_source_is_not_loaded() {
        let dir = TempDir::new().expect("tempdir");
        write(
            &dir,
            "filimon.toml",
            "[sources.off]\ncomponent = \"off.wasm\"\nconfig = \"does-not-exist.toml\"\nenabled = false\n",
        );

        let registry = load(&dir).expect("disabled source does not block loading");

        let off = &registry.sources["off"];
        assert!(!off.enabled);
        assert!(off.plugin.is_none());
        assert_eq!(off.config_path, dir.path().join("does-not-exist.toml"));
    }
}
