//! The per-feed publication-mark cursor.
//!
//! The host stores the bytes verbatim between polls and restarts, so the layout
//! is the compact JSON map `{"<feed hash>":[<epoch ms>,[<id hash>,...]]}`. Each
//! mark is the newest publication time seen in that feed plus the hashes of the
//! entries published at exactly that time (so an entry sharing the newest
//! timestamp is neither repeated nor lost). Changing the layout later needs a
//! migration or a one-poll reset.

use crate::text::fnv1a;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Mirror of the host cap (`src/plugins/host/cursor.rs`). A cursor over this
/// makes the host disable the plugin.
pub const MAX_CURSOR_BYTES: usize = 4096;
/// Entry hashes kept per feed for entries sharing the newest timestamp.
pub const MAX_IDS_PER_MARK: usize = 8;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Cursor(pub BTreeMap<String, (i64, Vec<u32>)>);

/// The stable cursor key of a feed URL.
pub fn feed_key(url: &str) -> String {
    format!("{:08x}", fnv1a(url))
}

impl Cursor {
    pub fn mark(&self, key: &str) -> Option<&(i64, Vec<u32>)> {
        self.0.get(key)
    }

    pub fn set(&mut self, key: &str, time_ms: i64, ids: Vec<u32>) {
        self.0.insert(key.to_string(), (time_ms, ids));
    }

    /// Drop the marks of feeds that are no longer configured.
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
        return Err(format!("cursor is {} bytes, over the {MAX_CURSOR_BYTES}-byte cap", bytes.len()));
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::MAX_FEEDS;

    #[test]
    fn round_trips() {
        let mut c = Cursor::default();
        c.set(&feed_key("https://a.example/feed"), 1_749_556_800_000, vec![1, 2]);
        let bytes = encode(&c).unwrap();
        assert_eq!(decode(Some(&bytes)), (c, false));
    }

    #[test]
    fn garbage_is_a_first_poll_with_a_flag() {
        assert_eq!(decode(Some(b"not json")), (Cursor::default(), true));
        assert_eq!(decode(None), (Cursor::default(), false));
    }

    #[test]
    fn worst_case_cursor_fits_the_host_cap() {
        let mut c = Cursor::default();
        for i in 0..MAX_FEEDS {
            c.set(
                &feed_key(&format!("https://worst{i}.example/")),
                9_999_999_999_999,
                vec![u32::MAX; MAX_IDS_PER_MARK],
            );
        }
        let bytes = encode(&c).expect("MAX_FEEDS worst-case marks must fit");
        assert!(bytes.len() <= MAX_CURSOR_BYTES, "{}", bytes.len());
    }
}
