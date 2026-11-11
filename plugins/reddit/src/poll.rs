//! Poll orchestration: a pure function of settings, cursor, token and the
//! responses the `Http` seam returns. Any feed error aborts the whole poll with
//! no items and no new cursor, so a failure can only cause a re-fetch and never
//! a silent gap. Nothing here logs or echoes a url, header, token or body.

use crate::api::{self, Item};
use crate::cursor::{self, Cursor, Mark};
use crate::http::{Http, HttpError};
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

/// Run one poll.
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

    let mut next = Cursor::default();
    let mut items: Vec<Item> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    for feed in settings.feeds() {
        let mark = old.0.get(&feed.key);
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
        if response.status != 200 {
            return Err(PollError::Transient(format!(
                "Reddit HTTP {}",
                response.status
            )));
        }
        let posts = api::parse_listing(&response.body).map_err(PollError::Transient)?;

        // Strictly newer than the mark by whole seconds (D-12): `>=` would
        // re-emit a same-second sibling on every poll.
        let mut fresh: Vec<(i64, api::Post)> = posts
            .into_iter()
            .filter_map(|post| api::created_seconds(&post).map(|t| (t, post)))
            .filter(|(t, _)| mark.is_none_or(|m| *t > m.t))
            .collect();
        fresh.sort_by(|(ta, a), (tb, b)| ta.cmp(tb).then_with(|| a.name.cmp(&b.name)));

        match fresh.last() {
            Some((t, post)) => {
                next.0.insert(
                    feed.key.clone(),
                    Mark {
                        t: *t,
                        n: post.name.clone(),
                    },
                );
            }
            None => {
                if let Some(mark) = mark {
                    next.0.insert(feed.key.clone(), mark.clone());
                }
            }
        }
        for (_, post) in &fresh {
            if let Some(item) = api::post_to_item(post) {
                if seen.insert(item.id.clone()) {
                    items.push(item);
                }
            }
        }
    }

    let next_cursor = cursor::encode(&next).map_err(PollError::Fatal)?;
    Ok(PollOutcome {
        items,
        next_cursor: Some(next_cursor),
        warnings,
    })
}
