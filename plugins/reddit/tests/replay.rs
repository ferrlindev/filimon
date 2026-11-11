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
