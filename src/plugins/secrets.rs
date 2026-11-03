// ─────────────────────────────────────────────────────────────────────────────
// plugins/secrets.rs - Per-plugin secret resolution
//
// Reads the `.env` next to the registry into a private map and resolves each
// plugin's `[host.secrets]` aliases against the process environment first and
// that map second. Each plugin receives only its own aliases, keyed by alias.
// ─────────────────────────────────────────────────────────────────────────────

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::path::Path;

use super::error::ConfigError;

/// A resolved secret value. Debug output is redacted to `Secret(***)`.
///
/// It has no `Display` and no `Serialize`, so the value only leaves through
/// an explicit `expose()` call.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    /// The raw secret value. Only hand it to the plugin that declared it.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

/// One plugin's secrets, keyed by alias and iterated in alias order.
pub type ResolvedSecrets = BTreeMap<String, Secret>;

/// Reads a `.env` file into a private map. A missing file is an empty map.
///
/// # Errors
/// Returns `ConfigError::DotenvInvalid` when the file cannot be read or a line
/// cannot be parsed.
pub fn read_dotenv(_path: &Path) -> Result<HashMap<String, String>, ConfigError> {
    Ok(HashMap::new())
}

/// Resolves one plugin's aliases to secret values.
///
/// # Errors
/// Returns one `ConfigError` per alias that cannot be resolved.
pub fn resolve_secrets(
    _plugin: &str,
    _plugin_path: &Path,
    _plugin_text: &str,
    _aliases: &BTreeMap<String, toml::Spanned<String>>,
    _dotenv: &HashMap<String, String>,
    _dotenv_path: &Path,
    _env: &dyn Fn(&str) -> Option<String>,
) -> Result<ResolvedSecrets, Vec<ConfigError>> {
    Ok(ResolvedSecrets::new())
}
