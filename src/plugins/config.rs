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
use super::secrets::{self, ResolvedSecrets, Secret};

// ── Defaults ──────────────────────────────────────────────────────────────────

const DEFAULT_TIMEOUT_SECS: NonZeroU64 = NonZeroU64::new(30).unwrap();
const DEFAULT_BACKOFF_INITIAL_SECS: NonZeroU64 = NonZeroU64::new(60).unwrap();
const DEFAULT_BACKOFF_MAX_SECS: NonZeroU64 = NonZeroU64::new(3600).unwrap();
const DEFAULT_BACKOFF_MULTIPLIER: f64 = 2.0;
/// The news crawler's fetch timeout: the built-in client's own timeout.
const DEFAULT_CRAWLER_TIMEOUT_SECS: NonZeroU64 = NonZeroU64::new(15).unwrap();

/// The loopback port of the login callback when a plugin sets no
/// `redirect_port` (D-04). A provider app registers the exact redirect URI
/// `http://127.0.0.1:17420/callback`, so changing this default breaks every app
/// registered with it; `[host.oauth] redirect_port` overrides it per plugin.
pub const DEFAULT_REDIRECT_PORT: u16 = 17420;

/// Authorize-request parameters the host sets itself; `authorize_params` may
/// not name one of them, compared ignoring ASCII case (D-14).
const STANDARD_AUTHORIZE_PARAMS: [&str; 7] = [
    "state",
    "code_challenge",
    "code_challenge_method",
    "redirect_uri",
    "client_id",
    "response_type",
    "scope",
];

fn default_true() -> bool {
    true
}

fn default_timeout_secs() -> NonZeroU64 {
    DEFAULT_TIMEOUT_SECS
}

fn default_crawler_timeout_secs() -> NonZeroU64 {
    DEFAULT_CRAWLER_TIMEOUT_SECS
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
    crawler: Option<CrawlerEntry>,
}

/// The `[crawler]` table: how `fili produce` fetches its pages.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CrawlerEntry {
    fetcher: toml::Spanned<String>,
    #[serde(default = "default_crawler_timeout_secs")]
    timeout_secs: NonZeroU64,
    /// Hosts the crawler may reach besides its seed urls' hosts.
    #[serde(default)]
    allowlist: Vec<String>,
}

/// The two tables `load_crawler` reads. Unlike `Registry` it ignores every
/// other key, so a broken source entry cannot stop `fili produce`.
#[derive(Debug, Deserialize)]
struct CrawlerView {
    crawler: Option<CrawlerEntry>,
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
    /// The `[host.oauth]` table: host-managed OAuth for this plugin (D-11).
    #[serde(default)]
    oauth: Option<OAuthSection>,
}

/// The `[host.oauth]` table. It sits beside the other host-owned keys, never
/// under `[settings]`, so the guest never sees it. Each value keeps its span so
/// a diagnostic can label it.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct OAuthSection {
    authorize_url: toml::Spanned<String>,
    token_url: toml::Spanned<String>,
    #[serde(default)]
    scopes: Vec<toml::Spanned<String>>,
    client_id: toml::Spanned<String>,
    /// Name of the env var (process environment or `.env`) holding the client
    /// secret. The secret itself is never written in the file (D-12).
    client_secret_env: Option<toml::Spanned<String>>,
    revocation_url: Option<toml::Spanned<String>>,
    redirect_port: Option<toml::Spanned<u16>>,
    /// Extra authorize-request parameters (D-14), iterated in key order.
    #[serde(default)]
    authorize_params: BTreeMap<String, toml::Spanned<String>>,
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
    /// The `[crawler]` table, when the registry has one and it is valid.
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "`fili produce` reads the crawler through `load_crawler`; only tests read it here"
        )
    )]
    pub crawler: Option<LoadedCrawler>,
}

/// The news crawler's fetch configuration, validated and typed.
#[derive(Debug, Clone)]
pub struct LoadedCrawler {
    /// The fetcher's registry name.
    pub fetcher: String,
    /// The fetcher's component, resolved against the registry directory.
    pub fetcher_component: PathBuf,
    /// The deadline of one page fetch.
    pub timeout: Duration,
    /// Hosts the crawler may reach besides its seed urls' hosts.
    pub extra_hosts: Vec<String>,
}

/// One source plugin. `plugin` is `Some` exactly when the source is enabled.
#[derive(Debug, Clone)]
pub struct LoadedSource {
    pub name: String,
    pub component: PathBuf,
    #[cfg_attr(
        not(test),
        allow(
            dead_code,
            reason = "the plugin file's path, kept for diagnostics; only tests read it so far"
        )
    )]
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
    /// The plugin's `[host.oauth]` table; `None` when it declares none.
    pub oauth: Option<OAuthConfig>,
}

/// A plugin's host-managed OAuth configuration (D-11). Every endpoint is an
/// `https://` URL with a host and no user-info (D-13). `client_secret` is the
/// only secret; it is never part of `PluginConfig::secrets` (D-12).
#[derive(Debug, Clone)]
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "the login flow and the token client read these fields in later plans of this phase"
    )
)]
pub struct OAuthConfig {
    pub authorize_url: url::Url,
    pub token_url: url::Url,
    pub scopes: Vec<String>,
    pub client_id: String,
    /// The client secret, when `client_secret_env` is set. It stays here: the
    /// plugin's `init` receives only `PluginConfig::secrets`.
    pub client_secret: Option<Secret>,
    pub revocation_url: Option<url::Url>,
    /// The loopback callback port; [`DEFAULT_REDIRECT_PORT`] when not set.
    pub redirect_port: u16,
    /// Extra authorize parameters, in key order.
    pub authorize_params: Vec<(String, String)>,
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
    load_registry_inner(path, env, None)
}

/// Loads only the plugin called `name` from the registry at `path`, reading
/// secrets from the process environment and, when that plugin declares any,
/// the `.env` next to the registry.
///
/// The returned `LoadedRegistry` holds only the named source (fetchers are all
/// listed, they need no file) and exists to validate one plugin: no other
/// plugin's file is read, and no other plugin's problem is reported.
///
/// # Errors
/// Returns every problem that concerns the registry as a whole or `name`.
pub fn load_plugin(path: &Path, name: &str) -> Result<LoadedRegistry, ConfigErrors> {
    load_plugin_with_env(path, name, &|key| std::env::var(key).ok())
}

