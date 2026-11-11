//! Fixture replay: the plain-Rust core driven through a recording mock of the
//! `Http` seam. No network, no keys, no environment, no shared state, so the
//! default parallel runner and a single-threaded run give the same result.

use reddit_source::cursor::{self, MAX_CURSOR_BYTES};
use reddit_source::http::{Http, HttpError, HttpRequest, HttpResponse};
use reddit_source::poll::{PollError, PollOutcome, poll};
use reddit_source::settings::Settings;
use serde_json::Value;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;

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

fn rust_settings() -> Settings {
    settings("subreddits = [\"rust\"]\nuser_agent_contact = \"u/tester\"\n")
}

const TOKEN: &str = "TOKEN-1";

fn first_poll(http: &ScriptedHttp) -> PollOutcome {
    poll(http, &rust_settings(), None, Some(TOKEN)).expect("first poll")
}

/// The `t3` children of the fixture, as the API sent them.
fn fixture_posts() -> Vec<Value> {
    let root: Value = serde_json::from_slice(&fixture("listing_new").body).unwrap();
    root["data"]["children"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|child| child["kind"] == "t3")
        .map(|child| child["data"].clone())
        .collect()
}

#[test]
fn tracer_first_poll_emits_the_page_as_items() {
    let http = ScriptedHttp::new(vec![Ok(fixture("listing_new"))]);
    let outcome = first_poll(&http);
    let posts = fixture_posts();
    assert_eq!(outcome.items.len(), posts.len());
    assert_eq!(posts.len(), 3);

    for item in &outcome.items {
        let post = posts
            .iter()
            .find(|post| post["name"] == item.id.as_str())
            .expect("item matches a post");
        assert!(item.id.starts_with("t3_"));
        assert!(item.url.starts_with("https://www.reddit.com/r/rust/comments/"));
        assert_eq!(
            item.url,
            format!("https://www.reddit.com{}", post["permalink"].as_str().unwrap())
        );
        assert_eq!(item.platform, "reddit");
        assert_eq!(item.title.as_deref(), post["title"].as_str());
        assert_eq!(item.author.as_deref(), post["author"].as_str());
        assert_eq!(
            item.published,
            Some((post["created_utc"].as_f64().unwrap() * 1000.0) as u64)
        );
        let expected_text = if post["is_self"] == true {
            post["selftext"].as_str().unwrap()
        } else {
            post["title"].as_str().unwrap()
        };
        assert_eq!(item.text, expected_text);
        assert!(!item.text.is_empty());
        let raw: Value = serde_json::from_str(item.raw.as_deref().unwrap()).unwrap();
        assert_eq!(raw["name"], item.id.as_str());
    }
}

#[test]
fn tracer_request_targets_oauth_reddit_with_bearer_and_user_agent() {
    let http = ScriptedHttp::new(vec![Ok(fixture("listing_new"))]);
    first_poll(&http);
    let requests = http.requests();
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(
        request.url,
        "https://oauth.reddit.com/r/rust/new?limit=25&raw_json=1"
    );
    let header = |name: &str| reddit_source::http::header(&request.headers, name);
    assert_eq!(header("authorization"), Some("bearer TOKEN-1"));
    assert_eq!(header("accept"), Some("application/json"));
    assert_eq!(
        header("user-agent"),
        Some(format!("filimon:reddit:{} (by /u/tester)", env!("CARGO_PKG_VERSION")).as_str())
    );
}

