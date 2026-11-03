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
    #[error("plugin `{plugin}`: `host.secrets.{alias}`: env var `{env_var}` is set but empty")]
    #[diagnostic(
        code(fili::config::empty_secret),
        help("give `{env_var}` a value in the process environment or the .env file")
    )]
    EmptySecret {
        plugin: String,
        alias: String,
        env_var: String,
        #[source_code]
        src: NamedSource<String>,
        #[label("declared here")]
        span: SourceSpan,
    },

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
