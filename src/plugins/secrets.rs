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

use miette::NamedSource;

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

// ── .env Reading ──────────────────────────────────────────────────────────────

/// Reads a `.env` file into a private map. A missing file is an empty map.
///
/// The file is read once and parsed by dotenvy from that in-memory text. The
/// process environment is never modified: values stay in the returned map.
///
/// # Errors
/// Returns `ConfigError::DotenvInvalid` when the file cannot be read or a line
/// cannot be parsed. The error carries the line number, never the line text,
/// because the text can be or contain a secret value.
#[allow(
    clippy::result_large_err,
    reason = "cold one-shot config load; ConfigError carries its diagnostic source"
)]
pub fn read_dotenv(path: &Path) -> Result<HashMap<String, String>, ConfigError> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => return Err(dotenv_invalid(path, None, e.to_string())),
    };

    let mut values = HashMap::new();
    for item in dotenvy::from_read_iter(text.as_bytes()) {
        match item {
            Ok((key, value)) => {
                values.insert(key, value);
            }
            // dotenvy's own Display and Debug echo the offending text, so the
            // error is never formatted; only a line number is derived from it.
            Err(dotenvy::Error::LineParse(offending, _)) => {
                return Err(dotenv_invalid(
                    path,
                    line_containing(&text, &offending),
                    "malformed line".to_string(),
                ));
            }
            Err(dotenvy::Error::Io(e)) => {
                return Err(dotenv_invalid(path, None, e.to_string()));
            }
            Err(_) => {
                return Err(dotenv_invalid(path, None, "unreadable .env".to_string()));
            }
        }
    }
    Ok(values)
}

fn dotenv_invalid(path: &Path, line: Option<usize>, reason: String) -> ConfigError {
    ConfigError::DotenvInvalid {
        path: path.display().to_string(),
        line,
        reason,
    }
}

/// The 1-based number of the first line of `text` that contains the first
/// line of `offending`. dotenvy reports either the whole line or only its
/// value part, and a multi-line value spans several lines.
fn line_containing(text: &str, offending: &str) -> Option<usize> {
    let needle = offending.lines().next().unwrap_or_default().trim();
    if needle.is_empty() {
        return None;
    }
    text.lines()
        .position(|line| line.contains(needle))
        .map(|index| index + 1)
}

// ── Alias Resolution ──────────────────────────────────────────────────────────

/// Resolves one plugin's aliases to secret values.
///
/// Each alias's env var is looked up through `env` (the process environment
/// in production) first and the `.env` map second. Lookup is exact and
/// case-sensitive. Only the aliases in `aliases` are resolved, so a plugin
/// never sees another plugin's secrets or unrelated `.env` keys.
///
/// # Errors
/// Returns one `ConfigError` per alias that cannot be resolved, labelled at
/// that alias's env var name in the plugin file.
pub fn resolve_secrets(
    plugin: &str,
    plugin_path: &Path,
    plugin_text: &str,
    aliases: &BTreeMap<String, toml::Spanned<String>>,
    dotenv: &HashMap<String, String>,
    dotenv_path: &Path,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<ResolvedSecrets, Vec<ConfigError>> {
    let mut resolved = ResolvedSecrets::new();
    let mut problems = Vec::new();

    for (alias, env_var) in aliases {
        let name = env_var.get_ref();
        match env(name).or_else(|| dotenv.get(name).cloned()) {
            Some(value) => {
                resolved.insert(alias.clone(), Secret(value));
            }
            None => problems.push(ConfigError::MissingSecret {
                plugin: plugin.to_string(),
                alias: alias.clone(),
                env_var: name.clone(),
                dotenv: dotenv_path.display().to_string(),
                src: NamedSource::new(plugin_path.display().to_string(), plugin_text.to_string()),
                span: env_var.span().into(),
            }),
        }
    }

    if problems.is_empty() {
        Ok(resolved)
    } else {
        Err(problems)
    }
}
