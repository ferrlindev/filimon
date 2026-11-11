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
    get_request(
        format!(
            "{API_BASE}/r/{}/{}?limit={limit}&raw_json=1",
            enc(subreddit),
            enc(listing)
        ),
        token,
        user_agent,
    )
}

/// `GET {API_BASE}/new?limit={limit}&raw_json=1`: the logged-in user's home
/// feed (RDDT-02, flagged assumption A1).
pub fn home_request(limit: u32, token: &str, user_agent: &str) -> HttpRequest {
    get_request(
        format!("{API_BASE}/new?limit={limit}&raw_json=1"),
        token,
        user_agent,
    )
}

/// `GET {API_BASE}/user/{username}/saved?limit={limit}&raw_json=1`: the user's
/// saved items (flagged assumption A2). `username` comes only from the
/// validated setting, never from API data, and is percent-encoded anyway
/// (T-07-29).
pub fn saved_request(username: &str, limit: u32, token: &str, user_agent: &str) -> HttpRequest {
    get_request(
        format!(
            "{API_BASE}/user/{}/saved?limit={limit}&raw_json=1",
            enc(username)
        ),
        token,
        user_agent,
    )
}

fn get_request(url: String, token: &str, user_agent: &str) -> HttpRequest {
    HttpRequest {
        url,
        headers: vec![
            ("authorization".to_string(), format!("bearer {token}")),
            ("user-agent".to_string(), user_agent.to_string()),
            ("accept".to_string(), "application/json".to_string()),
        ],
    }
}

/// Which listing a feed reads. The sticky rule differs (D-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Listing {
    New,
    Hot,
}

impl Listing {
    /// The listing named by a feed (`"new"` or `"hot"`).
    pub fn from_name(name: &str) -> Option<Listing> {
        match name {
            "new" => Some(Listing::New),
            "hot" => Some(Listing::Hot),
            _ => None,
        }
    }
}

/// Largest `raw` JSON text an item carries. Over it, `raw` is dropped and the
/// item is kept: well under the host's 1,000,000-byte record cap (T-07-16).
pub const RAW_MAX_BYTES: usize = 256 * 1024;

/// Largest accepted `created_utc`, in seconds (year 2286). Larger values are
/// not real post times, and the cursor size budget assumes a 10-digit mark.
pub const MAX_CREATED_UTC: f64 = 9_999_999_999.0;

/// The body is not a listing: not JSON, or no `data.children` array. The
/// text of an error built from it never echoes the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseError {
    Unreadable,
}

/// The usable posts of one page and how many children were skipped.
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedPage {
    pub posts: Vec<Post>,
    /// Children that were not a usable `t3` post: another kind, or missing a
    /// name, title, permalink or a usable `created_utc`. A stickied post that
    /// the `new` listing leaves out on purpose (D-12) is not counted here.
    pub skipped: usize,
}

