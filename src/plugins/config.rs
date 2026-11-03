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

/// The `[host.backoff]` table. Each value keeps its span when it is given, so
/// a range diagnostic can label it; an absent value takes its default.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct BackoffSection {
    initial_secs: Option<toml::Spanned<NonZeroU64>>,
    max_secs: Option<toml::Spanned<NonZeroU64>>,
    multiplier: Option<toml::Spanned<f64>>,
}

impl BackoffSection {
    fn initial(&self) -> NonZeroU64 {
        self.initial_secs
            .as_ref()
            .map_or(DEFAULT_BACKOFF_INITIAL_SECS, |v| *v.get_ref())
    }

    fn max(&self) -> NonZeroU64 {
        self.max_secs
            .as_ref()
            .map_or(DEFAULT_BACKOFF_MAX_SECS, |v| *v.get_ref())
    }

    fn multiplier(&self) -> f64 {
        self.multiplier
            .as_ref()
            .map_or(DEFAULT_BACKOFF_MULTIPLIER, |v| *v.get_ref())
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
    let registry_src = RegistrySource {
        name: &registry_name,
        text: &registry_text,
    };

    // Problems are reported in a fixed order: registry-level problems first,
    // then each enabled, validly named source's problems in name order.
    let mut registry_problems = registry_level_problems(&registry);
    let mut source_problems: BTreeMap<&str, Vec<ConfigError>> = BTreeMap::new();

    // Pass 1: check each enabled source's fetcher reference, then read and
    // type-check its plugin file and range-check its [host] values.
    let mut parsed: BTreeMap<&str, ParsedPlugin> = BTreeMap::new();
    for (name, entry) in loadable(&registry.sources).filter(|(_, entry)| entry.enabled) {
        let bucket = source_problems.entry(name).or_default();
        if let Some(problem) = fetcher_problem(name, entry, &registry.fetchers, registry_src) {
            bucket.push(problem);
        }
        let config_path = base_dir.join(entry.config.get_ref());
        match read_plugin(name, entry, &config_path, registry_src) {
            Ok(mut plugin) => {
                bucket.append(&mut plugin.invalid);
                parsed.insert(name, plugin);
            }
            Err(problem) => bucket.push(problem),
        }
    }

    // Pass 2: `.env` is read only when some enabled source declares secrets,
    // so a `.env` shared with other tools cannot break plugin-free registries.
    // `None` means the file is unusable: its DotenvInvalid problems are
    // recorded here in line order, and pass 3 reports no MissingSecret.
    let dotenv_path = base_dir.join(".env");
    let dotenv: Option<HashMap<String, String>> =
        if parsed.values().any(|plugin| !plugin.aliases.is_empty()) {
            match secrets::read_dotenv(&dotenv_path) {
                Ok(map) => Some(map),
                Err(mut problems) => {
                    registry_problems.append(&mut problems);
                    None
                }
            }
        } else {
            Some(HashMap::new())
        };

    // Pass 3: resolve secrets per enabled source and assemble the output.
    let mut sources = BTreeMap::new();
    for (name, entry) in loadable(&registry.sources) {
        let plugin = if entry.enabled {
            // A missing entry means its plugin file problem is already recorded.
            let Some(parsed) = parsed.remove(name) else {
                continue;
            };
            match finish_plugin(name, parsed, dotenv.as_ref(), &dotenv_path, env) {
                Ok(plugin) => Some(plugin),
                Err(mut missing) => {
                    source_problems
                        .entry(name)
                        .or_default()
                        .append(&mut missing);
                    continue;
                }
            }
        } else {
            None
        };
        sources.insert(
            name.to_string(),
            LoadedSource {
                name: name.to_string(),
                component: base_dir.join(entry.component.get_ref()),
                config_path: base_dir.join(entry.config.get_ref()),
                enabled: entry.enabled,
                fetcher: entry.fetcher.as_ref().map(|f| f.get_ref().clone()),
                plugin,
            },
        );
    }

    let fetchers = load_fetchers(&registry.fetchers, &base_dir);

    let problems: Vec<ConfigError> = registry_problems
        .into_iter()
        .chain(source_problems.into_values().flatten())
        .collect();
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

/// Resolves a parsed plugin's secrets and builds its `PluginConfig`.
fn finish_plugin(
    name: &str,
    parsed: ParsedPlugin,
    dotenv: Option<&HashMap<String, String>>,
    dotenv_path: &Path,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<PluginConfig, Vec<ConfigError>> {
    let secrets = secrets::resolve_secrets(
        name,
        &parsed.path,
        &parsed.text,
        &parsed.aliases,
        dotenv,
        dotenv_path,
        env,
    )?;
    // Names only, never values: makes alias mappings auditable.
    let env_vars: Vec<&str> = parsed
        .aliases
        .values()
        .map(|v| v.get_ref().as_str())
        .collect();
    tracing::info!(plugin = %name, ?env_vars, "resolved plugin secrets");
    Ok(PluginConfig {
        host: parsed.host,
        settings_toml: parsed.settings_toml,
        secrets,
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

/// The registry file's display name and text, for diagnostic snippets.
#[derive(Clone, Copy)]
struct RegistrySource<'a> {
    name: &'a str,
    text: &'a str,
}

impl RegistrySource<'_> {
    fn named(self) -> NamedSource<String> {
        NamedSource::new(self.name, self.text.to_string())
    }
}

/// `true` when `name` is non-empty and uses only `[a-z0-9_-]`. Names become
/// CLI arguments and per-plugin state keys, so nothing else is accepted.
fn is_valid_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-')
}

/// Sources whose names pass the name rule, in name order. A source with an
/// invalid name is reported once at registry level and not loaded further.
fn loadable(sources: &BTreeMap<String, SourceEntry>) -> impl Iterator<Item = (&str, &SourceEntry)> {
    sources
        .iter()
        .filter(|(name, _)| is_valid_name(name))
        .map(|(name, entry)| (name.as_str(), entry))
}

/// Name-rule problems across the whole registry: invalid names (sources, then
/// fetchers, each in name order), then names used under both tables.
fn registry_level_problems(registry: &Registry) -> Vec<ConfigError> {
    let invalid = |table: &str, names: Vec<&String>| {
        names
            .into_iter()
            .filter(|name| !is_valid_name(name))
            .map(|name| ConfigError::InvalidName {
                plugin: name.clone(),
                table: table.to_string(),
            })
            .collect::<Vec<_>>()
    };
    let mut problems = invalid("sources", registry.sources.keys().collect());
    problems.extend(invalid("fetchers", registry.fetchers.keys().collect()));
    problems.extend(
        registry
            .sources
            .keys()
            .filter(|name| registry.fetchers.contains_key(*name))
            .map(|name| ConfigError::DuplicateName {
                plugin: name.clone(),
            }),
    );
    problems
}

/// Checks an enabled source's `fetcher` reference: it must name a fetcher
/// defined under `[fetchers]`, and that fetcher must be enabled.
fn fetcher_problem(
    name: &str,
    entry: &SourceEntry,
    fetchers: &BTreeMap<String, FetcherEntry>,
    registry: RegistrySource<'_>,
) -> Option<ConfigError> {
    let fetcher = entry.fetcher.as_ref()?;
    match fetchers.get(fetcher.get_ref()) {
        Some(defined) if defined.enabled => None,
        Some(_) => Some(ConfigError::DisabledFetcher {
            plugin: name.to_string(),
            fetcher: fetcher.get_ref().clone(),
            src: registry.named(),
            span: fetcher.span().into(),
        }),
        None => {
            let defined = if fetchers.is_empty() {
                "none".to_string()
            } else {
                fetchers
                    .keys()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            Some(ConfigError::UnknownFetcher {
                plugin: name.to_string(),
                fetcher: fetcher.get_ref().clone(),
                defined,
                src: registry.named(),
                span: fetcher.span().into(),
            })
        }
    }
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
    /// `[host]` values that parsed but are out of range.
    invalid: Vec<ConfigError>,
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
    registry: RegistrySource<'_>,
) -> Result<ParsedPlugin, ConfigError> {
    let display = config_path.display().to_string();
    let text =
        std::fs::read_to_string(config_path).map_err(|e| ConfigError::PluginFileMissing {
            plugin: name.to_string(),
            path: display.clone(),
            reason: e.to_string(),
            src: registry.named(),
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

    let invalid = host_problems(name, &display, &text, &file.host);

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
            initial: Duration::from_secs(backoff.initial().get()),
            max: Duration::from_secs(backoff.max().get()),
            multiplier: backoff.multiplier(),
        },
    };

    Ok(ParsedPlugin {
        path: config_path.to_path_buf(),
        text,
        host,
        settings_toml,
        aliases: secrets,
        invalid,
    })
}

/// Range checks on a parsed `[host]` table, in a fixed order: allowlist
/// entries by index, then the backoff multiplier, then `max_secs`.
fn host_problems(plugin: &str, path: &str, text: &str, host: &HostSection) -> Vec<ConfigError> {
    let invalid =
        |field: String, message: String, span: Option<SourceSpan>| ConfigError::InvalidValue {
            plugin: plugin.to_string(),
            path: path.to_string(),
            field,
            message,
            src: NamedSource::new(path, text.to_string()),
            span,
        };
    let mut problems = Vec::new();

    // Phase 5 matches request hosts against these entries, so anything with a
    // scheme, port, path or other non-host text would never match as written.
    for (index, entry) in host.allowlist.iter().enumerate() {
        if url::Host::parse(entry.get_ref()).is_err() {
            problems.push(invalid(
                format!("host.allowlist[{index}]"),
                format!(
                    "`{}` is not a bare host name such as `api.example.com` (no scheme, port or path)",
                    entry.get_ref()
                ),
                Some(entry.span().into()),
            ));
        }
    }

    let backoff = &host.backoff;
    let multiplier = backoff.multiplier();
    if !(multiplier.is_finite() && multiplier >= 1.0) {
        problems.push(invalid(
            "host.backoff.multiplier".to_string(),
            "must be a finite number >= 1.0".to_string(),
            backoff.multiplier.as_ref().map(|v| v.span().into()),
        ));
    }

    let initial = backoff.initial();
    if backoff.max() < initial {
        // Label `max_secs` when given; otherwise the default max is exceeded
        // by an explicit `initial_secs`, so label that instead.
        let span = backoff
            .max_secs
            .as_ref()
            .or(backoff.initial_secs.as_ref())
            .map(|v| v.span().into());
        problems.push(invalid(
            "host.backoff.max_secs".to_string(),
            format!("must be >= initial_secs ({initial})"),
            span,
        ));
    }

    problems
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

        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        assert!(
            matches!(
                &errors.problems[0],
                ConfigError::DotenvInvalid { line: Some(2), .. }
            ),
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
    fn dotenv_single_quoted_dollar_round_trips() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_secret(&dir, "ALPHA_TOKEN", "ALPHA_TOKEN='pa$$word'\n");

        let registry = load(&dir).expect("a single-quoted `$` value loads");

        assert_eq!(secrets_of(&registry, "alpha")["token"].expose(), "pa$$word");
    }

    #[test]
    fn dotenv_unquoted_dollar_is_rejected() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_secret(&dir, "ALPHA_TOKEN", "ALPHA_TOKEN=pa$$word\n");

        let errors = load(&dir).expect_err("an unquoted `$` is rejected");

        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        assert!(
            errors.problems.iter().any(|problem| matches!(
                problem,
                ConfigError::DotenvInvalid { line: Some(1), reason, .. }
                    if reason.contains("single-quote")
            )),
            "{errors:?}"
        );
        let report = render(&errors);
        let debug = format!("{errors:?}");
        for leaked in ["pa$$word", "$$word"] {
            assert!(!report.contains(leaked), "leaked {leaked:?} in:\n{report}");
            assert!(!debug.contains(leaked), "leaked {leaked:?} in: {debug}");
        }
    }

    #[test]
    fn dotenv_never_expands_process_env() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_plugin(
            &dir,
            "[host]\npoll_interval_secs = 60\n\n[host.secrets]\ntoken = \"ALPHA_TOKEN\"\nbraced = \"ALPHA_BRACED\"\n",
        );
        // Plain literals: `${PATH}` inside a format macro would be a placeholder.
        write(&dir, ".env", "ALPHA_TOKEN='$PATH'\nALPHA_BRACED='${PATH}'\n");
        let env = |key: &str| -> Option<String> {
            assert!(key != "PATH", "loader looked up PATH");
            None
        };

        let registry = load_registry_with_env(&dir.path().join("filimon.toml"), &env)
            .expect("literal `$` values load");

        let secrets = secrets_of(&registry, "alpha");
        assert_eq!(secrets["token"].expose(), "$PATH");
        assert_eq!(secrets["braced"].expose(), "${PATH}");
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
        write(
            &dir,
            "filimon.toml",
            "[sources.alpha\ncomponent = \"a.wasm\"\n",
        );

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

    // ── Plugin-file diagnostics ───────────────────────────────────────────────

    /// Writes a registry with one enabled source `alpha` and the given
    /// `alpha.toml` text.
    fn alpha_with_plugin(dir: &TempDir, plugin_text: &str) {
        write(
            dir,
            "filimon.toml",
            "[sources.alpha]\ncomponent = \"alpha.wasm\"\nconfig = \"alpha.toml\"\n",
        );
        write(dir, "alpha.toml", plugin_text);
    }

    /// Loads `alpha` and returns its single `PluginFileMalformed` message,
    /// asserting the problem names the plugin.
    fn malformed_message(dir: &TempDir) -> String {
        let errors = load(dir).expect_err("a malformed plugin file is an error");
        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        let ConfigError::PluginFileMalformed {
            plugin, message, ..
        } = &errors.problems[0]
        else {
            panic!("expected PluginFileMalformed, got {:?}", errors.problems[0]);
        };
        assert_eq!(plugin, "alpha");
        message.clone()
    }

    /// Loads `alpha` and returns the fields of its `InvalidValue` problems.
    fn invalid_fields(dir: &TempDir) -> Vec<String> {
        let errors = load(dir).expect_err("an invalid host value is an error");
        errors
            .problems
            .iter()
            .map(|problem| match problem {
                ConfigError::InvalidValue { plugin, field, .. } => {
                    assert_eq!(plugin, "alpha");
                    field.clone()
                }
                other => panic!("expected InvalidValue, got {other:?}"),
            })
            .collect()
    }

    #[test]
    fn malformed_plugin_toml() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_plugin(&dir, "[host\npoll_interval_secs = 60\n");

        let errors = load(&dir).expect_err("malformed plugin TOML is an error");

        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        assert!(
            matches!(
                &errors.problems[0],
                ConfigError::PluginFileMalformed { plugin, message, span: Some(_), .. }
                    if plugin == "alpha" && message.contains("unclosed table")
            ),
            "{errors:?}"
        );
    }

    #[test]
    fn unknown_host_key() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_plugin(
            &dir,
            "[host]\npoll_interval_secs = 60\npoll_intervall_secs = 60\n",
        );

        assert!(malformed_message(&dir).contains("poll_intervall_secs"));
    }

    #[test]
    fn zero_poll_interval() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_plugin(&dir, "[host]\npoll_interval_secs = 0\n");

        assert!(malformed_message(&dir).contains("nonzero"));
    }

