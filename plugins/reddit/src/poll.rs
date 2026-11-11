//! Poll orchestration: a pure function of settings, cursor, token and the
//! responses the `Http` seam returns. Any feed error aborts the whole poll with
//! no items and no new cursor, so a failure can only cause a re-fetch and never
//! a silent gap. Nothing here logs or echoes a url, header, token or body.

use crate::api::{self, Item};
use crate::cursor::{self, Cursor, Mark};
use crate::http::{Http, HttpError, header};
use crate::settings::Settings;
use std::collections::HashSet;

/// What a successful poll returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollOutcome {
    /// Oldest first within each feed.
    pub items: Vec<Item>,
    pub next_cursor: Option<Vec<u8>>,
    /// Fixed text only, never post content.
    pub warnings: Vec<String>,
}

/// Mirror of the WIT `poll-error`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PollError {
    RateLimited(Option<u32>),
    AuthRequired,
    Config(String),
    Transient(String),
    Fatal(String),
}

fn map_http_error(error: HttpError) -> PollError {
    match error {
        HttpError::RateLimited(seconds) => PollError::RateLimited(seconds),
        HttpError::Denied(_) => PollError::Config(
            "Reddit request denied by host policy: check the plugin's allowlist for oauth.reddit.com"
                .to_string(),
        ),
        HttpError::NotConfigured => PollError::Config(
            "no fetcher is assigned to this source: set `fetcher` in the plugin's TOML".to_string(),
        ),
        HttpError::Timeout => PollError::Transient("Reddit request timed out".to_string()),
        HttpError::Transport(_) => PollError::Transient("Reddit request failed".to_string()),
        HttpError::InvalidRequest(_) => {
            PollError::Fatal("Reddit request was rejected as invalid".to_string())
        }
    }
}

/// A response header parsed as a float (Reddit can send `59.0`). An absent,
/// non-numeric or non-finite value is `None`.
fn parse_header_f64(headers: &[(String, String)], name: &str) -> Option<f64> {
    header(headers, name)?
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|value| value.is_finite())
}

/// Whole seconds for a reset header: rounded up, negatives clamp to 0, huge
/// values saturate at `u32::MAX`.
fn reset_seconds(value: f64) -> u32 {
    if value <= 0.0 {
        0
    } else if value >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        value.ceil() as u32
    }
}

/// Classify a response before any parsing (D-13, D-04). `None` means a 2xx
/// answer that is not rate limited, so parsing may continue. The texts hold
/// fixed phrases, the feed key and numbers only: never a body, a header value
/// or a url.
fn classify_response(
    status: u16,
    headers: &[(String, String)],
    feed_key: &str,
) -> Option<PollError> {
    // D-13: reactive and stateless. The host backoff does the waiting.
    let remaining_exhausted = parse_header_f64(headers, "x-ratelimit-remaining")
        .is_some_and(|remaining| remaining < 1.0);
    if status == 429 || remaining_exhausted {
        let reset = parse_header_f64(headers, "x-ratelimit-reset").map(reset_seconds);
        return Some(PollError::RateLimited(reset));
    }
    match status {
        200..=299 => None,
        401 => Some(PollError::AuthRequired),
        404 => Some(PollError::Config(format!(
            "Reddit reports no such listing for {feed_key} (HTTP 404); check the subreddits setting"
        ))),
        403 => Some(PollError::Transient(format!(
            "Reddit refused access to {feed_key} (HTTP 403); the subreddit may be private or banned, or the login lacks the history scope"
        ))),
        other => Some(PollError::Transient(format!(
            "Reddit HTTP {other} for {feed_key}"
        ))),
    }
}

