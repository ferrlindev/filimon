// ─────────────────────────────────────────────────────────────────────────────
// plugins/secrets.rs - Per-plugin secret resolution
//
// Reads the `.env` next to the registry into a private map and resolves each
// plugin's `[host.secrets]` aliases against the process environment first and
// that map second. Each plugin receives only its own aliases, keyed by alias.
// `.env` values are read literally and never expanded; the format is
// documented in plugins/README.md under "Plugin secrets".
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
/// Values are literal: `fili` never expands `$NAME` or `${NAME}` and never
/// processes backslash escapes. Each line is blank, a comment, or
/// `[export ]KEY=VALUE`, where VALUE is one of:
/// - empty (end of line, or whitespace then a `#` comment);
/// - single-quoted: anything except `'`, kept verbatim;
/// - double-quoted: kept verbatim, but must not contain `$` or a backslash;
/// - unquoted: no whitespace, `$`, backslash or quotes, and no leading `#`.
///
/// Comments are whole lines whose first non-whitespace character is `#`, or
/// text after whitespace that follows a value. LF and CRLF line endings and
/// one leading byte-order mark are accepted. A key set twice is rejected, so
/// no value depends on line order.
///
/// The file is read once and parsed from that in-memory text. Parsing takes
/// only the line text: the process environment is never read or modified.
///
/// # Errors
/// Returns every rejected line, in line order, as a `ConfigError::DotenvInvalid`
/// carrying its line number and a fixed reason, never the line text, because
/// the text can be or contain a secret value. A read failure other than a
/// missing file is one `DotenvInvalid` without a line number.
pub fn read_dotenv(path: &Path) -> Result<HashMap<String, String>, Vec<ConfigError>> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(HashMap::new()),
        Err(e) => return Err(vec![dotenv_invalid(path, None, e.to_string())]),
    };
    let body = text.strip_prefix('\u{feff}').unwrap_or(&text);

    let mut values = HashMap::new();
    let mut first_line: HashMap<String, usize> = HashMap::new();
    let mut problems = Vec::new();
    // Line numbers are counted, never found by searching the text.
    for (index, line) in body.lines().enumerate() {
        let number = index + 1;
        match parse_line(line) {
            Ok(None) => {}
            Ok(Some((key, value))) => {
                if let Some(first) = first_line.get(&key) {
                    // The key is an env var name, which diagnostics may carry.
                    let reason = format!("duplicate key `{key}` (first set on line {first})");
                    problems.push(dotenv_invalid(path, Some(number), reason));
                } else {
                    first_line.insert(key.clone(), number);
                    values.insert(key, value);
                }
            }
            Err(reason) => problems.push(dotenv_invalid(path, Some(number), reason.to_string())),
        }
    }

    if problems.is_empty() {
        Ok(values)
    } else {
        Err(problems)
    }
}

fn dotenv_invalid(path: &Path, line: Option<usize>, reason: String) -> ConfigError {
    ConfigError::DotenvInvalid {
        path: path.display().to_string(),
        line,
        reason,
    }
}

// ── .env Line Parsing ─────────────────────────────────────────────────────────
//
// Each rejection reason is a fixed phrase. None may ever include line text.

const DOLLAR: &str =
    "`$` outside single quotes; fili never expands variables, so single-quote the value";
const BACKSLASH: &str =
    "backslash outside single quotes; escapes are not supported, so single-quote the value";
const QUOTE_IN_UNQUOTED: &str = "quote inside an unquoted value; quote the whole value";
const LEADING_HASH: &str =
    "value starts with `#`; single-quote it, or put a space before a comment";
const TRAILING_TEXT: &str = "unexpected text after the value; quote the whole value";
const UNTERMINATED: &str = "unterminated quote; multi-line values are not supported";
const NOT_ASSIGNMENT: &str = "expected KEY=VALUE";

/// Parses one `.env` line. `Ok(None)` is a blank or comment line.
///
/// When a line breaks several rules, the first one met scanning left to right
/// is reported.
fn parse_line(line: &str) -> Result<Option<(String, String)>, &'static str> {
    let rest = line.trim_start();
    if rest.is_empty() || rest.starts_with('#') {
        return Ok(None);
    }
    let (key, rest) = split_key(strip_export(rest)).ok_or(NOT_ASSIGNMENT)?;
    let rest = rest
        .trim_start()
        .strip_prefix('=')
        .ok_or(NOT_ASSIGNMENT)?;
    let (value, trailer) = split_value(rest)?;
    check_trailer(trailer)?;
    Ok(Some((key.to_string(), value.to_string())))
}

