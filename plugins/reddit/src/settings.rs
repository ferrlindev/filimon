//! Operator `[settings]` for the Reddit plugin, parsed and validated at `init`.
//!
//! Every error text names the field and never repeats the rejected value (a
//! value can carry CR or LF, and a config error is logged by the host).

use serde::Deserialize;

/// Page size of the first poll of a feed (D-10).
pub const DEFAULT_FIRST_POLL_LIMIT: u32 = 25;

/// The raw `[settings]` table. Unknown keys are rejected.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    subreddits: Vec<String>,
    user_agent_contact: Option<String>,
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
    user_agent: String,
}

impl Settings {
    /// Parse and validate the raw `[settings]` TOML. The `Err` text becomes a
    /// `config` error.
    pub fn parse(text: &str) -> Result<Settings, String> {
        let raw: Raw = toml::from_str(text).map_err(|e| format!("settings: {}", e.message()))?;
        let contact = raw
            .user_agent_contact
            .as_deref()
            .ok_or_else(|| "settings.user_agent_contact: required".to_string())?;
        let user_agent = user_agent(contact, "reddit")?;
        Ok(Settings {
            subreddits: raw.subreddits,
            user_agent,
        })
    }

    /// The feeds to poll, in order.
    pub fn feeds(&self) -> Vec<Feed> {
        self.subreddits
            .iter()
            .map(|name| Feed {
                subreddit: name.clone(),
                listing: "new",
                key: format!("r/{}:new", name.to_ascii_lowercase()),
            })
            .collect()
    }

    /// The `User-Agent` header value every request carries.
    pub fn user_agent(&self) -> &str {
        &self.user_agent
    }

    /// Page size for a feed that has no mark yet.
    pub fn first_page_limit(&self) -> u32 {
        DEFAULT_FIRST_POLL_LIMIT
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
    Ok(format!(
        "filimon:{app_id}:{} (by /u/{name})",
        env!("CARGO_PKG_VERSION")
    ))
}
