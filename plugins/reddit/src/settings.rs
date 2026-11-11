//! Operator `[settings]` for the Reddit plugin, parsed and validated at `init`.
//!
//! Every error text names the field and never repeats the rejected value (a
//! value can carry CR or LF, and a config error is logged by the host).

use crate::cursor::MAX_FEEDS;
use serde::Deserialize;
use toml::Value;

/// Page size of the first poll of a feed (D-10).
pub const DEFAULT_FIRST_POLL_LIMIT: u32 = 25;
/// The app id in the user agent unless `[settings] app_id` overrides it (D-22).
const DEFAULT_APP_ID: &str = "reddit";
/// Page size of a later poll of a feed: Reddit's page maximum.
pub const LATER_POLL_LIMIT: u32 = 100;
/// Mirror of the host cap `MAX_ITEMS_PER_POLL` in `src/plugins/host/map.rs`:
/// the host keeps the first 1000 items of a poll and silently drops the rest.
pub const MAX_ITEMS_PER_POLL: usize = 1000;

/// The raw `[settings]` table. Unknown keys are rejected. Values stay untyped
/// so a wrong type is reported by this module, which never echoes the value
/// (serde's own type errors quote it).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    subreddits: Option<Value>,
    listings: Option<Value>,
    user_agent_contact: Option<Value>,
    app_id: Option<Value>,
    first_poll_limit: Option<Value>,
}

/// Longest subreddit name Reddit allows.
const MAX_SUBREDDIT_LEN: usize = 21;
/// Page size bounds for `first_poll_limit`.
const MAX_PAGE_LIMIT: i64 = 100;

fn valid_subreddit(name: &str) -> bool {
    (1..=MAX_SUBREDDIT_LEN).contains(&name.len())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

fn parse_subreddits(value: Option<Value>) -> Result<Vec<String>, String> {
    const RULE: &str = "settings.subreddits: required, a non-empty list of subreddit names of 1 to 21 letters, digits or _ (no r/ prefix)";
    let Some(Value::Array(entries)) = value else {
        return Err(RULE.to_string());
    };
    if entries.is_empty() {
        return Err(RULE.to_string());
    }
    let mut names: Vec<String> = Vec::new();
    for entry in entries {
        let Value::String(name) = entry else {
            return Err(RULE.to_string());
        };
        if !valid_subreddit(&name) {
            return Err(RULE.to_string());
        }
        if !names.iter().any(|seen| seen.eq_ignore_ascii_case(&name)) {
            names.push(name);
        }
    }
    Ok(names)
}

fn parse_listings(value: Option<Value>) -> Result<Vec<&'static str>, String> {
    const RULE: &str = "settings.listings: expected a non-empty list of \"new\" or \"hot\"";
    let Some(value) = value else {
        return Ok(vec!["new"]);
    };
    let Value::Array(entries) = value else {
        return Err(RULE.to_string());
    };
    if entries.is_empty() {
        return Err(RULE.to_string());
    }
    let mut listings: Vec<&'static str> = Vec::new();
    for entry in entries {
        let listing = match entry.as_str() {
            Some("new") => "new",
            Some("hot") => "hot",
            _ => return Err(RULE.to_string()),
        };
        if !listings.contains(&listing) {
            listings.push(listing);
        }
    }
    Ok(listings)
}

fn parse_first_poll_limit(value: Option<Value>) -> Result<u32, String> {
    let Some(value) = value else {
        return Ok(DEFAULT_FIRST_POLL_LIMIT);
    };
    match value {
        Value::Integer(n) if (1..=MAX_PAGE_LIMIT).contains(&n) => Ok(n as u32),
        _ => Err("settings.first_poll_limit: expected a whole number from 1 to 100".to_string()),
    }
}

/// One feed: a subreddit and a listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feed {
    pub subreddit: String,
    pub listing: &'static str,
    /// Cursor key, `r/<lower-cased name>:<listing>`.
    pub key: String,
}

/// Validated settings.
#[derive(Debug, Clone)]
pub struct Settings {
    subreddits: Vec<String>,
    listings: Vec<&'static str>,
    user_agent: String,
    first_poll_limit: u32,
}

