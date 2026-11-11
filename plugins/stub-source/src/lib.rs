// ─────────────────────────────────────────────────────────────────────────────
// lib.rs  –  stub-source (filimon:plugin `source` world)
//
// Minimal but exercising source plugin, built from ../../wit alone:
//
//   init  Parse the raw [settings] TOML, log the greeting and secret aliases,
//         and call the imported `fetch` when `fetch_url` is set. The optional
//         `fetch_headers` table (name to value) is sent with that request, so a
//         host test can steer a test fetcher through a header.
//   poll  Return one hardcoded item and advance a one-byte cursor.
//
// The optional `record_inputs` setting (default off) is a proof fixture for the
// host's OAuth promise: `init` keeps the raw settings and the secrets it was
// given, and `poll` returns them, with the cursor and the access token, in the
// item's `raw`. A host test reads that back to show exactly what a plugin
// receives. With the setting off the item is exactly as before.
//
// Doubles as a Phase 3 host test fixture. Built for wasm32-unknown-unknown,
// so it has no filesystem, environment, clock, or stdout access.
// ─────────────────────────────────────────────────────────────────────────────

#[allow(warnings)]
mod bindings;

use bindings::Guest;
use bindings::filimon::plugin::fetch::fetch;
use bindings::filimon::plugin::log::{Level, log};
use bindings::filimon::plugin::types::{Item, Method, PollError, PollResponse, Request};
use serde::Deserialize;
use std::cell::RefCell;
use std::collections::BTreeMap;

thread_local! {
    /// What `init` received, kept only when `record_inputs` is on.
    static RECORDED_INIT: RefCell<Option<String>> = const { RefCell::new(None) };
}

// ── Settings ─────────────────────────────────────────────────────────────────

/// The stub's `[settings]` table, passed raw to `init`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    /// Text logged at info level during `init`.
    greeting: String,
    /// When set, `init` calls the imported `fetch` with a GET to this URL.
    fetch_url: Option<String>,
    /// Headers sent with the `fetch_url` request, in key order.
    fetch_headers: Option<BTreeMap<String, String>>,
    /// When true, `poll` returns everything the guest received in the item's
    /// `raw`. Off by default; a test fixture for the host's token handling.
    record_inputs: Option<bool>,
}

// ── Component ────────────────────────────────────────────────────────────────

struct Component;

impl Guest for Component {
    fn init(settings: String, secrets: Vec<(String, String)>) -> Result<(), PollError> {
        let raw_settings = settings;
        let settings: Settings =
            toml::from_str(&raw_settings).map_err(|e| PollError::Config(e.to_string()))?;

        let recording = settings.record_inputs == Some(true);
        RECORDED_INIT.with(|slot| {
            *slot.borrow_mut() =
                recording.then(|| format!("settings={raw_settings:?} secrets={secrets:?}"));
        });

        // Log alias names only, never secret values.
        let aliases: Vec<&str> = secrets.iter().map(|(alias, _)| alias.as_str()).collect();
        log(
            Level::Info,
            &format!(
                "init: {} ({} secrets: [{}])",
                settings.greeting,
                secrets.len(),
                aliases.join(", ")
            ),
        );

        // Settings-gated call: the component imports `fetch` only because
        // this code references it.
        if let Some(url) = settings.fetch_url {
            let request = Request {
                method: Method::Get,
                url,
                headers: settings
                    .fetch_headers
                    .unwrap_or_default()
                    .into_iter()
                    .collect(),
                body: None,
            };
            match fetch(&request) {
                Ok(response) => log(
                    Level::Debug,
                    &format!("init fetch status {}", response.status),
                ),
                Err(error) => log(Level::Warn, &format!("init fetch failed: {error:?}")),
            }
        }

        Ok(())
    }

    fn poll(
        cursor: Option<Vec<u8>>,
        access_token: Option<String>,
    ) -> Result<PollResponse, PollError> {
        let n = cursor
            .as_ref()
            .and_then(|c| c.first().copied())
            .unwrap_or(0);
        let recorded = RECORDED_INIT.with(|slot| slot.borrow().clone());
        let raw = match recorded {
            Some(recorded) => {
                format!("{recorded} cursor={cursor:?} access_token={access_token:?}")
            }
            None => format!("{{\"n\":{n}}}"),
        };

        let item = Item {
            id: format!("stub-{n}"),
            url: format!("https://example.invalid/stub/{n}"),
            title: Some("stub item".to_string()),
            text: "hello from stub-source".to_string(),
            author: Some("stub".to_string()),
            // 2026-01-01T00:00:00Z in Unix epoch milliseconds.
            published: Some(1_767_225_600_000),
            platform: "stub".to_string(),
            raw: Some(raw),
        };

        Ok(PollResponse {
            items: vec![item],
            next_cursor: Some(vec![n.wrapping_add(1)]),
        })
    }
}

bindings::export!(Component with_types_in bindings);
