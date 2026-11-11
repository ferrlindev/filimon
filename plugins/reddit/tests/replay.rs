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
    let mark = cursor.mark("r/rust:new").expect("feed key r/rust:new");
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
    assert_eq!(
        error,
        PollError::Transient("Reddit HTTP 500 for r/programming:new".to_string())
    );
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
    let mark = marks_of(&outcome).mark("r/rust:new").cloned().unwrap();
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
    assert!(marks_of(&outcome).mark("r/rust:new").is_none());
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

// ── Task 2: per-feed marks across several feeds, stable output, errors ───────

fn two_feed_settings() -> Settings {
    feed_settings(&["rust", "programming"], &["new"])
}

fn mark(cursor: &cursor::Cursor, key: &str) -> cursor::Mark {
    cursor
        .mark(key)
        .unwrap_or_else(|| panic!("no mark for {key}"))
        .clone()
}

#[test]
fn first_poll_emits_one_page_per_feed_and_writes_a_mark_per_feed() {
    let http = ScriptedHttp::new(vec![Ok(fixture("listing_new")), Ok(fixture("listing_new"))]);
    let outcome = run(&http, &two_feed_settings(), None).unwrap();
    let urls: Vec<String> = http.requests().into_iter().map(|r| r.url).collect();
    assert_eq!(
        urls,
        [
            "https://oauth.reddit.com/r/rust/new?limit=25&raw_json=1",
            "https://oauth.reddit.com/r/programming/new?limit=25&raw_json=1",
        ]
    );
    // The same three posts answer both feeds: one is emitted once per poll.
    assert_eq!(outcome.items.len(), 3);
    let keys: Vec<String> = marks_of(&outcome).0.keys().cloned().collect();
    assert_eq!(keys, ["r/programming:new", "r/rust:new"]);
}

#[test]
fn later_poll_emits_only_posts_strictly_newer_than_the_mark() {
    let http = ScriptedHttp::new(vec![Ok(fixture("listing_new")), Ok(fixture("listing_newer"))]);
    let first = first_poll(&http);
    let second = run(&http, &rust_settings(), first.next_cursor.as_deref()).unwrap();
    // Oldest first: the two posts above the previously seen page.
    assert_eq!(ids(&second), ["t3_1kx9ae5", "t3_1kx9ad4"]);
    let advanced = mark(&marks_of(&second), "r/rust:new");
    assert_eq!(advanced.t, 1_700_000_500);
    assert_eq!(advanced.n, "t3_1kx9ad4");
    assert_eq!(
        http.requests()[1].url,
        "https://oauth.reddit.com/r/rust/new?limit=100&raw_json=1"
    );
}

#[test]
fn a_post_with_the_same_created_utc_as_the_mark_is_not_re_emitted() {
    let http = ScriptedHttp::new(vec![
        Ok(page(vec![post_data("t3_a", 1_700_000_100.0)])),
        Ok(page(vec![
            post_data("t3_a", 1_700_000_100.0),
            post_data("t3_b", 1_700_000_100.0),
            post_data("t3_c", 1_700_000_100.7),
        ])),
    ]);
    let first = first_poll(&http);
    let second = run(&http, &rust_settings(), first.next_cursor.as_deref()).unwrap();
    assert!(second.items.is_empty(), "{:?}", ids(&second));
    assert_eq!(second.next_cursor, first.next_cursor);
}

#[test]
fn a_deleted_marked_post_does_not_cause_a_full_re_emit() {
    let http = ScriptedHttp::new(vec![
        Ok(fixture("listing_new")),
        Ok(fixture("listing_marked_post_gone")),
    ]);
    let first = first_poll(&http);
    let second = run(&http, &rust_settings(), first.next_cursor.as_deref()).unwrap();
    assert!(second.items.is_empty(), "{:?}", ids(&second));
    assert_eq!(second.next_cursor, first.next_cursor, "the old mark stays");
}

#[test]
fn the_mark_is_the_max_over_the_emitted_posts_not_the_first_item() {
    let http = ScriptedHttp::new(vec![Ok(page(vec![
        post_data("t3_a", 1_700_000_100.0),
        post_data("t3_c", 1_700_000_300.0),
        post_data("t3_b", 1_700_000_200.0),
    ]))]);
    let outcome = first_poll(&http);
    let newest = mark(&marks_of(&outcome), "r/rust:new");
    assert_eq!((newest.t, newest.n.as_str()), (1_700_000_300, "t3_c"));
}