impl Settings {
    /// Parse and validate the raw `[settings]` TOML. The `Err` text becomes a
    /// `config` error naming the field and never repeating the rejected value.
    pub fn parse(text: &str) -> Result<Settings, String> {
        let raw: Raw = toml::from_str(text).map_err(|e| format!("settings: {}", e.message()))?;
        let subreddits = parse_subreddits(raw.subreddits)?;
        let listings = parse_listings(raw.listings)?;
        let contact = match raw.user_agent_contact {
            Some(Value::String(contact)) => contact,
            Some(_) => {
                return Err("settings.user_agent_contact: expected a string".to_string());
            }
            None => {
                return Err(
                    "settings.user_agent_contact: required (a Reddit username such as \"u/yourname\")"
                        .to_string(),
                );
            }
        };
        let app_id = match raw.app_id {
            None => DEFAULT_APP_ID.to_string(),
            Some(Value::String(app_id)) => app_id,
            Some(_) => return Err("settings.app_id: expected a string".to_string()),
        };
        let user_agent = user_agent(&contact, &app_id)?;
        let first_poll_limit = parse_first_poll_limit(raw.first_poll_limit)?;

        // The cursor holds one mark per feed, and the host disables a plugin
        // whose cursor passes its cap (Pitfall 2).
        let feed_count = subreddits.len() * listings.len();
        if feed_count > MAX_FEEDS {
            return Err(format!(
                "settings.subreddits: {feed_count} feeds (subreddits x listings) exceed the most the cursor can hold ({MAX_FEEDS})"
            ));
        }
        Ok(Settings {
            subreddits,
            listings,
            user_agent,
            first_poll_limit,
        })
    }

    /// The feeds to poll: every subreddit, each with every listing, in order.
    pub fn feeds(&self) -> Vec<Feed> {
        let mut feeds = Vec::with_capacity(self.subreddits.len() * self.listings.len());
        for name in &self.subreddits {
            for listing in &self.listings {
                feeds.push(Feed {
                    subreddit: name.clone(),
                    listing,
                    key: format!("r/{}:{listing}", name.to_ascii_lowercase()),
                });
            }
        }
        feeds
    }

    /// The `User-Agent` header value every request carries.
    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }

    /// The most items per feed such that one poll never returns more than the
    /// host keeps (the host silently drops the excess), and at least 1.
    fn page_cap(&self) -> u32 {
        let feeds = (self.subreddits.len() * self.listings.len()).max(1);
        ((MAX_ITEMS_PER_POLL / feeds) as u32).max(1)
    }

    /// Page size for a feed that has no mark yet: `first_poll_limit`, shrunk
    /// to fit the per-poll item cap.
    pub fn first_page_limit(&self) -> u32 {
        self.first_poll_limit.min(self.page_cap()).max(1)
    }

    /// Page size for a feed that already has a mark: Reddit's page maximum,
    /// shrunk to fit the per-poll item cap.
    pub fn later_page_limit(&self) -> u32 {
        LATER_POLL_LIMIT.min(self.page_cap()).max(1)
    }
}