fn text_of(data: &Value, key: &str) -> String {
    data.get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

/// One child as a post, or `None` when it is not a usable `t3` post. No
/// indexing, no `unwrap`: the child is attacker-influenced JSON (T-07-15).
fn parse_post(child: &Value) -> Option<Post> {
    if child.get("kind").and_then(Value::as_str) != Some("t3") {
        return None;
    }
    let data = child.get("data").filter(|data| data.is_object())?;
    let name = text_of(data, "name");
    let title = text_of(data, "title");
    let permalink = text_of(data, "permalink");
    let created_utc = data.get("created_utc").and_then(Value::as_f64)?;
    let usable_time = created_utc.is_finite() && (0.0..=MAX_CREATED_UTC).contains(&created_utc);
    if name.is_empty() || title.is_empty() || !permalink.starts_with('/') || !usable_time {
        return None;
    }
    Some(Post {
        name,
        title,
        selftext: text_of(data, "selftext"),
        permalink,
        created_utc,
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
    })
}

/// Parse a listing body. The top level must be an object whose `data.children`
/// is an array, otherwise [`ParseError::Unreadable`]: an error page that
/// happens to be JSON is never mistaken for an empty page. A stickied post is
/// left out of a `new` listing and kept on `hot` (D-12).
pub fn parse_listing(body: &[u8], listing: Listing) -> Result<ParsedPage, ParseError> {
    let root: Value = serde_json::from_slice(body).map_err(|_| ParseError::Unreadable)?;
    let children = root
        .get("data")
        .and_then(|data| data.get("children"))
        .and_then(Value::as_array)
        .ok_or(ParseError::Unreadable)?;
    let mut posts = Vec::new();
    let mut skipped = 0;
    for child in children {
        match parse_post(child) {
            None => skipped += 1,
            Some(post) if post.stickied && listing == Listing::New => {}
            Some(post) => posts.push(post),
        }
    }
    Ok(ParsedPage { posts, skipped })
}

/// Longest fullname the seen set stores. Real fullnames are `t3_` plus up to
/// about 8 base-36 characters; the bound keeps a hostile name from inflating
/// the cursor past the host cap.
const MAX_FULLNAME_LEN: usize = 20;

/// A fullname the seen set can hold: short, and only letters, digits and `_`.
fn valid_fullname(name: &str) -> bool {
    (1..=MAX_FULLNAME_LEN).contains(&name.len())
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// One child of a `saved` page, in listing order.
#[derive(Debug, Clone, PartialEq)]
pub struct SavedChild {
    /// The child's fullname, of any kind (`t3_..` or `t1_..`). `None` when it
    /// is missing or not a plausible fullname: such a child cannot be
    /// remembered.
    pub name: Option<String>,
    /// The child as a post. `None` for a comment or any unusable child.
    pub post: Option<Post>,
}

/// Parse a `saved` page into its children in listing order, comments
/// included (D-19). Same readability rule as [`parse_listing`].
pub fn parse_saved(body: &[u8]) -> Result<Vec<SavedChild>, ParseError> {
    let root: Value = serde_json::from_slice(body).map_err(|_| ParseError::Unreadable)?;
    let children = root
        .get("data")
        .and_then(|data| data.get("children"))
        .and_then(Value::as_array)
        .ok_or(ParseError::Unreadable)?;
    Ok(children
        .iter()
        .map(|child| {
            let name = child
                .get("data")
                .and_then(|data| data.get("name"))
                .and_then(Value::as_str)
                .filter(|name| valid_fullname(name))
                .map(str::to_string);
            // A post whose name cannot be remembered would be re-emitted on
            // every poll, so it counts as unusable.
            let post = parse_post(child).filter(|_| name.is_some());
            SavedChild { name, post }
        })
        .collect())
}

/// Whole seconds of a representable `created_utc`, or `None` for a value that
/// is not finite or is negative.
pub fn created_seconds(post: &Post) -> Option<i64> {
    (post.created_utc.is_finite() && post.created_utc >= 0.0).then(|| post.created_utc.floor() as i64)
}

/// Map a post to an item (D-08). `None` when `created_utc` is not
/// representable, so that post is skipped rather than emitted with a bad time.
/// `raw` is `None` when the post object is larger than [`RAW_MAX_BYTES`].
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
        raw: serde_json::to_string(&post.raw)
            .ok()
            .filter(|raw| raw.len() <= RAW_MAX_BYTES),
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
    fn published_is_created_utc_in_epoch_milliseconds_rounded() {
        let published = |created_utc: f64| post_to_item(&post(created_utc)).unwrap().published;
        assert_eq!(published(1_700_000_000.0), Some(1_700_000_000_000));
        assert_eq!(published(1_700_000_000.123), Some(1_700_000_000_123));
        assert_eq!(published(1_700_000_000.0004), Some(1_700_000_000_000));
        assert_eq!(published(1_700_000_000.9996), Some(1_700_000_001_000));
        assert_eq!(published(1_700_000_000.1236), Some(1_700_000_000_124));
    }

    #[test]
    fn created_utc_that_is_negative_or_not_a_number_is_skipped_without_a_panic() {
        // Each bad child sits beside a good one, which must still map.
        for bad in [
            "-1",
            r#""1700000000""#,
            "null",
            "[]",
            "1e300",
            "10000000000",
        ] {
            let body = format!(
                r#"{{"data":{{"children":[
                {{"kind":"t3","data":{{"name":"t3_bad","title":"t","permalink":"/r/a/x/","created_utc":{bad}}}}},
                {{"kind":"t3","data":{{"name":"t3_ok","title":"t","permalink":"/r/a/y/","created_utc":1.0}}}}]}}}}"#
            );
            let page = parse_listing(body.as_bytes(), Listing::New).unwrap();
            assert_eq!(page.skipped, 1, "created_utc {bad}");
            assert_eq!(page.posts.len(), 1, "created_utc {bad}");
            assert_eq!(page.posts[0].name, "t3_ok");
        }
        // 1e400 is not even a representable JSON number for the parser: the
        // whole body is unreadable, which is an error and never a panic.
        let overflow = br#"{"data":{"children":[{"kind":"t3","data":{"name":"t3_a","title":"t","created_utc":1e400}}]}}"#;
        assert_eq!(
            parse_listing(overflow, Listing::New),
            Err(ParseError::Unreadable)
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
        let err = parse_listing(b"<html>secret-token</html>", Listing::New).unwrap_err();
        assert_eq!(err, ParseError::Unreadable);
        assert!(!format!("{err:?}").contains("secret-token"));
        assert!(parse_listing(br#"{"kind":"Listing"}"#, Listing::New).is_err());
    }

    #[test]
    fn only_usable_t3_children_are_kept_and_the_rest_are_counted() {
        let body = br#"{"data":{"children":[
            {"kind":"t3","data":{"name":"t3_a","title":"t","permalink":"/r/a/x/","created_utc":1.0}},
            {"kind":"t1","data":{"name":"t1_b","created_utc":2.0}},
            {"kind":"t5","data":{"name":"t5_c"}}]}}"#;
        let page = parse_listing(body, Listing::New).unwrap();
        assert_eq!(page.posts.len(), 1);
        assert_eq!(page.posts[0].name, "t3_a");
        assert_eq!(page.skipped, 2);
    }

    #[test]
    fn user_feed_requests_are_official_authenticated_and_encode_the_username() {
        let home = home_request(25, "tok", "ua/1");
        assert_eq!(home.url, "https://oauth.reddit.com/new?limit=25&raw_json=1");
        let saved = saved_request("a b/c?d", 100, "tok", "ua/1");
        assert_eq!(
            saved.url,
            "https://oauth.reddit.com/user/a%20b%2Fc%3Fd/saved?limit=100&raw_json=1"
        );
        assert_eq!(
            saved_request("Snow_fall-77", 25, "tok", "ua/1").url,
            "https://oauth.reddit.com/user/Snow_fall-77/saved?limit=25&raw_json=1"
        );
        for request in [home, saved] {
            let header = |name: &str| crate::http::header(&request.headers, name);
            assert_eq!(header("authorization"), Some("bearer tok"));
            assert_eq!(header("user-agent"), Some("ua/1"));
            assert_eq!(header("accept"), Some("application/json"));
        }
    }

    #[test]
    fn a_saved_page_keeps_every_child_name_in_order_and_only_posts_as_posts() {
        let body = br#"{"data":{"children":[
            {"kind":"t3","data":{"name":"t3_a","title":"t","permalink":"/r/a/x/","created_utc":1.0}},
            {"kind":"t1","data":{"name":"t1_b","body":"c","created_utc":2.0}},
            {"kind":"t3","data":{"name":"t3_nolink","title":"t","created_utc":3.0}},
            {"kind":"t3","data":{"name":"t3_waytoolongtoremember_xxxxxxxx","title":"t","permalink":"/r/a/y/","created_utc":4.0}},
            {"kind":"t5","data":{}}]}}"#;
        let children = parse_saved(body).unwrap();
        let names: Vec<Option<&str>> = children.iter().map(|c| c.name.as_deref()).collect();
        assert_eq!(
            names,
            [Some("t3_a"), Some("t1_b"), Some("t3_nolink"), None, None]
        );
        let posts: Vec<bool> = children.iter().map(|c| c.post.is_some()).collect();
        assert_eq!(posts, [true, false, false, false, false]);
        assert_eq!(parse_saved(b"<html>"), Err(ParseError::Unreadable));
        assert_eq!(parse_saved(br#"{"data":{}}"#), Err(ParseError::Unreadable));
    }

    #[test]
    fn a_stickied_post_is_left_out_of_new_and_kept_on_hot() {
        let body = br#"{"data":{"children":[
            {"kind":"t3","data":{"name":"t3_a","title":"t","permalink":"/r/a/x/","created_utc":1.0,"stickied":true}}]}}"#;
        let on_new = parse_listing(body, Listing::New).unwrap();
        assert!(on_new.posts.is_empty());
        assert_eq!(on_new.skipped, 0, "a sticky left out by rule is not a skip");
        let on_hot = parse_listing(body, Listing::Hot).unwrap();
        assert_eq!(on_hot.posts.len(), 1);
    }
}
