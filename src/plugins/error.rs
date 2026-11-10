// ─────────────────────────────────────────────────────────────────────────────
// plugins/error.rs - Plugin configuration diagnostics
//
// Every problem found while loading the registry, a plugin TOML or `.env` is a
// `ConfigError`. Loading collects them all into one `ConfigErrors`, whose
// `#[related]` list renders each problem as its own miette diagnostic.
//
// Error text carries plugin names, file paths, field paths, aliases and env
// var NAMES only. Secret values and raw `.env` lines never appear here.
// ─────────────────────────────────────────────────────────────────────────────

use miette::{Diagnostic, NamedSource, SourceSpan};

/// One problem in the plugin configuration.
#[derive(Debug, thiserror::Error, Diagnostic)]
pub enum ConfigError {
    /// The registry file could not be read. Loading stops here.
    #[error("cannot read plugin registry `{path}`: {reason}")]
    #[diagnostic(
        code(fili::config::registry_unreadable),
        help("pass --registry <path> or set FILI_REGISTRY")
    )]
    RegistryUnreadable { path: String, reason: String },

    /// The registry file is not valid registry TOML. Loading stops here.
    #[error("plugin registry `{path}`: {message}")]
    #[diagnostic(code(fili::config::registry_malformed))]
    RegistryMalformed {
        path: String,
        message: String,
        #[source_code]
        src: NamedSource<String>,
        #[label("here")]
        span: Option<SourceSpan>,
    },

    /// An enabled source's plugin TOML could not be read.
    #[error("`sources.{plugin}.config`: cannot read `{path}`: {reason}")]
    #[diagnostic(code(fili::config::plugin_file_missing))]
    PluginFileMissing {
        plugin: String,
        path: String,
        reason: String,
        #[source_code]
        src: NamedSource<String>,
        #[label("referenced here")]
        span: SourceSpan,
    },

    /// An enabled source's plugin TOML is not valid plugin TOML.
    #[error("plugin `{plugin}` (`{path}`): {message}")]
    #[diagnostic(code(fili::config::plugin_file_malformed))]
    PluginFileMalformed {
        plugin: String,
        path: String,
        message: String,
        #[source_code]
        src: NamedSource<String>,
        #[label("here")]
        span: Option<SourceSpan>,
    },

    /// A plugin file parses, but a `[host]` value is out of range.
    #[error("plugin `{plugin}` (`{path}`): `{field}`: {message}")]
    #[diagnostic(code(fili::config::invalid_value))]
    InvalidValue {
        plugin: String,
        path: String,
        field: String,
        message: String,
        #[source_code]
        src: NamedSource<String>,
        #[label("invalid value")]
        span: Option<SourceSpan>,
    },

    /// A `[host.secrets]` alias names an env var that is set nowhere.
    #[error(
        "plugin `{plugin}`: `host.secrets.{alias}` needs env var `{env_var}`, which is not set in the process environment or {dotenv}"
    )]
    #[diagnostic(
        code(fili::config::missing_secret),
        help("add `{env_var}=...` to {dotenv}")
    )]
    MissingSecret {
        plugin: String,
        alias: String,
        env_var: String,
        dotenv: String,
        #[source_code]
        src: NamedSource<String>,
        #[label("declared here")]
        span: SourceSpan,
    },

    /// A `[host.secrets]` alias names an env var whose effective value is empty.
    ///
    /// `origin` is the source that supplied the empty value: `the process
    /// environment` (which wins over `.env` even when empty, D-12) or the
    /// `.env` path. `fix` is the help text that works for that source.
    #[error(
        "plugin `{plugin}`: `host.secrets.{alias}`: env var `{env_var}` is set but empty in {origin}"
    )]
    #[diagnostic(code(fili::config::empty_secret))]
    EmptySecret {
        plugin: String,
        alias: String,
        env_var: String,
        origin: String,
        #[help]
        fix: String,
        #[source_code]
        src: NamedSource<String>,
        #[label("declared here")]
        span: SourceSpan,
    },

    /// An enabled source's `fetcher` names no `[fetchers.<name>]` table.
    #[error(
        "`sources.{plugin}.fetcher` references fetcher `{fetcher}`, which is not defined under [fetchers]"
    )]
    #[diagnostic(
        code(fili::config::unknown_fetcher),
        help("defined fetchers: {defined}")
    )]
    UnknownFetcher {
        plugin: String,
        fetcher: String,
        /// Comma-separated defined fetcher names, or `none`.
        defined: String,
        #[source_code]
        src: NamedSource<String>,
        #[label("not defined under [fetchers]")]
        span: SourceSpan,
    },

    /// An enabled source's `fetcher` names a fetcher with `enabled = false`.
    #[error("`sources.{plugin}.fetcher` references fetcher `{fetcher}`, which is disabled")]
    #[diagnostic(code(fili::config::disabled_fetcher))]
    DisabledFetcher {
        plugin: String,
        fetcher: String,
        #[source_code]
        src: NamedSource<String>,
        #[label("this fetcher has enabled = false")]
        span: SourceSpan,
    },

    /// A plugin name uses characters outside `[a-z0-9_-]`, or is empty.
    #[error("`{table}.{plugin}`: plugin names may only contain a-z, 0-9, '_' and '-'")]
    #[diagnostic(code(fili::config::invalid_name))]
    InvalidName { plugin: String, table: String },

    /// The same name appears under both `[sources]` and `[fetchers]`.
    #[error(
        "`{plugin}` is defined under both [sources] and [fetchers]; plugin names must be unique"
    )]
    #[diagnostic(code(fili::config::duplicate_name))]
    DuplicateName { plugin: String },

    /// The `.env` file exists but cannot be used. Never carries line content.
    #[error(
        "cannot use `{path}`{}: {reason}",
        .line.map_or_else(String::new, |n| format!(" (line {n})"))
    )]
    #[diagnostic(code(fili::config::dotenv_invalid))]
    DotenvInvalid {
        path: String,
        line: Option<usize>,
        reason: String,
    },
}

impl ConfigError {
    /// The plugin this problem belongs to, or `None` for a problem that
    /// concerns the registry or the `.env` as a whole.
    #[must_use]
    pub fn plugin(&self) -> Option<&str> {
        match self {
            Self::PluginFileMissing { plugin, .. }
            | Self::PluginFileMalformed { plugin, .. }
            | Self::InvalidValue { plugin, .. }
            | Self::MissingSecret { plugin, .. }
            | Self::EmptySecret { plugin, .. }
            | Self::UnknownFetcher { plugin, .. }
            | Self::DisabledFetcher { plugin, .. }
            | Self::InvalidName { plugin, .. }
            | Self::DuplicateName { plugin } => Some(plugin),
            Self::RegistryUnreadable { .. }
            | Self::RegistryMalformed { .. }
            | Self::DotenvInvalid { .. } => None,
        }
    }
}

/// Every problem found in one load of the plugin configuration.
///
/// Deliberately carries no diagnostic code, so each `fili::config::` code in a
/// rendered report corresponds to exactly one problem.
#[derive(Debug, thiserror::Error, Diagnostic)]
#[error("{} problem(s) found in plugin configuration", problems.len())]
pub struct ConfigErrors {
    #[related]
    pub problems: Vec<ConfigError>,
}
