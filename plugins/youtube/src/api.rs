//! YouTube Data API: the `playlistItems.list` request, the page parser and the
//! video to item mapping (D-09, D-20). Only the official API host is ever
//! requested, and the API key travels in a header, never in the url.

use crate::http::HttpRequest;
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
