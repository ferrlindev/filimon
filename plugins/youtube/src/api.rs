//! YouTube Data API: the `playlistItems.list` request, the page parser and the
//! video to item mapping (D-09, D-20). Only the official API host is ever
//! requested, and the API key travels in a header, never in the url.

use crate::http::HttpRequest;
use crate::poll::PollError;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::Value;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// The official Data API base.
pub const API_BASE: &str = "https://www.googleapis.com/youtube/v3";
/// Prefix for the public watch page of a video.
pub const WATCH_BASE: &str = "https://www.youtube.com/watch?v=";

/// Latest instant a cursor mark may carry: 13 digits of epoch milliseconds
/// (year 2286). It keeps the worst-case cursor entry size fixed.
const MAX_TIME_MS: i64 = 9_999_999_999_999;
/// A video id is at most 11 characters of letters, digits, `_` and `-`.
const MAX_VIDEO_ID_LEN: usize = 11;

/// Keep unreserved characters readable; encode everything else.
const COMPONENT: &AsciiSet = &NON_ALPHANUMERIC
    .remove(b'_')
    .remove(b'-')
    .remove(b'.')
    .remove(b'~');

fn enc(value: &str) -> String {
    utf8_percent_encode(value, COMPONENT).to_string()
}

/// One item, with the same field set as the WIT `item`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub id: String,
    pub url: String,
    pub title: Option<String>,
    pub text: String,
    pub author: Option<String>,
    pub published: Option<u64>,
    pub platform: String,
    pub raw: Option<String>,
}

/// Which credential the request carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Auth<'a> {
    /// An OAuth access token, sent as `authorization: Bearer <token>`.
    Bearer(&'a str),
    /// An API key, sent as the `x-goog-api-key` header.
    ApiKey(&'a str),
}

/// `GET {API_BASE}/playlistItems?part=snippet,contentDetails&playlistId=<id>&maxResults=<n>`
/// with the credential in a header and a JSON accept header (D-09, D-02).
pub fn playlist_request(playlist_id: &str, max_results: u32, auth: Auth<'_>) -> HttpRequest {
    let credential = match auth {
        Auth::ApiKey(secret) => ("x-goog-api-key".to_string(), secret.to_string()),
        Auth::Bearer(token) => ("authorization".to_string(), format!("Bearer {token}")),
    };
    HttpRequest {
        url: format!(
            "{API_BASE}/playlistItems?part=snippet%2CcontentDetails&playlistId={}&maxResults={max_results}",
            enc(playlist_id)
        ),
        headers: vec![
            credential,
            ("accept".to_string(), "application/json".to_string()),
        ],
    }
}

/// One playlist entry as the page carried it.
#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    /// The video id, when the entry has a valid one.
    pub video_id: Option<String>,
    pub title: String,
    pub description: String,
    pub channel_title: String,
    pub owner_title: String,
    /// Publication instant in epoch milliseconds by the D-20 rule, when the
    /// entry carries a time that parses.
    pub published_ms: Option<i64>,
    /// The entry's `snippet` as received.
    pub snippet: Option<Value>,
}

impl Entry {
    /// A private or deleted video, or an entry with no usable video id.
    pub fn is_placeholder(&self) -> bool {
        self.video_id.is_none()
            || self.title.eq_ignore_ascii_case("Private video")
            || self.title.eq_ignore_ascii_case("Deleted video")
    }
}

/// The parsed page.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedPage {
    pub entries: Vec<Entry>,
    /// Array elements that were not objects at all.
    pub skipped: usize,
}

/// Why a page could not be read. Carries no body text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    Unreadable,
}

