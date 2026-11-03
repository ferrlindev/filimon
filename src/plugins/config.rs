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

use std::collections::{BTreeMap, HashMap};
use std::num::NonZeroU64;
use std::path::{Path, PathBuf};
use std::time::Duration;

use miette::{NamedSource, SourceSpan};
use serde::Deserialize;

use super::error::{ConfigError, ConfigErrors};
use super::secrets::{self, ResolvedSecrets};

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
    path: &Path,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<LoadedRegistry, ConfigErrors> {
    let registry_name = path.display().to_string();

    // The registry is read once; its parse result and its diagnostic source
    // both come from this one String. Only registry-level failures stop early.
    let registry_text = std::fs::read_to_string(path).map_err(|e| {
        single(ConfigError::RegistryUnreadable {
            path: registry_name.clone(),
            reason: e.to_string(),
        })
    })?;
    let registry: Registry = toml::from_str(&registry_text).map_err(|e| {
        single(ConfigError::RegistryMalformed {
            path: registry_name.clone(),
            message: e.message().to_string(),
            src: NamedSource::new(&registry_name, registry_text.clone()),
            span: e.span().map(Into::into),
        })
    })?;

    // `Path::new("filimon.toml").parent()` is `Some("")`, which joins correctly.
    let base_dir = path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
    let mut problems = Vec::new();

    // Pass 1: read and type-check each enabled source's plugin file.
    let mut parsed: BTreeMap<&str, ParsedPlugin> = BTreeMap::new();
    for (name, entry) in &registry.sources {
        if !entry.enabled {
            continue;
        }
        if let Some(problem) =
            fetcher_problem(name, entry, &registry.fetchers, &registry_name, &registry_text)
        {
            problems.push(problem);
        }
        let config_path = base_dir.join(entry.config.get_ref());
        match read_plugin(name, entry, &config_path, &registry_name, &registry_text) {
            Ok(plugin) => {
                parsed.insert(name, plugin);
            }
            Err(problem) => problems.push(problem),
        }
    }

    // Pass 2: `.env` is read only when some enabled source declares secrets,
    // so a `.env` shared with other tools cannot break plugin-free registries.
    let dotenv_path = base_dir.join(".env");
    let dotenv = if parsed.values().any(|plugin| !plugin.aliases.is_empty()) {
        secrets::read_dotenv(&dotenv_path).unwrap_or_else(|problem| {
            problems.push(problem);
            HashMap::new()
        })
    } else {
        HashMap::new()
    };

    // Pass 3: resolve secrets per enabled source and assemble the output.
    let mut sources = BTreeMap::new();
    for (name, entry) in &registry.sources {
        let plugin = if entry.enabled {
            // A missing entry means its plugin file problem is already recorded.
            let Some(parsed) = parsed.remove(name.as_str()) else {
                continue;
            };
            match secrets::resolve_secrets(
                name,
                &parsed.path,
                &parsed.text,
                &parsed.aliases,
                &dotenv,
                &dotenv_path,
                env,
            ) {
                Ok(secrets) => {
                    // Names only, never values: makes alias mappings auditable.
                    let env_vars: Vec<&str> = parsed
                        .aliases
                        .values()
                        .map(|v| v.get_ref().as_str())
                        .collect();
                    tracing::info!(plugin = %name, ?env_vars, "resolved plugin secrets");
                    Some(PluginConfig {
                        host: parsed.host,
                        settings_toml: parsed.settings_toml,
                        secrets,
                    })
                }
                Err(mut missing) => {
                    problems.append(&mut missing);
                    continue;
                }
            }
        } else {
            None
        };
        sources.insert(
            name.clone(),
            LoadedSource {
                name: name.clone(),
                component: base_dir.join(entry.component.get_ref()),
                config_path: base_dir.join(entry.config.get_ref()),
                enabled: entry.enabled,
                fetcher: entry.fetcher.as_ref().map(|f| f.get_ref().clone()),
                plugin,
            },
        );
    }

    let fetchers = load_fetchers(&registry.fetchers, &base_dir);

    if !problems.is_empty() {
        return Err(ConfigErrors { problems });
    }
    Ok(LoadedRegistry {
        path: path.to_path_buf(),
        base_dir,
        sources,
        fetchers,
    })
}

