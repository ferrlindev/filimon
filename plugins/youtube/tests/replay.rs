//! Fixture replay: the plain-Rust core driven through a recording mock of the
//! `Http` seam. No network, no keys, no environment, no shared state, so the
//! default parallel runner and a single-threaded run give the same result.

use serde_json::Value;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;
use youtube_source::cursor::{self, MAX_CURSOR_BYTES};
use youtube_source::http::{Http, HttpError, HttpRequest, HttpResponse};
use youtube_source::poll::{PollError, PollOutcome, poll};
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
    assert_eq!(result.unwrap_err(), PollError::AuthRequired);
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

// ── Task 2: later polls, marks, placeholders, capacity ───────────────────────

/// A status 200 response carrying `body`.
fn json_page(body: &Value) -> HttpResponse {
    HttpResponse {
        status: 200,
        headers: Vec::new(),
        body: serde_json::to_vec(body).expect("json"),
    }
}

/// One schema-shaped playlist entry for the inline pages.
fn entry(video_id: &str, title: &str, published_at: &str) -> Value {
    serde_json::json!({
        "snippet": {
            "publishedAt": published_at,
            "title": title,
            "description": "d",
            "channelTitle": "C",
            "resourceId": { "videoId": video_id }
        },
        "contentDetails": { "videoId": video_id }
    })
}

fn page(entries: Vec<Value>) -> HttpResponse {
    json_page(&serde_json::json!({ "items": entries }))
}

fn channel_b() -> String {
    format!("UC{}", "b".repeat(22))
}

fn playlist_b() -> String {
    format!("UU{}", "b".repeat(22))
}

fn two_channels() -> Settings {
    settings(&format!(
        "channels = [\"{CHANNEL}\", \"{}\"]\n",
        channel_b()
    ))
}

fn later_poll(http: &ScriptedHttp, settings: &Settings, cursor: Option<&[u8]>) -> PollOutcome {
    poll(http, settings, Some(KEY), cursor, None).expect("poll")
}

fn urls(http: &ScriptedHttp) -> Vec<String> {
    http.requests().into_iter().map(|r| r.url).collect()
}

fn mark_of(outcome: &PollOutcome, playlist: &str) -> Option<(i64, String)> {
    let (cursor, undecodable) = cursor::decode(outcome.next_cursor.as_deref());
    assert!(!undecodable);
    cursor.mark(playlist).cloned()
}

#[test]
fn later_poll_emits_only_videos_strictly_newer_than_the_mark() {
    let http = ScriptedHttp::new(vec![
        Ok(fixture("playlist_page")),
        Ok(fixture("playlist_newer")),
    ]);
    let first = first_poll(&http);
    let second = later_poll(&http, &one_channel(), first.next_cursor.as_deref());
    let ids: Vec<&str> = second.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["vidNew00004", "vidNew00005"]);
    assert_eq!(
        mark_of(&second, PLAYLIST),
        Some((1_710_936_000_000, "vidNew00005".to_string()))
    );
    // A playlist with a mark takes the later page size: min(50, 1000 / 1).
    assert_eq!(
        urls(&http),
        [
            "https://www.googleapis.com/youtube/v3/playlistItems?part=snippet%2CcontentDetails&playlistId=UU_x5XG1OV2P6uZZ5FSM9Ttw&maxResults=10",
            "https://www.googleapis.com/youtube/v3/playlistItems?part=snippet%2CcontentDetails&playlistId=UU_x5XG1OV2P6uZZ5FSM9Ttw&maxResults=50",
        ]
    );
}

#[test]
fn a_video_published_at_the_mark_instant_with_a_different_id_is_not_re_emitted() {
    let http = ScriptedHttp::new(vec![
        Ok(page(vec![entry("vidA", "A", "2024-05-01T00:00:00Z")])),
        Ok(page(vec![entry("vidB", "B", "2024-05-01T00:00:00Z")])),
    ]);
    let first = first_poll(&http);
    assert_eq!(first.items.len(), 1);
    let second = later_poll(&http, &one_channel(), first.next_cursor.as_deref());
    assert!(second.items.is_empty());
    assert_eq!(second.next_cursor, first.next_cursor);
}

#[test]
fn the_mark_is_the_max_publication_time_not_the_first_item() {
    let http = ScriptedHttp::new(vec![Ok(page(vec![
        entry("vidA", "A", "2024-05-01T00:00:00Z"),
        entry("vidB", "B", "2024-05-03T00:00:00Z"),
        entry("vidC", "C", "2024-05-02T00:00:00Z"),
    ]))]);
    let outcome = first_poll(&http);
    assert_eq!(
        mark_of(&outcome, PLAYLIST),
        Some((1_714_694_400_000, "vidB".to_string()))
    );
}

#[test]
fn private_and_deleted_placeholders_are_skipped_but_still_advance_the_mark() {
    let http = ScriptedHttp::new(vec![Ok(fixture("playlist_placeholders"))]);
    let outcome = first_poll(&http);
    let ids: Vec<&str> = outcome.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["vidOk000001"]);
    // Three skipped entries are counted, in fixed text.
    assert_eq!(outcome.warnings.len(), 1, "{:?}", outcome.warnings);
    let warning = &outcome.warnings[0];
    assert!(warning.contains('3'), "{warning}");
    assert!(!warning.contains("Private") && !warning.contains("Deleted"));
    // The mark is the newest entry's time, placeholders included.
    let mark = mark_of(&outcome, PLAYLIST).expect("mark");
    assert_eq!(mark.0, 1_712_275_200_000);
}

#[test]
fn an_empty_playlist_page_yields_no_items_and_writes_no_mark() {
    let http = ScriptedHttp::new(vec![Ok(fixture("playlist_empty"))]);
    let outcome = first_poll(&http);
    assert!(outcome.items.is_empty());
    assert_eq!(mark_of(&outcome, PLAYLIST), None);
}

