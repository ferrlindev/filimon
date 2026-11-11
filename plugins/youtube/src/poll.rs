//! Poll orchestration: a pure function of settings, credentials, cursor and the
//! responses the `Http` seam returns. Any playlist error aborts the whole poll
//! with no items and no new cursor, so a failure can only cause a re-fetch and
//! never a silent gap. Nothing here logs or echoes a url, header, key, token or
//! body.

use crate::api::{self, Auth, Item};
use crate::cursor::{self, Cursor};
use crate::http::{Http, HttpError};
use crate::settings::{FIRST_POLL_LIMIT, Settings};

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

/// Run one poll. A token means `Auth::Bearer`, else an API key means
/// `Auth::ApiKey`, else the poll is auth-required before any request.
pub fn poll(
    http: &dyn Http,
    settings: &Settings,
    api_key: Option<&str>,
    cursor: Option<&[u8]>,
    token: Option<&str>,
) -> Result<PollOutcome, PollError> {
    let token = token.filter(|token| !token.is_empty());
    let api_key = api_key.filter(|key| !key.is_empty());
    let auth = match (token, api_key) {
        (Some(token), _) => Auth::Bearer(token),
        (None, Some(key)) => Auth::ApiKey(key),
        (None, None) => return Err(PollError::AuthRequired),
    };

    let (old, _undecodable) = cursor::decode(cursor);
    let mut next = Cursor::default();
    let mut items: Vec<Item> = Vec::new();

    for playlist in settings.playlists() {
        let mark = old.mark(&playlist.playlist_id);
        let request = api::playlist_request(&playlist.playlist_id, FIRST_POLL_LIMIT, auth);
        let response = http.get(&request).map_err(map_http_error)?;
        if response.status != 200 {
            return Err(PollError::Transient(format!(
                "YouTube HTTP {}",
                response.status
            )));
        }
        let page = api::parse_playlist_page(&response.body)
            .map_err(|_| PollError::Transient("YouTube answer could not be parsed".to_string()))?;

        // Strictly newer than the mark by publication instant: `>=` would
        // re-emit a same-instant sibling on every poll.
        let mut fresh: Vec<(i64, String, Item)> = page
            .entries
            .iter()
            .filter_map(|entry| {
                let time = entry.published_ms?;
                let id = entry.video_id.clone()?;
                let item = api::video_to_item(entry)?;
                Some((time, id, item))
            })
            .filter(|(time, _, _)| mark.is_none_or(|(mark_time, _)| *time > *mark_time))
            .collect();
        fresh.sort_by(|(ta, ia, _), (tb, ib, _)| ta.cmp(tb).then_with(|| ia.cmp(ib)));

        match fresh.last() {
            Some((time, id, _)) => next.set(&playlist.playlist_id, *time, id),
            None => {
                if let Some((time, id)) = mark {
                    next.set(&playlist.playlist_id, *time, id);
                }
            }
        }
        items.extend(fresh.into_iter().map(|(_, _, item)| item));
    }

    let next_cursor = cursor::encode(&next).map_err(PollError::Fatal)?;
    Ok(PollOutcome {
        items,
        next_cursor: Some(next_cursor),
        warnings: Vec::new(),
    })
}