/// Like [`load_plugin`], looking env vars up through `env` before falling
/// back to the `.env` next to the registry. The returned `LoadedRegistry`
/// holds only the named source and is for validating that one plugin.
///
/// # Errors
/// Returns every problem that concerns the registry as a whole or `name`.
pub fn load_plugin_with_env(
    path: &Path,
    name: &str,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<LoadedRegistry, ConfigErrors> {
    load_registry_inner(path, env, Some(name))
}

/// The shared loader. With `only` set, passes 1 to 3 consider just that
/// source and registry-level problems are kept only when they concern it.
fn load_registry_inner(
    path: &Path,
    env: &dyn Fn(&str) -> Option<String>,
    only: Option<&str>,
) -> Result<LoadedRegistry, ConfigErrors> {
    let wanted = |name: &str| only.is_none_or(|only| only == name);

    // The registry is read once; its parse result and its diagnostic source
    // both come from this one String. Only registry-level failures stop early.
    let (registry, registry_name, registry_text) = read_registry(path)?;

    // `Path::new("filimon.toml").parent()` is `Some("")`, which joins correctly.
    let base_dir = path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
    let registry_src = RegistrySource {
        name: &registry_name,
        text: &registry_text,
    };

    // Problems are reported in a fixed order: registry-level problems first,
    // then each enabled, validly named source's problems in name order.
    let mut registry_problems = registry_level_problems(&registry);
    // The crawler's problem is judged here, before the `wanted` filter, so a
    // scoped plugin load drops it (`ConfigError::plugin` names `crawler`).
    let crawler = registry.crawler.as_ref().and_then(|entry| {
        if let Some(problem) = crawler_problem(entry, &registry.fetchers, registry_src) {
            registry_problems.push(problem);
            None
        } else {
            Some(loaded_crawler(entry, &registry.fetchers, &base_dir))
        }
    });
    registry_problems.retain(|problem| problem.plugin().is_none_or(&wanted));
    let mut source_problems: BTreeMap<&str, Vec<ConfigError>> = BTreeMap::new();

    // Pass 1: check each enabled source's fetcher reference, then read and
    // type-check its plugin file and range-check its [host] values.
    let mut parsed: BTreeMap<&str, ParsedPlugin> = BTreeMap::new();
    for (name, entry) in
        loadable(&registry.sources).filter(|(name, entry)| entry.enabled && wanted(name))
    {
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

    // Pass 2: `.env` is read only when some enabled source declares secrets
    // (`[host.secrets]` aliases or an OAuth `client_secret_env`), so a `.env`
    // shared with other tools cannot break plugin-free registries.
    // `None` means the file is unusable: its DotenvInvalid problems are
    // recorded here in line order, and pass 3 reports no MissingSecret.
    let dotenv_path = base_dir.join(".env");
    let dotenv: Option<HashMap<String, String>> = if parsed
        .values()
        .any(|plugin| !plugin.aliases.is_empty() || plugin.client_secret_env.is_some())
    {
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
    for (name, entry) in loadable(&registry.sources).filter(|(name, _)| wanted(name)) {
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
        crawler,
    })
}

/// Reads and parses `filimon.toml`. Returns the parsed registry plus its
/// display name and text, which the diagnostics of later passes need.
fn read_registry(path: &Path) -> Result<(Registry, String, String), ConfigErrors> {
    read_toml(path)
}

/// Reads `path` once and parses it as `T`; a failure of either step is one
/// registry-level problem.
fn read_toml<T: serde::de::DeserializeOwned>(
    path: &Path,
) -> Result<(T, String, String), ConfigErrors> {
    let registry_name = path.display().to_string();
    let registry_text = std::fs::read_to_string(path).map_err(|e| {
        single(ConfigError::RegistryUnreadable {
            path: registry_name.clone(),
            reason: e.to_string(),
        })
    })?;
    let parsed: T = toml::from_str(&registry_text).map_err(|e| {
        single(ConfigError::RegistryMalformed {
            path: registry_name.clone(),
            message: e.message().to_string(),
            src: NamedSource::new(&registry_name, registry_text.clone()),
            span: e.span().map(Into::into),
        })
    })?;
    Ok((parsed, registry_name, registry_text))
}

/// Loads only the `[crawler]` table of the registry at `path`.
///
/// Reads nothing but `[crawler]` and `[fetchers]`: no source's plugin file, no
/// `.env` and no secret, so a broken source entry cannot stop `fili produce`
/// (WD-23). `Ok(None)` when the registry has no `[crawler]` table.
///
/// # Errors
/// `RegistryUnreadable` or `RegistryMalformed` when the file cannot be read
/// or parsed, and the unknown or disabled fetcher problem when `[crawler]`
/// names a fetcher that cannot be used.
pub fn load_crawler(path: &Path) -> Result<Option<LoadedCrawler>, ConfigErrors> {
    let (view, name, text): (CrawlerView, _, _) = read_toml(path)?;
    let Some(entry) = view.crawler else {
        return Ok(None);
    };
    let base_dir = path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
    let src = RegistrySource {
        name: &name,
        text: &text,
    };
    if let Some(problem) = crawler_problem(&entry, &view.fetchers, src) {
        return Err(single(problem));
    }
    Ok(Some(loaded_crawler(&entry, &view.fetchers, &base_dir)))
}

// ── Listing ───────────────────────────────────────────────────────────────────

/// What `filimon.toml` alone says about its plugins: no plugin file, no
/// secret and no `.env` is read.
#[derive(Debug, Clone)]
pub struct RegistryListing {
    /// The registry file as given.
    pub path: PathBuf,
    /// The registry file's directory; every relative path resolves against it.
    pub base_dir: PathBuf,
    /// Sources with valid names, in name order.
    pub sources: Vec<ListedPlugin>,
    /// Fetchers with valid names, in name order.
    pub fetchers: Vec<ListedPlugin>,
    /// Names that fail the name rule: excluded from the rows above, so a name
    /// from the file is never used to build a path.
    pub skipped: Vec<SkippedName>,
}

/// One plugin as the registry lists it.
#[derive(Debug, Clone)]
pub struct ListedPlugin {
    pub name: String,
    pub component: PathBuf,
    pub enabled: bool,
    /// A source's `fetcher` reference; always `None` for a fetcher.
    pub fetcher: Option<String>,
}

/// A registry name that fails the `[a-z0-9_-]` rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkippedName {
    /// `"sources"` or `"fetchers"`.
    pub table: &'static str,
    pub name: String,
}

/// Lists the plugins in the registry at `path` from `filimon.toml` alone.
///
/// # Errors
/// Returns `RegistryUnreadable` or `RegistryMalformed` when the registry file
/// cannot be read or parsed.
pub fn list_registry(path: &Path) -> Result<RegistryListing, ConfigErrors> {
    let (registry, _, _) = read_registry(path)?;
    let base_dir = path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
    let mut skipped = Vec::new();

    let mut sources = Vec::new();
    for (name, entry) in &registry.sources {
        if !is_valid_name(name) {
            skipped.push(SkippedName {
                table: "sources",
                name: name.clone(),
            });
            continue;
        }
        sources.push(ListedPlugin {
            name: name.clone(),
            component: base_dir.join(entry.component.get_ref()),
            enabled: entry.enabled,
            fetcher: entry.fetcher.as_ref().map(|f| f.get_ref().clone()),
        });
    }

    let mut fetchers = Vec::new();
    for (name, entry) in &registry.fetchers {
        if !is_valid_name(name) {
            skipped.push(SkippedName {
                table: "fetchers",
                name: name.clone(),
            });
            continue;
        }
        fetchers.push(ListedPlugin {
            name: name.clone(),
            component: base_dir.join(entry.component.get_ref()),
            enabled: entry.enabled,
            fetcher: None,
        });
    }

    Ok(RegistryListing {
        path: path.to_path_buf(),
        base_dir,
        sources,
        fetchers,
        skipped,
    })
}

// ── OAuth Probe ───────────────────────────────────────────────────────────────

/// Whether a source's plugin file declares `[host.oauth]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OAuthDeclared {
    Yes,
    No,
    /// The plugin file could not be read or parsed; a fixed phrase that never
    /// echoes file content.
    Unknown(String),
}

/// One source and what its plugin file says about `[host.oauth]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OAuthProbe {
    pub name: String,
    pub declares: OAuthDeclared,
}

/// Which registered sources declare `[host.oauth]`, from a lenient read of
/// their plugin files.
#[derive(Debug, Clone)]
pub struct OAuthProbeReport {
    /// The registry file's directory; every relative path resolves against it.
    pub base_dir: PathBuf,
    /// Sources with valid names, enabled or not, in name order.
    pub sources: Vec<OAuthProbe>,
}

/// A plugin file seen only as far as `[host.oauth]`: unknown keys are fine and
/// nothing in the table is validated.
#[derive(Deserialize)]
struct OAuthView {
    host: Option<HostView>,
}

#[derive(Deserialize)]
struct HostView {
    oauth: Option<toml::Value>,
}