fn text_of(object: Option<&Value>, key: &str) -> String {
    object
        .and_then(|value| value.get(key))
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn valid_video_id(id: &str) -> bool {
    (1..=MAX_VIDEO_ID_LEN).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// RFC 3339 to epoch milliseconds (fractions and offsets handled). `None` for
/// text that is not RFC 3339 or whose instant is outside 1970 to year 2286.
pub fn parse_rfc3339_ms(text: &str) -> Option<i64> {
    let instant = OffsetDateTime::parse(text, &Rfc3339).ok()?;
    let millis = instant.unix_timestamp_nanos().div_euclid(1_000_000);
    let millis = i64::try_from(millis).ok()?;
    (0..=MAX_TIME_MS).contains(&millis).then_some(millis)
}

fn parse_entry(entry: &Value) -> Entry {
    let snippet = entry.get("snippet").filter(|value| value.is_object());
    let details = entry.get("contentDetails").filter(|value| value.is_object());

    let video_id = [
        text_of(details, "videoId"),
        text_of(snippet.and_then(|s| s.get("resourceId")), "videoId"),
    ]
    .into_iter()
    .find(|id| !id.is_empty())
    .filter(|id| valid_video_id(id));

    // D-20: the video's own publication time when the API gives it, else the
    // time it was added to the playlist.
    let time_text = [
        text_of(details, "videoPublishedAt"),
        text_of(snippet, "publishedAt"),
    ]
    .into_iter()
    .find(|text| !text.is_empty())
    .unwrap_or_default();

    Entry {
        video_id,
        title: text_of(snippet, "title"),
        description: text_of(snippet, "description"),
        channel_title: text_of(snippet, "channelTitle"),
        owner_title: text_of(snippet, "videoOwnerChannelTitle"),
        published_ms: parse_rfc3339_ms(&time_text),
        snippet: snippet.cloned(),
    }
}

/// Parse a `playlistItems` page. A body without an `items` array is an error,
/// never an empty page. Nothing here echoes the body.
pub fn parse_playlist_page(body: &[u8]) -> Result<ParsedPage, ParseError> {
    let root: Value = serde_json::from_slice(body).map_err(|_| ParseError::Unreadable)?;
    let items = root
        .get("items")
        .and_then(Value::as_array)
        .ok_or(ParseError::Unreadable)?;
    let mut entries = Vec::new();
    let mut skipped = 0;
    for item in items {
        if item.is_object() {
            entries.push(parse_entry(item));
        } else {
            skipped += 1;
        }
    }
    Ok(ParsedPage { entries, skipped })
}

/// The D-15 quota text, verbatim. It is the same for `quotaExceeded` and
/// `dailyLimitExceeded`, so the operator reads one phrase for "wait for the reset".
pub const QUOTA_EXCEEDED_TEXT: &str =
    "YouTube quota exceeded (quotaExceeded): daily Data API quota exhausted; resets at midnight Pacific";

/// Longest Google error reason that may appear in an error text.
const MAX_REASON_LEN: usize = 40;

/// The first `error.errors[].reason` of a Google error body, only when it is
/// 1 to 40 characters of letters, digits and `_`. Anything else (a non-JSON
/// body, a missing field, an unsafe or long value) gives `None`, so a body
/// is never echoed into an error text.
pub fn error_reason(body: &[u8]) -> Option<String> {
    let root: Value = serde_json::from_slice(body).ok()?;
    let reason = root
        .get("error")?
        .get("errors")?
        .as_array()?
        .first()?
        .get("reason")?
        .as_str()?;
    let safe = (1..=MAX_REASON_LEN).contains(&reason.len())
        && reason.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    safe.then(|| reason.to_string())
}

/// Classify a response by status and Google error reason (D-15, research
/// Pattern 4). `None` for 2xx. The texts are fixed phrases plus the status,
/// the configured channel id and a safe reason; never a body, header,
/// credential or url.
pub fn classify_youtube(status: u16, body: &[u8], channel_id: &str) -> Option<PollError> {
    if (200..300).contains(&status) {
        return None;
    }
    let reason = error_reason(body);
    let reason = reason.as_deref();
    let error = match (status, reason) {
        (403, Some("quotaExceeded" | "dailyLimitExceeded")) => {
            PollError::Transient(QUOTA_EXCEEDED_TEXT.to_string())
        }
        (429, _) | (403, Some("rateLimitExceeded" | "userRateLimitExceeded")) => {
            PollError::RateLimited(None)
        }
        (401, _) => PollError::AuthRequired,
        (400, Some("keyInvalid")) => PollError::Config(
            "YouTube rejected the api_key secret (keyInvalid); check YOUTUBE_API_KEY".to_string(),
        ),
        (403, Some("accessNotConfigured")) => PollError::Config(
            "the YouTube Data API v3 is not enabled for the project of this key (accessNotConfigured)"
                .to_string(),
        ),
        (403, Some(reason @ ("insufficientPermissions" | "forbidden"))) => PollError::Config(format!(
            "YouTube refused access ({reason}); check the key restrictions or the login scope"
        )),
        (404, _) => PollError::Config(format!(
            "settings.channels: YouTube found no uploads playlist for channel {channel_id} (HTTP 404); check the channel ID"
        )),
        (status, Some(reason)) => PollError::Transient(format!("YouTube HTTP {status} ({reason})")),
        (status, None) => PollError::Transient(format!("YouTube HTTP {status}")),
    };
    Some(error)
}

/// Map an entry to an item (D-09, D-20). `None` for a placeholder or an entry
/// with no usable time.
pub fn video_to_item(entry: &Entry) -> Option<Item> {
    if entry.is_placeholder() {
        return None;
    }
    let id = entry.video_id.clone()?;
    let published = u64::try_from(entry.published_ms?).ok()?;
    let author = [&entry.owner_title, &entry.channel_title]
        .into_iter()
        .find(|name| !name.is_empty())
        .cloned();
    Some(Item {
        url: format!("{WATCH_BASE}{}", enc(&id)),
        id,
        title: (!entry.title.is_empty()).then(|| entry.title.clone()),
        text: entry.description.clone(),
        author,
        published: Some(published),
        platform: "youtube".to_string(),
        raw: entry
            .snippet
            .as_ref()
            .and_then(|snippet| serde_json::to_string(snippet).ok()),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_at_parsing_handles_fractions_offsets_and_garbage() {
        assert_eq!(parse_rfc3339_ms("2024-01-01T00:00:00Z"), Some(1_704_067_200_000));
        assert_eq!(parse_rfc3339_ms("2024-01-01T00:00:00.123Z"), Some(1_704_067_200_123));
        assert_eq!(parse_rfc3339_ms("2024-01-01T02:00:00+02:00"), Some(1_704_067_200_000));
        assert_eq!(parse_rfc3339_ms("not a date"), None);
        assert_eq!(parse_rfc3339_ms(""), None);
        // Outside 1970 to year 2286: skipped rather than breaking the cursor size.
        assert_eq!(parse_rfc3339_ms("1969-12-31T23:59:59Z"), None);
        assert_eq!(parse_rfc3339_ms("9999-12-31T23:59:59Z"), None);
    }

    fn body_with_reason(reason: &str) -> Vec<u8> {
        serde_json::to_vec(&serde_json::json!({
            "error": { "errors": [{ "reason": reason }] }
        }))
        .unwrap()
    }

    const CHANNEL: &str = "UC_x5XG1OV2P6uZZ5FSM9Ttw";

    #[test]
    fn error_reason_text_is_limited_to_a_safe_alphabet() {
        assert_eq!(
            error_reason(&body_with_reason("quotaExceeded")).as_deref(),
            Some("quotaExceeded")
        );
        for unsafe_reason in [
            "<script>alert(1)</script>".to_string(),
            "a".repeat(41),
            String::new(),
            "has space".to_string(),
            "line\nbreak".to_string(),
        ] {
            assert_eq!(error_reason(&body_with_reason(&unsafe_reason)), None);
            // And it never reaches the resulting error text.
            let error =
                classify_youtube(500, &body_with_reason(&unsafe_reason), CHANNEL).unwrap();
            let PollError::Transient(text) = error else {
                panic!("expected Transient");
            };
            assert_eq!(text, "YouTube HTTP 500");
        }
        assert_eq!(error_reason(b"not json"), None);
        assert_eq!(error_reason(b"{}"), None);
        // A boundary-length reason is accepted.
        assert!(error_reason(&body_with_reason(&"a".repeat(40))).is_some());
    }

    #[test]
    fn two_hundred_range_statuses_are_not_errors() {
        assert_eq!(classify_youtube(200, b"", CHANNEL), None);
        assert_eq!(classify_youtube(204, b"", CHANNEL), None);
        assert!(classify_youtube(300, b"", CHANNEL).is_some());
    }
}