/// Drops a leading `export` plus whitespace when a key follows it, so a key
/// literally named `export` still parses.
fn strip_export(rest: &str) -> &str {
    let Some(after) = rest.strip_prefix("export") else {
        return rest;
    };
    let trimmed = after.trim_start();
    let had_space = trimmed.len() < after.len();
    if had_space && trimmed.starts_with(is_key_start) {
        trimmed
    } else {
        rest
    }
}

fn is_key_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_'
}

fn is_key_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.'
}

/// Splits a leading KEY off `rest`. `None` when `rest` does not start with one.
fn split_key(rest: &str) -> Option<(&str, &str)> {
    if !rest.starts_with(is_key_start) {
        return None;
    }
    let end = rest.find(|c: char| !is_key_char(c)).unwrap_or(rest.len());
    Some(rest.split_at(end))
}

/// Splits the text after `=` into the literal value and what follows it.
fn split_value(rest: &str) -> Result<(&str, &str), &'static str> {
    let start = rest.trim_start();
    let had_space = start.len() < rest.len();
    if start.is_empty() {
        return Ok(("", ""));
    }
    if let Some(quoted) = start.strip_prefix('\'') {
        return quoted.split_once('\'').ok_or(UNTERMINATED);
    }
    if let Some(quoted) = start.strip_prefix('"') {
        return split_double_quoted(quoted);
    }
    if start.starts_with('#') {
        return if had_space {
            Ok(("", ""))
        } else {
            Err(LEADING_HASH)
        };
    }
    split_unquoted(start)
}

/// `quoted` is the text after the opening `"`.
fn split_double_quoted(quoted: &str) -> Result<(&str, &str), &'static str> {
    match quoted.chars().find(|c| matches!(c, '$' | '\\' | '"')) {
        Some('$') => Err(DOLLAR),
        Some('\\') => Err(BACKSLASH),
        Some(_) => quoted.split_once('"').ok_or(UNTERMINATED),
        None => Err(UNTERMINATED),
    }
}

/// An unquoted value is the maximal run of non-whitespace characters.
fn split_unquoted(start: &str) -> Result<(&str, &str), &'static str> {
    let end = start.find(char::is_whitespace).unwrap_or(start.len());
    let (value, trailer) = start.split_at(end);
    for c in value.chars() {
        match c {
            '$' => return Err(DOLLAR),
            '\\' => return Err(BACKSLASH),
            '\'' | '"' => return Err(QUOTE_IN_UNQUOTED),
            _ => {}
        }
    }
    Ok((value, trailer))
}

/// After a value only whitespace, optionally followed by a `#` comment, may follow.
fn check_trailer(trailer: &str) -> Result<(), &'static str> {
    let rest = trailer.trim_start();
    let had_space = rest.len() < trailer.len();
    if trailer.is_empty() || (had_space && (rest.is_empty() || rest.starts_with('#'))) {
        Ok(())
    } else {
        Err(TRAILING_TEXT)
    }
}

// ── Alias Resolution ──────────────────────────────────────────────────────────

