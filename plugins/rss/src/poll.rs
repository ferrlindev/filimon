//! One poll: fetch each configured feed, parse it, keep only the entries newer
//! than the stored mark, and advance the cursor. Plain Rust behind the `Http`
//! trait, so the tests replay feeds natively.
//!
//! Feed content is untrusted. Titles, authors and bodies are turned into plain
//! text, links must be http(s), and no warning or error repeats feed content.

use crate::cursor::{self, MAX_IDS_PER_MARK};
use crate::feed::{self, Entry};
use crate::http::{Http, HttpError, HttpRequest, HttpResponse, header};
use crate::settings::Settings;
use crate::text::{fnv1a, html_to_text, plain_line, truncate_chars};

const ACCEPT: &str =
    "application/rss+xml, application/atom+xml, application/xml;q=0.9, text/xml;q=0.8, */*;q=0.5";
const USER_AGENT: &str = "filimon-rss-plugin/0.1";
/// Item text is replaced by the title when the body is empty; categories kept per item.
const MAX_CATEGORIES: usize = 10;

/// An item for the host. Mirrors `item` in `wit/types.wit`.
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

/// Mirrors `poll-error` in `wit/types.wit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollError {
    RateLimited(Option<u32>),
    AuthRequired,
    Config(String),
    Transient(String),
    Fatal(String),
}

#[derive(Debug, Default)]
pub struct Outcome {
    pub items: Vec<Item>,
    pub next_cursor: Option<Vec<u8>>,
    pub warnings: Vec<String>,
}

fn http_error(error: HttpError) -> PollError {
    match error {
        HttpError::Denied(_) => PollError::Config(
            "a feed host is not in the plugin's `allowlist` (host.allowlist)".to_string(),
        ),
        HttpError::Timeout => PollError::Transient("feed request timed out".to_string()),
        HttpError::Transport(_) => PollError::Transient("feed request failed (network)".to_string()),
        HttpError::RateLimited(seconds) => PollError::RateLimited(seconds),
        HttpError::NotConfigured => PollError::Config(
            "this source has no fetcher; set `fetcher` in filimon.toml".to_string(),
        ),
        HttpError::InvalidRequest(_) => PollError::Config("a feed URL was refused by the host".to_string()),
    }
}

fn status_error(response: &HttpResponse) -> Option<PollError> {
    match response.status {
        200..=299 => None,
        429 => {
            let retry = header(&response.headers, "retry-after").and_then(|v| v.trim().parse::<u32>().ok());
            Some(PollError::RateLimited(retry))
        }
        408 | 500..=599 => Some(PollError::Transient(format!("feed answered HTTP {}", response.status))),
        status => Some(PollError::Fatal(format!("feed answered HTTP {status}"))),
    }
}

/// Keep only http(s) URLs without control characters.
fn safe_url(url: &str) -> Option<String> {
    let url = url.trim();
    let ok = (url.starts_with("https://") || url.starts_with("http://"))
        && !url.chars().any(|c| c.is_control() || c.is_whitespace());
    ok.then(|| url.to_string())
}

fn build_item(feed_url: &str, entry: &Entry, max_text_chars: usize) -> Option<(u32, i64, Item)> {
    let id = entry
        .guid
        .as_deref()
        .or(entry.link.as_deref())
        .map(plain_line)
        .filter(|id| !id.is_empty())?;
    let url = entry
        .link
        .as_deref()
        .and_then(safe_url)
        .or_else(|| entry.guid.as_deref().and_then(safe_url))
        .unwrap_or_else(|| feed_url.to_string());
    let title = entry.title.as_deref().map(plain_line).filter(|t| !t.is_empty());
    let body = entry.content.as_deref().or(entry.summary.as_deref()).unwrap_or("");
    let mut text = truncate_chars(&html_to_text(body), max_text_chars);
    if text.is_empty() {
        text = title.clone().unwrap_or_default();
    }
    let categories: Vec<String> = entry
        .categories
        .iter()
        .map(|c| plain_line(c))
        .filter(|c| !c.is_empty())
        .take(MAX_CATEGORIES)
        .collect();
    let raw = serde_json::json!({ "feed": feed_url, "categories": categories }).to_string();
    let time = entry.published_ms.unwrap_or(0);
    let item = Item {
        id: id.clone(),
        url,
        title,
        text,
        author: entry.author.as_deref().map(plain_line).filter(|a| !a.is_empty()),
        published: entry.published_ms.and_then(|ms| u64::try_from(ms).ok()),
        platform: "rss".to_string(),
        raw: Some(raw),
    };
    Some((fnv1a(&id), time, item))
}

