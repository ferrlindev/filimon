//! Operator `[settings]` for the RSS plugin, parsed and validated at `init`.
//!
//! Every error text names the field and never repeats the rejected value (a
//! value can carry CR or LF, and a config error is logged by the host).

use serde::Deserialize;
use toml::Value;

/// Most feeds one instance follows. The cursor holds one entry per feed and the
/// host rejects a cursor over 4096 bytes; `cursor::tests` measures the worst case.
pub const MAX_FEEDS: usize = 30;
/// Longest accepted feed URL.
pub const MAX_URL_BYTES: usize = 2048;
pub const DEFAULT_MAX_ITEMS_PER_FEED: usize = 25;
pub const MAX_ITEMS_PER_FEED: usize = 100;
/// Mirror of the host cap `MAX_ITEMS_PER_POLL` in `src/plugins/host/map.rs`: the
/// host keeps the first 1000 items of a poll and silently drops the rest.
pub const MAX_ITEMS_PER_POLL: usize = 1000;
pub const DEFAULT_MAX_TEXT_CHARS: usize = 20_000;
pub const MIN_TEXT_CHARS: usize = 200;
pub const MAX_TEXT_CHARS: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    pub feeds: Vec<String>,
    pub max_items_per_feed: usize,
    pub max_text_chars: usize,
}

/// Values stay untyped so a wrong type is reported here without echoing the
/// value (serde's own type errors quote it).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    feeds: Option<Value>,
    max_items_per_feed: Option<Value>,
    max_text_chars: Option<Value>,
}

fn bounded(value: Option<Value>, field: &str, default: usize, min: usize, max: usize) -> Result<usize, String> {
    match value {
        None => Ok(default),
        Some(Value::Integer(n)) => usize::try_from(n)
            .ok()
            .filter(|n| (min..=max).contains(n))
            .ok_or_else(|| format!("settings: `{field}` must be between {min} and {max}")),
        Some(_) => Err(format!("settings: `{field}` must be an integer")),
    }
}

fn valid_feed_url(url: &str) -> bool {
    url.len() <= MAX_URL_BYTES
        && (url.starts_with("https://") || url.starts_with("http://"))
        && url.len() > "https://".len()
        && !url.chars().any(|c| c.is_control() || c.is_whitespace())
}

impl Settings {
    /// Parse and validate the raw `[settings]` TOML text.
    pub fn parse(text: &str) -> Result<Settings, String> {
        let raw: Raw = toml::from_str(text)
            .map_err(|_| "settings: not valid TOML or has an unknown key (allowed: feeds, max_items_per_feed, max_text_chars)".to_string())?;

        let feeds = match raw.feeds {
            None => return Err("settings: `feeds` is required".to_string()),
            Some(Value::Array(items)) => items,
            Some(_) => return Err("settings: `feeds` must be an array of URLs".to_string()),
        };
        if feeds.is_empty() {
            return Err("settings: `feeds` must list at least one URL".to_string());
        }
        if feeds.len() > MAX_FEEDS {
            return Err(format!("settings: `feeds` may list at most {MAX_FEEDS} URLs"));
        }
        let mut urls: Vec<String> = Vec::with_capacity(feeds.len());
        for (i, value) in feeds.into_iter().enumerate() {
            let Value::String(url) = value else {
                return Err(format!("settings: `feeds[{i}]` must be a string"));
            };
            if !valid_feed_url(&url) {
                return Err(format!("settings: `feeds[{i}]` must be an http or https URL"));
            }
            if urls.contains(&url) {
                return Err(format!("settings: `feeds[{i}]` is listed twice"));
            }
            urls.push(url);
        }

        let max_items_per_feed = bounded(
            raw.max_items_per_feed,
            "max_items_per_feed",
            DEFAULT_MAX_ITEMS_PER_FEED,
            1,
            MAX_ITEMS_PER_FEED,
        )?;
        if urls.len() * max_items_per_feed > MAX_ITEMS_PER_POLL {
            return Err(format!(
                "settings: `feeds` times `max_items_per_feed` may not exceed {MAX_ITEMS_PER_POLL}"
            ));
        }

        Ok(Settings {
            feeds: urls,
            max_items_per_feed,
            max_text_chars: bounded(
                raw.max_text_chars,
                "max_text_chars",
                DEFAULT_MAX_TEXT_CHARS,
                MIN_TEXT_CHARS,
                MAX_TEXT_CHARS,
            )?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn minimal_settings_get_defaults() {
        let s = Settings::parse(r#"feeds = ["https://context.ph/feed/"]"#).unwrap();
        assert_eq!(s.feeds, vec!["https://context.ph/feed/"]);
        assert_eq!(s.max_items_per_feed, DEFAULT_MAX_ITEMS_PER_FEED);
        assert_eq!(s.max_text_chars, DEFAULT_MAX_TEXT_CHARS);
    }

    #[test]
    fn rejections_never_echo_the_value() {
        for bad in [
            r#"feeds = ["ftp://secret.example/x"]"#,
            "feeds = [\"https://a.example/\\nX\"]",
            r#"feeds = ["https://a.example/", "https://a.example/"]"#,
            "feeds = []",
            r#"feeds = "https://a.example/""#,
            r#"feeds = ["https://a.example/"]
max_items_per_feed = 0"#,
            r#"feeds = ["https://a.example/"]
max_text_chars = "secret""#,
            r#"feeds = ["https://a.example/"]
unknown = 1"#,
            "",
        ] {
            let err = Settings::parse(bad).unwrap_err();
            assert!(!err.contains("secret"), "{err}");
            assert!(!err.contains('\n'), "{err}");
        }
    }

    #[test]
    fn too_many_feeds_rejected() {
        let urls: Vec<String> = (0..=MAX_FEEDS).map(|i| format!("\"https://a{i}.example/\"")).collect();
        let text = format!("feeds = [{}]", urls.join(","));
        assert!(Settings::parse(&text).is_err());
    }
}
