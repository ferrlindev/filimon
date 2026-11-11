//! Poll orchestration: a pure function of settings, credentials, cursor and the
//! responses the `Http` seam returns. Any playlist error aborts the whole poll
//! with no items and no new cursor, so a failure can only cause a re-fetch and
//! never a silent gap. Nothing here logs or echoes a url, header, key, token or
//! body.

use crate::api::{self, Auth, Entry, Item};
use crate::cursor;
use crate::http::{Http, HttpError};
use crate::settings::{
    FIRST_POLL_LIMIT, MAX_SUBSCRIPTION_PAGES, Playlist, SUBSCRIPTIONS_PAGE_SIZE, Settings,
    uploads_playlist,
};
use std::collections::HashSet;

/// What a successful poll returns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PollOutcome {
    /// Oldest first within each playlist.
    pub items: Vec<Item>,
    pub next_cursor: Option<Vec<u8>>,
    /// Fixed text only, never video content.
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
            "YouTube request denied by host policy: check the plugin's allowlist for www.googleapis.com"
                .to_string(),
        ),
        HttpError::NotConfigured => PollError::Config(
            "no fetcher is assigned to this source: set `fetcher` in the plugin's TOML".to_string(),
        ),
        HttpError::Timeout => PollError::Transient("YouTube request timed out".to_string()),
        HttpError::Transport(_) => PollError::Transient("YouTube request failed".to_string()),
        HttpError::InvalidRequest(_) => {
            PollError::Fatal("YouTube request was rejected as invalid".to_string())
        }
    }
}

/// The subscribed channels one poll follows.
struct Subscribed {
    channel_ids: Vec<String>,
    /// Entries without a usable channel id.
    skipped: usize,
    /// The page cap was hit while more pages were on offer.
    page_capped: bool,
}

/// List the user's subscriptions, 50 per page, until `limit` channels are
/// collected, no page token remains, or `MAX_SUBSCRIPTION_PAGES` requests were
/// made (a hostile endless token cannot loop the plugin). One unit per call.
fn collect_subscriptions(
    http: &dyn Http,
    token: &str,
    limit: u32,
) -> Result<Subscribed, PollError> {
    let limit = limit as usize;
    let mut subscribed = Subscribed {
        channel_ids: Vec::new(),
        skipped: 0,
        page_capped: false,
    };
    let mut page_token: Option<String> = None;
    let mut pages: u32 = 0;
    while subscribed.channel_ids.len() < limit {
        if pages >= MAX_SUBSCRIPTION_PAGES {
            subscribed.page_capped = true;
            break;
        }
        let wanted = (limit - subscribed.channel_ids.len()).min(SUBSCRIPTIONS_PAGE_SIZE as usize);
        let request = api::subscriptions_request(page_token.as_deref(), wanted as u32, token);
        let response = http.get(&request).map_err(map_http_error)?;
        pages += 1;
        // A failed list call has no channel to name and aborts the whole poll.
        if let Some(error) = api::classify_youtube(response.status, &response.body, None) {
            return Err(error);
        }
        let page = api::parse_subscriptions_page(&response.body).map_err(|_| {
            PollError::Transient("YouTube answer is not a readable subscriptions page".to_string())
        })?;
        subscribed.skipped += page.skipped;
        for id in page.channel_ids {
            if subscribed.channel_ids.len() < limit && !subscribed.channel_ids.contains(&id) {
                subscribed.channel_ids.push(id);
            }
        }
        match page.next_page_token {
            Some(next) => page_token = Some(next),
            None => break,
        }
    }
    Ok(subscribed)
}