#[test]
fn tracer_returns_a_cursor_marking_the_newest_post() {
    let http = ScriptedHttp::new(vec![Ok(fixture("listing_new"))]);
    let outcome = first_poll(&http);
    let bytes = outcome.next_cursor.expect("cursor");
    assert!(bytes.len() <= MAX_CURSOR_BYTES);
    let json: Value = serde_json::from_slice(&bytes).expect("cursor is JSON");
    let map = json.as_object().unwrap();
    assert_eq!(map.len(), 1);

    let newest = fixture_posts()
        .into_iter()
        .max_by(|a, b| {
            a["created_utc"]
                .as_f64()
                .partial_cmp(&b["created_utc"].as_f64())
                .unwrap()
        })
        .unwrap();
    let (cursor, undecodable) = cursor::decode(Some(&bytes));
    assert!(!undecodable);
    let mark = cursor.0.get("r/rust:new").expect("feed key r/rust:new");
    assert_eq!(mark.t, newest["created_utc"].as_f64().unwrap() as i64);
    assert_eq!(mark.n, newest["name"].as_str().unwrap());
}

#[test]
fn tracer_second_poll_with_the_same_page_emits_nothing() {
    let http = ScriptedHttp::new(vec![Ok(fixture("listing_new")), Ok(fixture("listing_new"))]);
    let first = first_poll(&http);
    let second = poll(
        &http,
        &rust_settings(),
        first.next_cursor.as_deref(),
        Some(TOKEN),
    )
    .expect("second poll");
    assert!(second.items.is_empty());
    assert_eq!(second.next_cursor, first.next_cursor);
}

#[test]
fn poll_is_deterministic_for_the_same_inputs() {
    let run = || {
        let http = ScriptedHttp::new(vec![Ok(fixture("listing_new"))]);
        first_poll(&http)
    };
    let a = run();
    let b = run();
    assert_eq!(a.items, b.items);
    assert_eq!(a.next_cursor, b.next_cursor);
}

#[test]
fn a_missing_token_is_auth_required_without_any_request() {
    let http = ScriptedHttp::new(vec![Ok(fixture("listing_new"))]);
    let result = poll(&http, &rust_settings(), None, None);
    assert_eq!(result.unwrap_err(), PollError::AuthRequired);
    assert!(http.requests().is_empty());
}

#[test]
fn a_failed_feed_fails_the_whole_poll_with_no_items_and_no_cursor() {
    let http = ScriptedHttp::new(vec![
        Ok(fixture("listing_new")),
        Ok(HttpResponse {
            status: 500,
            headers: Vec::new(),
            body: b"secret-body".to_vec(),
        }),
    ]);
    let two_feeds = settings(
        "subreddits = [\"rust\", \"programming\"]\nuser_agent_contact = \"u/tester\"\n",
    );
    let error = poll(&http, &two_feeds, None, Some(TOKEN)).unwrap_err();
    assert_eq!(error, PollError::Transient("Reddit HTTP 500".to_string()));
    let PollError::Transient(text) = error else {
        unreachable!()
    };
    assert!(!text.contains("secret-body") && !text.contains(TOKEN));
}

#[test]
fn first_poll_limit_changes_the_requested_page_size() {
    let ten = settings(
        "subreddits = [\"rust\"]\nuser_agent_contact = \"u/tester\"\nfirst_poll_limit = 10\n",
    );
    let http = ScriptedHttp::new(vec![Ok(fixture("listing_new")), Ok(fixture("listing_new"))]);
    let first = poll(&http, &ten, None, Some(TOKEN)).unwrap();
    // A feed that has a mark requests the later page size.
    poll(&http, &ten, first.next_cursor.as_deref(), Some(TOKEN)).unwrap();
    let urls: Vec<String> = http.requests().into_iter().map(|r| r.url).collect();
    assert_eq!(
        urls,
        [
            "https://oauth.reddit.com/r/rust/new?limit=10&raw_json=1",
            "https://oauth.reddit.com/r/rust/new?limit=100&raw_json=1",
        ]
    );
}

#[test]
fn only_the_official_oauth_host_is_ever_requested() {
    let http = ScriptedHttp::new(vec![Ok(fixture("listing_new"))]);
    first_poll(&http);
    for request in http.requests() {
        assert!(request.url.starts_with("https://oauth.reddit.com/"));
        assert!(!request.url.contains(".json"));
        assert!(!request.url.contains("www.reddit.com"));
    }
}