    #[test]
    fn missing_host_table() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_plugin(&dir, "[settings]\ngreeting = \"hi\"\n");

        let message = malformed_message(&dir);
        assert!(message.contains("missing field"), "{message}");
        assert!(message.contains("host"), "{message}");
    }

    #[test]
    fn invalid_allowlist_entry() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_plugin(
            &dir,
            "[host]\npoll_interval_secs = 60\nallowlist = [\"api.example.com\", \"https://www.reddit.com/r\"]\n",
        );

        let errors = load(&dir).expect_err("a non-host allowlist entry is an error");

        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        assert!(
            matches!(
                &errors.problems[0],
                ConfigError::InvalidValue { plugin, field, span: Some(_), .. }
                    if plugin == "alpha" && field == "host.allowlist[1]"
            ),
            "{errors:?}"
        );
        assert_contains(
            &render(&errors),
            &[
                "host.allowlist[1]",
                "https://www.reddit.com/r",
                "alpha.toml",
            ],
        );
    }

    #[test]
    fn invalid_backoff() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_plugin(
            &dir,
            "[host]\npoll_interval_secs = 60\n\n[host.backoff]\nmultiplier = 0.5\n",
        );
        assert_eq!(invalid_fields(&dir), ["host.backoff.multiplier"]);

        let dir = TempDir::new().expect("tempdir");
        alpha_with_plugin(
            &dir,
            "[host]\npoll_interval_secs = 60\n\n[host.backoff]\ninitial_secs = 60\nmax_secs = 10\n",
        );
        assert_eq!(invalid_fields(&dir), ["host.backoff.max_secs"]);
    }

    #[test]
    fn missing_secret() {
        let dir = TempDir::new().expect("tempdir");
        write(
            &dir,
            "filimon.toml",
            "[sources.reddit]\ncomponent = \"reddit.wasm\"\nconfig = \"reddit.toml\"\n",
        );
        write(
            &dir,
            "reddit.toml",
            "[host]\npoll_interval_secs = 60\n\n[host.secrets]\nclient_id = \"REDDIT_CLIENT_ID\"\n",
        );

        let errors = load(&dir).expect_err("a missing secret is an error");

        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        assert!(
            matches!(
                &errors.problems[0],
                ConfigError::MissingSecret { plugin, .. } if plugin == "reddit"
            ),
            "{errors:?}"
        );
        assert_contains(
            &render(&errors),
            &[
                "host.secrets.client_id",
                "REDDIT_CLIENT_ID",
                "declared here",
                "reddit.toml",
            ],
        );
    }

    // ── Collect-all report (D-14) ─────────────────────────────────────────────

    #[test]
    fn collects_all_problems() {
        let dir = TempDir::new().expect("tempdir");
        write(
            &dir,
            "filimon.toml",
            r#"
[sources."Bad Name"]
component = "bad.wasm"
config = "bad.toml"

[sources.alpha]
component = "alpha.wasm"
config = "missing.toml"

[sources.beta]
component = "beta.wasm"
config = "beta.toml"

[sources.gamma]
component = "gamma.wasm"
config = "gamma.toml"
fetcher = "nope"
"#,
        );
        write(&dir, "beta.toml", "[host\npoll_interval_secs = 60\n");
        write(
            &dir,
            "gamma.toml",
            "[host]\npoll_interval_secs = 60\n\n[host.secrets]\ntoken = \"GAMMA_TOKEN\"\n",
        );

        let first = load(&dir).expect_err("a broken registry fails closed");
        let second = load(&dir).expect_err("a broken registry fails closed");

        let order = |errors: &ConfigErrors| format!("{:?}", errors.problems);
        assert_eq!(order(&first), order(&second), "problem order is stable");

        let problems = &first.problems;
        assert_eq!(problems.len(), 5, "{first:?}");
        assert!(
            matches!(&problems[0], ConfigError::InvalidName { plugin, .. } if plugin == "Bad Name"),
            "{first:?}"
        );
        assert!(
            matches!(&problems[1], ConfigError::PluginFileMissing { plugin, .. } if plugin == "alpha"),
            "{first:?}"
        );
        assert!(
            matches!(&problems[2], ConfigError::PluginFileMalformed { plugin, .. } if plugin == "beta"),
            "{first:?}"
        );
        assert!(
            matches!(&problems[3], ConfigError::UnknownFetcher { plugin, .. } if plugin == "gamma"),
            "{first:?}"
        );
        assert!(
            matches!(&problems[4], ConfigError::MissingSecret { plugin, .. } if plugin == "gamma"),
            "{first:?}"
        );

        let report = render(&first);
        assert!(report.contains("5 problem(s)"), "{report}");
        assert_eq!(report.matches("fili::config::").count(), 5, "{report}");
    }

    #[test]
    fn rendered_report_omits_secret_values() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_plugin(
            &dir,
            "[host]\npoll_interval_secs = 60\n\n[host.secrets]\ntoken = \"ALPHA_TOKEN\"\nenvtok = \"ALPHA_ENV\"\nmissing = \"ALPHA_MISSING\"\n",
        );
        write(&dir, ".env", "ALPHA_TOKEN=pa55-present-VALUE\n");
        let env = |key: &str| (key == "ALPHA_ENV").then(|| "env-supplied-VALUE".to_string());

        let errors = load_registry_with_env(&dir.path().join("filimon.toml"), &env)
            .expect_err("a missing secret is an error");

        let report = render(&errors);
        assert!(report.contains("ALPHA_MISSING"), "{report}");
        for value in ["pa55-present-VALUE", "env-supplied-VALUE"] {
            assert!(!report.contains(value), "leaked {value:?} in:\n{report}");
            assert!(!format!("{errors:?}").contains(value), "leaked {value:?}");
        }
    }

    #[test]
    fn unusable_dotenv_suppresses_missing_secret() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_plugin(
            &dir,
            "[host]\npoll_interval_secs = 60\n\n[host.secrets]\ntoken = \"ALPHA_TOKEN\"\nkey = \"ALPHA_KEY\"\nenvtok = \"ALPHA_ENV\"\n",
        );
        write(&dir, ".env", "ALPHA_TOKEN=ok\nALPHA_KEY=bad value\n");
        let env = |key: &str| (key == "ALPHA_ENV").then(|| "from-env".to_string());

        let errors = load_registry_with_env(&dir.path().join("filimon.toml"), &env)
            .expect_err("an unusable .env is an error");

        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        assert!(
            matches!(
                &errors.problems[0],
                ConfigError::DotenvInvalid { line: Some(2), .. }
            ),
            "{errors:?}"
        );
        assert!(
            !errors
                .problems
                .iter()
                .any(|problem| matches!(problem, ConfigError::MissingSecret { .. })),
            "{errors:?}"
        );
    }

    #[test]
    fn dotenv_problems_keep_line_order() {
        let dir = TempDir::new().expect("tempdir");
        write(
            &dir,
            "filimon.toml",
            r#"
[sources."Bad Name"]
component = "bad.wasm"
config = "bad.toml"

[sources.alpha]
component = "alpha.wasm"
config = "alpha.toml"
"#,
        );
        write(
            &dir,
            "alpha.toml",
            "[host]\npoll_interval_secs = 60\n\n[host.secrets]\ntoken = \"ALPHA_TOKEN\"\n",
        );
        write(&dir, ".env", "ALPHA_TOKEN=x\nB=x y\nC=1\nD=\"$X\"\n");

        let first = load(&dir).expect_err("a broken .env fails closed");
        let second = load(&dir).expect_err("a broken .env fails closed");

        let order = |errors: &ConfigErrors| format!("{:?}", errors.problems);
        assert_eq!(order(&first), order(&second), "problem order is stable");

        let problems = &first.problems;
        assert_eq!(problems.len(), 3, "{first:?}");
        assert!(
            matches!(&problems[0], ConfigError::InvalidName { plugin, .. } if plugin == "Bad Name"),
            "{first:?}"
        );
        assert!(
            matches!(&problems[1], ConfigError::DotenvInvalid { line: Some(2), .. }),
            "{first:?}"
        );
        assert!(
            matches!(&problems[2], ConfigError::DotenvInvalid { line: Some(4), .. }),
            "{first:?}"
        );
    }
}
