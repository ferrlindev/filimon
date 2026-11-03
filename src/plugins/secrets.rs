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
/// Returns one `ConfigError` per alias that cannot be resolved (missing) or
/// whose effective value is empty, labelled at that alias's env var name in
/// the plugin file.
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

    let plugin_src =
        || NamedSource::new(plugin_path.display().to_string(), plugin_text.to_string());

    for (alias, env_var) in aliases {
        let name = env_var.get_ref();
        match env(name).or_else(|| dotenv.get(name).cloned()) {
            Some(value) if value.is_empty() => problems.push(ConfigError::EmptySecret {
                plugin: plugin.to_string(),
                alias: alias.clone(),
                env_var: name.clone(),
                src: plugin_src(),
                span: env_var.span().into(),
            }),
            Some(value) => {
                resolved.insert(alias.clone(), Secret(value));
            }
            None => problems.push(ConfigError::MissingSecret {
                plugin: plugin.to_string(),
                alias: alias.clone(),
                env_var: name.clone(),
                dotenv: dotenv_path.display().to_string(),
                src: plugin_src(),
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

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::config::{BackoffConfig, HostConfig, PluginConfig};
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    fn missing_dotenv_file_is_empty() {
        let dir = TempDir::new().expect("tempdir");

        let values = read_dotenv(&dir.path().join(".env")).expect("a missing .env is not an error");

        assert!(values.is_empty());
    }

    /// Writes `text` to a fresh `.env` and reads it back.
    fn read_text(text: &str) -> Result<HashMap<String, String>, Vec<ConfigError>> {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join(".env");
        std::fs::write(&path, text).expect("write fixture");
        read_dotenv(&path).map_err(|problem| vec![problem])
    }

    /// The 1-based line of a `DotenvInvalid`, or a panic for any other problem.
    fn dotenv_line(problem: &ConfigError) -> Option<usize> {
        match problem {
            ConfigError::DotenvInvalid { line, .. } => *line,
            other => panic!("expected DotenvInvalid, got {other:?}"),
        }
    }

    /// Asserts that neither Display nor Debug of `problem` carries `value`.
    fn assert_omits(problem: &ConfigError, value: &str) {
        let display = problem.to_string();
        let debug = format!("{problem:?}");
        assert!(!display.contains(value), "leaked {value:?} in: {display}");
        assert!(!debug.contains(value), "leaked {value:?} in: {debug}");
    }

    #[test]
    fn dotenv_export_prefix_and_double_quotes() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join(".env");
        std::fs::write(&path, "export ALPHA_TOKEN=\"a b\"\n").expect("write fixture");

        let values = read_dotenv(&path).expect(".env parses");

        assert_eq!(values.get("ALPHA_TOKEN").map(String::as_str), Some("a b"));
    }

    #[test]
    fn dotenv_dollar_outside_single_quotes_is_rejected() {
        for (text, value) in [
            ("PW=pa$$word\n", "pa$$word"),
            ("Q=\"x$HOME/y\"\n", "x$HOME/y"),
            ("S=abc$NOPE_UNSET_XYZ\n", "abc$NOPE_UNSET_XYZ"),
        ] {
            let problems = read_text(text).expect_err(text);

            assert_eq!(problems.len(), 1, "{text}: {problems:?}");
            assert_eq!(dotenv_line(&problems[0]), Some(1), "{text}");
            assert_omits(&problems[0], value);
        }

        let values = read_text("R='pa$$word'\n").expect("a single-quoted `$` is literal");
        assert_eq!(values.get("R").map(String::as_str), Some("pa$$word"));
    }

    #[test]
    fn secrets_iterate_in_alias_order() {
        let plugin_text = "zeta = \"Z_VAR\"\nalpha = \"A_VAR\"\nmid = \"M_VAR\"\n";
        let aliases: BTreeMap<String, toml::Spanned<String>> =
            toml::from_str(plugin_text).expect("alias table parses");
        let dotenv: HashMap<String, String> = [("Z_VAR", "z"), ("A_VAR", "a"), ("M_VAR", "m")]
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();

        let resolved = resolve_secrets(
            "alpha",
            Path::new("alpha.toml"),
            plugin_text,
            &aliases,
            &dotenv,
            Path::new(".env"),
            &|_| None,
        )
        .expect("all aliases resolve");

        assert_eq!(
            resolved.keys().map(String::as_str).collect::<Vec<_>>(),
            ["alpha", "mid", "zeta"]
        );
    }

    #[test]
    fn secret_debug_is_redacted() {
        assert_eq!(
            format!("{:?}", Secret("pa55word".to_string())),
            "Secret(***)"
        );

        let plugin = PluginConfig {
            host: HostConfig {
                poll_interval: Duration::from_secs(60),
                timeout: Duration::from_secs(30),
                allowlist: Vec::new(),
                backoff: BackoffConfig {
                    initial: Duration::from_secs(60),
                    max: Duration::from_secs(3600),
                    multiplier: 2.0,
                },
            },
            settings_toml: String::new(),
            secrets: ResolvedSecrets::from([("token".to_string(), Secret("pa55word".to_string()))]),
        };

        let debug = format!("{plugin:?}");
        assert!(!debug.contains("pa55word"), "{debug}");
        assert!(debug.contains("Secret(***)"), "{debug}");
    }
}