#[test]
fn a_json_body_without_an_items_array_is_an_unreadable_page_not_an_empty_one() {
    for body in ["{}", r#"{"pageInfo":{}}"#] {
        let http = ScriptedHttp::new(vec![Ok(HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: body.as_bytes().to_vec(),
        })]);
        let error = poll(&http, &one_channel(), Some(KEY), None, None).unwrap_err();
        let PollError::Transient(text) = error else {
            panic!("expected Transient, got {error:?}");
        };
        assert!(text.contains("not a readable playlist page"), "{text}");
        assert!(!text.contains("pageInfo"));
    }
}

#[test]
fn hostile_shapes_return_errors_or_skips_never_a_panic() {
    let nested = format!("{}{}", "[".repeat(300), "]".repeat(300));
    let nested_in_items = format!(r#"{{"items":{nested}}}"#);
    let bodies = [
        "null".to_string(),
        "[]".to_string(),
        r#"{"items":null}"#.to_string(),
        r#"{"items":[null,1,"x",{"snippet":3}]}"#.to_string(),
        nested,
        nested_in_items,
    ];
    for body in bodies {
        let http = ScriptedHttp::new(vec![Ok(HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: body.clone().into_bytes(),
        })]);
        match poll(&http, &one_channel(), Some(KEY), None, None) {
            Ok(outcome) => assert!(outcome.items.is_empty(), "{body}"),
            Err(PollError::Transient(_)) => {}
            Err(other) => panic!("unexpected {other:?} for {body}"),
        }
    }
}

#[test]
fn channels_have_independent_marks() {
    let http = ScriptedHttp::new(vec![
        Ok(fixture("playlist_page")),
        Ok(page(vec![entry("vidB1", "B1", "2024-06-01T00:00:00Z")])),
        Ok(fixture("playlist_page")),
        Ok(page(vec![
            entry("vidB2", "B2", "2024-06-02T00:00:00Z"),
            entry("vidB1", "B1", "2024-06-01T00:00:00Z"),
        ])),
    ]);
    let both = two_channels();
    let first = later_poll(&http, &both, None);
    let second = later_poll(&http, &both, first.next_cursor.as_deref());
    let ids: Vec<&str> = second.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["vidB2"]);
    assert_eq!(mark_of(&second, PLAYLIST), mark_of(&first, PLAYLIST));
    assert_ne!(
        mark_of(&second, &playlist_b()),
        mark_of(&first, &playlist_b())
    );
    assert_eq!(
        mark_of(&second, &playlist_b()),
        Some((1_717_286_400_000, "vidB2".to_string()))
    );
    // Two channels: the later page size is min(50, 1000 / 2).
    assert!(urls(&http)[2].ends_with("maxResults=50"));
}

#[test]
fn duplicate_channels_collapse_keeping_order() {
    let http = ScriptedHttp::new(vec![Ok(fixture("playlist_page")), Ok(fixture("playlist_empty"))]);
    let repeated = settings(&format!(
        "channels = [\"{CHANNEL}\", \"{}\", \"{CHANNEL}\"]\n",
        channel_b()
    ));
    later_poll(&http, &repeated, None);
    let requested = urls(&http);
    assert_eq!(requested.len(), 2);
    assert!(requested[0].contains(&format!("playlistId={PLAYLIST}")));
    assert!(requested[1].contains(&format!("playlistId={}", playlist_b())));
}

#[test]
fn output_is_oldest_first_with_ties_broken_by_video_id() {
    let http = ScriptedHttp::new(vec![Ok(page(vec![
        entry("vidC", "C", "2024-05-02T00:00:00Z"),
        entry("vidB", "B", "2024-05-01T00:00:00Z"),
        entry("vidA", "A", "2024-05-01T00:00:00Z"),
    ]))]);
    let outcome = first_poll(&http);
    let ids: Vec<&str> = outcome.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["vidA", "vidB", "vidC"]);
}

#[test]
fn an_all_empty_first_poll_still_returns_a_cursor_not_none() {
    let http = ScriptedHttp::new(vec![Ok(fixture("playlist_empty"))]);
    let outcome = first_poll(&http);
    let bytes = outcome.next_cursor.expect("a None cursor would reset the host's mark");
    let (decoded, undecodable) = cursor::decode(Some(&bytes));
    assert!(!undecodable);
    assert!(decoded.0.is_empty());
}

#[test]
fn marks_for_playlists_no_longer_configured_are_dropped() {
    let http = ScriptedHttp::new(vec![Ok(fixture("playlist_page"))]);
    let mut previous = cursor::Cursor::default();
    previous.set(&playlist_b(), 1_717_286_400_000, "vidB2");
    previous.set(PLAYLIST, 1_709_000_000_000, "vidOld");
    let bytes = cursor::encode(&previous).unwrap();
    let outcome = later_poll(&http, &one_channel(), Some(&bytes));
    let (decoded, _) = cursor::decode(outcome.next_cursor.as_deref());
    assert_eq!(decoded.mark(&playlist_b()), None);
    assert!(decoded.mark(PLAYLIST).is_some());
    assert_eq!(decoded.0.len(), 1);
}

#[test]
fn an_undecodable_cursor_is_a_first_poll_with_a_warning() {
    let http = ScriptedHttp::new(vec![Ok(fixture("playlist_page"))]);
    let outcome = later_poll(&http, &one_channel(), Some(b"not json"));
    assert_eq!(outcome.items.len(), 3);
    assert_eq!(outcome.warnings.len(), 1, "{:?}", outcome.warnings);
    assert!(outcome.warnings[0].contains("cursor"));
    assert!(urls(&http)[0].ends_with("maxResults=10"));
}

#[test]
fn a_failed_channel_fails_the_whole_poll_with_no_items_and_no_new_cursor() {
    let http = ScriptedHttp::new(vec![
        Ok(fixture("playlist_page")),
        Err(HttpError::Timeout),
    ]);
    let result = poll(&http, &two_channels(), Some(KEY), None, None);
    assert!(matches!(result, Err(PollError::Transient(_))), "{result:?}");
}