fn load_fetchers(
    entries: &BTreeMap<String, FetcherEntry>,
    base_dir: &Path,
) -> BTreeMap<String, LoadedFetcher> {
    entries
        .iter()
        .map(|(name, entry)| {
            let fetcher = LoadedFetcher {
                name: name.clone(),
                component: base_dir.join(entry.component.get_ref()),
                enabled: entry.enabled,
            };
            (name.clone(), fetcher)
        })
        .collect()
}

/// Checks an enabled source's `fetcher` reference against `[fetchers]`.
fn fetcher_problem(
    name: &str,
    entry: &SourceEntry,
    fetchers: &BTreeMap<String, FetcherEntry>,
    registry_name: &str,
    registry_text: &str,
) -> Option<ConfigError> {
    let fetcher = entry.fetcher.as_ref()?;
    if fetchers.contains_key(fetcher.get_ref()) {
        return None;
    }
    let defined = if fetchers.is_empty() {
        "none".to_string()
    } else {
        fetchers.keys().map(String::as_str).collect::<Vec<_>>().join(", ")
    };
    Some(ConfigError::UnknownFetcher {
        plugin: name.to_string(),
        fetcher: fetcher.get_ref().clone(),
        defined,
        src: NamedSource::new(registry_name, registry_text.to_string()),
        span: fetcher.span().into(),
    })
}

fn single(problem: ConfigError) -> ConfigErrors {
    ConfigErrors {
        problems: vec![problem],
    }
}

/// An enabled source's plugin file, parsed but with secrets not yet resolved.
struct ParsedPlugin {
    path: PathBuf,
    text: String,
    host: HostConfig,
    settings_toml: String,
    aliases: BTreeMap<String, toml::Spanned<String>>,
}

