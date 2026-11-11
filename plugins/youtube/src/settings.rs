//! Operator `[settings]` for the YouTube plugin, parsed and validated at `init`.
//!
//! Every error text names the field and never repeats the rejected value (a
//! value can carry CR or LF, and a config error is logged by the host).

use crate::cursor::MAX_FEEDS;
use serde::Deserialize;
use toml::Value;

/// Page size of the first poll of a playlist (D-10, not overridable).
pub const FIRST_POLL_LIMIT: u32 = 10;
/// YouTube's page maximum for `playlistItems.list`.
pub const MAX_LATER_LIMIT: u32 = 50;
/// Mirror of the host cap `MAX_ITEMS_PER_POLL` in `src/plugins/host/map.rs`:
/// the host keeps the first 1000 items of a poll and silently drops the rest.
pub const MAX_ITEMS_PER_POLL: usize = 1000;
/// Default `subscriptions_limit`: subscribed channels one poll follows (D-06).
pub const DEFAULT_SUBSCRIPTIONS_LIMIT: u32 = 50;
/// YouTube's page maximum for `subscriptions.list`.
pub const SUBSCRIPTIONS_PAGE_SIZE: u32 = 50;
/// Hard cap on `subscriptions.list` requests in one poll, so a hostile endless
/// `nextPageToken` cannot loop the plugin (T-07-38).
pub const MAX_SUBSCRIPTION_PAGES: u32 = 10;

/// Quota units one poll costs. Every Data API call used here is one unit:
/// `ceil(subscription_channels / 50)` `subscriptions.list` pages plus one
/// `playlistItems.list` per polled playlist (research O4).
pub fn quota_units_per_poll(subscription_channels: u32, playlists: u32) -> u32 {
    let _ = (subscription_channels, playlists);
    0
}

/// The raw `[settings]` table. Unknown keys are rejected. Values stay untyped
/// so a wrong type is reported by this module, which never echoes the value
/// (serde's own type errors quote it).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Raw {
    channels: Option<Value>,
}

