//! The per-feed high-water-mark cursor (D-11).
//!
//! The host stores the bytes verbatim between polls and between restarts, so
//! the layout is the compact JSON map `{"<feed key>":<state>}`. A subreddit or
//! `home` feed stores a time mark `{"t":<seconds>,"n":"<fullname>"}`; the
//! `saved` feed stores a seen set `{"s":["<fullname>",..]}` (D-19).
//! Changing it later needs a migration or a one-poll reset.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Mirror of the host cap (`src/plugins/host/cursor.rs`). A cursor over this
/// makes the host disable the plugin.
pub const MAX_CURSOR_BYTES: usize = 4096;

/// Bytes of the longest possible cursor entry plus its comma:
/// `"r/<21 chars>:<3 chars>":{"t":<10 digits>,"n":"t3_<10 chars>"},`
/// is 29 + 1 + 36 + 1 bytes. A test measures it against a real encode.
pub const WORST_ENTRY_BYTES: usize = 67;

/// Most feeds whose worst-case cursor still fits the host cap, leaving room
/// for the two braces.
pub const MAX_FEEDS: usize = (MAX_CURSOR_BYTES - 2) / WORST_ENTRY_BYTES;

/// The newest post seen in one feed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    /// `created_utc` in whole seconds.
    pub t: i64,
    /// Fullname of that post.
    pub n: String,
}

/// Most fullnames the `saved` feed remembers (D-19). 25 names of at most 20
/// bytes each are about 600 bytes, far under the host cap.
pub const SEEN_SET_MAX: usize = 25;

/// The fullnames at the top of the previous `saved` page, in listing order
/// (D-19): `{"s":["t3_..","t1_.."]}`. Comments are included so the stop rule
/// works when the first seen entry is a comment.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SeenSet {
    pub s: Vec<String>,
}

impl SeenSet {
    /// The seen set of a page: its first `SEEN_SET_MAX` fullnames.
    pub fn from_names<'a>(names: impl IntoIterator<Item = &'a str>) -> SeenSet {
        SeenSet {
            s: names
                .into_iter()
                .take(SEEN_SET_MAX)
                .map(str::to_string)
                .collect(),
        }
    }

    pub fn contains(&self, name: &str) -> bool {
        self.s.iter().any(|seen| seen == name)
    }
}

/// What one feed remembers (D-11, D-19). Serialised untagged, so a time mark
/// keeps its exact bytes `{"t":..,"n":".."}` and a seen set is `{"s":[..]}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FeedState {
    Time(Mark),
    Seen(SeenSet),
}

/// One state per feed key.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Cursor(pub BTreeMap<String, FeedState>);

impl Cursor {
    /// The time mark of one feed. `None` when the feed has no state or holds
    /// a value of the other shape, so a changed `feed` setting is a first
    /// poll of that feed (T-07-32).
    pub fn mark(&self, key: &str) -> Option<&Mark> {
        match self.0.get(key) {
            Some(FeedState::Time(mark)) => Some(mark),
            _ => None,
        }
    }

    /// The seen set of one feed, under the same wrong-shape rule as `mark`.
    pub fn seen(&self, key: &str) -> Option<&SeenSet> {
        match self.0.get(key) {
            Some(FeedState::Seen(seen)) => Some(seen),
            _ => None,
        }
    }

    /// Set (or replace) the time mark of one feed.
    pub fn set_mark(&mut self, key: String, mark: Mark) {
        self.0.insert(key, FeedState::Time(mark));
    }

    /// Set (or replace) the seen set of one feed.
    pub fn set_seen(&mut self, key: String, seen: SeenSet) {
        self.0.insert(key, FeedState::Seen(seen));
    }

    /// Forget one feed's state.
    pub fn clear(&mut self, key: &str) {
        self.0.remove(key);
    }

    /// Drop the marks of feeds that are no longer configured, so the cursor
    /// cannot grow without bound as the settings change.
    pub fn retain_feeds(&mut self, keys: &[String]) {
        self.0.retain(|key, _| keys.contains(key));
    }
}

/// Decode the cursor the host passed back. The `bool` is true when bytes were
/// present but undecodable: the caller treats that as a first poll with a
/// warning, so a layout change self-heals.
pub fn decode(bytes: Option<&[u8]>) -> (Cursor, bool) {
    match bytes {
        None => (Cursor::default(), false),
        Some(bytes) => match serde_json::from_slice::<Cursor>(bytes) {
            Ok(cursor) => (cursor, false),
            Err(_) => (Cursor::default(), true),
        },
    }
}