/// Run one poll. Without subscriptions a token means `Auth::Bearer`, else an
/// API key means `Auth::ApiKey`, else the poll is auth-required before any
/// request. With `subscriptions = true` only a token will do: an API key can
/// never read `mine=true`, so a missing token is auth-required even when a key
/// is present.
pub fn poll(
    http: &dyn Http,
    settings: &Settings,
    api_key: Option<&str>,
    cursor: Option<&[u8]>,
    token: Option<&str>,
) -> Result<PollOutcome, PollError> {
    let token = token.filter(|token| !token.is_empty());
    let api_key = api_key.filter(|key| !key.is_empty());
    let subscription_limit = settings.subscriptions();
    let auth = match (token, api_key) {
        (Some(token), _) => Auth::Bearer(token),
        (None, _) if subscription_limit.is_some() => return Err(PollError::AuthRequired),
        (None, Some(key)) => Auth::ApiKey(key),
        (None, None) => return Err(PollError::AuthRequired),
    };

    let mut warnings = Vec::new();
    let (old, undecodable) = cursor::decode(cursor);
    if undecodable {
        warnings.push("cursor could not be decoded; treating this as a first poll".to_string());
    }

    // Explicit channels first, then the subscribed ones in subscription order;
    // a channel in both lists is polled once, as an explicit one.
    let mut playlists: Vec<(Playlist, bool)> = settings
        .playlists()
        .into_iter()
        .map(|playlist| (playlist, false))
        .collect();
    let mut subscription_skipped = 0;
    let mut page_capped = false;
    if let (Some(limit), Auth::Bearer(token)) = (subscription_limit, auth) {
        let subscribed = collect_subscriptions(http, token, limit)?;
        subscription_skipped = subscribed.skipped;
        page_capped = subscribed.page_capped;
        for channel_id in subscribed.channel_ids {
            if !playlists.iter().any(|(p, _)| p.channel_id == channel_id) {
                let playlist_id = uploads_playlist(&channel_id);
                playlists.push((
                    Playlist {
                        channel_id,
                        playlist_id,
                    },
                    true,
                ));
            }
        }
    }
    if page_capped {
        warnings.push("subscription list stopped at the page limit; some subscriptions were not followed".to_string());
    }
    if subscription_skipped > 0 {
        warnings.push(format!(
            "skipped {subscription_skipped} subscription entries without a usable channel id"
        ));
    }

    let playlist_ids: Vec<&str> = playlists
        .iter()
        .map(|(p, _)| p.playlist_id.as_str())
        .collect();
    // Marks of playlists that are no longer in the list are dropped; the marks
    // of listed playlists carry forward unless a newer video replaces them. A
    // subscription that leaves the limit window and returns is a first poll.
    let mut next = old.clone();
    next.retain_feeds(&playlist_ids);

    // The worst case (explicit channels plus the limit) sizes the later page.
    let later_limit = Settings::later_page_limit(settings.max_playlists());
    let mut items: Vec<Item> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut skipped: usize = 0;
    let mut subscribed_missing: usize = 0;

    for (playlist, subscription_only) in &playlists {
        let mark = old.mark(&playlist.playlist_id);
        // D-10: the first poll of a playlist is one small page; later polls
        // return only newer videos, so they take the largest page that keeps a
        // whole poll under the host's item cap.
        let limit = if mark.is_some() {
            later_limit
        } else {
            FIRST_POLL_LIMIT
        };
        let request = api::playlist_request(&playlist.playlist_id, limit, auth);
        let response = http.get(&request).map_err(map_http_error)?;
        // A terminated or hidden subscribed channel is not the operator's to
        // fix in config: skip it, count it, name nothing. An explicit channel
        // keeps the named config error below.
        if *subscription_only && response.status == 404 {
            subscribed_missing += 1;
            continue;
        }
        // Every non-2xx response is a typed error, never a quiet empty page.
        // No retry or sleep: the host's backoff waits and the next poll starts
        // clean (a quota error carries no cursor, so nothing needs resetting).
        if let Some(error) =
            api::classify_youtube(response.status, &response.body, Some(&playlist.channel_id))
        {
            return Err(error);
        }
        let page = api::parse_playlist_page(&response.body).map_err(|_| {
            PollError::Transient("YouTube answer is not a readable playlist page".to_string())
        })?;
        skipped += page.skipped;

        // Strictly newer than the mark by publication instant: `>=` would
        // re-emit a same-instant sibling on every poll. An entry without a
        // usable time cannot be compared, so it is skipped and counted.
        let mut newer: Vec<(i64, &Entry)> = Vec::new();
        for entry in &page.entries {
            match entry.published_ms {
                None => skipped += 1,
                Some(time) if mark.is_none_or(|(mark_time, _)| time > *mark_time) => {
                    newer.push((time, entry));
                }
                Some(_) => {}
            }
        }

        // The mark covers every newer entry, placeholders included, so a
        // private or deleted video does not keep the window open (Pitfall 8).
        let newest = newer
            .iter()
            .map(|(time, entry)| (*time, entry.video_id.clone().unwrap_or_default()))
            .max();
        if let Some((time, id)) = newest {
            next.set(&playlist.playlist_id, time, &id);
        }

        skipped += newer
            .iter()
            .filter(|(_, entry)| entry.is_placeholder())
            .count();
        let mut fresh: Vec<(i64, Item)> = newer
            .iter()
            .filter_map(|(time, entry)| Some((*time, api::video_to_item(entry)?)))
            .collect();
        fresh.sort_by(|(ta, a), (tb, b)| ta.cmp(tb).then_with(|| a.id.cmp(&b.id)));
        for (_, item) in fresh {
            // The same video can appear in two playlists.
            if seen.insert(item.id.clone()) {
                items.push(item);
            }
        }
    }

    if subscribed_missing > 0 {
        warnings.push(format!(
            "skipped {subscribed_missing} subscribed channels whose uploads playlist was not found (HTTP 404)"
        ));
    }
    if skipped > 0 {
        warnings.push(format!(
            "skipped {skipped} private, deleted or unreadable playlist entries"
        ));
    }

    let next_cursor = cursor::encode(&next).map_err(PollError::Fatal)?;
    Ok(PollOutcome {
        items,
        next_cursor: Some(next_cursor),
        warnings,
    })
}