/// Reads one plugin file once and builds its typed values and its diagnostic
/// source from that single read.
#[allow(
    clippy::result_large_err,
    reason = "cold one-shot config load; ConfigError carries its diagnostic source"
)]
fn read_plugin(
    name: &str,
    entry: &SourceEntry,
    config_path: &Path,
    registry_name: &str,
    registry_text: &str,
) -> Result<ParsedPlugin, ConfigError> {
    let display = config_path.display().to_string();
    let text =
        std::fs::read_to_string(config_path).map_err(|e| ConfigError::PluginFileMissing {
            plugin: name.to_string(),
            path: display.clone(),
            reason: e.to_string(),
            src: NamedSource::new(registry_name, registry_text.to_string()),
            span: entry.config.span().into(),
        })?;

    let malformed = |message: String, span: Option<SourceSpan>| ConfigError::PluginFileMalformed {
        plugin: name.to_string(),
        path: display.clone(),
        message,
        src: NamedSource::new(&display, text.clone()),
        span,
    };

    let file: PluginFile = toml::from_str(&text)
        .map_err(|e| malformed(e.message().to_string(), e.span().map(Into::into)))?;

    let settings_toml = if file.settings.is_empty() {
        String::new()
    } else {
        toml::to_string(&file.settings)
            .map_err(|e| malformed(format!("cannot re-serialise [settings]: {e}"), None))?
    };

    let HostSection {
        poll_interval_secs,
        timeout_secs,
        allowlist,
        backoff,
        secrets,
    } = file.host;
    let host = HostConfig {
        poll_interval: Duration::from_secs(poll_interval_secs.get()),
        timeout: Duration::from_secs(timeout_secs.get()),
        allowlist: allowlist
            .into_iter()
            .map(toml::Spanned::into_inner)
            .collect(),
        backoff: BackoffConfig {
            initial: Duration::from_secs(backoff.initial_secs.get()),
            max: Duration::from_secs(backoff.max_secs.get()),
            multiplier: backoff.multiplier,
        },
    };

    Ok(ParsedPlugin {
        path: config_path.to_path_buf(),
        text,
        host,
        settings_toml,
        aliases: secrets,
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

        assert_eq!(registry.path, dir.path().join("filimon.toml"));
        assert_eq!(registry.base_dir, dir.path());

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

    // ── Secret isolation (CONF-02) ────────────────────────────────────────────

    /// Writes a registry with one enabled source `alpha` whose only secret is
    /// `token = "<env_var>"`, plus the given `.env` text.
    fn alpha_with_secret(dir: &TempDir, env_var: &str, dotenv: &str) {
        write(
            dir,
            "filimon.toml",
            "[sources.alpha]\ncomponent = \"alpha.wasm\"\nconfig = \"alpha.toml\"\n",
        );
        write(
            dir,
            "alpha.toml",
            &format!("[host]\npoll_interval_secs = 60\n\n[host.secrets]\ntoken = \"{env_var}\"\n"),
        );
        write(dir, ".env", dotenv);
    }

    fn secrets_of<'a>(registry: &'a LoadedRegistry, name: &str) -> &'a ResolvedSecrets {
        &registry.sources[name]
            .plugin
            .as_ref()
            .expect("enabled source is loaded")
            .secrets
    }

    #[test]
    fn only_aliased_secrets_resolved() {
        let dir = TempDir::new().expect("tempdir");
        write(
            &dir,
            "filimon.toml",
            r#"
[sources.alpha]
component = "alpha.wasm"
config = "alpha.toml"

[sources.beta]
component = "beta.wasm"
config = "beta.toml"
"#,
        );
        write(
            &dir,
            "alpha.toml",
            "[host]\npoll_interval_secs = 60\n\n[host.secrets]\ntoken = \"ALPHA_TOKEN\"\n",
        );
        write(
            &dir,
            "beta.toml",
            "[host]\npoll_interval_secs = 60\n\n[host.secrets]\nkey = \"BETA_KEY\"\n",
        );
        write(
            &dir,
            ".env",
            "ALPHA_TOKEN=alpha-value\nBETA_KEY=beta-value\nUNRELATED_TOKEN=unrelated-value\n",
        );

        let registry = load(&dir).expect("registry loads");
        let alpha = secrets_of(&registry, "alpha");
        let beta = secrets_of(&registry, "beta");

        assert_eq!(
            alpha.keys().map(String::as_str).collect::<Vec<_>>(),
            ["token"]
        );
        assert_eq!(beta.keys().map(String::as_str).collect::<Vec<_>>(), ["key"]);
        assert_eq!(alpha["token"].expose(), "alpha-value");
        assert_eq!(beta["key"].expose(), "beta-value");

        for secrets in [alpha, beta] {
            for env_var in ["UNRELATED_TOKEN", "ALPHA_TOKEN", "BETA_KEY"] {
                assert!(
                    !secrets.contains_key(env_var),
                    "env var name {env_var} used as a key"
                );
            }
            assert!(secrets.values().all(|s| s.expose() != "unrelated-value"));
        }
        assert!(alpha.values().all(|s| s.expose() != "beta-value"));
        assert!(beta.values().all(|s| s.expose() != "alpha-value"));
    }

    #[test]
    fn process_env_overrides_dotenv() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_secret(&dir, "ALPHA_TOKEN", "ALPHA_TOKEN=from-dotenv\n");
        let env = |key: &str| (key == "ALPHA_TOKEN").then(|| "from-env".to_string());

        let registry =
            load_registry_with_env(&dir.path().join("filimon.toml"), &env).expect("registry loads");

        assert_eq!(secrets_of(&registry, "alpha")["token"].expose(), "from-env");
    }

    #[test]
    fn empty_secret_value_is_error() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_secret(&dir, "ALPHA_TOKEN", "ALPHA_TOKEN=\n");

        let errors = load(&dir).expect_err("an empty secret is an error");

        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        let ConfigError::EmptySecret {
            plugin,
            alias,
            env_var,
            ..
        } = &errors.problems[0]
        else {
            panic!("expected EmptySecret, got {:?}", errors.problems[0]);
        };
        assert_eq!(plugin, "alpha");
        assert_eq!(alias, "token");
        assert_eq!(env_var, "ALPHA_TOKEN");
    }

    #[test]
    fn env_lookup_is_case_sensitive() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_secret(&dir, "ALPHA_TOKEN", "alpha_token=x\n");

        let errors = load(&dir).expect_err("a case-mismatched key does not satisfy the alias");

        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        assert!(
            matches!(
                &errors.problems[0],
                ConfigError::MissingSecret { alias, env_var, .. }
                    if alias == "token" && env_var == "ALPHA_TOKEN"
            ),
            "{errors:?}"
        );
    }

    #[test]
    fn malformed_dotenv_reports_line_not_content() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_secret(&dir, "ALPHA_TOKEN", "OK_KEY=1\nALPHA_TOKEN=s3cr3t value\n");

        let errors = load(&dir).expect_err("a malformed .env is an error");

        assert!(
            errors
                .problems
                .iter()
                .any(|problem| matches!(problem, ConfigError::DotenvInvalid { line: Some(2), .. })),
            "{errors:?}"
        );
        for problem in &errors.problems {
            assert!(
                !problem.to_string().contains("s3cr3t"),
                "leaked in: {problem}"
            );
        }
        assert!(!errors.to_string().contains("s3cr3t"));
        assert!(!format!("{errors:?}").contains("s3cr3t"));
    }

    #[test]
    fn loading_does_not_mutate_process_env() {
        const KEY: &str = "FILI_TEST_DOTENV_ONLY_7F3A";
        let dir = TempDir::new().expect("tempdir");
        alpha_with_secret(&dir, KEY, &format!("{KEY}=x\n"));

        let registry = load(&dir).expect("registry loads");

        assert_eq!(secrets_of(&registry, "alpha")["token"].expose(), "x");
        assert!(
            std::env::var(KEY).is_err(),
            "{KEY} leaked into the process env"
        );
    }

    // ── Diagnostics (CONF-03) ─────────────────────────────────────────────────

    /// Renders the full miette report as a terminal would show it, without
    /// colour. The wide width stops wrapping from splitting asserted text.
    fn render(errors: &ConfigErrors) -> String {
        let mut out = String::new();
        miette::GraphicalReportHandler::new_themed(miette::GraphicalTheme::unicode_nocolor())
            .with_width(400)
            .render_report(&mut out, errors)
            .expect("render report");
        out
    }

    #[test]
    fn unknown_fetcher() {
        let dir = TempDir::new().expect("tempdir");
        write(
            &dir,
            "filimon.toml",
            r#"
[sources.alpha]
component = "alpha.wasm"
config = "alpha.toml"
fetcher = "htp"

[fetchers.http]
component = "http.wasm"
"#,
        );
        write(&dir, "alpha.toml", "[host]\npoll_interval_secs = 60\n");

        let errors = load(&dir).expect_err("an unknown fetcher is an error");

        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        assert!(
            matches!(
                &errors.problems[0],
                ConfigError::UnknownFetcher { plugin, fetcher, .. }
                    if plugin == "alpha" && fetcher == "htp"
            ),
            "{errors:?}"
        );
        let report = render(&errors);
        for needle in ["sources.alpha.fetcher", "htp", "filimon.toml", "http"] {
            assert!(report.contains(needle), "missing {needle:?} in:\n{report}");
        }
    }

    fn assert_contains(report: &str, needles: &[&str]) {
        for needle in needles {
            assert!(report.contains(needle), "missing {needle:?} in:\n{report}");
        }
    }

    // ── Registry-level diagnostics ────────────────────────────────────────────

    #[test]
    fn registry_missing() {
        let dir = TempDir::new().expect("tempdir");

        let errors = load_registry_with_env(&dir.path().join("nonexistent.toml"), &no_env)
            .expect_err("a missing registry is an error");

        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        assert!(
            matches!(&errors.problems[0], ConfigError::RegistryUnreadable { .. }),
            "{errors:?}"
        );
        assert_contains(&render(&errors), &["nonexistent.toml", "FILI_REGISTRY"]);
    }

    #[test]
    fn registry_malformed() {
        let dir = TempDir::new().expect("tempdir");
        write(&dir, "filimon.toml", "[sources.alpha\ncomponent = \"a.wasm\"\n");

        let errors = load(&dir).expect_err("malformed registry TOML is an error");

        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        assert!(
            matches!(
                &errors.problems[0],
                ConfigError::RegistryMalformed { span: Some(_), .. }
            ),
            "{errors:?}"
        );
    }

    #[test]
    fn registry_unknown_key() {
        let dir = TempDir::new().expect("tempdir");
        write(
            &dir,
            "filimon.toml",
            "[sources.alpha]\ncomponent = \"alpha.wasm\"\nconfg = \"x.toml\"\n",
        );

        let errors = load(&dir).expect_err("an unknown registry key is an error");

        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        assert!(
            matches!(
                &errors.problems[0],
                ConfigError::RegistryMalformed { message, .. } if message.contains("confg")
            ),
            "{errors:?}"
        );
    }

    #[test]
    fn duplicate_table_is_malformed() {
        let dir = TempDir::new().expect("tempdir");
        write(
            &dir,
            "filimon.toml",
            r#"
[sources.alpha]
component = "alpha.wasm"
config = "alpha.toml"

[sources.alpha]
component = "alpha.wasm"
config = "alpha.toml"
"#,
        );

        let errors = load(&dir).expect_err("a duplicate table is an error");

        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        assert!(
            matches!(&errors.problems[0], ConfigError::RegistryMalformed { .. }),
            "{errors:?}"
        );
    }

    #[test]
    fn missing_plugin_file() {
        let dir = TempDir::new().expect("tempdir");
        write(
            &dir,
            "filimon.toml",
            "[sources.alpha]\ncomponent = \"alpha.wasm\"\nconfig = \"missing.toml\"\n",
        );

        let errors = load(&dir).expect_err("a missing plugin file is an error");

        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        assert!(
            matches!(
                &errors.problems[0],
                ConfigError::PluginFileMissing { plugin, .. } if plugin == "alpha"
            ),
            "{errors:?}"
        );
        assert_contains(&render(&errors), &["sources.alpha.config", "missing.toml"]);
    }

    #[test]
    fn disabled_fetcher() {
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
enabled = false
"#,
        );
        write(&dir, "alpha.toml", "[host]\npoll_interval_secs = 60\n");

        let errors = load(&dir).expect_err("a disabled fetcher reference is an error");

        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        assert!(
            matches!(
                &errors.problems[0],
                ConfigError::DisabledFetcher { plugin, fetcher, .. }
                    if plugin == "alpha" && fetcher == "beta"
            ),
            "{errors:?}"
        );
        assert_contains(&render(&errors), &["sources.alpha.fetcher", "disabled"]);
    }

    #[test]
    fn invalid_plugin_name() {
        let dir = TempDir::new().expect("tempdir");
        write(
            &dir,
            "filimon.toml",
            r#"
[sources."Bad Name"]
component = "bad.wasm"
config = "bad.toml"

[fetchers.UPPER]
component = "upper.wasm"
"#,
        );

        let errors = load(&dir).expect_err("invalid plugin names are errors");

        assert_eq!(errors.problems.len(), 2, "{errors:?}");
        assert!(
            matches!(
                &errors.problems[0],
                ConfigError::InvalidName { plugin, table }
                    if plugin == "Bad Name" && table == "sources"
            ),
            "{errors:?}"
        );
        assert!(
            matches!(
                &errors.problems[1],
                ConfigError::InvalidName { plugin, table }
                    if plugin == "UPPER" && table == "fetchers"
            ),
            "{errors:?}"
        );
        assert_contains(&render(&errors), &["Bad Name", "UPPER"]);
    }

    #[test]
    fn duplicate_plugin_name() {
        let dir = TempDir::new().expect("tempdir");
        write(
            &dir,
            "filimon.toml",
            r#"
[sources.shared]
component = "shared.wasm"
config = "shared.toml"

[fetchers.shared]
component = "shared-fetcher.wasm"
"#,
        );
        write(&dir, "shared.toml", "[host]\npoll_interval_secs = 60\n");

        let errors = load(&dir).expect_err("a name in both tables is an error");

        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        assert!(
            matches!(
                &errors.problems[0],
                ConfigError::DuplicateName { plugin } if plugin == "shared"
            ),
            "{errors:?}"
        );
    }

    #[test]
    fn unknown_fetcher_on_disabled_source_ignored() {
        let dir = TempDir::new().expect("tempdir");
        write(
            &dir,
            "filimon.toml",
            r#"
[sources.off]
component = "off.wasm"
config = "off.toml"
enabled = false
fetcher = "nope"
"#,
        );

        load(&dir).expect("a disabled source's fetcher reference is not checked");
    }
}
