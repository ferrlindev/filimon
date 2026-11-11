//! Fixture replay: the plain-Rust core driven through a recording mock of the
//! `Http` seam. No network, no keys, no environment, no shared state, so the
//! default parallel runner and a single-threaded run give the same result.

use serde_json::Value;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use youtube_source::cursor::{self, MAX_CURSOR_BYTES};
use youtube_source::http::{Http, HttpError, HttpRequest, HttpResponse};
use youtube_source::poll::{PollOutcome, poll};
use youtube_source::settings::Settings;

/// A scripted `Http`: answers from a queue and records every request.
struct ScriptedHttp {
    responses: RefCell<VecDeque<Result<HttpResponse, HttpError>>>,
    requests: RefCell<Vec<HttpRequest>>,
}

impl ScriptedHttp {
    fn new(responses: Vec<Result<HttpResponse, HttpError>>) -> Self {
        ScriptedHttp {
            responses: RefCell::new(responses.into()),
            requests: RefCell::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<HttpRequest> {
        self.requests.borrow().clone()
    }
}

impl Http for ScriptedHttp {
    fn get(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError> {
        self.requests.borrow_mut().push(request.clone());
        self.responses
            .borrow_mut()
            .pop_front()
            .unwrap_or_else(|| Err(HttpError::Transport("script exhausted".to_string())))
    }
}

fn fixture_path(name: &str, extension: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(format!("{name}.{extension}"))
}

/// Load `tests/fixtures/<name>.json` as the response body, with status 200 and
/// no headers unless a `<name>.meta.json` says otherwise.
fn fixture(name: &str) -> HttpResponse {
    let body = std::fs::read(fixture_path(name, "json")).expect("fixture body");
    let (status, headers) = match std::fs::read(fixture_path(name, "meta.json")) {
        Ok(meta) => {
            let meta: Value = serde_json::from_slice(&meta).expect("fixture meta");
            let status = meta["status"].as_u64().expect("meta status") as u16;
            let headers = meta["headers"]
                .as_object()
                .map(|map| {
                    map.iter()
                        .map(|(k, v)| (k.to_ascii_lowercase(), v.as_str().unwrap().to_string()))
                        .collect()
                })
                .unwrap_or_default();
            (status, headers)
        }
        Err(_) => (200, Vec::new()),
    };
    HttpResponse {
        status,
        headers,
        body,
    }
}

fn settings(text: &str) -> Settings {
    Settings::parse(text).expect("settings")
}

const CHANNEL: &str = "UC_x5XG1OV2P6uZZ5FSM9Ttw";
const PLAYLIST: &str = "UU_x5XG1OV2P6uZZ5FSM9Ttw";
const KEY: &str = "KEY-1";

fn one_channel() -> Settings {
    settings(&format!("channels = [\"{CHANNEL}\"]\n"))
}

fn first_poll(http: &ScriptedHttp) -> PollOutcome {
    poll(http, &one_channel(), Some(KEY), None, None).expect("first poll")
}

/// The entries of the fixture, as the API sent them.
fn fixture_entries() -> Vec<Value> {
    let root: Value = serde_json::from_slice(&fixture("playlist_page").body).unwrap();
    root["items"].as_array().unwrap().clone()
}

#[test]
fn first_poll_emits_one_page_for_the_channel() {
    let http = ScriptedHttp::new(vec![Ok(fixture("playlist_page"))]);
    let outcome = first_poll(&http);
    let entries = fixture_entries();
    assert_eq!(outcome.items.len(), entries.len());
    assert_eq!(entries.len(), 3);
    // Oldest first.
    let ids: Vec<&str> = outcome.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["vidOld00003", "vidMid00002", "vidNew00001"]);
}

#[test]
fn request_targets_the_uploads_playlist_with_the_api_key_header() {
    let http = ScriptedHttp::new(vec![Ok(fixture("playlist_page"))]);
    first_poll(&http);
    let requests = http.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(
        request.url,
        "https://www.googleapis.com/youtube/v3/playlistItems?part=snippet%2CcontentDetails&playlistId=UU_x5XG1OV2P6uZZ5FSM9Ttw&maxResults=10"
    );
    let header = |name: &str| youtube_source::http::header(&request.headers, name);
    assert_eq!(header("x-goog-api-key"), Some(KEY));
    assert_eq!(header("accept"), Some("application/json"));
    assert_eq!(header("authorization"), None);
    // The key is a header, never part of the url.
    assert!(!request.url.contains(KEY));
    assert!(!request.url.to_ascii_lowercase().contains("key"));
}

#[test]
fn item_mapping_follows_d09_with_d20_dates() {
    let http = ScriptedHttp::new(vec![Ok(fixture("playlist_page"))]);
    let outcome = first_poll(&http);
    let entries = fixture_entries();
    for item in &outcome.items {
        let entry = entries
            .iter()
            .find(|entry| entry["contentDetails"]["videoId"] == item.id.as_str())
            .expect("item matches an entry");
        let snippet = &entry["snippet"];
        assert_eq!(item.url, format!("https://www.youtube.com/watch?v={}", item.id));
        assert_eq!(item.title.as_deref(), snippet["title"].as_str());
        assert_eq!(item.text, snippet["description"].as_str().unwrap());
        assert_eq!(item.platform, "youtube");
        let expected_author = snippet["videoOwnerChannelTitle"]
            .as_str()
            .or_else(|| snippet["channelTitle"].as_str());
        assert_eq!(item.author.as_deref(), expected_author);
        let raw: Value = serde_json::from_str(item.raw.as_deref().unwrap()).unwrap();
        assert_eq!(&raw, snippet);
    }
    let by_id = |id: &str| outcome.items.iter().find(|i| i.id == id).unwrap();
    assert_eq!(
        by_id("vidNew00001").author.as_deref(),
        Some("Synthetic Channel Owner")
    );
    // No owner title: the playlist channel title is the fallback.
    assert_eq!(
        by_id("vidMid00002").author.as_deref(),
        Some("Synthetic Channel")
    );
}

#[test]
fn published_prefers_video_published_at_and_falls_back_to_snippet_published_at() {
    let http = ScriptedHttp::new(vec![Ok(fixture("playlist_page"))]);
    let outcome = first_poll(&http);
    let by_id = |id: &str| outcome.items.iter().find(|i| i.id == id).unwrap();
    // snippet.publishedAt is 2024-03-12T10:00:00Z; videoPublishedAt wins.
    assert_eq!(by_id("vidNew00001").published, Some(1_710_061_200_000));
    // No videoPublishedAt: snippet.publishedAt, with its fraction.
    assert_eq!(by_id("vidMid00002").published, Some(1_709_627_415_250));
    assert_eq!(by_id("vidOld00003").published, Some(1_709_276_400_000));
}

#[test]
fn returns_a_cursor_with_the_newest_publication_mark_per_playlist() {
    let http = ScriptedHttp::new(vec![Ok(fixture("playlist_page"))]);
    let outcome = first_poll(&http);
    let bytes = outcome.next_cursor.expect("cursor");
    assert!(bytes.len() <= MAX_CURSOR_BYTES);
    let json: Value = serde_json::from_slice(&bytes).expect("cursor is JSON");
    let map = json.as_object().unwrap();
    assert_eq!(map.len(), 1);
    let mark = map.get(PLAYLIST).expect("playlist key");
    assert_eq!(mark[0], 1_710_061_200_000_i64);
    assert_eq!(mark[1], "vidNew00001");
    let (decoded, undecodable) = cursor::decode(Some(&bytes));
    assert!(!undecodable);
    assert_eq!(
        decoded.mark(PLAYLIST),
        Some(&(1_710_061_200_000, "vidNew00001".to_string()))
    );
}

#[test]
fn second_poll_with_the_same_page_emits_nothing() {
    let http = ScriptedHttp::new(vec![
        Ok(fixture("playlist_page")),
        Ok(fixture("playlist_page")),
    ]);
    let first = first_poll(&http);
    let second = poll(
        &http,
        &one_channel(),
        Some(KEY),
        first.next_cursor.as_deref(),
        None,
    )
    .expect("second poll");
    assert!(second.items.is_empty());
    assert_eq!(second.next_cursor, first.next_cursor);
}

#[test]
fn a_missing_credential_is_auth_required_without_any_request() {
    let http = ScriptedHttp::new(vec![Ok(fixture("playlist_page"))]);
    let result = poll(&http, &one_channel(), None, None, None);
    assert_eq!(result.unwrap_err(), youtube_source::poll::PollError::AuthRequired);
    assert!(http.requests().is_empty());
}

#[test]
fn youtube_poll_is_deterministic_for_the_same_inputs() {
    let run = || {
        let http = ScriptedHttp::new(vec![Ok(fixture("playlist_page"))]);
        first_poll(&http)
    };
    let a = run();
    let b = run();
    assert_eq!(a.items, b.items);
    assert_eq!(a.next_cursor, b.next_cursor);
}