#[test]
fn feeds_have_independent_marks() {
    let both = feed_settings(&["rust"], &["new", "hot"]);
    let http = ScriptedHttp::new(vec![
        Ok(page(vec![post_data("t3_a", 1_700_000_100.0)])),
        Ok(page(vec![post_data("t3_b", 1_700_000_150.0)])),
        Ok(page(vec![post_data("t3_a", 1_700_000_100.0)])),
        Ok(page(vec![
            post_data("t3_c", 1_700_000_400.0),
            post_data("t3_b", 1_700_000_150.0),
        ])),
    ]);
    let first = run(&http, &both, None).unwrap();
    let before = marks_of(&first);
    let second = run(&http, &both, first.next_cursor.as_deref()).unwrap();
    assert_eq!(ids(&second), ["t3_c"]);
    let after = marks_of(&second);
    assert_eq!(mark(&after, "r/rust:new"), mark(&before, "r/rust:new"));
    assert_eq!(mark(&after, "r/rust:hot").n, "t3_c");
    assert_ne!(mark(&after, "r/rust:hot"), mark(&before, "r/rust:hot"));
}

#[test]
fn a_post_in_both_new_and_hot_is_emitted_once_per_poll() {
    let both = feed_settings(&["rust"], &["new", "hot"]);
    let http = ScriptedHttp::new(vec![
        Ok(page(vec![
            post_data("t3_a", 1_700_000_100.0),
            post_data("t3_b", 1_700_000_200.0),
        ])),
        Ok(page(vec![
            post_data("t3_b", 1_700_000_200.0),
            post_data("t3_c", 1_700_000_300.0),
        ])),
    ]);
    let outcome = run(&http, &both, None).unwrap();
    assert_eq!(ids(&outcome), ["t3_a", "t3_b", "t3_c"]);
    // The deduped post still moves its own feed's mark.
    let marks = marks_of(&outcome);
    assert_eq!(mark(&marks, "r/rust:new").n, "t3_b");
    assert_eq!(mark(&marks, "r/rust:hot").n, "t3_c");
}

#[test]
fn output_is_oldest_first_with_ties_broken_by_fullname() {
    let http = ScriptedHttp::new(vec![Ok(page(vec![
        post_data("t3_z", 1_700_000_100.0),
        post_data("t3_b", 1_700_000_100.0),
        post_data("t3_m", 1_700_000_050.0),
        post_data("t3_a", 1_700_000_100.0),
        post_data("t3_y", 1_700_000_900.0),
    ]))]);
    let outcome = first_poll(&http);
    assert_eq!(ids(&outcome), ["t3_m", "t3_a", "t3_b", "t3_z", "t3_y"]);
    assert_eq!(mark(&marks_of(&outcome), "r/rust:new").n, "t3_y");
}

#[test]
fn an_all_empty_first_poll_still_returns_a_cursor_not_none() {
    let http = ScriptedHttp::new(vec![Ok(fixture("listing_empty"))]);
    let outcome = first_poll(&http);
    assert!(outcome.items.is_empty());
    assert_eq!(outcome.next_cursor.as_deref(), Some(&b"{}"[..]));
    assert!(marks_of(&outcome).0.is_empty());
}

#[test]
fn marks_for_feeds_no_longer_configured_are_dropped() {
    let mut stored = cursor::Cursor::default();
    for key in ["r/rust:new", "r/removed_sub:new"] {
        stored.set_mark(
            key.to_string(),
            cursor::Mark {
                t: 1_700_000_300,
                n: "t3_1kx9aa1".to_string(),
            },
        );
    }
    let bytes = cursor::encode(&stored).unwrap();
    let http = ScriptedHttp::new(vec![Ok(fixture("listing_new"))]);
    let outcome = run(&http, &rust_settings(), Some(&bytes)).unwrap();
    assert!(outcome.items.is_empty());
    let keys: Vec<String> = marks_of(&outcome).0.keys().cloned().collect();
    assert_eq!(keys, ["r/rust:new"]);
}

#[test]
fn an_undecodable_cursor_is_a_first_poll_with_a_warning() {
    let http = ScriptedHttp::new(vec![Ok(fixture("listing_new"))]);
    let outcome = run(&http, &rust_settings(), Some(b"not json")).unwrap();
    assert_eq!(outcome.items.len(), 3, "a first poll emits the page");
    assert_eq!(outcome.warnings.len(), 1, "{:?}", outcome.warnings);
    assert!(!outcome.warnings[0].contains("not json"));
    assert_eq!(
        http.requests()[0].url,
        "https://oauth.reddit.com/r/rust/new?limit=25&raw_json=1",
        "a first poll takes the first-page limit"
    );
}

