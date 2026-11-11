//! The per-feed high-water-mark cursor (D-11).
//!
//! The host stores the bytes verbatim between polls and between restarts, so
//! the layout is the compact JSON map `{"<feed key>":{"t":<seconds>,"n":"<fullname>"}}`.
//! Changing it later needs a migration or a one-poll reset.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Mirror of the host cap (`src/plugins/host/cursor.rs`). A cursor over this
/// makes the host disable the plugin.
pub const MAX_CURSOR_BYTES: usize = 4096;

/// The newest post seen in one feed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Mark {
    /// `created_utc` in whole seconds.
    pub t: i64,
    /// Fullname of that post.
    pub n: String,
}

/// One mark per feed key.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Cursor(pub BTreeMap<String, Mark>);

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
        cursor.0.insert(
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
    fn an_undecodable_cursor_is_flagged_and_treated_as_empty() {
        assert_eq!(decode(Some(b"\x00garbage")), (Cursor::default(), true));
        assert_eq!(decode(None), (Cursor::default(), false));
    }
}
