// ─────────────────────────────────────────────────────────────────────────────
// lib.rs  –  template-source (filimon:plugin `source` world)
//
// A source plugin template. Copy this crate, rename it, edit the places marked
// `EDIT:`, and build it with `cargo component build --release`.
//
// Rename checklist:
//   1. Cargo.toml: `name`, and `[package.metadata.component] package`.
//   2. Cargo.toml: `[package.metadata.component.target] path` once the `wit/`
//      directory sits beside Cargo.toml (`path = "wit"`).
//   3. template.toml: copy it, name it after your source, and register it in
//      filimon.toml.
//
// The build target (wasm32-unknown-unknown) comes from `.cargo/config.toml`
// and `rust-toolchain.toml`: copy both files too. The component has no
// filesystem, environment, clock or stdout access; it talks to the host
// through the imported `log` and `fetch` only.
//
//   init  Parse the raw [settings] TOML and keep it for `poll`.
//   poll  Return one item and advance a one-byte cursor. With `fetch_url` set
//         the item reports a GET through the source's assigned fetcher.
// ─────────────────────────────────────────────────────────────────────────────

#[allow(warnings)]
mod bindings;

use bindings::Guest;
use bindings::filimon::plugin::fetch::fetch;
use bindings::filimon::plugin::log::{Level, log};
use bindings::filimon::plugin::types::{Item, Method, PollError, PollResponse, Request};
use serde::Deserialize;
use std::cell::RefCell;

// ── Settings ─────────────────────────────────────────────────────────────────

/// The `[settings]` table of the plugin TOML, passed raw to `init`.
// EDIT: replace these fields with your own settings. `deny_unknown_fields`
// turns a typo in the TOML into a `Config` error instead of a silent default.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    /// Text used as the item text when no `fetch_url` is set.
    greeting: String,
    /// When set, `poll` GETs this url through the source's fetcher.
    fetch_url: Option<String>,
}

thread_local! {
    // `poll` receives no settings, so `init` keeps them here. The host keeps
    // one instance across polls and builds a new one (calling `init` again)
    // after a failure, so this is rebuilt whenever it has to be.
    static SETTINGS: RefCell<Option<Settings>> = const { RefCell::new(None) };
}

// ── Component ────────────────────────────────────────────────────────────────

struct Component;

impl Guest for Component {
    fn init(settings: String, secrets: Vec<(String, String)>) -> Result<(), PollError> {
        // A bad `[settings]` table is a `Config` error: the host reports it to
        // the operator and does not retry it.
        let settings: Settings =
            toml::from_str(&settings).map_err(|e| PollError::Config(e.to_string()))?;

        // Log secret alias names only, never their values.
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

        SETTINGS.with(|slot| *slot.borrow_mut() = Some(settings));
        Ok(())
    }

    fn poll(
        cursor: Option<Vec<u8>>,
        _access_token: Option<String>,
    ) -> Result<PollResponse, PollError> {
        // The cursor is opaque to the host: it stores it and hands it back
        // unchanged. This template keeps a single counter byte in it.
        let n = cursor
            .as_ref()
            .and_then(|c| c.first().copied())
            .unwrap_or(0);

        let (greeting, fetch_url) = SETTINGS.with(|slot| {
            slot.borrow()
                .as_ref()
                .map(|s| (s.greeting.clone(), s.fetch_url.clone()))
        })
        .ok_or_else(|| PollError::Config("poll called before init".to_string()))?;

        // EDIT: build your items here. Every `Item` field is shown below.
        let text = match fetch_url {
            // Without `fetch_url` the template needs no fetcher at all.
            None => greeting,
            // With it, the imported `fetch` goes through the fetcher the
            // operator assigned to this source in filimon.toml, and only to
            // hosts in the plugin TOML's `allowlist`.
            Some(url) => {
                let request = Request {
                    method: Method::Get,
                    url,
                    headers: vec![],
                    body: None,
                };
                match fetch(&request) {
                    Ok(response) => {
                        let excerpt = &response.body[..response.body.len().min(200)];
                        format!(
                            "status {}: {}",
                            response.status,
                            String::from_utf8_lossy(excerpt)
                        )
                    }
                    // A temporary failure: the host retries with backoff.
                    Err(error) => {
                        return Err(PollError::Transient(format!(
                            "fetch failed: {error:?}"
                        )));
                    }
                }
            }
        };

        let item = Item {
            // Platform-stable id: the host uses it to drop duplicates.
            id: format!("template-{n}"),
            url: format!("https://example.invalid/template/{n}"),
            title: Some("template item".to_string()),
            text,
            author: Some("template".to_string()),
            // Unix epoch milliseconds (2026-01-01T00:00:00Z). A
            // wasm32-unknown-unknown component has no clock, so use the time
            // the platform reports, or `None`.
            published: Some(1_767_225_600_000),
            // EDIT: a short platform name.
            platform: "template".to_string(),
            raw: Some(format!("{{\"n\":{n}}}")),
        };

        Ok(PollResponse {
            items: vec![item],
            next_cursor: Some(vec![n.wrapping_add(1)]),
        })
    }
}

bindings::export!(Component with_types_in bindings);