#[test]
fn a_failed_feed_fails_the_whole_poll_with_no_items_and_no_new_cursor() {
    let http = ScriptedHttp::new(vec![
        Ok(fixture("listing_new")),
        Err(HttpError::Timeout),
    ]);
    let result = run(&http, &two_feed_settings(), None);
    // `Err` carries no items and no cursor by construction: the host keeps
    // the cursor it stored, so a failure can only cause a re-fetch.
    assert!(matches!(result, Err(PollError::Transient(_))), "{result:?}");
    assert_eq!(http.requests().len(), 2);
}

#[test]
fn fetch_level_errors_map_to_the_poll_error_variants() {
    let poll_with = |error: HttpError| {
        let http = ScriptedHttp::new(vec![Err(error)]);
        run(&http, &rust_settings(), None).unwrap_err()
    };
    assert!(matches!(
        poll_with(HttpError::Denied("detail-zzz".to_string())),
        PollError::Config(text) if !text.contains("detail-zzz")
    ));
    assert!(matches!(
        poll_with(HttpError::NotConfigured),
        PollError::Config(_)
    ));
    assert!(matches!(
        poll_with(HttpError::Timeout),
        PollError::Transient(_)
    ));
    match poll_with(HttpError::Transport(
        "https://secret.example/path?token=abc".to_string(),
    )) {
        PollError::Transient(text) => {
            assert!(!text.contains("secret") && !text.contains("token"), "{text}");
        }
        other => panic!("expected transient, got {other:?}"),
    }
    assert_eq!(
        poll_with(HttpError::RateLimited(Some(30))),
        PollError::RateLimited(Some(30))
    );
    assert_eq!(
        poll_with(HttpError::RateLimited(None)),
        PollError::RateLimited(None)
    );
    assert!(matches!(
        poll_with(HttpError::InvalidRequest("detail-zzz".to_string())),
        PollError::Fatal(text) if !text.contains("detail-zzz")
    ));
}

// ── Plan 07-06 Task 1: response classification (D-13, D-04, SC4) ─────────────

/// A response with the given status and headers around a fixed opaque body.
fn status_response(status: u16, headers: &[(&str, &str)], body: &str) -> HttpResponse {
    HttpResponse {
        status,
        headers: headers
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
        body: body.as_bytes().to_vec(),
    }
}

/// The `listing_new` body with the given status and headers.
fn listing_with(status: u16, headers: &[(&str, &str)]) -> HttpResponse {
    let mut response = fixture("listing_new");
    response.status = status;
    response.headers = headers
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    response
}

fn poll_error_for(response: HttpResponse) -> PollError {
    let http = ScriptedHttp::new(vec![Ok(response)]);
    run(&http, &rust_settings(), None).unwrap_err()
}

#[test]
fn http_429_is_rate_limited_with_the_reset_seconds() {
    let response = fixture("rate_limited_429");
    assert_eq!(response.status, 429);
    let http = ScriptedHttp::new(vec![Ok(response)]);
    let result = run(&http, &rust_settings(), None);
    assert_eq!(result.unwrap_err(), PollError::RateLimited(Some(37)));
}

#[test]
fn remaining_below_one_on_a_200_is_rate_limited_at_the_boundary() {
    for remaining in ["1", "1.0", "59.0", "2"] {
        let http = ScriptedHttp::new(vec![Ok(listing_with(
            200,
            &[("x-ratelimit-remaining", remaining), ("x-ratelimit-reset", "30")],
        ))]);
        let outcome = run(&http, &rust_settings(), None)
            .unwrap_or_else(|e| panic!("remaining {remaining} must not be limited: {e:?}"));
        assert_eq!(outcome.items.len(), 3, "remaining {remaining}");
    }
    for remaining in ["0.99", "0.5", "0", "0.0"] {
        let http = ScriptedHttp::new(vec![Ok(listing_with(
            200,
            &[("x-ratelimit-remaining", remaining), ("x-ratelimit-reset", "30")],
        ))]);
        let error = run(&http, &rust_settings(), None).unwrap_err();
        assert_eq!(
            error,
            PollError::RateLimited(Some(30)),
            "remaining {remaining}"
        );
    }
}