// ── Task 1: item mapping, the new versus hot sticky rule, hostile input ──────

/// A `t3` data object with the fields the mapping reads.
fn post_data(name: &str, created_utc: f64) -> Value {
    serde_json::json!({
        "subreddit": "rust",
        "name": name,
        "id": name.trim_start_matches("t3_"),
        "title": format!("Title of {name}"),
        "selftext": format!("Body of {name}"),
        "author": "someone",
        "created_utc": created_utc,
        "permalink": format!("/r/rust/comments/{}/x/", name.trim_start_matches("t3_")),
        "url": "https://example.org/external",
        "is_self": true,
        "stickied": false,
    })
}

/// A 200 listing response carrying the given `t3` data objects.
fn page(posts: Vec<Value>) -> HttpResponse {
    let children: Vec<Value> = posts
        .into_iter()
        .map(|data| serde_json::json!({"kind": "t3", "data": data}))
        .collect();
    text_response(
        &serde_json::json!({"kind": "Listing", "data": {"children": children}}).to_string(),
    )
}

fn text_response(body: &str) -> HttpResponse {
    HttpResponse {
        status: 200,
        headers: Vec::new(),
        body: body.as_bytes().to_vec(),
    }
}

fn feed_settings(subreddits: &[&str], listings: &[&str]) -> Settings {
    let quote = |items: &[&str]| {
        items
            .iter()
            .map(|item| format!("\"{item}\""))
            .collect::<Vec<_>>()
            .join(", ")
    };
    settings(&format!(
        "subreddits = [{}]\nlistings = [{}]\nuser_agent_contact = \"u/tester\"\n",
        quote(subreddits),
        quote(listings)
    ))
}

fn run(
    http: &ScriptedHttp,
    settings: &Settings,
    cursor: Option<&[u8]>,
) -> Result<PollOutcome, PollError> {
    poll(http, settings, cursor, Some(TOKEN))
}

fn marks_of(outcome: &PollOutcome) -> cursor::Cursor {
    let bytes = outcome
        .next_cursor
        .as_deref()
        .expect("a cursor is always returned");
    let (cursor, undecodable) = cursor::decode(Some(bytes));
    assert!(!undecodable);
    cursor
}

fn ids(outcome: &PollOutcome) -> Vec<&str> {
    outcome.items.iter().map(|item| item.id.as_str()).collect()
}

#[test]
fn self_post_text_is_selftext_and_link_post_text_is_the_title() {
    let http = ScriptedHttp::new(vec![Ok(fixture("listing_new"))]);
    let outcome = first_poll(&http);
    let posts = fixture_posts();
    let by_id = |id: &str| outcome.items.iter().find(|item| item.id == id).unwrap();
    let self_post = posts.iter().find(|post| post["is_self"] == true).unwrap();
    let link_post = posts.iter().find(|post| post["is_self"] == false).unwrap();
    assert_eq!(
        by_id(self_post["name"].as_str().unwrap()).text,
        self_post["selftext"].as_str().unwrap()
    );
    assert_eq!(
        by_id(link_post["name"].as_str().unwrap()).text,
        link_post["title"].as_str().unwrap()
    );

    // A self post whose self text is only whitespace also gets the title.
    let mut blank = post_data("t3_blank", 1_700_000_000.0);
    blank["selftext"] = Value::String(" \n\t ".to_string());
    let http = ScriptedHttp::new(vec![Ok(page(vec![blank]))]);
    let outcome = first_poll(&http);
    assert_eq!(outcome.items[0].text, "Title of t3_blank");
}