#[test]
fn fetch_level_errors_map_to_the_poll_error_variants() {
    let run = |error: HttpError| {
        let http = ScriptedHttp::new(vec![Err(error)]);
        poll(&http, &one_channel(), Some(KEY), None, None).unwrap_err()
    };
    assert!(matches!(
        run(HttpError::Denied("secret-detail".to_string())),
        PollError::Config(_)
    ));
    assert!(matches!(run(HttpError::NotConfigured), PollError::Config(_)));
    assert!(matches!(run(HttpError::Timeout), PollError::Transient(_)));
    assert!(matches!(
        run(HttpError::Transport("secret-detail".to_string())),
        PollError::Transient(_)
    ));
    assert_eq!(
        run(HttpError::RateLimited(Some(30))),
        PollError::RateLimited(Some(30))
    );
    assert!(matches!(
        run(HttpError::InvalidRequest("secret-detail".to_string())),
        PollError::Fatal(_)
    ));
    for error in [
        run(HttpError::Denied("secret-detail".to_string())),
        run(HttpError::Transport("secret-detail".to_string())),
        run(HttpError::InvalidRequest("secret-detail".to_string())),
    ] {
        let (PollError::Config(text) | PollError::Transient(text) | PollError::Fatal(text)) = error
        else {
            unreachable!()
        };
        assert!(!text.contains("secret-detail"), "{text}");
    }
}

#[test]
fn a_video_listed_by_two_playlists_is_emitted_once() {
    let shared = entry("vidShared", "S", "2024-05-01T00:00:00Z");
    let http = ScriptedHttp::new(vec![
        Ok(page(vec![shared.clone()])),
        Ok(page(vec![shared])),
    ]);
    let outcome = later_poll(&http, &two_channels(), None);
    assert_eq!(outcome.items.len(), 1);
}

#[test]
fn only_the_official_api_host_and_playlist_items_are_ever_requested() {
    let http = ScriptedHttp::new(vec![
        Ok(fixture("playlist_page")),
        Ok(fixture("playlist_page")),
        Ok(fixture("playlist_newer")),
    ]);
    let first = first_poll(&http);
    later_poll(&http, &one_channel(), first.next_cursor.as_deref());
    for request in http.requests() {
        assert!(
            request
                .url
                .starts_with("https://www.googleapis.com/youtube/v3/playlistItems?"),
            "{}",
            request.url
        );
        assert!(!request.url.contains("/search") && !request.url.contains("/videos"));
    }
}

// ── Plan 07-08 Task 2: credential modes and the low-cost request sweep ──────

/// A strict recording mock: it answers every request with an empty page and
/// fails on the first request that breaks a request rule (host, endpoint,
/// credential header count, accept header, credential in the url).
struct StrictHttp {
    requests: RefCell<Vec<HttpRequest>>,
    secrets: Vec<String>,
}