#[test]
fn a_missing_reset_header_gives_rate_limited_none() {
    let error = poll_error_for(status_response(
        429,
        &[("x-ratelimit-remaining", "0")],
        "{}",
    ));
    assert_eq!(error, PollError::RateLimited(None));
    let error = poll_error_for(status_response(
        429,
        &[("x-ratelimit-reset", "soon")],
        "{}",
    ));
    assert_eq!(error, PollError::RateLimited(None));
    let error = poll_error_for(status_response(429, &[], "{}"));
    assert_eq!(error, PollError::RateLimited(None));
}

#[test]
fn http_401_is_auth_required() {
    let response = fixture("unauthorized_401");
    assert_eq!(response.status, 401);
    assert_eq!(poll_error_for(response), PollError::AuthRequired);
}

#[test]
fn http_403_is_a_named_transient_and_404_is_a_named_config_error() {
    match poll_error_for(status_response(403, &[], "forbidden-body-text")) {
        PollError::Transient(text) => {
            assert!(text.contains("r/rust:new"), "{text}");
            assert!(text.contains("403"), "{text}");
            assert!(!text.contains("forbidden-body-text"), "{text}");
        }
        other => panic!("403 must be transient, got {other:?}"),
    }
    match poll_error_for(status_response(404, &[], "missing-body-text")) {
        PollError::Config(text) => {
            assert!(text.contains("r/rust:new"), "{text}");
            assert!(text.contains("404"), "{text}");
            assert!(!text.contains("missing-body-text"), "{text}");
        }
        other => panic!("404 must be a config error, got {other:?}"),
    }
}

#[test]
fn http_5xx_is_transient() {
    for status in [500u16, 502, 503] {
        match poll_error_for(status_response(status, &[], "oops-body-text")) {
            PollError::Transient(text) => {
                assert!(text.contains(&status.to_string()), "{text}");
                assert!(!text.contains("oops-body-text"), "{text}");
            }
            other => panic!("{status} must be transient, got {other:?}"),
        }
    }
}

#[test]
fn malformed_json_is_transient_without_echoing_the_body() {
    let response = fixture("malformed");
    assert_eq!(response.status, 200);
    match poll_error_for(response) {
        PollError::Transient(text) => {
            assert!(!text.contains("SENTINEL_TRUNCATED_BODY_TEXT"), "{text}");
        }
        other => panic!("malformed JSON must be transient, got {other:?}"),
    }
}

#[test]
fn every_error_status_maps_to_an_error_never_an_empty_success() {
    for status in [301u16, 302, 400, 401, 403, 404, 408, 429, 500, 502, 503] {
        // An empty-but-valid listing body: a naive implementation that ignored
        // the status would turn it into a quiet empty success.
        let body = serde_json::json!({"kind": "Listing", "data": {"children": []}}).to_string();
        let http = ScriptedHttp::new(vec![Ok(status_response(status, &[], &body))]);
        let result = run(&http, &rust_settings(), None);
        assert!(result.is_err(), "status {status} returned {result:?}");
    }
}

#[test]
fn no_rate_limit_retry_inside_one_poll() {
    let three = feed_settings(&["rust", "programming", "news"], &["new"]);
    let http = ScriptedHttp::new(vec![
        Ok(fixture("rate_limited_429")),
        Ok(fixture("listing_new")),
        Ok(fixture("listing_new")),
    ]);
    let error = run(&http, &three, None).unwrap_err();
    assert_eq!(error, PollError::RateLimited(Some(37)));
    assert_eq!(http.requests().len(), 1, "no second attempt was made");
}

#[test]
fn a_poll_after_a_rate_limit_succeeds_with_no_state_to_reset() {
    let http = ScriptedHttp::new(vec![
        Ok(fixture("rate_limited_429")),
        Ok(fixture("listing_new")),
    ]);
    assert!(matches!(
        run(&http, &rust_settings(), None),
        Err(PollError::RateLimited(Some(37)))
    ));
    let outcome = run(&http, &rust_settings(), None).expect("the next poll succeeds");
    assert_eq!(outcome.items.len(), 3);
}

// ── Plan 07-06 Task 2: compliance sweep over every request (RDDT-03, D-14) ───

/// A recording `Http` that checks every request as it arrives and panics on the
/// first violation, so a scenario cannot pass with a non-compliant request.
struct StrictHttp {
    inner: ScriptedHttp,
    expected_user_agent: String,
    token: &'static str,
}