#[test]
fn url_is_the_reddit_permalink_for_every_post_and_the_external_link_stays_in_raw() {
    let http = ScriptedHttp::new(vec![Ok(fixture("listing_new"))]);
    let outcome = first_poll(&http);
    for item in &outcome.items {
        assert!(
            item.url
                .starts_with("https://www.reddit.com/r/rust/comments/")
        );
    }
    let link = fixture_posts()
        .into_iter()
        .find(|post| post["is_self"] == false)
        .unwrap();
    let external = link["url"].as_str().unwrap();
    assert!(external.starts_with("https://blog.example.org/"));
    let item = outcome
        .items
        .iter()
        .find(|item| item.id == link["name"].as_str().unwrap())
        .unwrap();
    assert_ne!(item.url, external);
    let raw: Value = serde_json::from_str(item.raw.as_deref().unwrap()).unwrap();
    assert_eq!(raw["url"], external);
}

#[test]
fn raw_is_the_full_post_object_and_an_oversize_raw_is_dropped() {
    let http = ScriptedHttp::new(vec![Ok(fixture("listing_new"))]);
    let outcome = first_poll(&http);
    assert!(outcome.warnings.is_empty(), "{:?}", outcome.warnings);
    for post in fixture_posts() {
        let item = outcome
            .items
            .iter()
            .find(|item| item.id == post["name"].as_str().unwrap())
            .unwrap();
        let raw: Value = serde_json::from_str(item.raw.as_deref().unwrap()).unwrap();
        assert_eq!(raw, post, "raw carries every original field");
    }

    let mut huge = post_data("t3_huge", 1_700_000_000.0);
    huge["selftext"] = Value::String("x".repeat(300 * 1024));
    let http = ScriptedHttp::new(vec![Ok(page(vec![
        huge,
        post_data("t3_small", 1_700_000_001.0),
    ]))]);
    let outcome = first_poll(&http);
    assert_eq!(ids(&outcome), ["t3_huge", "t3_small"]);
    assert_eq!(
        outcome.items[0].raw, None,
        "an oversize raw is dropped, the item stays"
    );
    assert!(outcome.items[1].raw.is_some());
    assert_eq!(outcome.items[0].text.len(), 300 * 1024);
    assert!(
        outcome.warnings.iter().any(|w| w.contains("raw")),
        "{:?}",
        outcome.warnings
    );
}

#[test]
fn stickied_posts_are_skipped_on_new_and_kept_on_hot() {
    let new_http = ScriptedHttp::new(vec![Ok(fixture("listing_hot"))]);
    let hot_http = ScriptedHttp::new(vec![Ok(fixture("listing_hot"))]);
    let on_new = run(&new_http, &feed_settings(&["rust"], &["new"]), None).unwrap();
    let on_hot = run(&hot_http, &feed_settings(&["rust"], &["hot"]), None).unwrap();
    assert_eq!(on_hot.items.len(), 3);
    assert_eq!(on_new.items.len(), 2);
    assert!(!ids(&on_new).contains(&"t3_1kx9ba1"));
    assert!(ids(&on_hot).contains(&"t3_1kx9ba1"));
}

#[test]
fn hot_listing_requests_the_hot_path() {
    let http = ScriptedHttp::new(vec![Ok(fixture("listing_hot"))]);
    run(&http, &feed_settings(&["rust"], &["hot"]), None).unwrap();
    assert_eq!(
        http.requests()[0].url,
        "https://oauth.reddit.com/r/rust/hot?limit=25&raw_json=1"
    );
}

#[test]
fn non_t3_children_are_skipped() {
    let http = ScriptedHttp::new(vec![Ok(fixture("listing_removed"))]);
    let outcome = first_poll(&http);
    assert!(
        !outcome
            .items
            .iter()
            .any(|item| item.id.starts_with("t1_") || item.id.starts_with("t5_"))
    );
    assert_eq!(outcome.items.len(), 2);
    assert!(
        outcome.warnings.iter().any(|w| w.contains('4')),
        "the skipped children are counted: {:?}",
        outcome.warnings
    );
}