/// Encode compactly. Fails when the JSON would exceed `MAX_CURSOR_BYTES`.
pub fn encode(cursor: &Cursor) -> Result<Vec<u8>, String> {
    let bytes = serde_json::to_vec(cursor).map_err(|_| "cursor could not be encoded".to_string())?;
    if bytes.len() > MAX_CURSOR_BYTES {
        return Err(format!(
            "cursor would be {} bytes (cap {MAX_CURSOR_BYTES})",
            bytes.len()
        ));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_encoded_cursor_decodes_to_the_same_marks() {
        let mut cursor = Cursor::default();
        cursor.set_mark(
            "r/rust:new".to_string(),
            Mark {
                t: 1_700_000_300,
                n: "t3_1kx9ab2".to_string(),
            },
        );
        let bytes = encode(&cursor).unwrap();
        assert_eq!(
            std::str::from_utf8(&bytes).unwrap(),
            r#"{"r/rust:new":{"t":1700000300,"n":"t3_1kx9ab2"}}"#
        );
        assert_eq!(decode(Some(&bytes)), (cursor, false));
    }

    #[test]
    fn marks_are_read_set_and_dropped_for_feeds_that_are_no_longer_configured() {
        let mark = |t: i64, n: &str| Mark {
            t,
            n: n.to_string(),
        };
        let mut cursor = Cursor::default();
        assert_eq!(cursor.mark("r/rust:new"), None);
        cursor.set_mark("r/rust:new".to_string(), mark(1, "t3_a"));
        cursor.set_mark("r/old:new".to_string(), mark(2, "t3_b"));
        cursor.set_mark("r/rust:new".to_string(), mark(3, "t3_c"));
        assert_eq!(cursor.mark("r/rust:new"), Some(&mark(3, "t3_c")));

        cursor.retain_feeds(&["r/rust:new".to_string(), "r/other:hot".to_string()]);
        assert_eq!(cursor.0.len(), 1);
        assert!(cursor.mark("r/old:new").is_none());
        cursor.retain_feeds(&[]);
        assert!(cursor.0.is_empty());
    }

    #[test]
    fn a_seen_set_and_a_time_mark_round_trip_and_the_wrong_shape_reads_as_no_state() {
        let mut cursor = Cursor::default();
        cursor.set_seen(
            "saved".to_string(),
            SeenSet::from_names(["t3_a", "t1_b"].into_iter()),
        );
        cursor.set_mark(
            "home".to_string(),
            Mark {
                t: 5,
                n: "t3_h".to_string(),
            },
        );
        let bytes = encode(&cursor).unwrap();
        assert_eq!(
            std::str::from_utf8(&bytes).unwrap(),
            r#"{"home":{"t":5,"n":"t3_h"},"saved":{"s":["t3_a","t1_b"]}}"#
        );
        let (decoded, undecodable) = decode(Some(&bytes));
        assert!(!undecodable);
        assert_eq!(decoded, cursor);
        assert!(decoded.seen("saved").unwrap().contains("t1_b"));
        assert!(decoded.mark("saved").is_none() && decoded.seen("home").is_none());
        assert!(decoded.mark("missing").is_none() && decoded.seen("missing").is_none());
    }

    #[test]
    fn a_seen_set_keeps_at_most_the_first_25_names() {
        let names: Vec<String> = (0..40).map(|i| format!("t3_{i}")).collect();
        let set = SeenSet::from_names(names.iter().map(String::as_str));
        assert_eq!(set.s.len(), SEEN_SET_MAX);
        assert_eq!(set.s[0], "t3_0");
        assert_eq!(set.s[24], "t3_24");
    }

    #[test]
    fn an_undecodable_cursor_is_flagged_and_treated_as_empty() {
        assert_eq!(decode(Some(b"\x00garbage")), (Cursor::default(), true));
        assert_eq!(decode(None), (Cursor::default(), false));
    }

    /// The longest possible entry: a 21-character subreddit, a 3-character
    /// listing, a 10-digit time and a 13-character fullname.
    fn worst_cursor(count: usize) -> Cursor {
        let mut cursor = Cursor::default();
        for i in 0..count {
            cursor.set_mark(
                format!("r/{i:0>21}:hot"),
                Mark {
                    t: 9_999_999_999,
                    n: "t3_abcdefghij".to_string(),
                },
            );
        }
        cursor
    }

    #[test]
    fn cursor_size_at_the_configured_cap_fits_the_host_limit() {
        // One entry plus its comma, measured rather than assumed.
        let one = encode(&worst_cursor(1)).unwrap().len();
        assert_eq!(WORST_ENTRY_BYTES, one - 1);
        assert_eq!(MAX_FEEDS, (MAX_CURSOR_BYTES - 2) / WORST_ENTRY_BYTES);

        let at_cap = encode(&worst_cursor(MAX_FEEDS)).unwrap();
        assert!(at_cap.len() <= MAX_CURSOR_BYTES, "{} bytes", at_cap.len());

        let over = encode(&worst_cursor(MAX_FEEDS + 1));
        assert!(over.is_err(), "one more worst-case entry must not fit");
    }
}