impl StrictHttp {
    fn new(secrets: &[&str]) -> Self {
        StrictHttp {
            requests: RefCell::new(Vec::new()),
            secrets: secrets.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn count(&self) -> usize {
        self.requests.borrow().len()
    }
}

impl Http for StrictHttp {
    fn get(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError> {
        let url = &request.url;
        assert!(
            url.starts_with("https://www.googleapis.com/youtube/v3/playlistItems?"),
            "unexpected endpoint {url}"
        );
        let path = url.split('?').next().unwrap();
        assert!(
            path.split('/')
                .all(|segment| segment != "search" && segment != "videos"),
            "expensive endpoint {url}"
        );
        for secret in &self.secrets {
            assert!(!url.contains(secret.as_str()), "credential in the url");
        }
        let header = |name: &str| youtube_source::http::header(&request.headers, name);
        assert_eq!(header("accept"), Some("application/json"));
        let credentials = request
            .headers
            .iter()
            .filter(|(name, _)| {
                name.eq_ignore_ascii_case("authorization")
                    || name.eq_ignore_ascii_case("x-goog-api-key")
            })
            .count();
        assert_eq!(credentials, 1, "exactly one credential header per request");
        self.requests.borrow_mut().push(request.clone());
        Ok(page(Vec::new()))
    }
}

#[test]
fn an_access_token_is_preferred_over_the_api_key_and_sent_as_bearer() {
    let http = ScriptedHttp::new(vec![Ok(fixture("playlist_page"))]);
    poll(&http, &one_channel(), Some(KEY), None, Some("TOKEN-1")).expect("poll");
    let requests = http.requests();
    assert_eq!(requests.len(), 1);
    let header = |name: &str| youtube_source::http::header(&requests[0].headers, name);
    assert_eq!(header("authorization"), Some("Bearer TOKEN-1"));
    assert_eq!(header("x-goog-api-key"), None);
    assert!(!requests[0].url.contains("TOKEN-1") && !requests[0].url.contains(KEY));
}

#[test]
fn without_a_token_the_api_key_header_is_used() {
    let http = ScriptedHttp::new(vec![Ok(fixture("playlist_page"))]);
    poll(&http, &one_channel(), Some(KEY), None, None).expect("poll");
    let requests = http.requests();
    let header = |name: &str| youtube_source::http::header(&requests[0].headers, name);
    assert_eq!(header("x-goog-api-key"), Some(KEY));
    assert_eq!(header("authorization"), None);
}

fn many_channels(count: usize) -> Settings {
    let list: Vec<String> = (0..count).map(|i| format!("\"UC{i:0>22}\"")).collect();
    settings(&format!("channels = [{}]\n", list.join(", ")))
}

#[test]
fn only_playlistitems_requests_are_ever_made_and_each_poll_costs_one_unit_per_playlist() {
    for count in [1, 3, youtube_source::cursor::MAX_FEEDS] {
        let settings = many_channels(count);
        for (key, token) in [(Some(KEY), None), (Some(KEY), Some("TOKEN-1")), (None, Some("TOKEN-1"))]
        {
            let http = StrictHttp::new(&[KEY, "TOKEN-1"]);
            // First poll, then a later poll with the cursor the first returned.
            let first = poll(&http, &settings, key, None, token).expect("first poll");
            assert_eq!(http.count(), count, "first poll of {count} channels");
            poll(&http, &settings, key, first.next_cursor.as_deref(), token).expect("later poll");
            assert_eq!(http.count(), 2 * count, "two polls of {count} channels");
        }
    }
}

// ── Plan 07-08 Task 1: response classification (D-15, YTUB-03) ───────────────

const QUOTA_TEXT: &str = "YouTube quota exceeded (quotaExceeded): daily Data API quota exhausted; resets at midnight Pacific";

/// A Google error envelope with `status` and (optionally) one error `reason`.
fn error_response(status: u16, reason: Option<&str>) -> HttpResponse {
    let errors: Vec<Value> = reason
        .map(|reason| serde_json::json!({ "domain": "youtube.test", "reason": reason }))
        .into_iter()
        .collect();
    let body = serde_json::json!({
        "error": { "code": status, "message": "synthetic", "errors": errors }
    });
    HttpResponse {
        status,
        headers: Vec::new(),
        body: serde_json::to_vec(&body).expect("json"),
    }
}

fn poll_error(response: HttpResponse) -> PollError {
    let http = ScriptedHttp::new(vec![Ok(response)]);
    poll(&http, &one_channel(), Some(KEY), None, None).unwrap_err()
}

#[test]
fn quota_exceeded_403_is_a_named_transient_with_the_exact_d15_text() {
    let error = poll_error(fixture("quota_exceeded_403"));
    assert_eq!(error, PollError::Transient(QUOTA_TEXT.to_string()));
}

#[test]
fn daily_limit_exceeded_uses_the_same_named_error() {
    let error = poll_error(error_response(403, Some("dailyLimitExceeded")));
    assert_eq!(error, PollError::Transient(QUOTA_TEXT.to_string()));
}

#[test]
fn quota_recovers_on_the_next_poll_without_a_restart() {
    let http = ScriptedHttp::new(vec![
        Ok(fixture("quota_exceeded_403")),
        Ok(fixture("playlist_page")),
    ]);
    let settings = one_channel();
    let first = poll(&http, &settings, Some(KEY), None, None);
    assert!(matches!(first, Err(PollError::Transient(_))), "{first:?}");
    let second = poll(&http, &settings, Some(KEY), None, None).expect("recovered");
    assert_eq!(second.items.len(), 3);
}

#[test]
fn no_retry_after_a_quota_error() {
    let http = ScriptedHttp::new(vec![
        Ok(fixture("quota_exceeded_403")),
        Ok(fixture("playlist_page")),
        Ok(fixture("playlist_page")),
    ]);
    let three = settings(&format!(
        "channels = [\"{CHANNEL}\", \"{}\", \"UC{}\"]\n",
        channel_b(),
        "c".repeat(22)
    ));
    let result = poll(&http, &three, Some(KEY), None, None);
    assert!(matches!(result, Err(PollError::Transient(_))), "{result:?}");
    assert_eq!(http.requests().len(), 1);
}

#[test]
fn rate_limit_exceeded_and_429_are_rate_limited_none() {
    assert_eq!(
        poll_error(error_response(429, None)),
        PollError::RateLimited(None)
    );
    for reason in ["rateLimitExceeded", "userRateLimitExceeded"] {
        assert_eq!(
            poll_error(error_response(403, Some(reason))),
            PollError::RateLimited(None),
            "{reason}"
        );
    }
}

#[test]
fn http_401_is_auth_required() {
    assert_eq!(poll_error(fixture("unauthorized_401")), PollError::AuthRequired);
}

#[test]
fn bad_key_and_disabled_api_and_forbidden_are_config_errors_naming_the_credential() {
    let PollError::Config(text) = poll_error(error_response(400, Some("keyInvalid"))) else {
        panic!("expected Config for keyInvalid");
    };
    assert!(text.contains("api_key"), "{text}");

    let PollError::Config(text) = poll_error(error_response(403, Some("accessNotConfigured")))
    else {
        panic!("expected Config for accessNotConfigured");
    };
    assert!(text.contains("not enabled"), "{text}");

    for reason in ["forbidden", "insufficientPermissions"] {
        let PollError::Config(text) = poll_error(error_response(403, Some(reason))) else {
            panic!("expected Config for {reason}");
        };
        assert!(text.contains(reason), "{text}");
    }
}

#[test]
fn http_404_names_the_channel_entry() {
    let PollError::Config(text) = poll_error(error_response(404, Some("playlistNotFound"))) else {
        panic!("expected Config for 404");
    };
    assert!(text.contains("channels"), "{text}");
    assert!(text.contains(CHANNEL), "{text}");
    assert!(text.contains("404"), "{text}");
}

#[test]
fn http_5xx_is_transient() {
    for status in [500_u16, 502, 503] {
        let PollError::Transient(text) = poll_error(error_response(status, None)) else {
            panic!("expected Transient for {status}");
        };
        assert!(text.contains(&status.to_string()), "{text}");
    }
}

#[test]
fn malformed_json_is_transient_without_echoing_the_body() {
    // `malformed.json` has no meta file, so it arrives with status 200.
    let response = fixture("malformed");
    assert_eq!(response.status, 200);
    let PollError::Transient(text) = poll_error(response) else {
        panic!("expected Transient");
    };
    assert!(!text.contains("SENTINELTRUNCATED"), "{text}");
}

#[test]
fn every_error_status_maps_to_an_error_never_an_empty_success() {
    for status in [301_u16, 302, 400, 401, 403, 404, 408, 429, 500, 502, 503] {
        // An empty, otherwise well-formed page: it would be an `Ok` with no
        // items if the status were ignored.
        let response = HttpResponse {
            status,
            headers: Vec::new(),
            body: br#"{"items":[]}"#.to_vec(),
        };
        let http = ScriptedHttp::new(vec![Ok(response)]);
        let result = poll(&http, &one_channel(), Some(KEY), None, None);
        assert!(result.is_err(), "status {status} gave {result:?}");
    }
}

// ── Plan 07-09: subscriptions (YTUB-02, YTUB-03, D-06, D-07, D-15) ───────────

use std::rc::Rc;
use youtube_source::settings::{MAX_SUBSCRIPTION_PAGES, quota_units_per_poll};

const USER_TOKEN: &str = "TOKEN-1";
const SUBSCRIPTIONS_URL_PREFIX: &str = "https://www.googleapis.com/youtube/v3/subscriptions?";
const PLAYLIST_URL_PREFIX: &str = "https://www.googleapis.com/youtube/v3/playlistItems?";

type Responder = Box<dyn Fn(&HttpRequest) -> Result<HttpResponse, HttpError>>;

/// A mock that answers by endpoint: one responder for `subscriptions.list`,
/// one for `playlistItems.list`. It records every request.
struct RoutedHttp {
    subscriptions: Responder,
    playlists: Responder,
    requests: RefCell<Vec<HttpRequest>>,
}

impl RoutedHttp {
    fn new(
        subscriptions: impl Fn(&HttpRequest) -> Result<HttpResponse, HttpError> + 'static,
        playlists: impl Fn(&HttpRequest) -> Result<HttpResponse, HttpError> + 'static,
    ) -> Self {
        RoutedHttp {
            subscriptions: Box::new(subscriptions),
            playlists: Box::new(playlists),
            requests: RefCell::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<HttpRequest> {
        self.requests.borrow().clone()
    }

    fn subscription_requests(&self) -> Vec<HttpRequest> {
        self.requests()
            .into_iter()
            .filter(|r| r.url.starts_with(SUBSCRIPTIONS_URL_PREFIX))
            .collect()
    }

    fn playlist_requests(&self) -> Vec<HttpRequest> {
        self.requests()
            .into_iter()
            .filter(|r| r.url.starts_with(PLAYLIST_URL_PREFIX))
            .collect()
    }
}

impl Http for RoutedHttp {
    fn get(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError> {
        self.requests.borrow_mut().push(request.clone());
        if request.url.starts_with(SUBSCRIPTIONS_URL_PREFIX) {
            (self.subscriptions)(request)
        } else {
            (self.playlists)(request)
        }
    }
}

/// The value of a url query parameter, if present.
fn query_param(url: &str, name: &str) -> Option<String> {
    let query = url.split_once('?')?.1;
    query
        .split('&')
        .find_map(|pair| pair.strip_prefix(&format!("{name}=")).map(str::to_string))
}

fn sub_channel(index: usize) -> String {
    format!("UCSubscribedChannel{index:05}")
}

fn sub_playlist(index: usize) -> String {
    format!("UUSubscribedChannel{index:05}")
}

fn sub_channels(count: usize) -> Vec<String> {
    (1..=count).map(sub_channel).collect()
}

/// A `subscriptions.list` answer with one entry per channel id.
fn subs_response(ids: &[String], next_page_token: Option<&str>) -> HttpResponse {
    let items: Vec<Value> = ids
        .iter()
        .map(|id| {
            serde_json::json!({
                "kind": "youtube#subscription",
                "snippet": {
                    "title": format!("Title of {id}"),
                    "resourceId": { "kind": "youtube#channel", "channelId": id }
                }
            })
        })
        .collect();
    let mut body = serde_json::json!({
        "kind": "youtube#subscriptionListResponse",
        "items": items,
    });
    if let Some(token) = next_page_token {
        body["nextPageToken"] = Value::String(token.to_string());
    }
    json_page(&body)
}

/// Always the same single page, whatever is asked.
fn fixed_subscriptions(
    ids: Vec<String>,
) -> impl Fn(&HttpRequest) -> Result<HttpResponse, HttpError> {
    move |_| Ok(subs_response(&ids, None))
}

/// A faithful `subscriptions.list`: it serves `all` in pages of the requested
/// `maxResults`, using the page token as a numeric offset.
fn simulated_subscriptions(
    all: Vec<String>,
) -> impl Fn(&HttpRequest) -> Result<HttpResponse, HttpError> {
    move |request| {
        let size: usize = query_param(&request.url, "maxResults")
            .and_then(|v| v.parse().ok())
            .expect("maxResults");
        let offset: usize = query_param(&request.url, "pageToken")
            .and_then(|v| v.strip_prefix("OFFSET").and_then(|n| n.parse().ok()))
            .unwrap_or(0);
        let end = (offset + size).min(all.len());
        let next = (end < all.len()).then(|| format!("OFFSET{end}"));
        Ok(subs_response(&all[offset.min(end)..end], next.as_deref()))
    }
}

/// Every answer: one new channel and a new token, forever.
fn endless_subscriptions() -> impl Fn(&HttpRequest) -> Result<HttpResponse, HttpError> {
    |request| {
        let n: usize = query_param(&request.url, "pageToken")
            .and_then(|t| t.strip_prefix("TOKEN").and_then(|n| n.parse().ok()))
            .unwrap_or(0);
        Ok(subs_response(
            &[sub_channel(n + 1)],
            Some(&format!("TOKEN{}", n + 1)),
        ))
    }
}

/// One video per playlist, named after the last digits of the playlist id.
fn one_video_per_playlist() -> impl Fn(&HttpRequest) -> Result<HttpResponse, HttpError> {
    |request| {
        let playlist = query_param(&request.url, "playlistId").expect("playlistId");
        let suffix = &playlist[playlist.len() - 5..];
        Ok(page(vec![entry(
            &format!("v{suffix}"),
            "T",
            "2024-05-01T00:00:00Z",
        )]))
    }
}

fn empty_playlists() -> impl Fn(&HttpRequest) -> Result<HttpResponse, HttpError> {
    |_| Ok(page(Vec::new()))
}

fn subs_only() -> Settings {
    settings("subscriptions = true\n")
}

fn poll_user(
    http: &dyn Http,
    settings: &Settings,
    cursor: Option<&[u8]>,
) -> Result<PollOutcome, PollError> {
    poll(http, settings, Some(KEY), cursor, Some(USER_TOKEN))
}

/// The fixture `subscriptions_page` as an `HttpResponse`, optionally without
/// its `nextPageToken`.
fn fixture_subscriptions(keep_token: bool) -> HttpResponse {
    let mut root: Value = serde_json::from_slice(&fixture("subscriptions_page").body).unwrap();
    if !keep_token {
        root.as_object_mut().unwrap().remove("nextPageToken");
    }
    json_page(&root)
}

/// Channel ids and titles of the fixture: the private strings that no
/// warning or error text may carry.
fn privacy_sentinels() -> Vec<String> {
    let root: Value = serde_json::from_slice(&fixture("subscriptions_page").body).unwrap();
    let mut sentinels = Vec::new();
    for item in root["items"].as_array().unwrap() {
        let id = item["snippet"]["resourceId"]["channelId"].as_str().unwrap();
        sentinels.push(id.to_string());
        sentinels.push(format!("UU{}", &id[2..]));
        sentinels.push(item["snippet"]["title"].as_str().unwrap().to_string());
    }
    assert_eq!(sentinels.len(), 9);
    sentinels
}

#[test]
fn subscriptions_mode_requires_a_token_and_never_falls_back_to_the_api_key() {
    for token in [None, Some("")] {
        let http = RoutedHttp::new(fixed_subscriptions(sub_channels(3)), empty_playlists());
        let result = poll(&http, &subs_only(), Some(KEY), None, token);
        assert_eq!(result.unwrap_err(), PollError::AuthRequired);
        assert!(http.requests().is_empty(), "no request without a token");
    }
    // Explicit channels alongside subscriptions do not change that.
    let both = settings(&format!(
        "channels = [\"{CHANNEL}\"]\nsubscriptions = true\n"
    ));
    let http = RoutedHttp::new(fixed_subscriptions(sub_channels(1)), empty_playlists());
    assert_eq!(
        poll(&http, &both, Some(KEY), None, None).unwrap_err(),
        PollError::AuthRequired
    );
    assert!(http.requests().is_empty());
}

#[test]
fn subscriptions_poll_sends_the_bearer_token_on_every_request_and_never_the_api_key() {
    let http = RoutedHttp::new(fixed_subscriptions(sub_channels(2)), empty_playlists());
    poll_user(&http, &subs_only(), None).expect("poll");
    assert_eq!(http.requests().len(), 3);
    for request in http.requests() {
        let header = |name: &str| youtube_source::http::header(&request.headers, name);
        assert_eq!(header("authorization"), Some("Bearer TOKEN-1"), "{}", request.url);
        assert_eq!(header("x-goog-api-key"), None, "{}", request.url);
        assert!(!request.url.contains(KEY) && !request.url.contains(USER_TOKEN));
    }
}

#[test]
fn subscriptions_then_one_playlist_request_per_channel() {
    let http = RoutedHttp::new(
        |_| Ok(fixture_subscriptions(false)),
        one_video_per_playlist(),
    );
    let outcome = poll_user(&http, &subs_only(), None).expect("poll");
    let urls: Vec<String> = http.requests().into_iter().map(|r| r.url).collect();
    assert_eq!(urls.len(), 4);
    assert_eq!(
        urls[0],
        "https://www.googleapis.com/youtube/v3/subscriptions?part=snippet&mine=true&maxResults=50&order=relevance"
    );
    // One playlist per channel, in subscription order, as a first poll.
    for (i, url) in urls[1..].iter().enumerate() {
        assert!(url.starts_with(PLAYLIST_URL_PREFIX), "{url}");
        assert!(url.contains(&format!("playlistId={}", sub_playlist(i + 1))), "{url}");
        assert!(url.ends_with("maxResults=10"), "{url}");
    }
    let ids: Vec<&str> = outcome.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["v00001", "v00002", "v00003"]);
    let (decoded, _) = cursor::decode(outcome.next_cursor.as_deref());
    assert_eq!(decoded.0.len(), 3);
}

#[test]
fn subscriptions_pagination_follows_next_page_token_until_the_limit() {
    let first = subs_response(&sub_channels(2), Some("NEXTPAGE2"));
    let second = subs_response(&[sub_channel(3)], Some("NEXTPAGE3"));
    let pages = RefCell::new(VecDeque::from([first, second]));
    let http = RoutedHttp::new(
        move |_| {
            Ok(pages
                .borrow_mut()
                .pop_front()
                .expect("no third subscriptions request"))
        },
        empty_playlists(),
    );
    let limited = settings("subscriptions = true\nsubscriptions_limit = 3\n");
    poll_user(&http, &limited, None).expect("poll");
    let subs = http.subscription_requests();
    assert_eq!(subs.len(), 2, "polling stops once three channels are collected");
    assert!(subs[0].url.contains("maxResults=3") && !subs[0].url.contains("pageToken"));
    assert!(subs[1].url.contains("maxResults=1"), "{}", subs[1].url);
    assert!(subs[1].url.ends_with("&pageToken=NEXTPAGE2"), "{}", subs[1].url);
    assert_eq!(http.playlist_requests().len(), 3);
}

#[test]
fn a_hostile_next_page_token_loop_stops_at_the_page_cap() {
    let http = RoutedHttp::new(endless_subscriptions(), empty_playlists());
    let outcome = poll_user(&http, &subs_only(), None).expect("what it has is returned");
    assert_eq!(
        http.subscription_requests().len(),
        MAX_SUBSCRIPTION_PAGES as usize
    );
    assert_eq!(http.playlist_requests().len(), MAX_SUBSCRIPTION_PAGES as usize);
    let capped = outcome
        .warnings
        .iter()
        .filter(|w| w.contains("page limit"))
        .count();
    assert_eq!(capped, 1, "{:?}", outcome.warnings);
    assert!(outcome.next_cursor.is_some());
}

#[test]
fn explicit_channels_and_subscriptions_are_combined_without_duplicates() {
    let explicit_and_subscribed = sub_channel(2);
    let combined = settings(&format!(
        "channels = [\"{CHANNEL}\", \"{explicit_and_subscribed}\"]\nsubscriptions = true\n"
    ));
    let http = RoutedHttp::new(fixed_subscriptions(sub_channels(3)), empty_playlists());
    poll_user(&http, &combined, None).expect("poll");
    let playlist_ids: Vec<String> = http
        .playlist_requests()
        .iter()
        .map(|r| query_param(&r.url, "playlistId").unwrap())
        .collect();
    // Explicit channels first, then the subscriptions in order, each once.
    assert_eq!(
        playlist_ids,
        [
            PLAYLIST.to_string(),
            sub_playlist(2),
            sub_playlist(1),
            sub_playlist(3)
        ]
    );
}

#[test]
fn a_404_for_a_subscribed_channel_is_skipped_and_counted_not_fatal() {
    let http = RoutedHttp::new(fixed_subscriptions(sub_channels(3)), |request| {
        let playlist = query_param(&request.url, "playlistId").unwrap();
        if playlist == sub_playlist(2) {
            Ok(error_response(404, Some("playlistNotFound")))
        } else {
            one_video_per_playlist()(request)
        }
    });
    let outcome = poll_user(&http, &subs_only(), None).expect("the poll still succeeds");
    let ids: Vec<&str> = outcome.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["v00001", "v00003"]);
    assert_eq!(outcome.warnings.len(), 1, "{:?}", outcome.warnings);
    let warning = &outcome.warnings[0];
    assert!(warning.contains('1') && warning.contains("404"), "{warning}");
    assert!(!warning.contains("SubscribedChannel"), "{warning}");
    // The skipped channel has no mark, so it is retried on the next poll.
    let (decoded, _) = cursor::decode(outcome.next_cursor.as_deref());
    assert_eq!(decoded.mark(&sub_playlist(2)), None);

    // The same 404 for an explicit channel is still the named config error.
    let explicit = settings(&format!(
        "channels = [\"{CHANNEL}\"]\nsubscriptions = true\n"
    ));
    let http = RoutedHttp::new(fixed_subscriptions(sub_channels(1)), |_| {
        Ok(error_response(404, Some("playlistNotFound")))
    });
    let PollError::Config(text) = poll_user(&http, &explicit, None).unwrap_err() else {
        panic!("expected Config for an explicit channel");
    };
    assert!(text.contains("channels") && text.contains(CHANNEL), "{text}");
}

#[test]
fn subscription_list_errors_use_the_same_classification() {
    let run = |response: HttpResponse| {
        let http = RoutedHttp::new(move |_| Ok(response.clone()), empty_playlists());
        let error = poll_user(&http, &subs_only(), None).unwrap_err();
        assert!(
            http.playlist_requests().is_empty(),
            "no playlist request after a failure"
        );
        assert_eq!(http.requests().len(), 1);
        error
    };
    assert_eq!(
        run(fixture("quota_exceeded_403")),
        PollError::Transient(QUOTA_TEXT.to_string())
    );
    assert_eq!(run(fixture("unauthorized_401")), PollError::AuthRequired);
    assert_eq!(run(error_response(429, None)), PollError::RateLimited(None));
    // A 404 on the list itself has no channel to name.
    assert_eq!(
        run(error_response(404, Some("notFound"))),
        PollError::Transient("YouTube HTTP 404".to_string())
    );
    // A 200 that is not a subscriptions page is a transient, never an empty list.
    let PollError::Transient(text) = run(fixture("malformed")) else {
        panic!("expected Transient");
    };
    assert!(!text.contains("SENTINELTRUNCATED"), "{text}");
    assert!(matches!(
        run(HttpResponse {
            status: 200,
            headers: Vec::new(),
            body: b"{}".to_vec()
        }),
        PollError::Transient(_)
    ));
}

#[test]
fn a_user_with_no_subscriptions_gets_an_empty_poll_and_a_cursor() {
    let http = RoutedHttp::new(fixed_subscriptions(Vec::new()), empty_playlists());
    let outcome = poll_user(&http, &subs_only(), None).expect("poll");
    assert!(outcome.items.is_empty());
    let bytes = outcome
        .next_cursor
        .expect("Some, so the host's mark is not reset");
    let (decoded, undecodable) = cursor::decode(Some(&bytes));
    assert!(!undecodable);
    assert!(decoded.0.is_empty());
    assert_eq!(http.requests().len(), 1);
}

#[test]
fn a_channel_that_leaves_and_re_enters_the_limit_is_treated_as_a_first_poll() {
    let window = Rc::new(RefCell::new(sub_channels(3)));
    let http = RoutedHttp::new(
        {
            let window = Rc::clone(&window);
            move |_| Ok(subs_response(&window.borrow(), None))
        },
        one_video_per_playlist(),
    );
    let first = poll_user(&http, &subs_only(), None).expect("first");
    assert_eq!(first.items.len(), 3);

    // Channel 3 drops out of the window: its mark is dropped with it.
    *window.borrow_mut() = sub_channels(2);
    let second = poll_user(&http, &subs_only(), first.next_cursor.as_deref()).expect("second");
    assert!(second.items.is_empty());
    let (decoded, _) = cursor::decode(second.next_cursor.as_deref());
    assert_eq!(decoded.mark(&sub_playlist(3)), None);
    assert!(decoded.mark(&sub_playlist(1)).is_some());

    // It returns: a first poll again, so the page is the first-poll size.
    *window.borrow_mut() = sub_channels(3);
    let before = http.playlist_requests().len();
    let third = poll_user(&http, &subs_only(), second.next_cursor.as_deref()).expect("third");
    let ids: Vec<&str> = third.items.iter().map(|i| i.id.as_str()).collect();
    assert_eq!(ids, ["v00003"]);
    let fresh: Vec<HttpRequest> = http.playlist_requests().split_off(before);
    assert!(fresh[2].url.contains(&format!("playlistId={}", sub_playlist(3))));
    assert!(fresh[2].url.ends_with("maxResults=10"), "{}", fresh[2].url);
    // The channels that stayed use the later size: 1000 / (50 worst case) = 20.
    assert!(fresh[0].url.ends_with("maxResults=20"), "{}", fresh[0].url);
}

#[test]
fn subscription_data_is_not_logged_or_echoed() {
    let sentinels = privacy_sentinels();
    let mut texts: Vec<String> = Vec::new();
    let mut record = |result: Result<PollOutcome, PollError>| match result {
        Ok(outcome) => texts.extend(outcome.warnings),
        Err(error) => texts.push(format!("{error:?}")),
    };

    // Success with the fixture's own channels.
    let http = RoutedHttp::new(
        |_| Ok(fixture_subscriptions(false)),
        one_video_per_playlist(),
    );
    record(poll_user(&http, &subs_only(), None));
    // Every fixture channel's playlist is missing.
    let http = RoutedHttp::new(
        |_| Ok(fixture_subscriptions(false)),
        |_| Ok(error_response(404, Some("playlistNotFound"))),
    );
    record(poll_user(&http, &subs_only(), None));
    // Empty list.
    let http = RoutedHttp::new(fixed_subscriptions(Vec::new()), empty_playlists());
    record(poll_user(&http, &subs_only(), None));
    // A skipped invalid id next to the fixture's channels.
    let http = RoutedHttp::new(
        |_| {
            let mut ids: Vec<String> = privacy_sentinels()
                .into_iter()
                .filter(|s| s.starts_with("UCSub"))
                .collect();
            ids.push("UCshort".to_string());
            Ok(subs_response(&ids, None))
        },
        empty_playlists(),
    );
    record(poll_user(&http, &subs_only(), None));
    // Failures of the list call itself.
    for response in [
        fixture("unauthorized_401"),
        fixture("quota_exceeded_403"),
        fixture("malformed"),
    ] {
        let http = RoutedHttp::new(move |_| Ok(response.clone()), empty_playlists());
        record(poll_user(&http, &subs_only(), None));
    }
    // The page cap.
    let http = RoutedHttp::new(endless_subscriptions(), empty_playlists());
    record(poll_user(&http, &subs_only(), None));

    assert!(texts.len() >= 6, "{texts:?}");
    for text in &texts {
        for sentinel in &sentinels {
            assert!(!text.contains(sentinel.as_str()), "{text:?} carries {sentinel}");
        }
        assert!(!text.contains("SubscribedChannel"), "{text:?}");
    }
}

/// The request rules of the low-cost sweep: only the two list endpoints, none
/// of `search`, `videos` or `channels`, exactly one credential header, a
/// bearer token for subscriptions, and no secret in the url.
fn assert_low_cost(requests: &[HttpRequest], secrets: &[&str]) {
    for request in requests {
        let url = &request.url;
        assert!(
            url.starts_with(PLAYLIST_URL_PREFIX) || url.starts_with(SUBSCRIPTIONS_URL_PREFIX),
            "unexpected endpoint {url}"
        );
        let path = url.split('?').next().unwrap();
        assert!(
            path.split('/')
                .all(|s| s != "search" && s != "videos" && s != "channels"),
            "expensive endpoint {url}"
        );
        for secret in secrets {
            assert!(!url.contains(secret), "credential in the url");
        }
        let header = |name: &str| youtube_source::http::header(&request.headers, name);
        assert_eq!(header("accept"), Some("application/json"));
        let credentials = request
            .headers
            .iter()
            .filter(|(name, _)| {
                name.eq_ignore_ascii_case("authorization")
                    || name.eq_ignore_ascii_case("x-goog-api-key")
            })
            .count();
        assert_eq!(credentials, 1, "exactly one credential header per request: {url}");
        if url.starts_with(SUBSCRIPTIONS_URL_PREFIX) {
            assert_eq!(header("authorization"), Some("Bearer TOKEN-1"), "{url}");
        }
    }
}

#[test]
fn only_low_cost_list_endpoints_are_requested_in_every_mode() {
    let combined = settings(&format!(
        "channels = [\"{CHANNEL}\"]\nsubscriptions = true\n"
    ));
    let scenarios: Vec<(&str, Settings, Option<&str>, Option<&str>)> = vec![
        ("channels only, key", one_channel(), Some(KEY), None),
        ("channels only, token", one_channel(), None, Some(USER_TOKEN)),
        ("subscriptions only", subs_only(), Some(KEY), Some(USER_TOKEN)),
        ("combined", combined, Some(KEY), Some(USER_TOKEN)),
    ];
    for (name, settings, key, token) in scenarios {
        let http = RoutedHttp::new(
            simulated_subscriptions(sub_channels(5)),
            one_video_per_playlist(),
        );
        let first = poll(&http, &settings, key, None, token).expect(name);
        poll(&http, &settings, key, first.next_cursor.as_deref(), token).expect(name);
        assert!(!http.requests().is_empty(), "{name}");
        assert_low_cost(&http.requests(), &[KEY, USER_TOKEN]);
    }
}

#[test]
fn quota_formula_matches_the_requests_actually_made() {
    // (explicit channels, subscribed channels the account has, limit)
    let cases: [(usize, usize, usize); 6] = [
        (0, 3, 50),
        (0, 60, 50),
        (0, 120, 71),
        (2, 3, 50),
        (1, 51, 60),
        (0, 50, 50),
    ];
    for (explicit, subscribed, limit) in cases {
        let mut text = String::from("subscriptions = true\n");
        text.push_str(&format!("subscriptions_limit = {limit}\n"));
        if explicit > 0 {
            let quoted: Vec<String> = (0..explicit).map(|i| format!("\"UC{i:0>22}\"")).collect();
            text.push_str(&format!("channels = [{}]\n", quoted.join(", ")));
        }
        let http = RoutedHttp::new(
            simulated_subscriptions(sub_channels(subscribed)),
            empty_playlists(),
        );
        poll_user(&http, &settings(&text), None).expect("poll");
        let fetched = subscribed.min(limit);
        let playlists = http.playlist_requests().len();
        assert_eq!(playlists, explicit + fetched, "case {explicit}/{subscribed}/{limit}");
        assert_eq!(
            http.requests().len() as u32,
            quota_units_per_poll(fetched as u32, playlists as u32),
            "case {explicit}/{subscribed}/{limit}: one unit per request"
        );
    }
    // Channels only: no subscription call, one unit per playlist.
    let http = RoutedHttp::new(fixed_subscriptions(Vec::new()), empty_playlists());
    poll(&http, &many_channels(7), Some(KEY), None, None).expect("poll");
    assert_eq!(http.requests().len() as u32, quota_units_per_poll(0, 7));
}
