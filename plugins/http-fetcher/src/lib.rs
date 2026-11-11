// ─────────────────────────────────────────────────────────────────────────────
// lib.rs  –  http-fetcher (filimon:plugin `fetcher` world)
//
// Default HTTP fetcher plugin, built from ../../wit alone:
//
//   fetch  Forward the request to the host's `outbound-http.send` and return
//          the answer unchanged.
//
// It holds no state between calls, adds no header, follows no redirect and
// never logs a url, header or body: its only log line is a fixed text. TLS
// validation, the allowlist, timeouts, redirects and size limits are all the
// host's, so a fetcher cannot weaken them.
// ─────────────────────────────────────────────────────────────────────────────

#[allow(warnings)]
mod bindings;

use bindings::exports::filimon::plugin::fetch::Guest;
use bindings::filimon::plugin::log::{Level, log};
use bindings::filimon::plugin::outbound_http::send;
use bindings::filimon::plugin::types::{FetchError, Request, Response};

// ── Component ────────────────────────────────────────────────────────────────

struct Component;

impl Guest for Component {
    fn fetch(request: Request) -> Result<Response, FetchError> {
        log(Level::Debug, "http-fetcher: forwarding request");

        send(&request)
    }
}

bindings::export!(Component with_types_in bindings);