impl StrictHttp {
    fn new(
        responses: Vec<Result<HttpResponse, HttpError>>,
        app_id: &str,
        contact_name: &str,
    ) -> Self {
        StrictHttp {
            inner: ScriptedHttp::new(responses),
            expected_user_agent: format!(
                "filimon:{app_id}:{} (by /u/{contact_name})",
                env!("CARGO_PKG_VERSION")
            ),
            token: TOKEN,
        }
    }
}

impl Http for StrictHttp {
    fn get(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError> {
        let url = &request.url;
        assert!(
            url.starts_with("https://oauth.reddit.com/"),
            "request to an unofficial host: {url}"
        );
        let header = |name: &str| reddit_source::http::header(&request.headers, name);
        assert_eq!(
            header("authorization"),
            Some(format!("bearer {}", self.token).as_str()),
            "{url}"
        );
        assert_eq!(header("accept"), Some("application/json"), "{url}");
        assert_eq!(
            header("user-agent"),
            Some(self.expected_user_agent.as_str()),
            "{url}"
        );
        self.inner.get(request)
    }
}

fn sweep_settings(subreddits: &[&str], listings: &[&str], app_id: Option<&str>) -> Settings {
    let quote = |items: &[&str]| {
        items
            .iter()
            .map(|item| format!("\"{item}\""))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let app_id_line = app_id
        .map(|id| format!("app_id = \"{id}\"\n"))
        .unwrap_or_default();
    settings(&format!(
        "subreddits = [{}]\nlistings = [{}]\nuser_agent_contact = \"u/sweeper\"\n{app_id_line}",
        quote(subreddits),
        quote(listings)
    ))
}

#[test]
fn every_request_is_official_authenticated_and_carries_the_enforced_user_agent() {
    // One subreddit, `new`: a first poll then a later poll.
    let one = sweep_settings(&["rust"], &["new"], None);
    let http = StrictHttp::new(
        vec![Ok(fixture("listing_new")), Ok(fixture("listing_newer"))],
        "reddit",
        "sweeper",
    );
    let first = poll(&http, &one, None, Some(TOKEN)).expect("first poll");
    poll(&http, &one, first.next_cursor.as_deref(), Some(TOKEN)).expect("later poll");
    assert_eq!(http.inner.requests().len(), 2);

    // Several subreddits, `new` and `hot`: four feeds, first and later polls.
    let many = sweep_settings(&["rust", "programming"], &["new", "hot"], None);
    let responses = (0..8).map(|_| Ok(fixture("listing_hot"))).collect();
    let http = StrictHttp::new(responses, "reddit", "sweeper");
    let first = poll(&http, &many, None, Some(TOKEN)).expect("first poll");
    poll(&http, &many, first.next_cursor.as_deref(), Some(TOKEN)).expect("later poll");
    assert_eq!(http.inner.requests().len(), 8);

    // A rate-limited poll still sends a compliant request.
    let http = StrictHttp::new(vec![Ok(fixture("rate_limited_429"))], "reddit", "sweeper");
    assert_eq!(
        poll(&http, &one, None, Some(TOKEN)).unwrap_err(),
        PollError::RateLimited(Some(37))
    );
    assert_eq!(http.inner.requests().len(), 1);

    // An authorization failure and a server error also go out compliant.
    for response in [fixture("unauthorized_401"), status_response(503, &[], "")] {
        let http = StrictHttp::new(vec![Ok(response)], "reddit", "sweeper");
        assert!(poll(&http, &one, None, Some(TOKEN)).is_err());
        assert_eq!(http.inner.requests().len(), 1);
    }

    // A non-default app id flows into the user agent.
    let custom = sweep_settings(&["rust"], &["new"], Some("my-app"));
    let http = StrictHttp::new(vec![Ok(fixture("listing_new"))], "my-app", "sweeper");
    poll(&http, &custom, None, Some(TOKEN)).expect("custom app id poll");
    let requests = http.inner.requests();
    let agent = reddit_source::http::header(&requests[0].headers, "user-agent").unwrap();
    assert!(agent.starts_with("filimon:my-app:"), "{agent}");
}

// ── Plan 07-07 Task 1: the logged-in feeds, home and saved (D-05, D-19) ──────

fn home_settings() -> Settings {
    settings("feed = \"home\"\nuser_agent_contact = \"u/tester\"\n")
}

const SAVED_USER: &str = "snowfall77";

fn saved_settings() -> Settings {
    settings(&format!(
        "feed = \"saved\"\nusername = \"{SAVED_USER}\"\nuser_agent_contact = \"u/tester\"\n"
    ))
}

/// The children of a fixture listing, kinds included, in listing order.
fn fixture_children(name: &str) -> Vec<Value> {
    let root: Value = serde_json::from_slice(&fixture(name).body).unwrap();
    root["data"]["children"].as_array().unwrap().clone()
}

/// A 200 listing response around raw children (any kind).
fn listing_of(children: Vec<Value>) -> HttpResponse {
    text_response(
        &serde_json::json!({"kind": "Listing", "data": {"children": children}}).to_string(),
    )
}

/// A saved `t3` child.
fn saved_t3(name: &str, created_utc: f64) -> Value {
    serde_json::json!({"kind": "t3", "data": post_data(name, created_utc)})
}

/// A saved `t1` comment child.
fn saved_t1(name: &str) -> Value {
    serde_json::json!({"kind": "t1", "data": {
        "name": name,
        "body": "a saved comment body",
        "link_id": "t3_zzz",
        "author": "commenter",
        "created_utc": 1_699_999_000.0,
    }})
}

fn child_name(child: &Value) -> String {
    child["data"]["name"].as_str().unwrap().to_string()
}

fn stored_json(outcome: &PollOutcome) -> Value {
    serde_json::from_slice(outcome.next_cursor.as_deref().expect("a cursor")).unwrap()
}

/// The seen fullnames the cursor stores for one feed key.
fn seen_of(outcome: &PollOutcome, key: &str) -> Vec<String> {
    stored_json(outcome)[key]["s"]
        .as_array()
        .unwrap_or_else(|| panic!("no seen set for {key}: {}", stored_json(outcome)))
        .iter()
        .map(|name| name.as_str().unwrap().to_string())
        .collect()
}

/// Poll `saved` once with the given children, optionally from a stored cursor.
fn saved_poll(
    children: Vec<Value>,
    cursor: Option<&[u8]>,
) -> (ScriptedHttp, Result<PollOutcome, PollError>) {
    let http = ScriptedHttp::new(vec![Ok(listing_of(children))]);
    let result = run(&http, &saved_settings(), cursor);
    (http, result)
}

#[test]
fn home_feed_requests_new_with_the_bearer_token() {
    let http = ScriptedHttp::new(vec![Ok(fixture("home_page"))]);
    let outcome = run(&http, &home_settings(), None).expect("home poll");
    let requests = http.requests();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0].url,
        "https://oauth.reddit.com/new?limit=25&raw_json=1"
    );
    let header = |name: &str| reddit_source::http::header(&requests[0].headers, name);
    assert_eq!(header("authorization"), Some("bearer TOKEN-1"));
    let json = stored_json(&outcome);
    let keys: Vec<&String> = json.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["home"]);
}

