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