/// Reports, for every source in the registry at `path` (enabled or not),
/// whether its plugin file declares `[host.oauth]`.
///
/// The read is lenient on purpose: it parses only far enough to see whether
/// the table is there, never validates it, and touches no environment or
/// secret, so it works for a plugin whose client secret is not set yet. A
/// plugin file that cannot be read or parsed is `Unknown` with a fixed phrase.
///
/// # Errors
/// `RegistryUnreadable` or `RegistryMalformed` when the registry file cannot
/// be read or parsed.
pub fn probe_oauth_sources(path: &Path) -> Result<OAuthProbeReport, ConfigErrors> {
    let _ = path;
    todo!("RED stub")
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
    );
    // D-12: the client secret goes only into `OAuthConfig::client_secret`,
    // never into `ResolvedSecrets`, which is what the plugin's `init` receives.
    let client_secret = match &parsed.client_secret_env {
        Some(env_var) => secrets::resolve_oauth_secret(
            name,
            &parsed.path,
            &parsed.text,
            env_var,
            dotenv,
            dotenv_path,
            env,
        )
        .map_err(|problem| vec![problem]),
        None => Ok(None),
    };
    let (secrets, client_secret) = match (secrets, client_secret) {
        (Ok(secrets), Ok(client_secret)) => (secrets, client_secret),
        (secrets, client_secret) => {
            let mut problems = secrets.err().unwrap_or_default();
            problems.extend(client_secret.err().unwrap_or_default());
            return Err(problems);
        }
    };
    let oauth = parsed.oauth.map(|mut oauth| {
        oauth.client_secret = client_secret;
        oauth
    });
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
        oauth,
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
pub(crate) fn is_valid_name(name: &str) -> bool {
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

/// Checks the `[crawler]` table's `fetcher` reference the way
/// `fetcher_problem` checks a source's: it must name a fetcher defined under
/// `[fetchers]`, and that fetcher must be enabled.
fn crawler_problem(
    entry: &CrawlerEntry,
    fetchers: &BTreeMap<String, FetcherEntry>,
    registry: RegistrySource<'_>,
) -> Option<ConfigError> {
    let fetcher = &entry.fetcher;
    match fetchers.get(fetcher.get_ref()) {
        Some(defined) if defined.enabled => None,
        Some(_) => Some(ConfigError::DisabledCrawlerFetcher {
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
            Some(ConfigError::UnknownCrawlerFetcher {
                fetcher: fetcher.get_ref().clone(),
                defined,
                src: registry.named(),
                span: fetcher.span().into(),
            })
        }
    }
}

/// Builds the typed crawler from a table `crawler_problem` accepted.
fn loaded_crawler(
    entry: &CrawlerEntry,
    fetchers: &BTreeMap<String, FetcherEntry>,
    base_dir: &Path,
) -> LoadedCrawler {
    let fetcher = entry.fetcher.get_ref().clone();
    let component = fetchers
        .get(&fetcher)
        .map(|defined| base_dir.join(defined.component.get_ref()))
        .unwrap_or_default();
    LoadedCrawler {
        fetcher,
        fetcher_component: component,
        timeout: Duration::from_secs(entry.timeout_secs.get()),
        extra_hosts: entry.allowlist.clone(),
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
    /// The `[host.oauth]` table, when it had no problems.
    oauth: Option<OAuthConfig>,
    /// The `client_secret_env` of that table, resolved in pass 3.
    client_secret_env: Option<toml::Spanned<String>>,
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

    let mut invalid = host_problems(name, &display, &text, &file.host);
    let oauth_valid = match &file.host.oauth {
        Some(section) => {
            let problems = oauth_problems(name, &display, &text, section);
            let valid = problems.is_empty();
            invalid.extend(problems);
            valid
        }
        None => true,
    };

    let HostSection {
        poll_interval_secs,
        timeout_secs,
        allowlist,
        backoff,
        secrets,
        oauth,
    } = file.host;
    // A table with problems is reported through `invalid`; it never builds a
    // config, so nothing half-valid can reach the login flow.
    let (oauth, client_secret_env) = match oauth {
        Some(section) if oauth_valid => {
            let config = build_oauth(&section);
            let secret_env = config.as_ref().and(section.client_secret_env);
            (config, secret_env)
        }
        _ => (None, None),
    };
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
        oauth,
        client_secret_env,
        invalid,
    })
}

/// `Some` only when the table satisfies every rule `oauth_problems` reports
/// against; the client secret is resolved later, in pass 3.
fn build_oauth(section: &OAuthSection) -> Option<OAuthConfig> {
    let authorize_url = https_endpoint(section.authorize_url.get_ref())?;
    let token_url = https_endpoint(section.token_url.get_ref())?;
    let revocation_url = match &section.revocation_url {
        Some(value) => Some(https_endpoint(value.get_ref())?),
        None => None,
    };
    if section.client_id.get_ref().is_empty() {
        return None;
    }
    let redirect_port = section
        .redirect_port
        .as_ref()
        .map_or(DEFAULT_REDIRECT_PORT, |port| *port.get_ref());
    if redirect_port == 0 {
        return None;
    }
    Some(OAuthConfig {
        authorize_url,
        token_url,
        scopes: section
            .scopes
            .iter()
            .map(|scope| scope.get_ref().clone())
            .collect(),
        client_id: section.client_id.get_ref().clone(),
        client_secret: None,
        revocation_url,
        redirect_port,
        authorize_params: section
            .authorize_params
            .iter()
            .map(|(key, value)| (key.clone(), value.get_ref().clone()))
            .collect(),
    })
}

/// The parsed URL when `text` is an `https://` URL with a host and no
/// user-info (D-13).
fn https_endpoint(text: &str) -> Option<url::Url> {
    let url = url::Url::parse(text).ok()?;
    let plain = url.username().is_empty() && url.password().is_none();
    (url.scheme() == "https" && url.host_str().is_some() && plain).then_some(url)
}

/// `true` when `scope` is empty or contains whitespace: a joined scope list
/// splits on spaces, so one entry must be one scope.
fn scope_is_invalid(scope: &str) -> bool {
    scope.is_empty() || scope.chars().any(char::is_whitespace)
}

/// `[host.oauth]` checks. Every problem of the table is collected, in a fixed
/// order: the three endpoints, `client_id`, `redirect_port`, scopes by index,
/// `client_secret_env`, then `authorize_params` in key order. A message states
/// the rule and never repeats the value it rejects.
fn oauth_problems(
    plugin: &str,
    path: &str,
    text: &str,
    section: &OAuthSection,
) -> Vec<ConfigError> {
    let invalid = |field: String, message: &str, span: SourceSpan| ConfigError::InvalidValue {
        plugin: plugin.to_string(),
        path: path.to_string(),
        field,
        message: message.to_string(),
        src: NamedSource::new(path, text.to_string()),
        span: Some(span),
    };
    let mut problems = Vec::new();
    for (field, value) in [
        ("host.oauth.authorize_url", Some(&section.authorize_url)),
        ("host.oauth.token_url", Some(&section.token_url)),
        ("host.oauth.revocation_url", section.revocation_url.as_ref()),
    ] {
        let Some(value) = value else { continue };
        if https_endpoint(value.get_ref()).is_none() {
            problems.push(invalid(
                field.to_string(),
                "must be an https:// URL with a host and no user-info",
                value.span().into(),
            ));
        }
    }
    if section.client_id.get_ref().is_empty() {
        problems.push(invalid(
            "host.oauth.client_id".to_string(),
            "must not be empty",
            section.client_id.span().into(),
        ));
    }
    if let Some(port) = &section.redirect_port
        && *port.get_ref() == 0
    {
        problems.push(invalid(
            "host.oauth.redirect_port".to_string(),
            "must be between 1 and 65535",
            port.span().into(),
        ));
    }
    for (index, scope) in section.scopes.iter().enumerate() {
        if scope_is_invalid(scope.get_ref()) {
            problems.push(invalid(
                format!("host.oauth.scopes[{index}]"),
                "must be a non-empty scope without whitespace",
                scope.span().into(),
            ));
        }
    }
    if let Some(env_var) = &section.client_secret_env
        && env_var.get_ref().is_empty()
    {
        problems.push(invalid(
            "host.oauth.client_secret_env".to_string(),
            "must not be empty",
            env_var.span().into(),
        ));
    }
    for (key, value) in &section.authorize_params {
        if key.is_empty() {
            problems.push(invalid(
                "host.oauth.authorize_params".to_string(),
                "keys must not be empty",
                value.span().into(),
            ));
        } else if STANDARD_AUTHORIZE_PARAMS
            .iter()
            .any(|standard| key.eq_ignore_ascii_case(standard))
        {
            problems.push(invalid(
                format!("host.oauth.authorize_params.{key}"),
                "is a standard authorize parameter that the host sets itself",
                value.span().into(),
            ));
        }
    }
    problems
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

    // ── [host.oauth] (D-11, D-13) ─────────────────────────────────────────────

    /// A plugin file with `[host.oauth]` built from the given endpoint values.
    fn oauth_plugin(authorize_url: &str, token_url: &str) -> String {
        format!(
            "[host]\npoll_interval_secs = 60\n\n[host.oauth]\nauthorize_url = \"{authorize_url}\"\n\
             token_url = \"{token_url}\"\nscopes = [\"read\"]\nclient_id = \"client-1\"\n"
        )
    }

    #[test]
    fn oauth_table_loads_into_the_plugin_config() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_plugin(
            &dir,
            &format!(
                "{}\n[settings]\ngreeting = \"hi\"\n",
                oauth_plugin(
                    "https://auth.example/authorize",
                    "https://auth.example/token"
                )
            ),
        );

        let registry = load(&dir).expect("a plugin with [host.oauth] loads");
        let plugin = registry.sources["alpha"].plugin.as_ref().expect("loaded");
        let oauth = plugin.oauth.as_ref().expect("the oauth table");
        assert_eq!(
            oauth.authorize_url.as_str(),
            "https://auth.example/authorize"
        );
        assert_eq!(oauth.token_url.as_str(), "https://auth.example/token");
        assert_eq!(oauth.scopes, ["read"]);
        assert_eq!(oauth.client_id, "client-1");
        // Nothing of the table is placed under [settings].
        assert!(
            !plugin.settings_toml.contains("oauth"),
            "{}",
            plugin.settings_toml
        );
        assert!(
            !plugin.settings_toml.contains("client-1"),
            "{}",
            plugin.settings_toml
        );

        let dir = TempDir::new().expect("tempdir");
        alpha_with_plugin(&dir, "[host]\npoll_interval_secs = 60\n");
        let registry = load(&dir).expect("a plugin without [host.oauth] loads");
        let plugin = registry.sources["alpha"].plugin.as_ref().expect("loaded");
        assert!(plugin.oauth.is_none());
    }

    #[test]
    fn oauth_endpoints_must_be_https_naming_plugin_and_field() {
        let https = "https://auth.example/x";
        let cases = [
            (
                "http://auth.example/authorize",
                https,
                "host.oauth.authorize_url",
            ),
            ("not a url", https, "host.oauth.authorize_url"),
            (
                "ftp://auth.example/authorize",
                https,
                "host.oauth.authorize_url",
            ),
            (https, "http://auth.example/token", "host.oauth.token_url"),
            (https, "not a url", "host.oauth.token_url"),
            (https, "ftp://auth.example/token", "host.oauth.token_url"),
        ];
        for (authorize, token, field) in cases {
            let dir = TempDir::new().expect("tempdir");
            alpha_with_plugin(&dir, &oauth_plugin(authorize, token));

            let errors = load(&dir).expect_err("a non-https endpoint is an error");

            assert_eq!(errors.problems.len(), 1, "{authorize} {token}: {errors:?}");
            let ConfigError::InvalidValue {
                plugin,
                field: got,
                message,
                span,
                ..
            } = &errors.problems[0]
            else {
                panic!("expected InvalidValue, got {:?}", errors.problems[0]);
            };
            assert_eq!(plugin, "alpha");
            assert_eq!(got, field);
            assert!(span.is_some(), "the value is labelled");
            assert!(message.contains("https://"), "{message}");
            assert!(
                !message.contains("auth.example"),
                "no value echo: {message}"
            );
        }

        // An upper-case scheme is still https.
        let dir = TempDir::new().expect("tempdir");
        alpha_with_plugin(
            &dir,
            &oauth_plugin(
                "HTTPS://auth.example/authorize",
                "https://auth.example/token",
            ),
        );
        let registry = load(&dir).expect("an upper-case HTTPS endpoint is accepted");
        let oauth = registry.sources["alpha"]
            .plugin
            .as_ref()
            .and_then(|p| p.oauth.as_ref())
            .expect("oauth");
        assert_eq!(oauth.authorize_url.scheme(), "https");
    }

    // ── [host.oauth] remaining keys (D-04, D-11, D-13, D-14) ──────────────────

    /// A plugin file whose `[host.oauth]` has https endpoints, the given
    /// `scopes` array text and `client_id`, followed by `extra` lines (which
    /// must stay inside the table, so they come last).
    fn oauth_file(scopes: &str, extra: &str) -> String {
        format!(
            "[host]\npoll_interval_secs = 60\n\n[host.oauth]\n\
             authorize_url = \"https://auth.example/authorize\"\n\
             token_url = \"https://auth.example/token\"\nscopes = {scopes}\n\
             client_id = \"client-1\"\n{extra}"
        )
    }

    /// Loads `alpha` and returns its `[host.oauth]` config.
    fn loaded_oauth(dir: &TempDir) -> OAuthConfig {
        load(dir).expect("the plugin loads").sources["alpha"]
            .plugin
            .as_ref()
            .expect("loaded")
            .oauth
            .clone()
            .expect("the oauth table")
    }

    /// Loads `alpha` and returns `(field, message)` of every problem, each of
    /// which must be an `InvalidValue` naming `alpha` with a labelled span.
    fn invalid_problems(dir: &TempDir) -> Vec<(String, String)> {
        let errors = load(dir).expect_err("an invalid [host.oauth] is an error");
        errors
            .problems
            .iter()
            .map(|problem| match problem {
                ConfigError::InvalidValue {
                    plugin,
                    field,
                    message,
                    span,
                    ..
                } => {
                    assert_eq!(plugin, "alpha");
                    assert!(span.is_some(), "{field} is labelled");
                    (field.clone(), message.clone())
                }
                other => panic!("expected InvalidValue, got {other:?}"),
            })
            .collect()
    }

    #[test]
    fn oauth_optional_keys_load_with_defaults() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_plugin(&dir, &oauth_file("[\"read\"]", ""));

        let oauth = loaded_oauth(&dir);

        assert_eq!(oauth.redirect_port, DEFAULT_REDIRECT_PORT);
        assert_eq!(oauth.redirect_port, 17420);
        assert!(oauth.revocation_url.is_none());
        assert!(oauth.client_secret.is_none());
        assert!(oauth.authorize_params.is_empty());
    }

    #[test]
    fn oauth_all_keys_load() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_plugin(
            &dir,
            &oauth_file(
                "[\"read\", \"write\"]",
                "revocation_url = \"https://auth.example/revoke\"\nredirect_port = 8123\n\
                 authorize_params = { prompt = \"consent\", duration = \"permanent\" }\n",
            ),
        );

        let oauth = loaded_oauth(&dir);

        assert_eq!(oauth.scopes, ["read", "write"]);
        assert_eq!(
            oauth.revocation_url.as_ref().map(url::Url::as_str),
            Some("https://auth.example/revoke")
        );
        assert_eq!(oauth.redirect_port, 8123);
        assert_eq!(
            oauth.authorize_params,
            [
                ("duration".to_string(), "permanent".to_string()),
                ("prompt".to_string(), "consent".to_string()),
            ]
        );
    }

    #[test]
    fn an_unknown_oauth_key_is_rejected() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_plugin(&dir, &oauth_file("[\"read\"]", "client_secret = \"x\"\n"));

        let message = malformed_message(&dir);

        assert!(message.contains("client_secret"), "{message}");
    }

    #[test]
    fn revocation_url_must_be_https_naming_plugin_and_field() {
        for url in [
            "http://auth.example/revoke",
            "not a url",
            "ftp://auth.example/r",
        ] {
            let dir = TempDir::new().expect("tempdir");
            alpha_with_plugin(
                &dir,
                &oauth_file("[\"read\"]", &format!("revocation_url = \"{url}\"\n")),
            );

            let problems = invalid_problems(&dir);

            assert_eq!(problems.len(), 1, "{url}: {problems:?}");
            assert_eq!(problems[0].0, "host.oauth.revocation_url");
            assert!(problems[0].1.contains("https://"), "{}", problems[0].1);
            assert!(!problems[0].1.contains("auth.example"), "{}", problems[0].1);
        }
    }

    #[test]
    fn oauth_endpoints_reject_userinfo_naming_plugin_and_field() {
        let clean = "https://auth.example/x";
        for bad in [
            "https://user:pw@auth.example/x",
            "https://user@auth.example/x",
        ] {
            for field in ["authorize_url", "token_url", "revocation_url"] {
                let authorize = if field == "authorize_url" { bad } else { clean };
                let token = if field == "token_url" { bad } else { clean };
                let extra = if field == "revocation_url" {
                    format!("revocation_url = \"{bad}\"\n")
                } else {
                    String::new()
                };
                let dir = TempDir::new().expect("tempdir");
                alpha_with_plugin(&dir, &format!("{}{extra}", oauth_plugin(authorize, token)));

                let errors = load(&dir).expect_err("user-info in an endpoint is an error");

                assert_eq!(errors.problems.len(), 1, "{bad} {field}: {errors:?}");
                let ConfigError::InvalidValue {
                    plugin,
                    field: got,
                    message,
                    ..
                } = &errors.problems[0]
                else {
                    panic!("expected InvalidValue, got {:?}", errors.problems[0]);
                };
                assert_eq!(plugin, "alpha");
                assert_eq!(got, &format!("host.oauth.{field}"));
                assert!(!message.contains("pw"), "{message}");
                assert!(!message.contains("user@"), "{message}");
                assert!(!message.contains("user:"), "{message}");
                assert!(!message.contains("auth.example"), "{message}");
            }
        }
    }

    #[test]
    fn redirect_port_zero_is_rejected_naming_plugin_and_field() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_plugin(&dir, &oauth_file("[\"read\"]", "redirect_port = 0\n"));

        let problems = invalid_problems(&dir);

        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!(problems[0].0, "host.oauth.redirect_port");
        assert!(problems[0].1.contains("1 and 65535"), "{}", problems[0].1);
    }

    #[test]
    fn a_scope_with_whitespace_or_empty_is_rejected_naming_its_index() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_plugin(&dir, &oauth_file("[\"read\", \"two words\", \"\"]", ""));

        let problems = invalid_problems(&dir);

        let fields: Vec<&str> = problems.iter().map(|(field, _)| field.as_str()).collect();
        assert_eq!(fields, ["host.oauth.scopes[1]", "host.oauth.scopes[2]"]);
        assert!(
            problems
                .iter()
                .all(|(_, message)| !message.contains("two words"))
        );
    }

    #[test]
    fn authorize_params_colliding_with_a_standard_key_are_rejected_naming_plugin_and_field() {
        for key in [
            "state",
            "code_challenge",
            "code_challenge_method",
            "redirect_uri",
            "client_id",
            "response_type",
            "scope",
            "State",
        ] {
            let dir = TempDir::new().expect("tempdir");
            alpha_with_plugin(
                &dir,
                &oauth_file(
                    "[\"read\"]",
                    &format!("authorize_params = {{ \"{key}\" = \"v\" }}\n"),
                ),
            );

            let problems = invalid_problems(&dir);

            assert_eq!(problems.len(), 1, "{key}: {problems:?}");
            assert_eq!(problems[0].0, format!("host.oauth.authorize_params.{key}"));
            assert!(problems[0].1.contains("host"), "{}", problems[0].1);
        }

        let dir = TempDir::new().expect("tempdir");
        alpha_with_plugin(
            &dir,
            &oauth_file(
                "[\"read\"]",
                "authorize_params = { duration = \"permanent\" }\n",
            ),
        );
        assert_eq!(loaded_oauth(&dir).authorize_params.len(), 1);
    }

    #[test]
    fn oauth_problems_are_collected_not_first_only() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_plugin(
            &dir,
            &format!(
                "{}redirect_port = 0\n",
                oauth_plugin("http://auth.example/a", "http://auth.example/t")
            ),
        );

        let problems = invalid_problems(&dir);

        let fields: Vec<&str> = problems.iter().map(|(field, _)| field.as_str()).collect();
        assert_eq!(
            fields,
            [
                "host.oauth.authorize_url",
                "host.oauth.token_url",
                "host.oauth.redirect_port"
            ]
        );
    }

    // ── [host.oauth] client secret (D-12) ─────────────────────────────────────

    /// The value every secret test plants; no output may ever contain it.
    const SENTINEL: &str = "s3cret-sentinel";

    /// `alpha` with `client_secret_env = "STUB_CLIENT_SECRET"` and, when
    /// `with_token_alias` is set, `[host.secrets] token = "STUB_TOKEN"`.
    fn alpha_with_client_secret(dir: &TempDir, with_token_alias: bool, dotenv: Option<&str>) {
        let secrets = if with_token_alias {
            "\n[host.secrets]\ntoken = \"STUB_TOKEN\"\n"
        } else {
            ""
        };
        alpha_with_plugin(
            dir,
            &format!(
                "{}{secrets}",
                oauth_file("[\"read\"]", "client_secret_env = \"STUB_CLIENT_SECRET\"\n")
            ),
        );
        if let Some(text) = dotenv {
            write(dir, ".env", text);
        }
    }

    fn load_with(
        dir: &TempDir,
        env: &dyn Fn(&str) -> Option<String>,
    ) -> Result<LoadedRegistry, ConfigErrors> {
        load_registry_with_env(&dir.path().join("filimon.toml"), env)
    }

    fn plugin_of(registry: &LoadedRegistry) -> &PluginConfig {
        registry.sources["alpha"].plugin.as_ref().expect("loaded")
    }

    #[test]
    fn a_secret_only_plugin_reads_dotenv() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_client_secret(
            &dir,
            false,
            Some(&format!("STUB_CLIENT_SECRET={SENTINEL}\n")),
        );

        let registry = load(&dir).expect("the client secret comes from .env");

        let plugin = plugin_of(&registry);
        let secret = plugin
            .oauth
            .as_ref()
            .and_then(|oauth| oauth.client_secret.as_ref())
            .expect("a client secret");
        assert_eq!(secret.expose(), SENTINEL);
        assert!(plugin.secrets.is_empty(), "no [host.secrets] alias exists");
    }

    #[test]
    fn client_secret_never_appears_in_resolved_secrets() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_client_secret(
            &dir,
            true,
            Some(&format!(
                "STUB_TOKEN=tok-1\nSTUB_CLIENT_SECRET={SENTINEL}\n"
            )),
        );

        let registry = load(&dir).expect("both secrets resolve");

        let plugin = plugin_of(&registry);
        let keys: Vec<&str> = plugin.secrets.keys().map(String::as_str).collect();
        assert_eq!(keys, ["token"]);
        assert!(plugin.secrets.values().all(|s| s.expose() != SENTINEL));
        assert_eq!(plugin.secrets["token"].expose(), "tok-1");
        assert!(!plugin.settings_toml.contains(SENTINEL));
        let oauth = plugin.oauth.as_ref().expect("oauth");
        assert_eq!(
            oauth.client_secret.as_ref().map(Secret::expose),
            Some(SENTINEL)
        );
    }

    #[test]
    fn missing_client_secret_names_plugin_and_field() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_client_secret(&dir, false, Some("UNRELATED=1\n"));

        let errors = load(&dir).expect_err("a missing client secret is an error");

        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        let ConfigError::MissingOAuthSecret {
            plugin, env_var, ..
        } = &errors.problems[0]
        else {
            panic!("expected MissingOAuthSecret, got {:?}", errors.problems[0]);
        };
        assert_eq!(plugin, "alpha");
        assert_eq!(env_var, "STUB_CLIENT_SECRET");
        let text = errors.problems[0].to_string();
        for needle in [
            "alpha",
            "host.oauth.client_secret_env",
            "STUB_CLIENT_SECRET",
        ] {
            assert!(text.contains(needle), "{needle}: {text}");
        }
        let report = render(&errors);
        assert!(
            report.contains("fili::config::missing_oauth_secret"),
            "{report}"
        );
    }

    #[test]
    fn empty_client_secret_names_the_origin() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_client_secret(&dir, false, Some("STUB_CLIENT_SECRET=\n"));

        let errors = load(&dir).expect_err("an empty .env value is an error");

        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        let ConfigError::EmptyOAuthSecret {
            plugin,
            env_var,
            origin,
            ..
        } = &errors.problems[0]
        else {
            panic!("expected EmptyOAuthSecret, got {:?}", errors.problems[0]);
        };
        assert_eq!(plugin, "alpha");
        assert_eq!(env_var, "STUB_CLIENT_SECRET");
        assert_eq!(origin, &dir.path().join(".env").display().to_string());
        let text = errors.problems[0].to_string();
        assert!(text.contains("host.oauth.client_secret_env"), "{text}");
        let report = render(&errors);
        assert!(
            report.contains("fili::config::empty_oauth_secret"),
            "{report}"
        );

        // An empty process value names the process environment.
        let env = |key: &str| (key == "STUB_CLIENT_SECRET").then(String::new);
        let errors = load_with(&dir, &env).expect_err("an empty process value is an error");
        assert!(
            matches!(
                &errors.problems[0],
                ConfigError::EmptyOAuthSecret { origin, .. } if origin == "the process environment"
            ),
            "{errors:?}"
        );
    }

    #[test]
    fn process_env_wins_over_dotenv_even_when_empty() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_client_secret(
            &dir,
            false,
            Some(&format!("STUB_CLIENT_SECRET={SENTINEL}\n")),
        );

        let empty = |key: &str| (key == "STUB_CLIENT_SECRET").then(String::new);
        let errors = load_with(&dir, &empty).expect_err("an empty process value wins");
        assert!(
            matches!(
                &errors.problems[0],
                ConfigError::EmptyOAuthSecret { origin, .. } if origin == "the process environment"
            ),
            "{errors:?}"
        );

        let from_process =
            |key: &str| (key == "STUB_CLIENT_SECRET").then(|| "from-env".to_string());
        let registry = load_with(&dir, &from_process).expect("a non-empty process value wins");
        let secret = plugin_of(&registry)
            .oauth
            .as_ref()
            .and_then(|oauth| oauth.client_secret.as_ref())
            .expect("a client secret");
        assert_eq!(secret.expose(), "from-env");
    }

    #[test]
    fn an_unusable_dotenv_does_not_add_a_missing_client_secret_problem() {
        let dir = TempDir::new().expect("tempdir");
        alpha_with_client_secret(&dir, false, Some("STUB_CLIENT_SECRET=has a space\n"));

        let errors = load(&dir).expect_err("an invalid .env is an error");

        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        assert!(
            matches!(
                &errors.problems[0],
                ConfigError::DotenvInvalid { line: Some(1), .. }
            ),
            "{errors:?}"
        );
    }

    #[test]
    fn scoped_load_plugin_keeps_oauth_secret_problems_for_that_plugin() {
        let dir = TempDir::new().expect("tempdir");
        write(
            &dir,
            "filimon.toml",
            "[sources.a]\ncomponent = \"a.wasm\"\nconfig = \"a.toml\"\n\n\
             [sources.b]\ncomponent = \"b.wasm\"\nconfig = \"b.toml\"\n",
        );
        for (name, var) in [("a", "A_CLIENT_SECRET"), ("b", "B_CLIENT_SECRET")] {
            write(
                &dir,
                &format!("{name}.toml"),
                &oauth_file("[\"read\"]", &format!("client_secret_env = \"{var}\"\n")),
            );
        }

        let errors = load_plugin_with_env(&dir.path().join("filimon.toml"), "a", &no_env)
            .expect_err("a lacks its client secret");

        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        assert!(
            matches!(
                &errors.problems[0],
                ConfigError::MissingOAuthSecret { plugin, env_var, .. }
                    if plugin == "a" && env_var == "A_CLIENT_SECRET"
            ),
            "{errors:?}"
        );
        assert_eq!(errors.problems[0].plugin(), Some("a"));

        // The unscoped load reports both plugins.
        let all = load(&dir).expect_err("both lack their client secret");
        assert_eq!(all.problems.len(), 2, "{all:?}");
    }

    #[test]
    fn oauth_secret_value_never_in_debug_or_errors() {
        // Success: the secret is held, but no Debug text carries it.
        let dir = TempDir::new().expect("tempdir");
        alpha_with_client_secret(
            &dir,
            true,
            Some(&format!(
                "STUB_TOKEN=tok-1\nSTUB_CLIENT_SECRET={SENTINEL}\n"
            )),
        );
        let registry = load(&dir).expect("loads");
        let debug = format!("{registry:?}");
        assert!(!debug.contains(SENTINEL), "{debug}");
        assert!(debug.contains("Secret(***)"), "{debug}");

        // Failures that sit beside a present secret: a missing alias, an
        // empty alias, and an invalid .env.
        let cases = [
            format!("STUB_CLIENT_SECRET={SENTINEL}\n"),
            format!("STUB_TOKEN=\nSTUB_CLIENT_SECRET={SENTINEL}\n"),
            format!("STUB_CLIENT_SECRET={SENTINEL}\nBROKEN=has a space\n"),
        ];
        for dotenv in cases {
            let dir = TempDir::new().expect("tempdir");
            alpha_with_client_secret(&dir, true, Some(&dotenv));

            let errors = load(&dir).expect_err("a problem sits beside the client secret");

            let report = render(&errors);
            for text in [
                report,
                format!("{errors:?}"),
                errors.to_string(),
                format!("{:?}", miette::Report::new(errors)),
            ] {
                assert!(!text.contains(SENTINEL), "leaked: {text}");
            }
        }

        // The problems of the secret itself carry names only.
        for dotenv in ["", "STUB_CLIENT_SECRET=\n"] {
            let dir = TempDir::new().expect("tempdir");
            alpha_with_client_secret(&dir, false, Some(dotenv));
            let env = |key: &str| (key == "OTHER").then(|| SENTINEL.to_string());
            let errors = load_with(&dir, &env).expect_err("a client secret problem");
            assert!(!render(&errors).contains(SENTINEL));
        }
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

    // ── Plugin-scoped loading ─────────────────────────────────────────────────

    /// A registry with a valid `good` source, a `bad` source whose plugin TOML
    /// is malformed and a source called `Bad Name`.
    fn good_and_bad_registry(dir: &TempDir) {
        write(
            dir,
            "filimon.toml",
            r#"
[sources.good]
component = "good.wasm"
config = "good.toml"

[sources.bad]
component = "bad.wasm"
config = "bad.toml"

[sources."Bad Name"]
component = "x.wasm"
config = "x.toml"
"#,
        );
        write(dir, "good.toml", "[host]\npoll_interval_secs = 60\n");
        write(dir, "bad.toml", "[host\npoll_interval_secs = ");
    }

    fn scoped(dir: &TempDir, name: &str) -> Result<LoadedRegistry, ConfigErrors> {
        load_plugin_with_env(&dir.path().join("filimon.toml"), name, &no_env)
    }

    #[test]
    fn plugin_only_load_ignores_other_plugins_problems() {
        let dir = TempDir::new().expect("tempdir");
        good_and_bad_registry(&dir);

        let registry = scoped(&dir, "good").expect("good is valid on its own");

        let names: Vec<&str> = registry.sources.keys().map(String::as_str).collect();
        assert_eq!(names, ["good"]);
        assert!(registry.sources["good"].plugin.is_some());
        // The unscoped load still sees every problem.
        assert!(load(&dir).is_err());
    }

    #[test]
    fn plugin_only_load_reports_that_plugins_problems() {
        let dir = TempDir::new().expect("tempdir");
        good_and_bad_registry(&dir);

        let errors = scoped(&dir, "bad").expect_err("bad has a malformed plugin file");

        assert!(!errors.problems.is_empty());
        assert!(
            errors
                .problems
                .iter()
                .all(|problem| problem.plugin() == Some("bad")),
            "{errors:?}"
        );
        assert!(
            matches!(
                &errors.problems[0],
                ConfigError::PluginFileMalformed { plugin, .. } if plugin == "bad"
            ),
            "{errors:?}"
        );
    }

    #[test]
    fn config_error_plugin_names_every_per_plugin_variant() {
        // The crawler's two variants answer `crawler` (checked below).
        let src = || NamedSource::new("x.toml", String::new());
        let span = || SourceSpan::from(0..0);
        let per_plugin = [
            ConfigError::PluginFileMissing {
                plugin: "p".into(),
                path: "x".into(),
                reason: "r".into(),
                src: src(),
                span: span(),
            },
            ConfigError::PluginFileMalformed {
                plugin: "p".into(),
                path: "x".into(),
                message: "m".into(),
                src: src(),
                span: None,
            },
            ConfigError::InvalidValue {
                plugin: "p".into(),
                path: "x".into(),
                field: "f".into(),
                message: "m".into(),
                src: src(),
                span: None,
            },
            ConfigError::MissingSecret {
                plugin: "p".into(),
                alias: "a".into(),
                env_var: "E".into(),
                dotenv: "d".into(),
                src: src(),
                span: span(),
            },
            ConfigError::EmptySecret {
                plugin: "p".into(),
                alias: "a".into(),
                env_var: "E".into(),
                origin: "o".into(),
                fix: "f".into(),
                src: src(),
                span: span(),
            },
            ConfigError::UnknownFetcher {
                plugin: "p".into(),
                fetcher: "f".into(),
                defined: "none".into(),
                src: src(),
                span: span(),
            },
            ConfigError::DisabledFetcher {
                plugin: "p".into(),
                fetcher: "f".into(),
                src: src(),
                span: span(),
            },
            ConfigError::InvalidName {
                plugin: "p".into(),
                table: "sources".into(),
            },
            ConfigError::DuplicateName { plugin: "p".into() },
        ];
        for problem in &per_plugin {
            assert_eq!(problem.plugin(), Some("p"), "{problem:?}");
        }

        let crawler = [
            ConfigError::UnknownCrawlerFetcher {
                fetcher: "f".into(),
                defined: "none".into(),
                src: src(),
                span: span(),
            },
            ConfigError::DisabledCrawlerFetcher {
                fetcher: "f".into(),
                src: src(),
                span: span(),
            },
        ];
        for problem in &crawler {
            assert_eq!(problem.plugin(), Some("crawler"), "{problem:?}");
        }

        let registry_level = [
            ConfigError::RegistryUnreadable {
                path: "x".into(),
                reason: "r".into(),
            },
            ConfigError::RegistryMalformed {
                path: "x".into(),
                message: "m".into(),
                src: src(),
                span: None,
            },
            ConfigError::DotenvInvalid {
                path: "x".into(),
                line: Some(1),
                reason: "r".into(),
            },
        ];
        for problem in &registry_level {
            assert_eq!(problem.plugin(), None, "{problem:?}");
        }
    }

    #[test]
    fn config_error_plugin_names_the_oauth_secret_variants() {
        let src = || NamedSource::new("x.toml", String::new());
        let span = || SourceSpan::from(0..0);
        let problems = [
            ConfigError::MissingOAuthSecret {
                plugin: "p".into(),
                env_var: "E".into(),
                dotenv: "d".into(),
                src: src(),
                span: span(),
            },
            ConfigError::EmptyOAuthSecret {
                plugin: "p".into(),
                env_var: "E".into(),
                origin: "o".into(),
                fix: "f".into(),
                src: src(),
                span: span(),
            },
        ];
        for problem in &problems {
            assert_eq!(problem.plugin(), Some("p"), "{problem:?}");
        }
    }

    /// `plain` declares no secret; `needy` declares one; the `.env` is malformed.
    fn plain_and_needy_registry(dir: &TempDir) {
        write(
            dir,
            "filimon.toml",
            r#"
[sources.plain]
component = "plain.wasm"
config = "plain.toml"

[sources.needy]
component = "needy.wasm"
config = "needy.toml"
"#,
        );
        write(dir, "plain.toml", "[host]\npoll_interval_secs = 60\n");
        write(
            dir,
            "needy.toml",
            "[host]\npoll_interval_secs = 60\n\n[host.secrets]\ntoken = \"NEEDY_TOKEN\"\n",
        );
        write(dir, ".env", "NEEDY_TOKEN=has a space\n");
    }

    #[test]
    fn plugin_only_load_reads_dotenv_only_when_the_plugin_declares_secrets() {
        let dir = TempDir::new().expect("tempdir");
        plain_and_needy_registry(&dir);

        scoped(&dir, "plain").expect("a malformed .env does not touch a plugin without secrets");

        let errors = scoped(&dir, "needy").expect_err("the plugin that needs the .env fails");
        assert!(
            errors
                .problems
                .iter()
                .any(|problem| matches!(problem, ConfigError::DotenvInvalid { line: Some(1), .. })),
            "{errors:?}"
        );
        assert!(!format!("{errors:?}").contains("has a space"));
    }

    #[test]
    fn plugin_only_load_of_a_disabled_source_skips_its_config() {
        let dir = TempDir::new().expect("tempdir");
        write(
            &dir,
            "filimon.toml",
            "[sources.off]\ncomponent = \"off.wasm\"\nconfig = \"missing.toml\"\nenabled = false\n",
        );

        let registry = scoped(&dir, "off").expect("a disabled source's config is not read");

        let off = &registry.sources["off"];
        assert!(!off.enabled);
        assert!(off.plugin.is_none());
    }

    #[test]
    fn plugin_only_load_keeps_a_duplicate_name_problem_for_that_name() {
        let dir = TempDir::new().expect("tempdir");
        write(
            &dir,
            "filimon.toml",
            r#"
[sources.shared]
component = "shared.wasm"
config = "shared.toml"

[sources.other]
component = "other.wasm"
config = "other.toml"

[fetchers.shared]
component = "shared-fetcher.wasm"

[fetchers.other]
component = "other-fetcher.wasm"
"#,
        );
        write(&dir, "shared.toml", "[host]\npoll_interval_secs = 60\n");
        write(&dir, "other.toml", "[host]\npoll_interval_secs = 60\n");

        let errors = scoped(&dir, "shared").expect_err("the duplicate concerns `shared`");

        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        assert!(
            matches!(
                &errors.problems[0],
                ConfigError::DuplicateName { plugin } if plugin == "shared"
            ),
            "{errors:?}"
        );
    }

    // ── [crawler] table (FETCH-04) ────────────────────────────────────────────

    const HTTP_FETCHER: &str = "[fetchers.http]\ncomponent = \"http.wasm\"\n";

    fn crawler_registry(dir: &TempDir, text: &str) {
        write(dir, "filimon.toml", text);
    }

    #[test]
    fn crawler_table_loads_with_the_default_timeout_and_no_extra_hosts() {
        let dir = TempDir::new().expect("tempdir");
        crawler_registry(
            &dir,
            &format!("{HTTP_FETCHER}\n[crawler]\nfetcher = \"http\"\n"),
        );

        let registry = load(&dir).expect("registry loads");

        let crawler = registry.crawler.as_ref().expect("the crawler is loaded");
        assert_eq!(crawler.fetcher, "http");
        assert_eq!(crawler.timeout, Duration::from_secs(15));
        assert!(crawler.extra_hosts.is_empty());
        assert_eq!(crawler.fetcher_component, dir.path().join("http.wasm"));
    }

    #[test]
    fn crawler_table_reads_timeout_and_extra_hosts() {
        let dir = TempDir::new().expect("tempdir");
        crawler_registry(
            &dir,
            &format!(
                "{HTTP_FETCHER}\n[crawler]\nfetcher = \"http\"\ntimeout_secs = 3\nallowlist = [\"www.example.com\"]\n"
            ),
        );

        let registry = load(&dir).expect("registry loads");

        let crawler = registry.crawler.as_ref().expect("the crawler is loaded");
        assert_eq!(crawler.timeout, Duration::from_secs(3));
        assert_eq!(crawler.extra_hosts, vec!["www.example.com".to_string()]);
    }

    #[test]
    fn a_registry_without_a_crawler_table_has_none() {
        let dir = TempDir::new().expect("tempdir");
        crawler_registry(&dir, HTTP_FETCHER);

        let registry = load(&dir).expect("registry loads");

        assert!(registry.crawler.is_none());
        let crawler = load_crawler(&dir.path().join("filimon.toml")).expect("crawler loads");
        assert!(crawler.is_none());
    }

    #[test]
    fn a_crawler_naming_an_unknown_fetcher_is_reported_with_the_defined_names() {
        let dir = TempDir::new().expect("tempdir");
        crawler_registry(
            &dir,
            &format!("{HTTP_FETCHER}\n[crawler]\nfetcher = \"htp\"\n"),
        );

        let errors = load(&dir).expect_err("an unknown crawler fetcher is an error");

        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        let ConfigError::UnknownCrawlerFetcher {
            fetcher, defined, ..
        } = &errors.problems[0]
        else {
            panic!("expected UnknownCrawlerFetcher, got {errors:?}");
        };
        assert_eq!(fetcher, "htp");
        assert_eq!(defined, "http");
        assert_contains(&render(&errors), &["crawler.fetcher", "htp", "http"]);

        // With no fetcher defined at all the help says `none`.
        let dir = TempDir::new().expect("tempdir");
        crawler_registry(&dir, "[crawler]\nfetcher = \"http\"\n");
        let errors = load(&dir).expect_err("an unknown crawler fetcher is an error");
        assert!(
            matches!(
                &errors.problems[0],
                ConfigError::UnknownCrawlerFetcher { defined, .. } if defined == "none"
            ),
            "{errors:?}"
        );
    }

    #[test]
    fn a_crawler_naming_a_disabled_fetcher_is_reported() {
        let dir = TempDir::new().expect("tempdir");
        crawler_registry(
            &dir,
            "[fetchers.http]\ncomponent = \"http.wasm\"\nenabled = false\n\n[crawler]\nfetcher = \"http\"\n",
        );

        let errors = load(&dir).expect_err("a disabled crawler fetcher is an error");

        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        assert!(
            matches!(
                &errors.problems[0],
                ConfigError::DisabledCrawlerFetcher { fetcher, .. } if fetcher == "http"
            ),
            "{errors:?}"
        );
        assert_contains(&render(&errors), &["crawler.fetcher", "disabled"]);
    }

    #[test]
    fn a_crawler_zero_timeout_is_rejected() {
        let dir = TempDir::new().expect("tempdir");
        crawler_registry(
            &dir,
            &format!("{HTTP_FETCHER}\n[crawler]\nfetcher = \"http\"\ntimeout_secs = 0\n"),
        );

        let errors = load(&dir).expect_err("a zero timeout is an error");

        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        assert!(
            matches!(&errors.problems[0], ConfigError::RegistryMalformed { message, .. } if message.contains("nonzero")),
            "{errors:?}"
        );
    }

    #[test]
    fn a_crawler_unknown_key_is_rejected() {
        let dir = TempDir::new().expect("tempdir");
        crawler_registry(
            &dir,
            &format!("{HTTP_FETCHER}\n[crawler]\nfetcher = \"http\"\ntimeout = 3\n"),
        );

        let errors = load(&dir).expect_err("an unknown crawler key is an error");

        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        assert!(
            matches!(&errors.problems[0], ConfigError::RegistryMalformed { message, .. } if message.contains("timeout")),
            "{errors:?}"
        );
    }

    #[test]
    fn load_crawler_ignores_sources_and_secrets() {
        let dir = TempDir::new().expect("tempdir");
        crawler_registry(
            &dir,
            &format!(
                "{HTTP_FETCHER}\n[sources.gone]\ncomponent = \"gone.wasm\"\nconfig = \"gone.toml\"\n\n[sources.needy]\ncomponent = \"needy.wasm\"\nconfig = \"needy.toml\"\n\n[crawler]\nfetcher = \"http\"\n"
            ),
        );
        write(
            &dir,
            "needy.toml",
            "[host]\npoll_interval_secs = 60\n\n[host.secrets]\ntoken = \"NEEDY_TOKEN_UNSET\"\n",
        );

        let crawler = load_crawler(&dir.path().join("filimon.toml"))
            .expect("sources do not stop the crawler")
            .expect("the crawler is configured");

        assert_eq!(crawler.fetcher, "http");
        assert!(load(&dir).is_err(), "the strict loader still sees them");
    }

    #[test]
    fn a_scoped_plugin_load_does_not_report_crawler_problems() {
        let dir = TempDir::new().expect("tempdir");
        crawler_registry(
            &dir,
            "[sources.good]\ncomponent = \"good.wasm\"\nconfig = \"good.toml\"\n\n[crawler]\nfetcher = \"nope\"\n",
        );
        write(&dir, "good.toml", "[host]\npoll_interval_secs = 60\n");

        let registry = scoped(&dir, "good").expect("the crawler's problem is not good's");

        assert!(registry.sources["good"].plugin.is_some());
        assert!(registry.crawler.is_none());
        assert!(load(&dir).is_err(), "the strict loader reports it");
    }

    // ── probe_oauth_sources ──────────────────────────────────────────────────

    const OAUTH_PLUGIN: &str = "[host]\npoll_interval_secs = 60\n\n[host.oauth]\nauthorize_url = \"https://auth.example/authorize\"\ntoken_url = \"https://auth.example/token\"\nclient_id = \"c\"\nclient_secret_env = \"PROBE_SECRET_UNSET_EVERYWHERE\"\n";

    fn source(name: &str, enabled: bool) -> String {
        format!(
            "[sources.{name}]\ncomponent = \"{name}.wasm\"\nconfig = \"{name}.toml\"\nenabled = {enabled}\n\n"
        )
    }

    #[test]
    fn probe_oauth_sources_reports_declared_undeclared_and_unreadable() {
        let dir = TempDir::new().expect("tempdir");
        let mut registry = String::new();
        for name in ["declared", "plain", "missing", "broken"] {
            registry.push_str(&source(name, true));
        }
        // A disabled source is still probed, and a bad name is not listed.
        registry.push_str(&source("off", false));
        registry.push_str(&source("Bad_Name", true));
        write(&dir, "filimon.toml", &registry);
        write(&dir, "declared.toml", OAUTH_PLUGIN);
        write(&dir, "plain.toml", "[host]\npoll_interval_secs = 60\n");
        write(&dir, "broken.toml", "this is = = not toml SECRET-FILE-CONTENT\n");
        write(&dir, "off.toml", OAUTH_PLUGIN);

        let report = probe_oauth_sources(&dir.path().join("filimon.toml")).expect("probe");

        assert_eq!(report.base_dir, dir.path());
        let seen: Vec<(&str, &OAuthDeclared)> = report
            .sources
            .iter()
            .map(|probe| (probe.name.as_str(), &probe.declares))
            .collect();
        assert_eq!(seen.len(), 5, "{seen:?}");
        let find = |name: &str| {
            report
                .sources
                .iter()
                .find(|probe| probe.name == name)
                .map(|probe| probe.declares.clone())
        };
        assert_eq!(find("declared"), Some(OAuthDeclared::Yes));
        assert_eq!(find("plain"), Some(OAuthDeclared::No));
        assert_eq!(find("off"), Some(OAuthDeclared::Yes));
        assert_eq!(find("Bad_Name"), None);
        let Some(OAuthDeclared::Unknown(missing)) = find("missing") else {
            panic!("a missing file is unknown");
        };
        assert_eq!(missing, "the plugin file cannot be read");
        let Some(OAuthDeclared::Unknown(broken)) = find("broken") else {
            panic!("a broken file is unknown");
        };
        assert_eq!(broken, "the plugin file is not valid TOML");
        assert!(!broken.contains("SECRET-FILE-CONTENT"));
    }

    #[test]
    fn probe_does_not_read_dotenv_or_resolve_secrets() {
        let dir = TempDir::new().expect("tempdir");
        write(&dir, "filimon.toml", &source("declared", true));
        write(&dir, "declared.toml", OAUTH_PLUGIN);
        assert!(!dir.path().join(".env").exists());

        let report = probe_oauth_sources(&dir.path().join("filimon.toml"))
            .expect("an unset secret does not stop the probe");

        assert_eq!(report.sources.len(), 1);
        assert_eq!(report.sources[0].declares, OAuthDeclared::Yes);
        assert!(
            load(&dir).is_err(),
            "the strict loader does need that secret"
        );
    }
}
