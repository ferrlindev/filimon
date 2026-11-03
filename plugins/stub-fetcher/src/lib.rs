// ─────────────────────────────────────────────────────────────────────────────
// lib.rs  –  stub-fetcher (filimon:plugin `fetcher` world)
//
// Minimal fetcher plugin, built from ../../wit alone:
//
//   fetch  Log the request URL and return a canned 200 JSON response.
//
// It never calls `outbound-http` and holds no state between calls. Doubles as
// a Phase 3 host test fixture.
// ─────────────────────────────────────────────────────────────────────────────

#[allow(warnings)]
mod bindings;

use bindings::exports::filimon::plugin::fetch::Guest;
use bindings::filimon::plugin::log::{Level, log};
use bindings::filimon::plugin::types::{FetchError, Request, Response};

// ── Component ────────────────────────────────────────────────────────────────

struct Component;

impl Guest for Component {
    fn fetch(request: Request) -> Result<Response, FetchError> {
        log(Level::Debug, &format!("stub fetch: {}", request.url));

        Ok(Response {
            status: 200,
            headers: vec![
                ("content-type".to_string(), "application/json".to_string()),
                ("x-stub-url".to_string(), request.url),
            ],
            body: br#"{"stub":true}"#.to_vec(),
        })
    }
}

bindings::export!(Component with_types_in bindings);