#[test]
fn removed_and_deleted_posts_are_emitted_with_their_literal_markers() {
    let http = ScriptedHttp::new(vec![Ok(fixture("listing_removed"))]);
    let outcome = first_poll(&http);
    let removed = outcome
        .items
        .iter()
        .find(|item| item.id == "t3_1kx9ca1")
        .unwrap();
    assert_eq!(removed.text, "[removed]");
    let deleted = outcome
        .items
        .iter()
        .find(|item| item.id == "t3_1kx9cb2")
        .unwrap();
    assert_eq!(deleted.author.as_deref(), Some("[deleted]"));
    assert_eq!(deleted.text, "[deleted]");
    // The mark advances past them: the newest of the two is the removed post.
    let mark = marks_of(&outcome).0.get("r/rust:new").cloned().unwrap();
    assert_eq!(mark.n, "t3_1kx9ca1");
}

#[test]
fn children_missing_a_name_or_title_are_skipped_and_counted_in_a_warning() {
    let http = ScriptedHttp::new(vec![Ok(fixture("listing_removed"))]);
    let outcome = first_poll(&http);
    assert_eq!(outcome.warnings.len(), 1, "{:?}", outcome.warnings);
    let warning = &outcome.warnings[0];
    assert!(warning.contains('4'), "{warning}");
    // Fixed text: no post content, no ids, no titles.
    for leaked in [
        "nameless",
        "titleless",
        "no name",
        "no title",
        "t1_",
        "t5_",
        "comment",
    ] {
        assert!(!warning.contains(leaked), "{warning}");
    }
}

#[test]
fn an_empty_page_yields_no_items_and_writes_no_mark() {
    let http = ScriptedHttp::new(vec![Ok(fixture("listing_empty"))]);
    let outcome = first_poll(&http);
    assert!(outcome.items.is_empty());
    assert!(marks_of(&outcome).0.get("r/rust:new").is_none());
}

#[test]
fn a_json_body_without_a_children_array_is_an_unreadable_listing_not_an_empty_page() {
    for body in [
        "{}",
        r#"{"data":{}}"#,
        r#"{"message":"leaky-body-text"}"#,
        r#"{"data":{"children":{}}}"#,
    ] {
        let http = ScriptedHttp::new(vec![Ok(text_response(body))]);
        match poll(&http, &rust_settings(), None, Some(TOKEN)) {
            Err(PollError::Transient(text)) => {
                assert!(text.contains("not a readable listing"), "{text}");
                assert!(!text.contains("leaky-body-text"), "{text}");
            }
            other => panic!("{body}: expected a transient unreadable-listing error, got {other:?}"),
        }
    }
}

#[test]
fn hostile_shapes_return_errors_or_skips_never_a_panic() {
    let deep = format!("{}{}", "[".repeat(300), "]".repeat(300));
    let bodies = [
        "null",
        "[]",
        r#""x""#,
        r#"{"data":null}"#,
        r#"{"data":{"children":null}}"#,
        r#"{"data":{"children":[null,1,"x",{"kind":3}]}}"#,
        r#"{"data":{"children":[{"kind":"t3","data":null},{"kind":"t3","data":[]},{"kind":"t3","data":{"name":7,"title":[],"created_utc":{}}}]}}"#,
        r#"{"data":{"children":[{"kind":"t3","data":{"name":"t3_a","title":"t","created_utc":1e300}}]}}"#,
        r#"{"data":{"children":[{"kind":"t3","data":{"name":"t3_a","title":"t","created_utc":1e400}}]}}"#,
        "",
        "\u{0}\u{1}",
        deep.as_str(),
    ];
    for body in bodies {
        let http = ScriptedHttp::new(vec![Ok(text_response(body))]);
        // Reaching the end of the call is the assertion: it returned.
        match poll(&http, &rust_settings(), None, Some(TOKEN)) {
            Ok(outcome) => assert!(outcome.items.is_empty(), "{body}"),
            Err(error) => assert!(
                matches!(error, PollError::Transient(_)),
                "{body}: {error:?}"
            ),
        }
    }
}