#[test]
fn home_feed_uses_the_created_utc_mark_and_skips_stickied() {
    let children = fixture_children("home_page");
    assert_eq!(children.len(), 4);
    let http = ScriptedHttp::new(vec![Ok(fixture("home_page"))]);
    let first = run(&http, &home_settings(), None).expect("first poll");
    assert_eq!(first.items.len(), children.len() - 1, "one stickied post");

    let top = children
        .iter()
        .filter(|c| c["data"]["stickied"] == false)
        .map(|c| c["data"]["created_utc"].as_f64().unwrap())
        .fold(0.0, f64::max);
    let http = ScriptedHttp::new(vec![Ok(listing_of(vec![
        saved_t3("t3_hnew", top + 60.0),
        saved_t3("t3_hsame", top),
        saved_t3("t3_hold", top - 500.0),
    ]))]);
    let second = run(&http, &home_settings(), first.next_cursor.as_deref()).expect("second poll");
    assert_eq!(ids(&second), ["t3_hnew"]);
    assert_eq!(stored_json(&second)["home"]["t"], (top + 60.0) as i64);
    assert_eq!(
        http.requests()[0].url,
        "https://oauth.reddit.com/new?limit=100&raw_json=1"
    );
}

#[test]
fn saved_feed_requests_the_users_saved_path_percent_encoded() {
    let (http, result) = saved_poll(fixture_children("saved_page"), None);
    let outcome = result.expect("saved poll");
    assert_eq!(
        http.requests()[0].url,
        format!("https://oauth.reddit.com/user/{SAVED_USER}/saved?limit=25&raw_json=1")
    );
    let json = stored_json(&outcome);
    let keys: Vec<&String> = json.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["saved"]);

    // A prefixed name with `_` and `-` goes into the path unencoded.
    let prefixed = settings(
        "feed = \"saved\"\nusername = \"u/Snow_fall-77\"\nuser_agent_contact = \"u/tester\"\n",
    );
    let http = ScriptedHttp::new(vec![Ok(listing_of(Vec::new()))]);
    run(&http, &prefixed, None).expect("poll");
    assert_eq!(
        http.requests()[0].url,
        "https://oauth.reddit.com/user/Snow_fall-77/saved?limit=25&raw_json=1"
    );
}