/// Run one poll.
///
/// Documented limits (for the plugin README, plan 07-11):
/// - a feed with more new posts than one page loses the oldest overflow
///   (deferred paging);
/// - a post that arrives in the same second as the mark's post, after it, is
///   not emitted (the filter is strictly newer by whole seconds);
/// - `hot` only emits posts newer than its mark, so it is not a ranked re-poll.
pub fn poll(
    http: &dyn Http,
    settings: &Settings,
    cursor: Option<&[u8]>,
    token: Option<&str>,
) -> Result<PollOutcome, PollError> {
    // D-04: a missing token is auth-required before any request is made.
    let token = match token {
        Some(token) if !token.is_empty() => token,
        _ => return Err(PollError::AuthRequired),
    };

    let mut warnings = Vec::new();
    let (old, undecodable) = cursor::decode(cursor);
    if undecodable {
        warnings.push("cursor could not be decoded; treating this as a first poll".to_string());
    }

    // Start from the stored marks so a feed with nothing new carries its mark
    // forward unchanged; `retain_feeds` below drops the stale ones.
    let feeds = settings.feeds();
    let mut next: Cursor = old.clone();
    let mut items: Vec<Item> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    // Warning counters: only numbers ever reach a warning, never post content.
    let mut skipped = 0usize;

    for feed in &feeds {
        let mark = old.mark(&feed.key);
        // D-10: the first poll of a feed is one page; later polls return only
        // newer items, so they take the largest page.
        let limit = if mark.is_some() {
            settings.later_page_limit()
        } else {
            settings.first_page_limit()
        };
        let request = api::listing_request(
            &feed.subreddit,
            feed.listing,
            limit,
            token,
            settings.user_agent(),
        );
        let response = http.get(&request).map_err(map_http_error)?;
        if let Some(error) = classify_response(response.status, &response.headers, &feed.key) {
            return Err(error);
        }
        // The feed's listing decides the sticky rule (D-12). Settings only
        // produce `new` and `hot`, so the fallback is never taken.
        let listing = api::Listing::from_name(feed.listing).unwrap_or(api::Listing::New);
        let page = api::parse_listing(&response.body, listing).map_err(|_| {
            PollError::Transient("Reddit answer is not a readable listing".to_string())
        })?;
        skipped += page.skipped;
        let posts = page.posts;

        // Strictly newer than the mark by whole seconds (D-12): `>=` would
        // re-emit a same-second sibling on every poll.
        let mut fresh: Vec<(i64, api::Post)> = posts
            .into_iter()
            .filter_map(|post| api::created_seconds(&post).map(|t| (t, post)))
            .filter(|(t, _)| mark.is_none_or(|m| *t > m.t))
            .collect();
        // Oldest first; equal timestamps are ordered by fullname so the order
        // is stable between polls.
        fresh.sort_by(|(_, a), (_, b)| {
            a.created_utc
                .total_cmp(&b.created_utc)
                .then_with(|| a.name.cmp(&b.name))
        });

        // The new mark is the greatest post of this feed's page, including a
        // post another feed already emitted. Nothing new keeps the old mark.
        if let Some((t, post)) = fresh.last() {
            next.set_mark(
                feed.key.clone(),
                Mark {
                    t: *t,
                    n: post.name.clone(),
                },
            );
        }
        for (_, post) in &fresh {
            if let Some(item) = api::post_to_item(post) {
                if seen.insert(item.id.clone()) {
                    items.push(item);
                }
            }
        }
    }
    let keys: Vec<String> = feeds.iter().map(|feed| feed.key.clone()).collect();
    next.retain_feeds(&keys);

    if skipped > 0 {
        warnings.push(format!(
            "{skipped} listing children were not usable posts and were skipped"
        ));
    }
    // `raw` is `None` only when the post object was over the size cap.
    let raw_dropped = items.iter().filter(|item| item.raw.is_none()).count();
    if raw_dropped > 0 {
        warnings.push(format!(
            "{raw_dropped} items had a raw object over {} bytes; raw was dropped",
            api::RAW_MAX_BYTES
        ));
    }

    let next_cursor = cursor::encode(&next).map_err(PollError::Fatal)?;
    Ok(PollOutcome {
        items,
        next_cursor: Some(next_cursor),
        warnings,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn reset_of(value: &str) -> Option<u32> {
        parse_header_f64(&headers(&[("x-ratelimit-reset", value)]), "x-ratelimit-reset")
            .map(reset_seconds)
    }

    #[test]
    fn rate_limit_headers_parse_as_floats() {
        assert_eq!(reset_of("59.0"), Some(59));
        assert_eq!(reset_of("37"), Some(37));
        assert_eq!(reset_of("37.2"), Some(38));
        assert_eq!(reset_of("-5"), Some(0));
        assert_eq!(reset_of("abc"), None);
        assert_eq!(reset_of("1e12"), Some(u32::MAX));
        assert_eq!(reset_of("NaN"), None);
        assert_eq!(reset_of("inf"), None);
        assert_eq!(parse_header_f64(&headers(&[]), "x-ratelimit-reset"), None);
        assert_eq!(
            parse_header_f64(&headers(&[("x-ratelimit-remaining", "0.99")]), "x-ratelimit-remaining"),
            Some(0.99)
        );
    }
}
