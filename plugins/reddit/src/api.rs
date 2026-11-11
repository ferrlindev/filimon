//! Reddit API: the listing request, the listing parser and the post to item
//! mapping (D-08). Only the official OAuth host is ever requested.

use crate::http::HttpRequest;
use percent_encoding::{AsciiSet, NON_ALPHANUMERIC, utf8_percent_encode};
use serde_json::Value;

/// The official OAuth API host.
pub const API_BASE: &str = "https://oauth.reddit.com";
/// Prefix for the public permalink of a post.
pub const SITE_BASE: &str = "https://www.reddit.com";

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

/// A `t3` post out of a listing.
#[derive(Debug, Clone, PartialEq)]
pub struct Post {
    /// Fullname, for example `t3_1abc23`.
    pub name: String,
    pub title: String,
    pub selftext: String,
    /// Relative permalink, for example `/r/rust/comments/1abc23/title/`.
    pub permalink: String,
    /// Seconds since the epoch, as Reddit sends it.
    pub created_utc: f64,
    pub is_self: bool,
    pub stickied: bool,
    pub url: String,
    pub author: String,
    /// The API post object as received.
    pub raw: Value,
}

/// `GET {API_BASE}/r/{sub}/{listing}?limit={limit}&raw_json=1` with the
/// bearer token, the enforced user agent and a JSON accept header.
pub fn listing_request(
    subreddit: &str,
    listing: &str,
    limit: u32,
    token: &str,
    user_agent: &str,
) -> HttpRequest {
    HttpRequest {
        url: format!(
            "{API_BASE}/r/{}/{}?limit={limit}&raw_json=1",
            enc(subreddit),
            enc(listing)
        ),
        headers: vec![
            ("authorization".to_string(), format!("bearer {token}")),
            ("user-agent".to_string(), user_agent.to_string()),
            ("accept".to_string(), "application/json".to_string()),
        ],
    }
}

fn text_of(data: &Value, key: &str) -> String {
    data.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Parse a listing body. Keeps children whose `kind` is `t3`. The error text
/// never echoes the body.
pub fn parse_listing(body: &[u8]) -> Result<Vec<Post>, String> {
    let root: Value =
        serde_json::from_slice(body).map_err(|_| "malformed listing: not JSON".to_string())?;
    let children = root
        .get("data")
        .and_then(|data| data.get("children"))
        .and_then(Value::as_array)
        .ok_or_else(|| "malformed listing: no data.children".to_string())?;
    let mut posts = Vec::new();
    for child in children {
        if child.get("kind").and_then(Value::as_str) != Some("t3") {
            continue;
        }
        let Some(data) = child.get("data") else {
            continue;
        };
        let name = text_of(data, "name");
        if name.is_empty() {
            continue;
        }
        posts.push(Post {
            name,
            title: text_of(data, "title"),
            selftext: text_of(data, "selftext"),
            permalink: text_of(data, "permalink"),
            created_utc: data
                .get("created_utc")
                .and_then(Value::as_f64)
                .unwrap_or(f64::NAN),
            is_self: data
                .get("is_self")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            stickied: data
                .get("stickied")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            url: text_of(data, "url"),
            author: text_of(data, "author"),
            raw: data.clone(),
        });
    }
    Ok(posts)
}

/// Whole seconds of a representable `created_utc`, or `None` for a value that
/// is not finite or is negative.
pub fn created_seconds(post: &Post) -> Option<i64> {
    (post.created_utc.is_finite() && post.created_utc >= 0.0).then(|| post.created_utc.floor() as i64)
}

/// Map a post to an item (D-08). `None` when `created_utc` is not
/// representable, so that post is skipped rather than emitted with a bad time.
pub fn post_to_item(post: &Post) -> Option<Item> {
    created_seconds(post)?;
    let published = (post.created_utc * 1000.0).round() as u64;
    let text = if post.selftext.trim().is_empty() {
        post.title.clone()
    } else {
        post.selftext.clone()
    };
    Some(Item {
        id: post.name.clone(),
        url: format!("{SITE_BASE}{}", post.permalink),
        title: Some(post.title.clone()),
        text,
        author: (!post.author.is_empty()).then(|| post.author.clone()),
        published: Some(published),
        platform: "reddit".to_string(),
        raw: serde_json::to_string(&post.raw).ok(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(created_utc: f64) -> Post {
        Post {
            name: "t3_abc".to_string(),
            title: "A title".to_string(),
            selftext: String::new(),
            permalink: "/r/rust/comments/abc/a_title/".to_string(),
            created_utc,
            is_self: false,
            stickied: false,
            url: "https://example.com/".to_string(),
            author: "someone".to_string(),
            raw: serde_json::json!({"name": "t3_abc"}),
        }
    }

    #[test]
    fn created_utc_seconds_become_epoch_milliseconds_rounded_to_the_nearest() {
        assert_eq!(
            post_to_item(&post(1_700_000_000.0)).unwrap().published,
            Some(1_700_000_000_000)
        );
        assert_eq!(
            post_to_item(&post(1_700_000_000.1234)).unwrap().published,
            Some(1_700_000_000_123)
        );
        assert_eq!(
            post_to_item(&post(1_700_000_000.1236)).unwrap().published,
            Some(1_700_000_000_124)
        );
    }

    #[test]
    fn a_non_finite_or_negative_created_utc_is_skipped() {
        assert!(post_to_item(&post(f64::NAN)).is_none());
        assert!(post_to_item(&post(f64::INFINITY)).is_none());
        assert!(post_to_item(&post(-1.0)).is_none());
    }

    #[test]
    fn a_link_post_has_the_title_as_text_and_a_blank_selftext_does_too() {
        let mut p = post(1_700_000_000.0);
        p.selftext = "  \n".to_string();
        assert_eq!(post_to_item(&p).unwrap().text, "A title");
    }

    #[test]
    fn a_malformed_body_is_an_error_that_does_not_echo_it() {
        let err = parse_listing(b"<html>secret-token</html>").unwrap_err();
        assert!(!err.contains("secret-token"));
        assert!(parse_listing(br#"{"kind":"Listing"}"#).is_err());
    }

    #[test]
    fn only_t3_children_are_kept() {
        let body = br#"{"data":{"children":[
            {"kind":"t3","data":{"name":"t3_a","created_utc":1.0}},
            {"kind":"t1","data":{"name":"t1_b","created_utc":2.0}},
            {"kind":"t5","data":{"name":"t5_c"}}]}}"#;
        let posts = parse_listing(body).unwrap();
        assert_eq!(posts.len(), 1);
        assert_eq!(posts[0].name, "t3_a");
    }
}