/// Poll one feed. Returns the new items (oldest first) and the feed's new mark.
fn poll_feed(
    http: &dyn Http,
    settings: &Settings,
    url: &str,
    mark: Option<&(i64, Vec<u32>)>,
) -> Result<(Vec<Item>, Option<(i64, Vec<u32>)>, usize), PollError> {
    let response = http
        .get(&HttpRequest {
            url: url.to_string(),
            headers: vec![
                ("accept".to_string(), ACCEPT.to_string()),
                ("user-agent".to_string(), USER_AGENT.to_string()),
            ],
        })
        .map_err(http_error)?;
    if let Some(error) = status_error(&response) {
        return Err(error);
    }
    let entries = feed::parse(&response.body).map_err(|e| PollError::Fatal(e))?;

    let mut skipped = 0usize;
    let mut built: Vec<(u32, i64, Item)> = Vec::new();
    for entry in &entries {
        match build_item(url, entry, settings.max_text_chars) {
            Some(b) => built.push(b),
            None => skipped += 1,
        }
    }

    // The new mark: the newest time in the whole feed and the entries at it.
    let newest = built.iter().map(|(_, t, _)| *t).max();
    let new_mark = newest.map(|t| {
        let mut ids: Vec<u32> = built.iter().filter(|(_, bt, _)| *bt == t).map(|(h, _, _)| *h).collect();
        ids.sort_unstable();
        ids.dedup();
        ids.truncate(MAX_IDS_PER_MARK);
        (t, ids)
    });

    // Entries newer than the stored mark (everything on a feed's first poll).
    let mut fresh: Vec<(i64, Item)> = built
        .into_iter()
        .filter(|(hash, t, _)| match mark {
            None => true,
            Some((mark_t, mark_ids)) => *t > *mark_t || (*t == *mark_t && !mark_ids.contains(hash)),
        })
        .map(|(_, t, item)| (t, item))
        .collect();
    fresh.sort_by_key(|(t, _)| *t);
    // Past the cap keep the newest.
    if fresh.len() > settings.max_items_per_feed {
        fresh.drain(..fresh.len() - settings.max_items_per_feed);
    }
    Ok((fresh.into_iter().map(|(_, item)| item).collect(), new_mark, skipped))
}

/// Poll every configured feed once.
pub fn poll(http: &dyn Http, settings: &Settings, cursor_bytes: Option<&[u8]>) -> Result<Outcome, PollError> {
    let (mut cursor, undecodable) = cursor::decode(cursor_bytes);
    let mut outcome = Outcome::default();
    if undecodable {
        outcome
            .warnings
            .push("stored cursor was unreadable; treating this as a first poll".to_string());
    }
    let keys: Vec<String> = settings.feeds.iter().map(|u| cursor::feed_key(u)).collect();
    cursor.retain_feeds(&keys);

    let mut first_error: Option<PollError> = None;
    let mut failed = 0usize;
    for (index, url) in settings.feeds.iter().enumerate() {
        let key = &keys[index];
        match poll_feed(http, settings, url, cursor.mark(key)) {
            Ok((items, new_mark, skipped)) => {
                if skipped > 0 {
                    outcome.warnings.push(format!(
                        "feed #{}: skipped {skipped} entr{} with no id or link",
                        index + 1,
                        if skipped == 1 { "y" } else { "ies" }
                    ));
                }
                if let Some((t, ids)) = new_mark {
                    cursor.set(key, t, ids);
                }
                outcome.items.extend(items);
            }
            Err(error) => {
                failed += 1;
                outcome.warnings.push(format!("feed #{} failed: {}", index + 1, describe(&error)));
                first_error.get_or_insert(error);
            }
        }
    }
    // One broken feed does not stop the others; all of them failing is an error.
    if failed == settings.feeds.len() {
        if let Some(error) = first_error {
            return Err(error);
        }
    }
    outcome.next_cursor = Some(cursor::encode(&cursor).map_err(PollError::Fatal)?);
    Ok(outcome)
}