/// A channel ID is `UC` followed by 22 characters from letters, digits, `_`
/// and `-` (24 characters in total).
fn valid_channel_id(id: &str) -> bool {
    id.len() == 24
        && id.starts_with("UC")
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// The uploads playlist of a channel: the `UC` prefix swapped for `UU`. This is
/// an undocumented community convention (research T3a); it needs no lookup call.
pub fn uploads_playlist(channel_id: &str) -> String {
    match channel_id.strip_prefix("UC") {
        Some(rest) => format!("UU{rest}"),
        None => channel_id.to_string(),
    }
}

fn parse_channels(value: Option<Value>) -> Result<Vec<String>, String> {
    const RULE: &str = "settings.channels: required, a non-empty list of channel IDs (24 characters starting with UC)";
    let Some(Value::Array(entries)) = value else {
        return Err(RULE.to_string());
    };
    if entries.is_empty() {
        return Err(RULE.to_string());
    }
    let mut ids: Vec<String> = Vec::new();
    for (index, entry) in entries.into_iter().enumerate() {
        let id = match entry {
            Value::String(id) if valid_channel_id(&id) => id,
            _ => {
                return Err(format!(
                    "settings.channels[{index}]: expected a channel ID (24 characters starting with UC); channel handles are not supported"
                ));
            }
        };
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    if ids.len() > MAX_FEEDS {
        return Err(format!(
            "settings.channels: at most {MAX_FEEDS} channels are supported"
        ));
    }
    Ok(ids)
}

/// One channel and its derived uploads playlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playlist {
    pub channel_id: String,
    pub playlist_id: String,
}

/// Validated settings.
#[derive(Debug, Clone)]
pub struct Settings {
    channels: Vec<String>,
}

impl Settings {
    /// Parse and validate the raw `[settings]` TOML. The `Err` text becomes a
    /// `config` error naming the field and never repeating the rejected value.
    pub fn parse(text: &str) -> Result<Settings, String> {
        let raw: Raw = match toml::from_str(text) {
            Ok(raw) => raw,
            Err(error) => {
                let message = error.message();
                // serde's unknown-field text quotes the key: keep it out.
                return Err(if message.contains("unknown field") {
                    "settings: unknown key; the only supported key is `channels`".to_string()
                } else {
                    format!("settings: {message}")
                });
            }
        };
        Ok(Settings {
            channels: parse_channels(raw.channels)?,
        })
    }

    /// The uploads playlist of every configured channel, in configured order.
    pub fn playlists(&self) -> Vec<Playlist> {
        self.channels
            .iter()
            .map(|channel_id| Playlist {
                channel_id: channel_id.clone(),
                playlist_id: uploads_playlist(channel_id),
            })
            .collect()
    }

    /// The subscribed-channel limit when `subscriptions = true`, else `None`.
    pub fn subscriptions(&self) -> Option<u32> {
        None
    }

    /// Page size of a poll that has a mark: the largest page, shrunk so one
    /// poll over `feeds` playlists never exceeds the host's item cap.
    pub fn later_page_limit(feeds: usize) -> u32 {
        let share = MAX_ITEMS_PER_POLL / feeds.max(1);
        share.clamp(1, MAX_LATER_LIMIT as usize) as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CHANNEL: &str = "UC_x5XG1OV2P6uZZ5FSM9Ttw";

    fn parse(channels: &str) -> Result<Settings, String> {
        Settings::parse(&format!("channels = {channels}\n"))
    }

    #[test]
    fn uploads_playlist_is_derived_from_the_channel_id_by_swapping_uc_for_uu() {
        assert_eq!(uploads_playlist(CHANNEL), "UU_x5XG1OV2P6uZZ5FSM9Ttw");
        let settings = parse(&format!("[\"{CHANNEL}\"]")).unwrap();
        assert_eq!(
            settings.playlists(),
            [Playlist {
                channel_id: CHANNEL.to_string(),
                playlist_id: "UU_x5XG1OV2P6uZZ5FSM9Ttw".to_string(),
            }]
        );
    }

    #[test]
    fn channel_handles_and_bad_ids_are_rejected_naming_channels() {
        let bad = [
            "@googledevelopers",
            "",
            "UC_x5XG1OV2P6uZZ5FSM9Tt",    // 23 characters
            "UC_x5XG1OV2P6uZZ5FSM9Ttwa",  // 25 characters
            "uc_x5XG1OV2P6uZZ5FSM9Ttw",   // lower-case prefix
            "UC_x5XG1OV2P6uZZ5FSM9Tt w",  // forbidden character
        ];
        for value in bad {
            let error = parse(&format!("[\"{value}\"]")).unwrap_err();
            assert!(error.contains("channels"), "{error}");
            if !value.is_empty() {
                assert!(!error.contains(value), "{error}");
            }
        }
        // A non-string entry is rejected the same way.
        assert!(parse("[1]").unwrap_err().contains("channels"));
        assert!(parse(&format!("[\"{CHANNEL}\"]")).is_ok());
    }

    #[test]
    fn settings_errors_name_the_field_and_never_echo_the_value() {
        let unknown = Settings::parse("channels = []\nsecret_key = \"hunter2\"\n").unwrap_err();
        assert!(unknown.contains("settings"), "{unknown}");
        assert!(!unknown.contains("secret_key") && !unknown.contains("hunter2"));

        let missing = Settings::parse("").unwrap_err();
        assert!(missing.contains("channels"), "{missing}");

        let empty = parse("[]").unwrap_err();
        assert!(empty.contains("channels"), "{empty}");

        let wrong_type = parse("\"hunter2\"").unwrap_err();
        assert!(wrong_type.contains("channels") && !wrong_type.contains("hunter2"));

        let leaked = parse("[\"@hunter2\"]").unwrap_err();
        assert!(!leaked.contains("hunter2"), "{leaked}");
    }

    fn ids(count: usize) -> String {
        let list: Vec<String> = (0..count).map(|i| format!("\"UC{i:0>22}\"")).collect();
        format!("[{}]", list.join(", "))
    }

    #[test]
    fn channel_count_over_the_cursor_capacity_is_rejected_naming_channels() {
        assert_eq!(parse(&ids(MAX_FEEDS)).unwrap().playlists().len(), MAX_FEEDS);
        let error = parse(&ids(MAX_FEEDS + 1)).unwrap_err();
        assert!(error.contains("channels"), "{error}");
    }

    #[test]
    fn page_limit_shrinks_so_one_poll_never_exceeds_1000_items() {
        assert_eq!(Settings::later_page_limit(1), 50);
        assert_eq!(Settings::later_page_limit(20), 50);
        assert_eq!(Settings::later_page_limit(21), 47);
        assert_eq!(Settings::later_page_limit(100), 10);
        assert_eq!(Settings::later_page_limit(5000), 1);
        assert_eq!(Settings::later_page_limit(0), 50);
        for feeds in 1..=MAX_FEEDS {
            let per = Settings::later_page_limit(feeds) as usize;
            assert!(per * feeds <= MAX_ITEMS_PER_POLL, "{feeds} feeds");
            assert!(FIRST_POLL_LIMIT as usize * feeds <= MAX_ITEMS_PER_POLL);
        }
    }

    // ── Plan 07-09: subscriptions ────────────────────────────────────────────

    #[test]
    fn subscriptions_true_makes_channels_optional_but_one_source_is_required() {
        assert!(Settings::parse("subscriptions = true\n").is_ok());
        assert!(Settings::parse("subscriptions = true\nchannels = []\n").is_ok());
        assert!(parse(&format!("[\"{CHANNEL}\"]")).is_ok());
        for text in [
            "",
            "subscriptions = false\n",
            "subscriptions = false\nchannels = []\n",
        ] {
            let error = Settings::parse(text).unwrap_err();
            assert!(
                error.contains("channels") && error.contains("subscriptions"),
                "{error}"
            );
        }
        // A wrong type is still named and never echoed.
        let error = Settings::parse("subscriptions = \"hunter2\"\n").unwrap_err();
        assert!(error.contains("subscriptions") && !error.contains("hunter2"), "{error}");
    }

    #[test]
    fn subscriptions_limit_defaults_to_50_and_is_range_checked() {
        let default = Settings::parse("subscriptions = true\n").unwrap();
        assert_eq!(default.subscriptions(), Some(DEFAULT_SUBSCRIPTIONS_LIMIT));
        assert_eq!(DEFAULT_SUBSCRIPTIONS_LIMIT, 50);
        for limit in [1, MAX_FEEDS] {
            let settings =
                Settings::parse(&format!("subscriptions = true\nsubscriptions_limit = {limit}\n"))
                    .unwrap();
            assert_eq!(settings.subscriptions(), Some(limit as u32));
        }
        let too_big = (MAX_FEEDS + 1).to_string();
        for bad in ["0", too_big.as_str(), "-1", "1.5", "\"hunter2\"", "true"] {
            let error =
                Settings::parse(&format!("subscriptions = true\nsubscriptions_limit = {bad}\n"))
                    .unwrap_err();
            assert!(error.contains("subscriptions_limit"), "{error}");
            if bad.len() > 1 {
                assert!(!error.contains(bad.trim_matches('"')), "{error}");
            }
        }
        // `subscriptions = false` and no flag at all leave the feature off.
        assert_eq!(parse(&format!("[\"{CHANNEL}\"]")).unwrap().subscriptions(), None);
    }

    #[test]
    fn subscriptions_limit_without_subscriptions_is_rejected() {
        for text in [
            format!("channels = [\"{CHANNEL}\"]\nsubscriptions_limit = 10\n"),
            format!("channels = [\"{CHANNEL}\"]\nsubscriptions = false\nsubscriptions_limit = 10\n"),
        ] {
            let error = Settings::parse(&text).unwrap_err();
            assert!(error.contains("subscriptions_limit"), "{error}");
        }
    }

    #[test]
    fn channels_plus_subscriptions_limit_over_the_cursor_capacity_is_rejected_naming_subscriptions_limit()
     {
        let explicit = MAX_FEEDS - 10;
        let text = |limit: usize| {
            format!(
                "channels = {}\nsubscriptions = true\nsubscriptions_limit = {limit}\n",
                ids(explicit)
            )
        };
        assert!(Settings::parse(&text(10)).is_ok(), "a sum equal to the cap fits");
        let error = Settings::parse(&text(11)).unwrap_err();
        assert!(error.contains("subscriptions_limit"), "{error}");
    }

    #[test]
    fn quota_units_per_poll_formula() {
        assert_eq!(quota_units_per_poll(0, 20), 20);
        assert_eq!(quota_units_per_poll(50, 50), 51);
        assert_eq!(quota_units_per_poll(51, 51), 53);
        assert_eq!(quota_units_per_poll(100, 100), 102);
        assert_eq!(quota_units_per_poll(0, 0), 0);
    }

    #[test]
    fn settings_collapse_duplicate_channels_keeping_order() {
        let other = "UCaaaaaaaaaaaaaaaaaaaaaa";
        let settings = parse(&format!("[\"{CHANNEL}\", \"{other}\", \"{CHANNEL}\"]")).unwrap();
        let ids: Vec<String> = settings
            .playlists()
            .into_iter()
            .map(|p| p.channel_id)
            .collect();
        assert_eq!(ids, [CHANNEL, other]);
    }
}
