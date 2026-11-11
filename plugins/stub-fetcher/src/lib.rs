// ─────────────────────────────────────────────────────────────────────────────
// lib.rs  –  stub-fetcher (filimon:plugin `fetcher` world)
//
// Minimal fetcher plugin, built from ../../wit alone:
//
//   fetch  Log the request URL and return a canned 200 JSON response, unless
//          the request carries an `x-stub-mode` header (name compared without
//          regard to case):
//            spin             never return (a fetcher that outlives its deadline)
//            forward:<url>    call `outbound-http.send` with a GET for <url>
//                             and return its result unchanged (a fetcher that
//                             tries to reach somewhere the source may not)
//          Any other value answers the canned 200 like no header at all.
//
// It holds no state between calls. Doubles as a host test fixture: the modes
// exist for host tests only and are never listed in a sample registry.
// ─────────────────────────────────────────────────────────────────────────────

#[allow(warnings)]
mod bindings;

use bindings::exports::filimon::plugin::fetch::Guest;
use bindings::filimon::plugin::log::{Level, log};
use bindings::filimon::plugin::outbound_http::send;
use bindings::filimon::plugin::types::{FetchError, Method, Request, Response};

/// The header that selects a test behaviour.
const MODE_HEADER: &str = "x-stub-mode";

// ── Component ────────────────────────────────────────────────────────────────

struct Component;

impl Guest for Component {
    fn fetch(request: Request) -> Result<Response, FetchError> {
        log(Level::Debug, &format!("stub fetch: {}", request.url));

        let mode = request
            .headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(MODE_HEADER))
            .map(|(_, value)| value.as_str());
        match mode {
            Some("spin") => loop {
                std::hint::spin_loop();
            },
            Some(mode) => {
                if let Some(url) = mode.strip_prefix("forward:") {
                    return send(&Request {
                        method: Method::Get,
                        url: url.to_string(),
                        headers: vec![],
                        body: None,
                    });
                }
            }
            None => {}
        }

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