fn describe(error: &PollError) -> String {
    match error {
        PollError::RateLimited(_) => "rate limited".to_string(),
        PollError::AuthRequired => "authentication required".to_string(),
        PollError::Config(t) | PollError::Transient(t) | PollError::Fatal(t) => t.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::HashMap;

    /// Replays a body (or error) per URL and records every request.
    struct Replay {
        responses: HashMap<String, Result<HttpResponse, HttpError>>,
        seen: RefCell<Vec<HttpRequest>>,
    }

    impl Replay {
        fn new() -> Self {
            Replay { responses: HashMap::new(), seen: RefCell::new(Vec::new()) }
        }
        fn body(mut self, url: &str, body: &str) -> Self {
            self.responses.insert(
                url.to_string(),
                Ok(HttpResponse { status: 200, headers: vec![], body: body.as_bytes().to_vec() }),
            );
            self
        }
        fn status(mut self, url: &str, status: u16) -> Self {
            self.responses
                .insert(url.to_string(), Ok(HttpResponse { status, headers: vec![], body: vec![] }));
            self
        }
        fn error(mut self, url: &str, error: HttpError) -> Self {
            self.responses.insert(url.to_string(), Err(error));
            self
        }
    }

    impl Http for Replay {
        fn get(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError> {
            self.seen.borrow_mut().push(request.clone());
            self.responses.get(&request.url).cloned().unwrap_or(Err(HttpError::NotConfigured))
        }
    }

    fn settings(feeds: &[&str]) -> Settings {
        let list: Vec<String> = feeds.iter().map(|f| format!("\"{f}\"")).collect();
        Settings::parse(&format!("feeds = [{}]", list.join(","))).unwrap()
    }

    fn rss(items: &[(&str, &str, &str)]) -> String {
        let mut doc = String::from("<rss version=\"2.0\"><channel><title>T</title>");
        for (guid, date, title) in items {
            doc.push_str(&format!(
                "<item><guid>{guid}</guid><link>https://x.example/{guid}</link><title>{title}</title>\
                 <pubDate>{date}</pubDate><description>Body of {guid}</description></item>"
            ));
        }
        doc.push_str("</channel></rss>");
        doc
    }

    const D1: &str = "Mon, 09 Jun 2025 10:00:00 +0000";
    const D2: &str = "Tue, 10 Jun 2025 10:00:00 +0000";
    const D3: &str = "Wed, 11 Jun 2025 10:00:00 +0000";
    const FEED: &str = "https://x.example/feed";

    #[test]
    fn first_poll_returns_everything_oldest_first() {
        let http = Replay::new().body(FEED, &rss(&[("c", D3, "C"), ("a", D1, "A"), ("b", D2, "B")]));
        let out = poll(&http, &settings(&[FEED]), None).unwrap();
        let ids: Vec<&str> = out.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["a", "b", "c"]);
        let first = &out.items[0];
        assert_eq!(first.platform, "rss");
        assert_eq!(first.url, "https://x.example/a");
        assert_eq!(first.text, "Body of a");
        assert_eq!(first.published, Some(1_749_463_200_000));
        assert!(out.next_cursor.is_some());
    }

    #[test]
    fn second_poll_returns_only_new_entries() {
        let s = settings(&[FEED]);
        let first = poll(&Replay::new().body(FEED, &rss(&[("a", D1, "A"), ("b", D2, "B")])), &s, None).unwrap();
        let http = Replay::new().body(FEED, &rss(&[("a", D1, "A"), ("b", D2, "B"), ("c", D3, "C")]));
        let second = poll(&http, &s, first.next_cursor.as_deref()).unwrap();
        let ids: Vec<&str> = second.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["c"]);
        // Nothing new: empty, and the cursor still decodes.
        let third = poll(&http, &s, second.next_cursor.as_deref()).unwrap();
        assert!(third.items.is_empty());
    }

    #[test]
    fn entries_sharing_the_newest_timestamp_are_neither_repeated_nor_lost() {
        let s = settings(&[FEED]);
        let first = poll(&Replay::new().body(FEED, &rss(&[("a", D2, "A")])), &s, None).unwrap();
        let http = Replay::new().body(FEED, &rss(&[("a", D2, "A"), ("b", D2, "B")]));
        let second = poll(&http, &s, first.next_cursor.as_deref()).unwrap();
        let ids: Vec<&str> = second.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["b"]);
    }

    #[test]
    fn items_per_feed_cap_keeps_the_newest() {
        let s = Settings::parse(&format!("feeds = [\"{FEED}\"]\nmax_items_per_feed = 2")).unwrap();
        let http = Replay::new().body(FEED, &rss(&[("a", D1, "A"), ("b", D2, "B"), ("c", D3, "C")]));
        let out = poll(&http, &s, None).unwrap();
        let ids: Vec<&str> = out.items.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, ["b", "c"]);
    }

    #[test]
    fn one_failing_feed_does_not_stop_the_others() {
        let bad = "https://bad.example/feed";
        let http = Replay::new().status(bad, 500).body(FEED, &rss(&[("a", D1, "A")]));
        let out = poll(&http, &settings(&[bad, FEED]), None).unwrap();
        assert_eq!(out.items.len(), 1);
        assert!(out.warnings.iter().any(|w| w.contains("feed #1 failed")), "{:?}", out.warnings);
        // The warning names the feed by position, never its URL.
        assert!(out.warnings.iter().all(|w| !w.contains("bad.example")));
    }

    #[test]
    fn all_feeds_failing_returns_the_error() {
        let http = Replay::new().status(FEED, 404);
        assert!(matches!(poll(&http, &settings(&[FEED]), None), Err(PollError::Fatal(_))));
        let http = Replay::new().status(FEED, 503);
        assert!(matches!(poll(&http, &settings(&[FEED]), None), Err(PollError::Transient(_))));
        let http = Replay::new().status(FEED, 429);
        assert!(matches!(poll(&http, &settings(&[FEED]), None), Err(PollError::RateLimited(None))));
        let http = Replay::new().error(FEED, HttpError::Denied("x".into()));
        assert!(matches!(poll(&http, &settings(&[FEED]), None), Err(PollError::Config(_))));
    }

    #[test]
    fn an_html_page_instead_of_a_feed_is_a_fatal_error() {
        let http = Replay::new().body(FEED, "<!DOCTYPE html><html><body>nope</body></html>");
        assert!(matches!(poll(&http, &settings(&[FEED]), None), Err(PollError::Fatal(_))));
    }

    #[test]
    fn hostile_content_is_neutralised() {
        let doc = "<rss><channel><item>\
            <guid>g1</guid><link>javascript:alert(1)</link>\
            <title>&lt;script&gt;x&lt;/script&gt;Hi\u{1b}[31m</title>\
            <description>&lt;p&gt;ok&lt;/p&gt;&lt;script&gt;steal()&lt;/script&gt;</description>\
            </item></channel></rss>";
        let out = poll(&Replay::new().body(FEED, doc), &settings(&[FEED]), None).unwrap();
        let item = &out.items[0];
        // A non-http link falls back to the feed URL.
        assert_eq!(item.url, FEED);
        assert_eq!(item.text, "ok");
        assert!(!item.title.as_deref().unwrap().contains('\u{1b}'));
    }

    #[test]
    fn long_text_is_truncated() {
        let long = "word ".repeat(500);
        let doc = format!("<rss><channel><item><guid>g</guid><description>{long}</description></item></channel></rss>");
        let s = Settings::parse(&format!("feeds = [\"{FEED}\"]\nmax_text_chars = 300")).unwrap();
        let out = poll(&Replay::new().body(FEED, &doc), &s, None).unwrap();
        assert_eq!(out.items[0].text.chars().count(), 300);
        assert!(out.items[0].text.ends_with('…'));
    }

    #[test]
    fn unreadable_cursor_is_a_warned_first_poll() {
        let http = Replay::new().body(FEED, &rss(&[("a", D1, "A")]));
        let out = poll(&http, &settings(&[FEED]), Some(b"garbage")).unwrap();
        assert_eq!(out.items.len(), 1);
        assert!(out.warnings.iter().any(|w| w.contains("unreadable")));
    }

    #[test]
    fn request_carries_accept_and_user_agent() {
        let http = Replay::new().body(FEED, &rss(&[]));
        poll(&http, &settings(&[FEED]), None).unwrap();
        let seen = http.seen.borrow();
        assert_eq!(seen[0].url, FEED);
        assert!(header(&seen[0].headers, "accept").unwrap().contains("rss"));
        assert!(header(&seen[0].headers, "user-agent").is_some());
    }
}
