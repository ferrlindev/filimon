//! The per-playlist publication-mark cursor (D-11).
//!
//! The host stores the bytes verbatim between polls and between restarts, so
//! the layout is the compact JSON map `{"<playlist id>":[<epoch ms>,"<video id>"]}`.
//! Changing it later needs a migration or a one-poll reset.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Mirror of the host cap (`src/plugins/host/cursor.rs`). A cursor over this
/// makes the host disable the plugin.
pub const MAX_CURSOR_BYTES: usize = 4096;

/// Bytes of the longest possible cursor entry plus its comma:
/// `"<24 chars>":[<13 digits>,"<11 chars>"],` is 26 + 1 + 1 + 13 + 1 + 13 + 1 + 1
/// bytes. A test measures it against a real encode.
pub const WORST_ENTRY_BYTES: usize = 57;

/// Most playlists whose worst-case cursor still fits the host cap, leaving room
/// for the two braces.
pub const MAX_FEEDS: usize = (MAX_CURSOR_BYTES - 2) / WORST_ENTRY_BYTES;

/// One mark per playlist id: the newest publication time (epoch milliseconds)
/// seen and the id of that video.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Cursor(pub BTreeMap<String, (i64, String)>);

impl Cursor {
    /// The mark of a playlist, if it has one.
    pub fn mark(&self, playlist_id: &str) -> Option<&(i64, String)> {
        self.0.get(playlist_id)
    }

    /// Set the mark of a playlist.
    pub fn set(&mut self, playlist_id: &str, time_ms: i64, video_id: &str) {
        self.0
            .insert(playlist_id.to_string(), (time_ms, video_id.to_string()));
    }

    /// Drop the marks of playlists that are no longer configured.
    pub fn retain_feeds(&mut self, playlist_ids: &[&str]) {
        self.0.retain(|key, _| playlist_ids.contains(&key.as_str()));
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
        cursor.set("UU_x5XG1OV2P6uZZ5FSM9Ttw", 1_700_000_300_000, "vid_a");
        let bytes = encode(&cursor).unwrap();
        assert_eq!(
            std::str::from_utf8(&bytes).unwrap(),
            r#"{"UU_x5XG1OV2P6uZZ5FSM9Ttw":[1700000300000,"vid_a"]}"#
        );
        assert_eq!(decode(Some(&bytes)), (cursor, false));
    }

    #[test]
    fn an_undecodable_cursor_is_flagged_and_treated_as_empty() {
        assert_eq!(decode(Some(b"\x00garbage")), (Cursor::default(), true));
        assert_eq!(decode(None), (Cursor::default(), false));
    }

    /// The longest possible entry: a 24-character playlist id, a 13-digit
    /// time and an 11-character video id.
    fn worst_cursor(count: usize) -> Cursor {
        let mut cursor = Cursor::default();
        for i in 0..count {
            cursor.set(&format!("UU{i:0>22}"), 9_999_999_999_999, "abcdefghijk");
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
