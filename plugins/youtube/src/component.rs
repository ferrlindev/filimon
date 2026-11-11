//! wasm32-only glue: the `Guest` impl and the adapter that implements the core
//! `Http` trait over the imported `fetch`. Nothing here logs a request url,
//! header, API key, token or body.

use crate::api;
use crate::bindings::filimon::plugin::fetch::fetch;
use crate::bindings::filimon::plugin::log::{Level, log};
use crate::bindings::filimon::plugin::types as wit;
use crate::bindings::Guest;
use crate::http::{Http, HttpError, HttpRequest, HttpResponse};
use crate::poll::{self, PollError};
use crate::settings::Settings;
use std::cell::RefCell;

/// The secret alias that carries the API key (D-02).
const API_KEY_ALIAS: &str = "api_key";

/// Settings and the API key, kept between `init` and `poll`.
struct State {
    settings: Settings,
    api_key: Option<String>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

/// The core `Http` seam over the imported `fetch`.
struct FetchHttp;

impl Http for FetchHttp {
    fn get(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError> {
        let request = wit::Request {
            method: wit::Method::Get,
            url: request.url.clone(),
            headers: request.headers.clone(),
            body: None,
        };
        match fetch(&request) {
            Ok(response) => Ok(HttpResponse {
                status: response.status,
                headers: response.headers,
                body: response.body,
            }),
            Err(error) => Err(match error {
                wit::FetchError::Denied(text) => HttpError::Denied(text),
                wit::FetchError::Timeout => HttpError::Timeout,
                wit::FetchError::Transport(text) => HttpError::Transport(text),
                wit::FetchError::RateLimited(seconds) => HttpError::RateLimited(seconds),
                wit::FetchError::NotConfigured => HttpError::NotConfigured,
                wit::FetchError::InvalidRequest(text) => HttpError::InvalidRequest(text),
            }),
        }
    }
}

fn to_wit_error(error: PollError) -> wit::PollError {
    match error {
        PollError::RateLimited(seconds) => wit::PollError::RateLimited(seconds),
        PollError::AuthRequired => wit::PollError::AuthRequired,
        PollError::Config(text) => wit::PollError::Config(text),
        PollError::Transient(text) => wit::PollError::Transient(text),
        PollError::Fatal(text) => wit::PollError::Fatal(text),
    }
}

fn to_wit_item(item: api::Item) -> wit::Item {
    wit::Item {
        id: item.id,
        url: item.url,
        title: item.title,
        text: item.text,
        author: item.author,
        published: item.published,
        platform: item.platform,
        raw: item.raw,
    }
}

struct Component;

impl Guest for Component {
    fn init(settings: String, secrets: Vec<(String, String)>) -> Result<(), wit::PollError> {
        let parsed = Settings::parse(&settings).map_err(wit::PollError::Config)?;
        // Unknown aliases are ignored; the key itself is never logged.
        let api_key = secrets
            .into_iter()
            .find(|(alias, _)| alias == API_KEY_ALIAS)
            .map(|(_, value)| value);
        STATE.with(|slot| {
            *slot.borrow_mut() = Some(State {
                settings: parsed,
                api_key,
            });
        });
        Ok(())
    }

    fn poll(
        cursor: Option<Vec<u8>>,
        access_token: Option<String>,
    ) -> Result<wit::PollResponse, wit::PollError> {
        STATE.with(|slot| {
            let slot = slot.borrow();
            let Some(state) = slot.as_ref() else {
                return Err(wit::PollError::Fatal("poll called before init".to_string()));
            };
            let outcome = poll::poll(
                &FetchHttp,
                &state.settings,
                state.api_key.as_deref(),
                cursor.as_deref(),
                access_token.as_deref(),
            )
            .map_err(to_wit_error)?;
            for warning in &outcome.warnings {
                log(Level::Warn, warning);
            }
            Ok(wit::PollResponse {
                items: outcome.items.into_iter().map(to_wit_item).collect(),
                next_cursor: outcome.next_cursor,
            })
        })
    }
}

crate::bindings::export!(Component with_types_in crate::bindings);