/// Resolves one plugin's aliases to secret values.
///
/// Each alias's env var is looked up through `env` (the process environment
/// in production) first and the `.env` map second. Lookup is exact and
/// case-sensitive. Only the aliases in `aliases` are resolved, so a plugin
/// never sees another plugin's secrets or unrelated `.env` keys.
///
/// `dotenv` is `None` when the `.env` file was unusable. The caller has then
/// already recorded its `DotenvInvalid` problems and must fail the load, so
/// an alias found nowhere is not reported as missing: that `MissingSecret`
/// would send the operator to the wrong fix.
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
    dotenv: Option<&HashMap<String, String>>,
    dotenv_path: &Path,
    env: &dyn Fn(&str) -> Option<String>,
) -> Result<ResolvedSecrets, Vec<ConfigError>> {
    let mut resolved = ResolvedSecrets::new();
    let mut problems = Vec::new();

    let plugin_src =
        || NamedSource::new(plugin_path.display().to_string(), plugin_text.to_string());

    for (alias, env_var) in aliases {
        let name = env_var.get_ref();
        // D-12: a set process variable wins even when empty; empty is not unset.
        let (value, from_process) = match env(name) {
            Some(value) => (Some(value), true),
            None => (dotenv.and_then(|map| map.get(name).cloned()), false),
        };
        match value {
            Some(value) if value.is_empty() => {
                let (origin, fix) = empty_secret_source(name, from_process, dotenv_path);
                problems.push(ConfigError::EmptySecret {
                    plugin: plugin.to_string(),
                    alias: alias.clone(),
                    env_var: name.clone(),
                    origin,
                    fix,
                    src: plugin_src(),
                    span: env_var.span().into(),
                });
            }
            Some(value) => {
                resolved.insert(alias.clone(), Secret(value));
            }
            // The unusable `.env` already explains why the fallback is absent.
            None if dotenv.is_none() => {}
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

/// The `EmptySecret` origin and fix for an empty value. Neither fix suggests
/// editing `.env` while a process variable shadows it.
fn empty_secret_source(env_var: &str, from_process: bool, dotenv_path: &Path) -> (String, String) {
    let dotenv = dotenv_path.display();
    if from_process {
        (
            "the process environment".to_string(),
            format!(
                "unset `{env_var}` in the process environment or give it a non-empty value there; the process environment overrides {dotenv}"
            ),
        )
    } else {
        (
            dotenv.to_string(),
            format!("give `{env_var}` a non-empty value in {dotenv}"),
        )
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::config::{BackoffConfig, HostConfig, PluginConfig};
    use miette::Diagnostic;
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
        read_dotenv(&path)
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

    /// Every accepted example from the `.env` format (F1-F7), plus comment
    /// and blank lines that must yield no keys.
    const ACCEPTED_FORMS: &str = r#"# comment with $HOME, 'single' and "double" quotes

PLAIN=abc123
export EXPORTED=v
SPACED = v
EMPTY=
EMPTY_SQ=''
EMPTY_DQ=""
EMPTY_COMMENT= # note
SQ='pa$$word \ "x" # y'
DQ="a b # c"
PADDED='  a  '
HASH_INSIDE=a#b
TRAILING=abc # c
QUOTED_TRAILING='abc' # c
UNICODE=bücher
DOTTED.KEY=v
lower_case=v
"#;

    #[test]
    fn dotenv_accepts_documented_forms() {
        // A decomposed `u` + combining diaeresis must survive unnormalised.
        let text = format!("{ACCEPTED_FORMS}DECOMPOSED='bu\u{308}cher'\n");

        let values = read_text(&text).expect("every documented form is accepted");

        let expected: HashMap<String, String> = [
            ("PLAIN", "abc123"),
            ("EXPORTED", "v"),
            ("SPACED", "v"),
            ("EMPTY", ""),
            ("EMPTY_SQ", ""),
            ("EMPTY_DQ", ""),
            ("EMPTY_COMMENT", ""),
            ("SQ", r#"pa$$word \ "x" # y"#),
            ("DQ", "a b # c"),
            ("PADDED", "  a  "),
            ("HASH_INSIDE", "a#b"),
            ("TRAILING", "abc"),
            ("QUOTED_TRAILING", "abc"),
            ("UNICODE", "b\u{fc}cher"),
            ("DOTTED.KEY", "v"),
            ("lower_case", "v"),
            ("DECOMPOSED", "bu\u{308}cher"),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
        assert_eq!(values, expected);
    }

    #[test]
    fn dotenv_accepts_crlf_and_bom() {
        let values = read_text("A=1\r\nB='x y'\r\n").expect("CRLF lines parse");
        assert_eq!(values.len(), 2, "{values:?}");
        assert_eq!(values.get("A").map(String::as_str), Some("1"));
        assert_eq!(values.get("B").map(String::as_str), Some("x y"));

        let values = read_text("\u{feff}A=1\n").expect("a leading BOM is skipped");
        assert_eq!(values.keys().map(String::as_str).collect::<Vec<_>>(), ["A"]);
    }

    #[test]
    fn dotenv_rejects_ambiguous_forms() {
        let cases = [
            ("K=SECRETx$y", DOLLAR),
            ("K=\"SECRETx${Y}\"", DOLLAR),
            ("K=SECRETx\\y", BACKSLASH),
            ("K=\"SECRETx\\ny\"", BACKSLASH),
            ("K=SECRETx'y'", QUOTE_IN_UNQUOTED),
            ("K=#SECRETx", LEADING_HASH),
            ("K=SECRETx y", TRAILING_TEXT),
            ("K='SECRETx'y", TRAILING_TEXT),
            ("K='SECRETx'#y", TRAILING_TEXT),
            ("K='SECRETx", UNTERMINATED),
            ("K=\"SECRETx", UNTERMINATED),
            ("1K=SECRETx", NOT_ASSIGNMENT),
            ("SECRETx", NOT_ASSIGNMENT),
        ];
        for (case, expected) in cases {
            let problems = read_text(&format!("OK=1\n{case}\n")).expect_err(case);

            assert_eq!(problems.len(), 1, "{case}: {problems:?}");
            let ConfigError::DotenvInvalid { line, reason, .. } = &problems[0] else {
                panic!("{case}: expected DotenvInvalid, got {:?}", problems[0]);
            };
            assert_eq!(*line, Some(2), "{case}");
            assert_eq!(reason, expected, "{case}");
            assert_omits(&problems[0], "SECRETx");
        }
    }

    #[test]
    fn dotenv_duplicate_key_is_rejected() {
        let problems = read_text("A=first-SECRET\nB=2\nA=second-SECRET\n")
            .expect_err("a repeated key is rejected");

        assert_eq!(problems.len(), 1, "{problems:?}");
        let ConfigError::DotenvInvalid { line, reason, .. } = &problems[0] else {
            panic!("expected DotenvInvalid, got {:?}", problems[0]);
        };
        assert_eq!(*line, Some(3));
        assert!(reason.contains("`A`"), "{reason}");
        assert!(reason.contains("line 1"), "{reason}");
        assert_omits(&problems[0], "first-SECRET");
        assert_omits(&problems[0], "second-SECRET");
    }

    #[test]
    fn dotenv_reports_every_bad_line_in_order() {
        let problems =
            read_text("A=1\nB=x y\nC=2\nD=$X\nE=3\n").expect_err("two lines are rejected");

        assert_eq!(
            problems.iter().map(dotenv_line).collect::<Vec<_>>(),
            [Some(2), Some(4)]
        );
    }

    #[test]
    fn dotenv_line_number_is_counted() {
        // The bad line's text also appears in the line-1 comment.
        let problems = read_text("# x y\nGOOD=1\nBAD=x y\n").expect_err("line 3 is rejected");

        assert_eq!(problems.iter().map(dotenv_line).collect::<Vec<_>>(), [Some(3)]);
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
            Some(&dotenv),
            Path::new(".env"),
            &|_| None,
        )
        .expect("all aliases resolve");

        assert_eq!(
            resolved.keys().map(String::as_str).collect::<Vec<_>>(),
            ["alpha", "mid", "zeta"]
        );
    }

    const TOKEN_ALIAS: &str = "token = \"ALPHA_TOKEN\"\n";
    const DOTENV_PATH: &str = "/srv/fili/.env";

    /// Resolves the single alias `token = "ALPHA_TOKEN"` and returns its one problem.
    fn single_problem(
        dotenv: &HashMap<String, String>,
        env: &dyn Fn(&str) -> Option<String>,
    ) -> ConfigError {
        let aliases: BTreeMap<String, toml::Spanned<String>> =
            toml::from_str(TOKEN_ALIAS).expect("alias table parses");
        let mut problems = resolve_secrets(
            "alpha",
            Path::new("alpha.toml"),
            TOKEN_ALIAS,
            &aliases,
            Some(dotenv),
            Path::new(DOTENV_PATH),
            env,
        )
        .expect_err("an empty secret is an error");
        assert_eq!(problems.len(), 1, "{problems:?}");
        problems.remove(0)
    }

    fn help_of(problem: &ConfigError) -> String {
        problem.help().map(|h| h.to_string()).unwrap_or_default()
    }

    #[test]
    fn empty_process_var_names_process_env() {
        let dotenv = HashMap::from([("ALPHA_TOKEN".to_string(), "good".to_string())]);
        let env = |key: &str| (key == "ALPHA_TOKEN").then(String::new);

        // D-12: the set-but-empty process value wins over the `.env` value.
        let problem = single_problem(&dotenv, &env);

        assert!(
            matches!(
                &problem,
                ConfigError::EmptySecret { origin, .. } if origin == "the process environment"
            ),
            "{problem:?}"
        );
        let display = problem.to_string();
        assert!(display.contains("the process environment"), "{display}");
        let help = help_of(&problem);
        assert!(help.contains("unset `ALPHA_TOKEN`"), "{help}");
        assert!(help.contains(DOTENV_PATH), "{help}");
    }

    #[test]
    fn empty_dotenv_value_names_dotenv_file() {
        let dotenv = HashMap::from([("ALPHA_TOKEN".to_string(), String::new())]);

        let problem = single_problem(&dotenv, &|_| None);

        assert!(
            matches!(&problem, ConfigError::EmptySecret { origin, .. } if origin == DOTENV_PATH),
            "{problem:?}"
        );
        let display = problem.to_string();
        assert!(display.contains(DOTENV_PATH), "{display}");
        let help = help_of(&problem);
        assert!(help.contains("non-empty value in"), "{help}");
        assert!(help.contains(DOTENV_PATH), "{help}");
        assert!(!help.contains("unset"), "{help}");
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
            oauth: None,
        };

        let debug = format!("{plugin:?}");
        assert!(!debug.contains("pa55word"), "{debug}");
        assert!(debug.contains("Secret(***)"), "{debug}");
    }
}