#[test]
fn saved_first_poll_emits_the_posts_and_stores_the_top_25_fullnames() {
    let children = fixture_children("saved_page");
    let (_, result) = saved_poll(children.clone(), None);
    let outcome = result.expect("saved poll");
    let t3_names: Vec<String> = children
        .iter()
        .filter(|c| c["kind"] == "t3")
        .map(child_name)
        .collect();
    assert_eq!(t3_names.len(), 4);
    let mut emitted: Vec<String> = ids(&outcome).iter().map(|id| id.to_string()).collect();
    emitted.sort();
    let mut expected = t3_names.clone();
    expected.sort();
    assert_eq!(emitted, expected, "the t3 posts only");
    assert!(
        outcome.warnings.iter().any(|w| w.contains('1')),
        "the skipped comment is counted: {:?}",
        outcome.warnings
    );
    let all: Vec<String> = children.iter().map(child_name).collect();
    assert_eq!(seen_of(&outcome, "saved"), all, "comments included");
    assert_eq!(all.len(), 5);
}

#[test]
fn saved_later_poll_emits_entries_until_the_first_already_seen_fullname() {
    let base = fixture_children("saved_page");
    let (_, first) = saved_poll(base.clone(), None);
    let first = first.expect("first poll");

    let mut next_page = vec![
        saved_t3("t3_snew1", 1_700_001_000.0),
        saved_t3("t3_snew2", 1_700_001_100.0),
    ];
    next_page.extend(base.clone());
    let (http, second) = saved_poll(next_page, first.next_cursor.as_deref());
    let second = second.expect("second poll");
    assert_eq!(ids(&second), ["t3_snew2", "t3_snew1"]);
    let mut expected = vec!["t3_snew1".to_string(), "t3_snew2".to_string()];
    expected.extend(base.iter().map(child_name));
    assert_eq!(seen_of(&second, "saved"), expected);
    assert_eq!(
        http.requests()[0].url,
        format!("https://oauth.reddit.com/user/{SAVED_USER}/saved?limit=100&raw_json=1")
    );
}

#[test]
fn saved_stops_at_the_first_seen_fullname_even_for_a_comment_entry() {
    let (_, first) = saved_poll(
        vec![saved_t3("t3_sa", 1_700_000_100.0), saved_t1("t1_sc")],
        None,
    );
    let first = first.expect("first poll");
    let (_, second) = saved_poll(
        vec![
            saved_t3("t3_sn", 1_700_000_900.0),
            saved_t1("t1_sc"),
            saved_t3("t3_below", 1_700_000_800.0),
        ],
        first.next_cursor.as_deref(),
    );
    let second = second.expect("second poll");
    assert_eq!(ids(&second), ["t3_sn"], "nothing below the seen comment");
}

#[test]
fn an_old_post_saved_later_is_emitted_even_though_created_utc_is_old() {
    let (_, first) = saved_poll(vec![saved_t3("t3_sa", 1_700_000_000.0)], None);
    let first = first.expect("first poll");
    let (_, second) = saved_poll(
        vec![
            saved_t3("t3_old", 1_000_000_000.0),
            saved_t3("t3_sa", 1_700_000_000.0),
        ],
        first.next_cursor.as_deref(),
    );
    let second = second.expect("second poll");
    assert_eq!(ids(&second), ["t3_old"]);
}

#[test]
fn saved_with_nothing_new_keeps_the_seen_set_and_emits_nothing() {
    let page = fixture_children("saved_page");
    let (_, first) = saved_poll(page.clone(), None);
    let first = first.expect("first poll");
    let (_, second) = saved_poll(page, first.next_cursor.as_deref());
    let second = second.expect("second poll");
    assert!(second.items.is_empty(), "{:?}", ids(&second));
    assert_eq!(second.next_cursor, first.next_cursor);
}

