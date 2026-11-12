// ─────────────────────────────────────────────────────────────────────────────
// lib.rs  –  replay-fetcher (filimon:plugin `fetcher` world)
//
// Replay fetcher: serves frozen fixtures as if they were live data, so a
// source plugin can run end to end inside the host with no network and no keys.
//
//   fetch  Walk `fixtures::TABLE` in order and answer the first entry whose
//          needle is a substring of the request URL: that entry's status, a
//          JSON content type and its body bytes, unmodified. A URL no entry
//          matches gets a fixed 404 with the body `{"replay":"no fixture"}`.
//
// It is generic glue: the only file an author edits is `fixtures.rs`. It never
// imports the host's outbound HTTP interface, so it cannot leave the sandbox.
// It keeps no state, because every call runs in a fresh instance. It logs
// fixed texts only and never passes any part of a request to the log: a source
// may send an API key in a header.
//
// It serves frozen data as if it were live, so it is never registered for a
// real source (same rule as the stub-fetcher).
// ─────────────────────────────────────────────────────────────────────────────

#[allow(warnings)]
mod bindings;
mod fixtures;

use bindings::exports::filimon::plugin::fetch::Guest;
use bindings::filimon::plugin::log::{Level, log};
use bindings::filimon::plugin::types::{FetchError, Request, Response};

/// The body of the answer to a URL no fixture matches.
const NO_FIXTURE_BODY: &[u8] = br#"{"replay":"no fixture"}"#;

// ── Component ────────────────────────────────────────────────────────────────

struct Component;

impl Guest for Component {
    fn fetch(request: Request) -> Result<Response, FetchError> {
        for (needle, status, body) in fixtures::TABLE {
            if request.url.contains(needle) {
                log(Level::Debug, "served a fixture");
                return Ok(Response {
                    status: *status,
                    headers: vec![("content-type".to_string(), "application/json".to_string())],
                    body: body.to_vec(),
                });
            }
        }

        log(Level::Debug, "no fixture matched");
        Ok(Response {
            status: 404,
            headers: vec![],
            body: NO_FIXTURE_BODY.to_vec(),
        })
    }
}

bindings::export!(Component with_types_in bindings);