/// Build `filimon:<app_id>:<crate version> (by /u/<name>)`.
///
/// `contact` may be written `u/name`, `/u/name` or `name`; the name must be 3
/// to 20 characters from letters, digits, `_` and `-`. That charset is what
/// keeps CR, LF and separators out of the header.
pub fn user_agent(contact: &str, app_id: &str) -> Result<String, String> {
    let name = contact.strip_prefix('/').unwrap_or(contact);
    let name = name
        .strip_prefix("u/")
        .or_else(|| name.strip_prefix("U/"))
        .unwrap_or(name);
    let valid = (3..=20).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if !valid {
        return Err(
            "settings.user_agent_contact: expected a Reddit username of 3 to 20 letters, digits, _ or - (for example \"u/yourname\")"
                .to_string(),
        );
    }
    let app_id_valid = (1..=32).contains(&app_id.len())
        && app_id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_' || b == b'-');
    if !app_id_valid {
        return Err(
            "settings.app_id: expected 1 to 32 lower-case letters, digits, _ or -".to_string(),
        );
    }
    Ok(format!(
        "filimon:{app_id}:{} (by /u/{name})",
        env!("CARGO_PKG_VERSION")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cursor::MAX_FEEDS;

    const BASE: &str = "subreddits = [\"rust\"]\nuser_agent_contact = \"u/tester\"\n";

    fn parse(text: &str) -> Result<Settings, String> {
        Settings::parse(text)
    }

    fn with_contact(contact: &str) -> Result<Settings, String> {
        parse(&format!(
            "subreddits = [\"rust\"]\nuser_agent_contact = \"{contact}\"\n"
        ))
    }

    fn keys(settings: &Settings) -> Vec<String> {
        settings.feeds().into_iter().map(|feed| feed.key).collect()
    }

    #[test]
    fn missing_user_agent_contact_is_a_config_error_naming_the_field() {
        let err = parse("subreddits = [\"rust\"]\n").unwrap_err();
        assert!(err.contains("user_agent_contact"), "{err}");
    }

    #[test]
    fn user_agent_contact_accepts_three_spellings_and_enforces_3_to_20_chars() {
        let a = with_contact("u/abc").unwrap();
        let b = with_contact("/u/abc").unwrap();
        let c = with_contact("abc").unwrap();
        assert_eq!(a.user_agent(), b.user_agent());
        assert_eq!(b.user_agent(), c.user_agent());
        assert!(with_contact("ab").is_err());
        assert!(with_contact("u/ab").is_err());
        assert!(with_contact(&"a".repeat(3)).is_ok());
        assert!(with_contact(&"a".repeat(20)).is_ok());
        assert!(with_contact(&"a".repeat(21)).is_err());
        assert!(with_contact("a_b-c9").is_ok());
        assert!(with_contact("ab.c").is_err());
        assert!(with_contact("ab/c").is_err());
        assert!(with_contact("").is_err());
    }

    #[test]
    fn user_agent_contact_with_control_or_separator_characters_is_rejected() {
        // `\\r`, `\\n` and `\\t` are TOML escapes, so the parsed value holds the
        // real control character.
        for bad in ["abc\\rdef", "abc\\ndef", "abc def", "abc:def", "abc\\tdef"] {
            let err = with_contact(bad).unwrap_err();
            assert!(err.contains("user_agent_contact"), "{err}");
            assert!(!err.contains("def"), "echoed the value: {err}");
        }
    }

    #[test]
    fn user_agent_has_the_documented_format_and_default_app_id() {
        let settings = parse(BASE).unwrap();
        assert_eq!(
            settings.user_agent(),
            format!("filimon:reddit:{} (by /u/tester)", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn app_id_override_replaces_only_the_app_id_and_is_validated() {
        let settings = parse(&format!("{BASE}app_id = \"mybot\"\n")).unwrap();
        assert_eq!(
            settings.user_agent(),
            format!("filimon:mybot:{} (by /u/tester)", env!("CARGO_PKG_VERSION"))
        );
        let too_long = "a".repeat(33);
        for bad in ["", "My Bot", "my:bot", "MYBOT", too_long.as_str()] {
            let err = parse(&format!("{BASE}app_id = \"{bad}\"\n")).unwrap_err();
            assert!(err.contains("app_id"), "{err}");
        }
        assert!(parse(&format!("{BASE}app_id = \"{}\"\n", "a".repeat(32))).is_ok());
    }

    #[test]
    fn listings_default_to_new_accept_new_and_hot_and_reject_others() {
        assert_eq!(keys(&parse(BASE).unwrap()), ["r/rust:new"]);
        let both = parse(&format!("{BASE}listings = [\"new\", \"hot\"]\n")).unwrap();
        assert_eq!(keys(&both), ["r/rust:new", "r/rust:hot"]);
        for bad in ["listings = [\"top\"]", "listings = []", "listings = \"new\""] {
            let err = parse(&format!("{BASE}{bad}\n")).unwrap_err();
            assert!(err.contains("listings"), "{err}");
        }
    }

    #[test]
    fn duplicate_listings_and_subreddits_are_collapsed_keeping_order() {
        let settings = parse(
            "subreddits = [\"rust\", \"golang\", \"Rust\", \"rust\"]\n\
             listings = [\"new\", \"hot\", \"new\"]\n\
             user_agent_contact = \"u/tester\"\n",
        )
        .unwrap();
        assert_eq!(
            keys(&settings),
            ["r/rust:new", "r/rust:hot", "r/golang:new", "r/golang:hot"]
        );
    }

    #[test]
    fn subreddits_must_be_present_and_valid_names() {
        let contact = "user_agent_contact = \"u/tester\"\n";
        let too_long = format!("subreddits = [\"{}\"]\n{contact}", "a".repeat(22));
        let cases = [
            contact.to_string(),
            format!("subreddits = []\n{contact}"),
            format!("subreddits = [\"bad-name\"]\n{contact}"),
            format!("subreddits = [\"\"]\n{contact}"),
            format!("subreddits = [\"r/rust\"]\n{contact}"),
            format!("subreddits = [\"has space\"]\n{contact}"),
            format!("subreddits = \"rust\"\n{contact}"),
            too_long,
        ];
        for text in cases {
            let err = parse(&text).unwrap_err();
            assert!(err.contains("subreddits"), "{err}");
        }
        let longest = format!("subreddits = [\"{}\"]\n{contact}", "a".repeat(21));
        assert!(parse(&longest).is_ok());
        assert!(parse(&format!("subreddits = [\"_ok_9\"]\n{contact}")).is_ok());
    }

    #[test]
    fn first_poll_limit_accepts_1_to_100_and_defaults_to_25() {
        assert_eq!(parse(BASE).unwrap().first_page_limit(), 25);
        for good in [1u32, 100] {
            let settings = parse(&format!("{BASE}first_poll_limit = {good}\n")).unwrap();
            assert_eq!(settings.first_page_limit(), good);
        }
        for bad in ["0", "101", "-1", "\"10\"", "2.5"] {
            let err = parse(&format!("{BASE}first_poll_limit = {bad}\n")).unwrap_err();
            assert!(err.contains("first_poll_limit"), "{err}");
        }
    }

    #[test]
    fn unknown_keys_are_rejected_naming_the_key() {
        let err = parse(&format!("{BASE}feeed = \"home\"\n")).unwrap_err();
        assert!(err.contains("feeed"), "{err}");
    }

    fn many(count: usize, extra: &str) -> String {
        let names: Vec<String> = (0..count).map(|i| format!("\"sub{i}\"")).collect();
        format!(
            "subreddits = [{}]\nuser_agent_contact = \"u/tester\"\n{extra}",
            names.join(", ")
        )
    }

    #[test]
    fn feed_count_over_the_cursor_capacity_is_rejected_naming_subreddits() {
        let ok = parse(&many(MAX_FEEDS, "")).unwrap();
        assert_eq!(ok.feeds().len(), MAX_FEEDS);
        let err = parse(&many(MAX_FEEDS + 1, "")).unwrap_err();
        assert!(err.contains("subreddits"), "{err}");

        // The cap counts feeds (subreddits x listings), not subreddits.
        let err = parse(&many(MAX_FEEDS / 2 + 1, "listings = [\"new\", \"hot\"]\n")).unwrap_err();
        assert!(err.contains("subreddits"), "{err}");
    }

    #[test]
    fn page_limit_shrinks_so_one_poll_never_exceeds_1000_items() {
        let one = parse(&many(1, "")).unwrap();
        assert_eq!(one.first_page_limit(), DEFAULT_FIRST_POLL_LIMIT);
        assert_eq!(one.later_page_limit(), LATER_POLL_LIMIT);
        assert_eq!(
            parse(&many(1, "first_poll_limit = 100\n"))
                .unwrap()
                .first_page_limit(),
            100
        );

        let twenty = parse(&many(20, "first_poll_limit = 100\n")).unwrap();
        assert_eq!(twenty.first_page_limit(), 50);
        assert_eq!(twenty.later_page_limit(), 50);

        let most = parse(&many(MAX_FEEDS, "first_poll_limit = 100\n")).unwrap();
        for settings in [&one, &twenty, &most] {
            let feeds = settings.feeds().len() as u32;
            assert!(settings.first_page_limit() >= 1 && settings.later_page_limit() >= 1);
            assert!(feeds * settings.first_page_limit() <= MAX_ITEMS_PER_POLL as u32);
            assert!(feeds * settings.later_page_limit() <= MAX_ITEMS_PER_POLL as u32);
        }
        assert_eq!(most.later_page_limit(), 1000 / MAX_FEEDS as u32);
    }

    #[test]
    fn config_errors_never_echo_the_rejected_value() {
        let contact = "user_agent_contact = \"u/tester\"\n";
        let sub = "subreddits = [\"rust\"]\n";
        let cases = [
            format!("subreddits = [\"ZZQ bad ZZQ\"]\n{contact}"),
            format!("subreddits = \"ZZQ\"\n{contact}"),
            format!("subreddits = [1]\n{contact}"),
            format!("{sub}{contact}listings = [\"ZZQ\"]\n"),
            format!("{sub}{contact}listings = \"ZZQ\"\n"),
            format!("{sub}user_agent_contact = \"ZZQ ZZQ\"\n"),
            format!("{sub}user_agent_contact = 7\n"),
            format!("{sub}{contact}app_id = \"ZZQ ZZQ\"\n"),
            format!("{sub}{contact}app_id = 7\n"),
            format!("{sub}{contact}first_poll_limit = \"ZZQ\"\n"),
            format!("{sub}{contact}first_poll_limit = 99999\n"),
        ];
        for text in cases {
            let err = parse(&text).unwrap_err();
            assert!(!err.contains("ZZQ"), "echoed the value: {err}");
            assert!(!err.contains("99999"), "echoed the value: {err}");
            assert!(err.starts_with("settings"), "{err}");
        }
    }
}
