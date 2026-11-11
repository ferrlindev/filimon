//! The HTTP seam (D-17). The core never mentions a WIT type: it talks to this
//! trait, the wasm component implements it over the imported `fetch`, and the
//! tests implement it with a scripted recorder.

/// An outbound GET. The host enforces the allowlist and the timeout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
}

/// An HTTP response. Header names arrive lower-cased from the host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// Why a fetch failed. Mirrors `fetch-error` in `wit/types.wit`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HttpError {
    Denied(String),
    Timeout,
    Transport(String),
    RateLimited(Option<u32>),
    NotConfigured,
    InvalidRequest(String),
}

/// Performs one GET.
pub trait Http {
    fn get(&self, request: &HttpRequest) -> Result<HttpResponse, HttpError>;
}

/// Case-insensitive header lookup.
pub fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_lookup_ignores_case() {
        let headers = vec![("X-RateLimit-Remaining".to_string(), "59.0".to_string())];
        assert_eq!(header(&headers, "x-ratelimit-remaining"), Some("59.0"));
        assert_eq!(header(&headers, "x-ratelimit-reset"), None);
    }
}