#[test]
fn an_empty_saved_page_emits_nothing_and_stores_an_empty_seen_set() {
    let (_, result) = saved_poll(Vec::new(), None);
    let outcome = result.expect("saved poll");
    assert!(outcome.items.is_empty());
    assert_eq!(
        outcome.next_cursor.as_deref(),
        Some(&br#"{"saved":{"s":[]}}"#[..])
    );
    // The empty set is state: a post saved afterwards is emitted.
    let (_, later) = saved_poll(
        vec![saved_t3("t3_first", 1_600_000_000.0)],
        outcome.next_cursor.as_deref(),
    );
    assert_eq!(ids(&later.expect("later poll")), ["t3_first"]);
}

#[test]
fn saved_output_is_oldest_saved_first() {
    let (_, result) = saved_poll(
        vec![
            saved_t3("t3_third", 1_500_000_000.0),
            saved_t3("t3_second", 1_700_000_000.0),
            saved_t3("t3_first", 1_600_000_000.0),
        ],
        None,
    );
    assert_eq!(
        ids(&result.expect("saved poll")),
        ["t3_first", "t3_second", "t3_third"]
    );
}

#[test]
fn the_seen_set_is_capped_at_25_fullnames_and_the_cursor_fits() {
    let children: Vec<Value> = (0..100)
        .map(|i| saved_t3(&format!("t3_s{i:03}"), 1_700_000_000.0 + f64::from(i)))
        .collect();
    let (_, result) = saved_poll(children.clone(), None);
    let outcome = result.expect("saved poll");
    let seen = seen_of(&outcome, "saved");
    assert_eq!(seen.len(), 25);
    let expected: Vec<String> = children.iter().take(25).map(child_name).collect();
    assert_eq!(seen, expected, "the top of the page, in listing order");
    assert!(outcome.next_cursor.unwrap().len() <= MAX_CURSOR_BYTES);
}

#[test]
fn time_mark_cursor_json_layout_is_stable() {
    let http = ScriptedHttp::new(vec![Ok(page(vec![post_data("t3_a", 1_700_000_100.0)]))]);
    let outcome = first_poll(&http);
    assert_eq!(
        std::str::from_utf8(outcome.next_cursor.as_deref().unwrap()).unwrap(),
        r#"{"r/rust:new":{"t":1700000100,"n":"t3_a"}}"#
    );
    let http = ScriptedHttp::new(vec![Ok(page(vec![post_data("t3_h", 1_700_000_200.0)]))]);
    let outcome = run(&http, &home_settings(), None).expect("home poll");
    assert_eq!(
        std::str::from_utf8(outcome.next_cursor.as_deref().unwrap()).unwrap(),
        r#"{"home":{"t":1700000200,"n":"t3_h"}}"#
    );
}

#[test]
fn user_feed_without_a_token_is_auth_required_before_any_request() {
    for feed in [home_settings(), saved_settings()] {
        for token in [None, Some("")] {
            let http = ScriptedHttp::new(vec![Ok(fixture("home_page"))]);
            let result = poll(&http, &feed, None, token);
            assert_eq!(result.unwrap_err(), PollError::AuthRequired);
            assert!(http.requests().is_empty());
        }
    }
}

#[test]
fn a_stored_value_of_the_wrong_shape_is_a_first_poll_of_that_feed() {
    // A seen set stored under a key the settings now read as a time mark, and
    // the other way round: both are a first poll, and the stale value never
    // survives into the new cursor.
    let seen_cursor = br#"{"home":{"s":["t3_x"]}}"#;
    let http = ScriptedHttp::new(vec![Ok(fixture("home_page"))]);
    let outcome = run(&http, &home_settings(), Some(seen_cursor)).expect("home poll");
    assert_eq!(outcome.items.len(), 3, "a first poll emits the page");
    assert!(stored_json(&outcome)["home"]["t"].is_i64());
    assert!(outcome.warnings.iter().all(|w| !w.contains("decode")));

    let time_cursor = br#"{"saved":{"t":1700000000,"n":"t3_x"}}"#;
    let (http, result) = saved_poll(fixture_children("saved_page"), Some(time_cursor));
    let outcome = result.expect("saved poll");
    assert_eq!(outcome.items.len(), 4, "a first poll emits every t3 entry");
    assert!(stored_json(&outcome)["saved"]["s"].is_array());
    assert!(http.requests()[0].url.contains("limit=25"));
}
