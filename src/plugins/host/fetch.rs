// ─────────────────────────────────────────────────────────────────────────────
// plugins/host/fetch.rs - Routing a source's `fetch` call to its fetcher (FETCH-01)
//
// A `FetchRoute` is keyed by a consumer label (a source alias today, the news
// crawler later) and holds the fetcher component the registry assigns to that
// consumer. Every `fetch` call is first judged by the consumer's `FetchPolicy`
// (an exact-host allowlist), and only then runs in a fresh fetcher instance:
// the `fetcher` world has no state between calls, and one component may serve
// several consumers, so no instance is ever re-used.
//
// The whole call runs under one wall-clock deadline taken from the consumer's
// `timeout_secs`. Dropping the call future on timeout is the cancellation.
//
// Guest text is untrusted: a denial names the host and the consumer, never the
// path or query string, and every guest-derived string that reaches `tracing`
// or an error passes through `clean_guest_text` first.
// ─────────────────────────────────────────────────────────────────────────────

use std::collections::BTreeSet;
use std::error::Error as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;

use super::engine::source_world::filimon::plugin::types;
use super::engine::{FetcherPre, HostState, clean_guest_text, new_guest_store};
use crate::plugins::config::HostConfig;

/// Longest text kept from a fetcher failure, in characters.
const FAILURE_TEXT_CLIP: usize = 512;

/// Largest response body the host hands to a plugin, in bytes (8 MiB).
pub const MAX_RESPONSE_BYTES: usize = 8 * 1024 * 1024;

/// Most redirects one `send` follows before it fails.
pub const MAX_REDIRECTS: usize = 5;

/// Longest text kept from a transport failure, in characters.
const TRANSPORT_TEXT_CLIP: usize = 256;

/// Largest request body a plugin may send, in bytes (1 MiB; WD-14). Requests
/// are held whole in memory, so they are bounded like responses are.
pub const MAX_REQUEST_BODY_BYTES: usize = 1024 * 1024;

/// Most request headers a plugin may send (WD-14).
const MAX_REQUEST_HEADERS: usize = 100;

/// Most header text a plugin may send: names plus values, in bytes (64 KiB;
/// WD-14).
const MAX_REQUEST_HEADER_BYTES: usize = 64 * 1024;

/// Longest url a plugin may send, in bytes (8 KiB; WD-14).
const MAX_URL_BYTES: usize = 8 * 1024;

/// Request headers a plugin may not set (WD-13). The HTTP client owns message
/// framing and the connection, and a `host` header could point an allowed
/// address at another virtual host and so defeat the allowlist. All lowercase.
const FORBIDDEN_REQUEST_HEADERS: [&str; 8] = [
    "host",
    "content-length",
    "transfer-encoding",
    "connection",
    "proxy-connection",
    "upgrade",
    "te",
    "trailer",
];

// ── Policy ────────────────────────────────────────────────────────────────────

/// The hosts a consumer may fetch from.
#[derive(Debug, Clone, Default)]
pub struct Allowlist {
    hosts: BTreeSet<String>,
    rejected: Vec<String>,
}

impl Allowlist {
    /// Builds an allowlist from the configured host names.
    ///
    /// Each entry is trimmed, loses one trailing dot and is normalized with
    /// `url::Host::parse`, so case and internationalized names compare in
    /// their ASCII form. A blank entry is ignored. An entry that is not a bare
    /// host (a scheme, port, path, space or `*` wildcard) can never match and
    /// is kept in `rejected()`.
    pub fn new<I>(entries: I) -> Self
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let mut hosts = BTreeSet::new();
        let mut rejected = Vec::new();
        for entry in entries {
            let entry = entry.as_ref().trim();
            if entry.is_empty() {
                continue;
            }
            let bare = entry.strip_suffix('.').unwrap_or(entry);
            let parsed = if bare.is_empty() || bare.contains('*') {
                None
            } else {
                url::Host::parse(bare).ok()
            };
            match parsed {
                Some(host) => {
                    hosts.insert(normalize(&host.to_string()));
                }
                None => rejected.push(entry.to_string()),
            }
        }
        Self { hosts, rejected }
    }

    /// The non-blank configured entries that are not bare host names and so
    /// never match.
    #[must_use]
    pub fn rejected(&self) -> &[String] {
        &self.rejected
    }

    /// Whether `host` is allowed.
    #[must_use]
    pub fn permits(&self, host: &url::Host<&str>) -> bool {
        self.hosts.contains(&normalize(&host.to_string()))
    }
}

/// A host's comparison form: one trailing dot removed. The URL parser has
/// already lowercased domains and put IP literals in canonical form.
fn normalize(host: &str) -> String {
    host.strip_suffix('.').unwrap_or(host).to_string()
}

/// What the host enforces on one consumer's fetches.
#[derive(Debug, Clone)]
pub struct FetchPolicy {
    consumer: String,
    allowlist: Allowlist,
    timeout: Duration,
}

impl FetchPolicy {
    #[must_use]
    pub fn new(consumer: &str, allowlist: Allowlist, timeout: Duration) -> Self {
        for entry in allowlist.rejected() {
            // Operator config text, but still cleaned before it is logged.
            tracing::warn!(
                plugin = %clean_guest_text(consumer, 128),
                entry = %clean_guest_text(entry, 128),
                "allowlist entry is not a bare host name and will never match"
            );
        }
        Self {
            consumer: consumer.to_string(),
            allowlist,
            timeout,
        }
    }

    /// The policy of the source `name`: its own allowlist and timeout.
    #[must_use]
    pub fn for_source(name: &str, host: &HostConfig) -> Self {
        Self::new(name, Allowlist::new(&host.allowlist), host.timeout)
    }

    #[must_use]
    pub fn consumer(&self) -> &str {
        &self.consumer
    }

    #[must_use]
    pub fn timeout(&self) -> Duration {
        self.timeout
    }

    /// Judges `request` before any fetcher exists.
    ///
    /// # Errors
    /// `InvalidRequest` for a url that cannot be used and `Denied` for a host
    /// outside the allowlist.
    pub fn check(&self, request: &types::Request) -> Result<url::Url, types::FetchError> {
        let text = request.url.trim();
        if text.is_empty() {
            return Err(types::FetchError::InvalidRequest(
                "url is empty".to_string(),
            ));
        }
        // The url is never echoed back: it is guest data and may carry secrets.
        let url = url::Url::parse(text)
            .map_err(|_| types::FetchError::InvalidRequest("url cannot be parsed".to_string()))?;
        self.check_url(&url)?;
        Ok(url)
    }

    /// Judges one parsed url: scheme, host and allowlist. A redirect target
    /// is judged by exactly the same rules as the first url.
    ///
    /// # Errors
    /// `InvalidRequest` for a scheme or host that cannot be used and `Denied`
    /// for a host outside the allowlist.
    pub fn check_url(&self, url: &url::Url) -> Result<(), types::FetchError> {
        if !matches!(url.scheme(), "http" | "https") {
            return Err(types::FetchError::InvalidRequest(
                "only http and https urls are allowed".to_string(),
            ));
        }
        let Some(host) = url.host() else {
            return Err(types::FetchError::InvalidRequest(
                "url has no host".to_string(),
            ));
        };
        if !self.allowlist.permits(&host) {
            let host = clean_guest_text(&host.to_string(), 128);
            let consumer = clean_guest_text(&self.consumer, 128);
            tracing::warn!(plugin = %consumer, host = %host, "fetch denied: host not in allowlist");
            return Err(types::FetchError::Denied(format!(
                "host `{host}` is not in the allowlist of `{consumer}`"
            )));
        }
        Ok(())
    }
}

// ── Egress ────────────────────────────────────────────────────────────────────

/// The host's one HTTP client: certificate validation is the client's default
/// (on) and its own redirect following is off. It is built on first use and
/// shared by every fetcher call, so `HostEngine::new` stays cheap.
///
/// Certificate roots are the ones bundled with the client's rustls
/// configuration; the operating system's trust store is not read (WD-11).
#[derive(Default)]
pub struct Egress {
    client: OnceLock<Result<reqwest::Client, String>>,
    #[cfg(test)]
    extra_roots: Vec<reqwest::Certificate>,
}

impl Egress {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// An egress that also trusts the certificate in `pem`.
    ///
    /// # Panics
    /// When `pem` is not a certificate.
    #[cfg(test)]
    #[allow(dead_code, reason = "used by the tls_ tests in plan 05-05")]
    pub(crate) fn with_extra_root(pem: &[u8]) -> Self {
        let root = reqwest::Certificate::from_pem(pem).expect("test root certificate");
        Self {
            client: OnceLock::new(),
            extra_roots: vec![root],
        }
    }

    fn client(&self) -> Result<&reqwest::Client, types::FetchError> {
        let built = self.client.get_or_init(|| {
            #[allow(unused_mut, reason = "extra roots exist only under cfg(test)")]
            let mut builder =
                reqwest::Client::builder().redirect(reqwest::redirect::Policy::none());
            #[cfg(test)]
            for root in &self.extra_roots {
                builder = builder.add_root_certificate(root.clone());
            }
            builder.build().map_err(describe_transport_error)
        });
        built.as_ref().map_err(|reason| {
            types::FetchError::Transport(format!("http client unavailable: {reason}"))
        })
    }

    /// Sends `request` and returns the response, under one `deadline`.
    ///
    /// # Errors
    /// The policy's error, `InvalidRequest` for a method or header that cannot
    /// be sent, `Timeout` when the deadline passes, and `Transport` for a
    /// connection, certificate or body failure (never carrying the url).
    pub async fn execute(
        &self,
        policy: &FetchPolicy,
        request: types::Request,
        deadline: tokio::time::Instant,
    ) -> Result<types::Response, types::FetchError> {
        if let Ok(result) = tokio::time::timeout_at(deadline, self.run(policy, request)).await {
            result
        } else {
            tracing::warn!(consumer = %policy.consumer(), "outbound request timed out");
            Err(types::FetchError::Timeout)
        }
    }

    async fn run(
        &self,
        policy: &FetchPolicy,
        request: types::Request,
    ) -> Result<types::Response, types::FetchError> {
        let mut url = policy.check(&request)?;
        let mut method = to_method(&request.method)?;
        let mut headers = request.headers;
        let mut body = request.body;
        // Reject a header that cannot be sent before any connection.
        to_header_map(&headers)?;
        let client = self.client()?;

        let mut hops = 0;
        let (mut response, status) = loop {
            let mut builder = client
                .request(method.clone(), url.clone())
                .headers(to_header_map(&headers)?);
            if let Some(bytes) = &body {
                builder = builder.body(bytes.clone());
            }
            let response = builder.send().await.map_err(map_error)?;
            let status = response.status().as_u16();

            let location = if matches!(status, 301 | 302 | 303 | 307 | 308) {
                response.headers().get(reqwest::header::LOCATION)
            } else {
                None
            };
            // Anything but a redirect with a Location is returned as is.
            let Some(location) = location else {
                break (response, status);
            };
            if hops >= MAX_REDIRECTS {
                return Err(types::FetchError::Transport(format!(
                    "too many redirects (limit {MAX_REDIRECTS})"
                )));
            }
            let target = location
                .to_str()
                .ok()
                .and_then(|text| url.join(text).ok())
                .ok_or_else(|| {
                    types::FetchError::Transport(
                        "redirect location is not a valid url".to_string(),
                    )
                })?;
            // The same rules as the first url; a refusal makes no connection.
            policy.check_url(&target)?;

            if status == 303
                || (matches!(status, 301 | 302) && method == reqwest::Method::POST)
            {
                if method != reqwest::Method::HEAD {
                    method = reqwest::Method::GET;
                }
                body = None;
                drop_headers(&mut headers, &["content-type", "content-length", "content-encoding"]);
            }
            if target.origin() != url.origin() {
                drop_headers(&mut headers, &["authorization", "cookie", "proxy-authorization"]);
            }
            tracing::debug!(
                consumer = %policy.consumer(),
                from = %url.host_str().unwrap_or_default(),
                to = %target.host_str().unwrap_or_default(),
                "outbound redirect followed"
            );
            url = target;
            hops += 1;
        };

        let headers = response
            .headers()
            .iter()
            .map(|(name, value)| {
                (
                    name.as_str().to_string(),
                    String::from_utf8_lossy(value.as_bytes()).into_owned(),
                )
            })
            .collect();

        let body = read_capped(&mut response).await?;

        tracing::debug!(
            consumer = %policy.consumer(),
            method = %method,
            host = %url.host_str().unwrap_or_default(),
            status,
            body_len = body.len(),
            "outbound request"
        );
        Ok(types::Response {
            status,
            headers,
            body,
        })
    }
}

/// Removes every header named in `names`, in any letter case.
fn drop_headers(headers: &mut Vec<(String, String)>, names: &[&str]) {
    headers.retain(|(name, _)| !names.iter().any(|dropped| name.eq_ignore_ascii_case(dropped)));
}

/// Reads the body chunk by chunk, never holding more than the cap: a declared
/// length over it fails at once, and an undeclared one fails as soon as the
/// running total would pass it.
async fn read_capped(response: &mut reqwest::Response) -> Result<Vec<u8>, types::FetchError> {
    let too_big = || {
        types::FetchError::Transport(format!("response body exceeds {MAX_RESPONSE_BYTES} bytes"))
    };
    if response
        .content_length()
        .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
    {
        return Err(too_big());
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(map_error)? {
        if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return Err(too_big());
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

fn to_method(method: &types::Method) -> Result<reqwest::Method, types::FetchError> {
    Ok(match method {
        types::Method::Get => reqwest::Method::GET,
        types::Method::Head => reqwest::Method::HEAD,
        types::Method::Post => reqwest::Method::POST,
        types::Method::Put => reqwest::Method::PUT,
        types::Method::Patch => reqwest::Method::PATCH,
        types::Method::Delete => reqwest::Method::DELETE,
        types::Method::Options => reqwest::Method::OPTIONS,
        types::Method::Other(text) => {
            let method = reqwest::Method::from_bytes(text.as_bytes()).map_err(|_| {
                types::FetchError::InvalidRequest("method is not valid".to_string())
            })?;
            // CONNECT opens a tunnel and TRACE echoes request headers; neither
            // is something a plugin may ask the host to do, in any letter case.
            if ["CONNECT", "TRACE"]
                .iter()
                .any(|refused| method.as_str().eq_ignore_ascii_case(refused))
            {
                return Err(types::FetchError::InvalidRequest(
                    "method is not allowed".to_string(),
                ));
            }
            method
        }
    })
}

fn to_header_map(
    headers: &[(String, String)],
) -> Result<reqwest::header::HeaderMap, types::FetchError> {
    let invalid = || types::FetchError::InvalidRequest("header is not valid".to_string());
    let mut map = reqwest::header::HeaderMap::new();
    for (name, value) in headers {
        let name =
            reqwest::header::HeaderName::from_bytes(name.as_bytes()).map_err(|_| invalid())?;
        let value = reqwest::header::HeaderValue::from_str(value).map_err(|_| invalid())?;
        map.append(name, value);
    }
    Ok(map)
}

/// A client error as a fetch error: a timeout stays a timeout, anything else
/// is a transport failure whose text never carries the url.
fn map_error(error: reqwest::Error) -> types::FetchError {
    if error.is_timeout() {
        types::FetchError::Timeout
    } else {
        types::FetchError::Transport(describe_transport_error(error))
    }
}

/// The error's own text joined with its `source()` chain, with the url
/// removed (reqwest's `Display` includes it otherwise) and cleaned.
fn describe_transport_error(error: reqwest::Error) -> String {
    let error = error.without_url();
    let mut text = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        text.push_str(": ");
        text.push_str(&cause.to_string());
        source = cause.source();
    }
    clean_guest_text(&text, TRANSPORT_TEXT_CLIP)
}

/// What a fetcher store holds to reach the network: the calling consumer's own
/// policy and deadline, and the shared client. This is the only way a fetcher
/// reaches the network, so a fetcher can never go where its consumer may not.
#[derive(Clone)]
pub struct EgressGrant {
    pub policy: Arc<FetchPolicy>,
    pub egress: Arc<Egress>,
    pub deadline: tokio::time::Instant,
}

impl EgressGrant {
    /// Sends one request under the grant's policy and deadline.
    ///
    /// # Errors
    /// See `Egress::execute`.
    pub async fn send(
        &self,
        request: types::Request,
    ) -> Result<types::Response, types::FetchError> {
        self.egress
            .execute(&self.policy, request, self.deadline)
            .await
    }
}

// ── Route ─────────────────────────────────────────────────────────────────────

/// Answers one consumer's `fetch` calls with its assigned fetcher component.
pub struct FetchRoute {
    fetcher: Arc<str>,
    pre: Arc<FetcherPre<HostState>>,
    engine: wasmtime::Engine,
    policy: Arc<FetchPolicy>,
    egress: Arc<Egress>,
    calls: AtomicU64,
}

impl FetchRoute {
    #[must_use]
    pub fn new(
        fetcher: &str,
        pre: Arc<FetcherPre<HostState>>,
        engine: wasmtime::Engine,
        policy: Arc<FetchPolicy>,
        egress: Arc<Egress>,
    ) -> Self {
        Self {
            fetcher: Arc::from(fetcher),
            pre,
            engine,
            policy,
            egress,
            calls: AtomicU64::new(0),
        }
    }

    /// How many fetcher instances this route has started.
    #[cfg_attr(
        not(test),
        allow(dead_code, reason = "tests count instantiations to prove per-call isolation")
    )]
    pub(crate) fn calls(&self) -> u64 {
        self.calls.load(Ordering::Relaxed)
    }

    /// Runs one request: policy first (a denial starts no fetcher), then a
    /// fresh fetcher instance, all under the consumer's single deadline.
    ///
    /// # Errors
    /// The policy's error, `Timeout` when the deadline passes, `Transport`
    /// when the fetcher cannot be started or traps, or the fetcher's own error.
    pub async fn fetch(
        &self,
        request: types::Request,
    ) -> Result<types::Response, types::FetchError> {
        let url = self.policy.check(&request)?;
        let deadline = tokio::time::Instant::now() + self.policy.timeout();
        tracing::debug!(
            consumer = %self.policy.consumer(),
            fetcher = %self.fetcher,
            host = %url.host_str().unwrap_or_default(),
            "fetch routed"
        );
        if let Ok(result) = tokio::time::timeout_at(deadline, self.call_fetcher(&request, deadline)).await {
            result
        } else {
            tracing::warn!(
                consumer = %self.policy.consumer(),
                fetcher = %self.fetcher,
                timeout_secs = self.policy.timeout().as_secs(),
                "fetch timed out"
            );
            Err(types::FetchError::Timeout)
        }
    }

    /// One call in one new instance. The fetcher holds no state, so nothing
    /// is kept afterwards, and a dropped (timed out) call leaves nothing behind.
    async fn call_fetcher(
        &self,
        request: &types::Request,
        deadline: tokio::time::Instant,
    ) -> Result<types::Response, types::FetchError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let grant = EgressGrant {
            policy: Arc::clone(&self.policy),
            egress: Arc::clone(&self.egress),
            deadline,
        };
        let state = HostState::for_fetcher(&self.fetcher, grant);
        let mut store = new_guest_store(&self.engine, state);
        let fetcher = match self.pre.instantiate_async(&mut store).await {
            Ok(fetcher) => fetcher,
            Err(error) => return Err(self.failure(&error)),
        };
        match fetcher
            .filimon_plugin_fetch()
            .call_fetch(&mut store, request)
            .await
        {
            Ok(result) => result,
            Err(error) => Err(self.failure(&error)),
        }
    }

    /// A fetcher that cannot start or traps is a transport failure and one
    /// warning, never a host failure.
    fn failure(&self, error: &wasmtime::Error) -> types::FetchError {
        let full = format!("{error:#}");
        let first = full.lines().next().unwrap_or_default();
        let text = clean_guest_text(
            &format!("fetcher `{}` failed: {first}", self.fetcher),
            FAILURE_TEXT_CLIP,
        );
        tracing::warn!(fetcher = %self.fetcher, "{text}");
        types::FetchError::Transport(text)
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    use crate::plugins::host::engine::HostEngine;
    use crate::plugins::host::instance::WasmtimeFactory;
    use crate::plugins::host::loader::{load_fetcher, load_source};
    use crate::plugins::host::once::poll_once;
    use std::sync::atomic::AtomicU16;

    use crate::plugins::host::testing::{
        self, LogCapture, Reply, SeenRequest, TestHttpServer, fixture_wasm,
    };

    fn fetcher_route(engine: &HostEngine, allow: &[&str]) -> Arc<FetchRoute> {
        let path = fixture_wasm("stub-fetcher", "stub_fetcher.wasm");
        let pre = load_fetcher("stub-fetcher", &path, engine).expect("fetcher loads");
        let policy = FetchPolicy::new(
            "stub",
            Allowlist::new(allow),
            Duration::from_secs(30),
        );
        Arc::new(FetchRoute::new(
            "stub-fetcher",
            Arc::new(pre),
            engine.engine().clone(),
            Arc::new(policy),
            engine.egress(),
        ))
    }

    async fn poll_stub_source(
        engine: &HostEngine,
        route: Option<Arc<FetchRoute>>,
        settings: &str,
    ) {
        let path = fixture_wasm("stub-source", "stub_source.wasm");
        let pre = load_source("stub", &path, engine).expect("source loads");
        let factory = WasmtimeFactory::new(engine.engine().clone(), pre).with_route(route);
        let mut spec = testing::spec("stub", 60, 60, 3600, 2.0);
        spec.settings_toml = settings.to_string();
        poll_once(&spec, &factory, None)
            .await
            .expect("the source polls");
    }

    fn request(url: &str) -> types::Request {
        types::Request {
            method: types::Method::Get,
            url: url.to_string(),
            headers: vec![],
            body: None,
        }
    }

    #[tokio::test]
    #[ignore = "needs plugin fixtures: see plugins/README.md"]
    async fn wasm_tracer_fetch_is_served_by_the_assigned_fetcher() {
        let engine = HostEngine::new().expect("engine");
        let route = fetcher_route(&engine, &["example.invalid"]);
        let settings =
            "greeting = \"hi\"\nfetch_url = \"http://example.invalid/tracer?x=1\"\n";
        let (logs, _guard) = LogCapture::start();

        poll_stub_source(&engine, Some(Arc::clone(&route)), settings).await;

        let text = logs.text();
        assert!(text.contains("init fetch status 200"), "{text}");
        assert!(text.contains("plugin=stub-fetcher"), "{text}");
        assert!(
            text.contains("stub fetch: http://example.invalid/tracer?x=1"),
            "{text}"
        );
        assert_eq!(route.calls(), 1);

        // No route: the source is told `not-configured` and no fetcher runs.
        poll_stub_source(&engine, None, settings).await;
        let text = logs.text();
        assert!(text.contains("init fetch failed: FetchError::NotConfigured"), "{text}");
        assert_eq!(text.matches("stub fetch:").count(), 1, "{text}");
    }

    // ── Egress Tests (loopback server, no wasm) ───────────────────────────────

    fn policy_for(hosts: &[&str], timeout: Duration) -> FetchPolicy {
        FetchPolicy::new("test", Allowlist::new(hosts), timeout)
    }

    fn get(url: &str) -> types::Request {
        request(url)
    }

    fn in_five_seconds() -> tokio::time::Instant {
        tokio::time::Instant::now() + Duration::from_secs(5)
    }

    #[tokio::test]
    async fn egress_get_returns_status_headers_and_body() {
        let server = TestHttpServer::start(|seen| match seen.target.as_str() {
            "/hello" => Reply::ok("hello").with_header("x-test", "1"),
            _ => Reply::ok("other"),
        })
        .await;
        let policy = policy_for(&["127.0.0.1"], Duration::from_secs(5));

        let response = Egress::new()
            .execute(&policy, get(&server.url("/hello")), in_five_seconds())
            .await
            .expect("the request succeeds");

        assert_eq!(response.status, 200);
        let header = response
            .headers
            .iter()
            .find(|(name, _)| name == "x-test")
            .map(|(_, value)| value.as_str());
        assert_eq!(header, Some("1"));
        assert_eq!(response.body, b"hello");
        let seen = server.seen();
        assert_eq!(seen.len(), 1);
        assert_eq!((seen[0].method.as_str(), seen[0].target.as_str()), ("GET", "/hello"));
    }

    #[tokio::test]
    async fn egress_rejects_a_response_body_over_the_cap() {
        let big = vec![b'x'; 9 * 1024 * 1024];
        let declared = {
            let big = big.clone();
            TestHttpServer::start(move |_| Reply::ok(big.clone())).await
        };
        let undeclared = TestHttpServer::start(move |_| Reply::ok(big.clone()).without_length()).await;
        let policy = policy_for(&["127.0.0.1"], Duration::from_secs(20));
        let deadline = tokio::time::Instant::now() + Duration::from_secs(20);
        let egress = Egress::new();

        for server in [&declared, &undeclared] {
            let result = egress.execute(&policy, get(&server.url("/big")), deadline).await;
            let Err(types::FetchError::Transport(text)) = result else {
                panic!("expected a transport error, got {result:?}");
            };
            assert!(text.contains("8388608"), "{text}");
        }
    }

    #[tokio::test]
    async fn egress_transport_errors_never_contain_the_url() {
        let closed = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
            listener.local_addr().expect("address").port()
        };
        let policy = policy_for(&["127.0.0.1"], Duration::from_secs(5));
        let url = format!("http://127.0.0.1:{closed}/secret/path?token=SECRET123");

        let result = Egress::new()
            .execute(&policy, get(&url), in_five_seconds())
            .await;

        let Err(types::FetchError::Transport(text)) = result else {
            panic!("expected a transport error, got {result:?}");
        };
        assert!(!text.is_empty());
        assert!(!text.contains("SECRET123"), "{text}");
        assert!(!text.contains("/secret/path"), "{text}");
    }

    #[tokio::test]
    async fn egress_denies_a_host_outside_the_allowlist_on_the_send_path() {
        let server = TestHttpServer::start(|_| Reply::ok("x")).await;
        let grant = EgressGrant {
            policy: Arc::new(policy_for(&["127.0.0.1"], Duration::from_secs(5))),
            egress: Arc::new(Egress::new()),
            deadline: in_five_seconds(),
        };

        // `localhost` reaches the same server, but is not on the allowlist.
        let result = grant.send(get(&server.localhost_url("/x"))).await;

        assert!(matches!(result, Err(types::FetchError::Denied(_))), "{result:?}");
        assert!(server.seen().is_empty(), "a denied send made a connection");
    }

    // ── Redirect, deadline and method tests ───────────────────────────────────

    /// A server whose handler also learns the server's own port, so a reply
    /// can point at `localhost:<port>`.
    async fn start_with_port(
        handler: impl Fn(&SeenRequest, u16) -> Reply + Send + Sync + 'static,
    ) -> TestHttpServer {
        let port = Arc::new(AtomicU16::new(0));
        let shared = Arc::clone(&port);
        let server =
            TestHttpServer::start(move |seen| handler(seen, shared.load(Ordering::SeqCst))).await;
        port.store(server.port(), Ordering::SeqCst);
        server
    }

    fn post(url: &str, headers: &[(&str, &str)]) -> types::Request {
        types::Request {
            method: types::Method::Post,
            url: url.to_string(),
            headers: headers
                .iter()
                .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
                .collect(),
            body: Some(b"payload".to_vec()),
        }
    }

    fn targets(server: &TestHttpServer) -> Vec<String> {
        server.seen().into_iter().map(|seen| seen.target).collect()
    }

    #[tokio::test]
    async fn egress_follows_a_redirect_to_an_allowed_host() {
        let server = TestHttpServer::start(|seen| match seen.target.as_str() {
            "/start" => Reply::redirect(302, "/end"),
            _ => Reply::ok("done"),
        })
        .await;
        let policy = policy_for(&["127.0.0.1"], Duration::from_secs(5));

        let response = Egress::new()
            .execute(&policy, get(&server.url("/start")), in_five_seconds())
            .await
            .expect("the redirect is followed");

        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"done");
        assert_eq!(targets(&server), ["/start", "/end"]);
    }

    #[tokio::test]
    async fn egress_denies_a_redirect_to_a_host_outside_the_allowlist() {
        let server = start_with_port(|seen, port| match seen.target.as_str() {
            "/r" => Reply::redirect(302, &format!("http://localhost:{port}/x")),
            _ => Reply::ok("reached"),
        })
        .await;
        let policy = policy_for(&["127.0.0.1"], Duration::from_secs(5));

        let result = Egress::new()
            .execute(&policy, get(&server.url("/r")), in_five_seconds())
            .await;

        let Err(types::FetchError::Denied(text)) = result else {
            panic!("expected a denial, got {result:?}");
        };
        assert!(text.contains("localhost"), "{text}");
        assert_eq!(targets(&server), ["/r"], "the second host was contacted");
    }

    #[tokio::test]
    async fn egress_stops_after_five_redirects_with_a_named_error() {
        let server = TestHttpServer::start(|_| Reply::redirect(302, "/loop")).await;
        let policy = policy_for(&["127.0.0.1"], Duration::from_secs(5));

        let result = Egress::new()
            .execute(&policy, get(&server.url("/loop")), in_five_seconds())
            .await;

        let Err(types::FetchError::Transport(text)) = result else {
            panic!("expected a transport error, got {result:?}");
        };
        assert!(text.contains("too many redirects"), "{text}");
        assert!(text.contains('5'), "{text}");
        assert_eq!(server.seen().len(), 6, "the first request plus five followed");
    }

    #[tokio::test]
    async fn egress_a_303_redirect_turns_post_into_get_without_a_body() {
        let server = TestHttpServer::start(|seen| match seen.target.as_str() {
            "/p" => Reply::redirect(303, "/g"),
            _ => Reply::ok("got"),
        })
        .await;
        let policy = policy_for(&["127.0.0.1"], Duration::from_secs(5));
        let request = post(&server.url("/p"), &[("content-type", "text/plain")]);

        Egress::new()
            .execute(&policy, request, in_five_seconds())
            .await
            .expect("the redirect is followed");

        let seen = server.seen();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0].method, "POST");
        assert_eq!(seen[0].body, b"payload");
        assert_eq!(seen[1].method, "GET");
        assert!(seen[1].body.is_empty());
        assert_eq!(seen[1].header("content-type"), None);
    }

    #[tokio::test]
    async fn egress_a_307_redirect_keeps_method_and_body() {
        let server = TestHttpServer::start(|seen| match seen.target.as_str() {
            "/p" => Reply::redirect(307, "/g"),
            _ => Reply::ok("got"),
        })
        .await;
        let policy = policy_for(&["127.0.0.1"], Duration::from_secs(5));
        let request = post(&server.url("/p"), &[("content-type", "text/plain")]);

        Egress::new()
            .execute(&policy, request, in_five_seconds())
            .await
            .expect("the redirect is followed");

        let seen = server.seen();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[1].method, "POST");
        assert_eq!(seen[1].body, b"payload");
        assert_eq!(seen[1].header("content-type"), Some("text/plain"));
    }

    const CREDENTIALS: [(&str, &str); 3] = [
        ("authorization", "Bearer SECRET"),
        ("cookie", "a=b"),
        ("x-keep", "yes"),
    ];

    fn credentialed(url: &str) -> types::Request {
        types::Request {
            headers: CREDENTIALS
                .iter()
                .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
                .collect(),
            ..get(url)
        }
    }

    #[tokio::test]
    async fn egress_drops_credentials_when_a_redirect_changes_host() {
        let server = start_with_port(|seen, port| match seen.target.as_str() {
            "/a" => Reply::redirect(302, &format!("http://localhost:{port}/b")),
            _ => Reply::ok("b"),
        })
        .await;
        let policy = policy_for(&["127.0.0.1", "localhost"], Duration::from_secs(5));

        Egress::new()
            .execute(&policy, credentialed(&server.url("/a")), in_five_seconds())
            .await
            .expect("the redirect is followed");

        let seen = server.seen();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[1].header("x-keep"), Some("yes"));
        assert_eq!(seen[1].header("authorization"), None);
        assert_eq!(seen[1].header("cookie"), None);
    }

    #[tokio::test]
    async fn egress_keeps_credentials_when_a_redirect_stays_on_the_same_host() {
        let server = TestHttpServer::start(|seen| match seen.target.as_str() {
            "/a" => Reply::redirect(302, "/b"),
            _ => Reply::ok("b"),
        })
        .await;
        let policy = policy_for(&["127.0.0.1"], Duration::from_secs(5));

        Egress::new()
            .execute(&policy, credentialed(&server.url("/a")), in_five_seconds())
            .await
            .expect("the redirect is followed");

        let seen = server.seen();
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[1].header("authorization"), Some("Bearer SECRET"));
        assert_eq!(seen[1].header("cookie"), Some("a=b"));
        assert_eq!(seen[1].header("x-keep"), Some("yes"));
    }

    #[tokio::test]
    async fn egress_returns_a_redirect_without_location_as_is() {
        let server = TestHttpServer::start(|_| Reply::status(302)).await;
        let policy = policy_for(&["127.0.0.1"], Duration::from_secs(5));

        let response = Egress::new()
            .execute(&policy, get(&server.url("/n")), in_five_seconds())
            .await
            .expect("a redirect without Location is an ordinary response");

        assert_eq!(response.status, 302);
        assert_eq!(server.seen().len(), 1);
    }

    #[tokio::test]
    async fn egress_one_deadline_covers_every_redirect_hop() {
        let hop = Duration::from_millis(250);
        let server = TestHttpServer::start(move |seen| match seen.target.as_str() {
            "/h1" => Reply::redirect(302, "/h2").delayed(hop),
            "/h2" => Reply::redirect(302, "/h3").delayed(hop),
            _ => Reply::ok("done").delayed(hop),
        })
        .await;
        let policy = policy_for(&["127.0.0.1"], Duration::from_millis(600));
        let started = std::time::Instant::now();

        let result = Egress::new()
            .execute(
                &policy,
                get(&server.url("/h1")),
                tokio::time::Instant::now() + policy.timeout(),
            )
            .await;

        assert!(matches!(result, Err(types::FetchError::Timeout)), "{result:?}");
        assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
    }

    #[tokio::test]
    async fn egress_deadline_expires_on_a_slow_server() {
        let server =
            TestHttpServer::start(|_| Reply::ok("late").delayed(Duration::from_secs(5))).await;
        let policy = policy_for(&["127.0.0.1"], Duration::from_millis(300));
        let started = std::time::Instant::now();

        let result = Egress::new()
            .execute(
                &policy,
                get(&server.url("/slow")),
                tokio::time::Instant::now() + policy.timeout(),
            )
            .await;

        assert!(matches!(result, Err(types::FetchError::Timeout)), "{result:?}");
        assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
    }

    #[tokio::test]
    async fn egress_rejects_the_methods_connect_and_trace() {
        let server = TestHttpServer::start(|_| Reply::ok("x")).await;
        let policy = policy_for(&["127.0.0.1"], Duration::from_secs(5));
        let egress = Egress::new();

        for method in ["CONNECT", "TRACE", "connect", "Trace"] {
            let request = types::Request {
                method: types::Method::Other(method.to_string()),
                ..get(&server.url("/m"))
            };
            let result = egress.execute(&policy, request, in_five_seconds()).await;
            assert!(
                matches!(result, Err(types::FetchError::InvalidRequest(_))),
                "{method}: {result:?}"
            );
        }
        assert!(server.seen().is_empty(), "a refused method made a connection");
    }

    #[tokio::test]
    #[ignore = "needs plugin fixtures: see plugins/README.md"]
    async fn wasm_tracer_default_fetcher_reaches_a_local_server() {
        let server = TestHttpServer::start(|seen| match seen.target.as_str() {
            "/hello" => Reply::ok("hello"),
            _ => Reply::ok("other"),
        })
        .await;
        let engine = HostEngine::new().expect("engine");
        let path = fixture_wasm("http-fetcher", "http_fetcher.wasm");
        let pre = load_fetcher("http", &path, &engine).expect("fetcher loads");
        let route = Arc::new(FetchRoute::new(
            "http",
            Arc::new(pre),
            engine.engine().clone(),
            Arc::new(policy_for(&["127.0.0.1"], Duration::from_secs(30))),
            engine.egress(),
        ));
        let settings = format!("greeting = \"hi\"\nfetch_url = \"{}\"\n", server.url("/hello"));
        let (logs, _guard) = LogCapture::start();

        poll_stub_source(&engine, Some(route), &settings).await;

        let text = logs.text();
        assert!(text.contains("init fetch status 200"), "{text}");
        let seen = server.seen();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!((seen[0].method.as_str(), seen[0].target.as_str()), ("GET", "/hello"));
    }

    #[tokio::test]
    #[ignore = "needs plugin fixtures: see plugins/README.md"]
    async fn wasm_denied_host_never_instantiates_the_fetcher() {
        let engine = HostEngine::new().expect("engine");
        let route = fetcher_route(&engine, &["example.invalid"]);
        let settings = "greeting = \"hi\"\nfetch_url = \"http://other.invalid/x\"\n";
        let (logs, _guard) = LogCapture::start();

        poll_stub_source(&engine, Some(Arc::clone(&route)), settings).await;

        let text = logs.text();
        assert!(text.contains("init fetch failed: FetchError::Denied("), "{text}");
        assert!(!text.contains("stub fetch:"), "{text}");
        assert_eq!(route.calls(), 0);
    }

    #[tokio::test]
    #[ignore = "needs plugin fixtures: see plugins/README.md"]
    async fn wasm_concurrent_fetches_never_share_a_fetcher_instance() {
        let engine = HostEngine::new().expect("engine");
        let route = fetcher_route(&engine, &["example.invalid"]);

        let urls: Vec<String> = (0..8)
            .map(|n| format!("http://example.invalid/c/{n}"))
            .collect();
        let calls = urls.iter().map(|url| route.fetch(request(url)));
        let results = futures_util::future::join_all(calls).await;

        for (url, result) in urls.iter().zip(results) {
            let response = result.expect("each fetch succeeds");
            assert_eq!(response.status, 200);
            let echoed = response
                .headers
                .iter()
                .find(|(name, _)| name == "x-stub-url")
                .map(|(_, value)| value.as_str());
            assert_eq!(echoed, Some(url.as_str()), "answer for another request");
        }
        assert_eq!(route.calls(), 8);
    }

    // ── Request validation tests (no wasm) ────────────────────────────────────

    fn with_headers(url: &str, headers: &[(&str, &str)]) -> types::Request {
        types::Request {
            headers: headers
                .iter()
                .map(|(name, value)| ((*name).to_string(), (*value).to_string()))
                .collect(),
            ..get(url)
        }
    }

    fn invalid_text(result: &Result<impl std::fmt::Debug, types::FetchError>) -> Option<&str> {
        match result {
            Err(types::FetchError::InvalidRequest(text)) => Some(text.as_str()),
            _ => None,
        }
    }

    #[tokio::test]
    async fn a_host_header_is_refused_before_any_connection() {
        let server = TestHttpServer::start(|_| Reply::ok("x")).await;
        let policy = policy_for(&["127.0.0.1"], Duration::from_secs(5));
        let egress = Egress::new();

        for name in ["host", "Host", "HOST"] {
            let request = with_headers(&server.url("/h"), &[(name, "other.example")]);
            let result = egress.execute(&policy, request, in_five_seconds()).await;
            assert!(invalid_text(&result).is_some(), "{name}: {result:?}");
        }
        assert!(server.seen().is_empty(), "a refused request made a connection");
    }

    #[test]
    fn framing_and_hop_by_hop_headers_are_refused() {
        let policy = policy("p", &["127.0.0.1"]);
        for name in [
            "Content-Length",
            "TRANSFER-ENCODING",
            "Connection",
            "proxy-CONNECTION",
            "Upgrade",
            "TE",
            "Trailer",
        ] {
            let result = policy.check(&with_headers("http://127.0.0.1/", &[(name, "x")]));
            assert!(invalid_text(&result).is_some(), "{name}: {result:?}");
        }
        let harmless = policy.check(&with_headers("http://127.0.0.1/", &[("x-custom", "1")]));
        assert!(harmless.is_ok(), "{harmless:?}");
    }

    #[tokio::test]
    async fn a_malformed_header_is_refused_without_echoing_it() {
        let server = TestHttpServer::start(|_| Reply::ok("x")).await;
        let policy = policy_for(&["127.0.0.1"], Duration::from_secs(5));
        let egress = Egress::new();

        for (name, value) in [("bad name", "ok"), ("x-ok", "ok\r\nx-injected: 1")] {
            let request = with_headers(&server.url("/m"), &[(name, value)]);
            let result = egress.execute(&policy, request, in_five_seconds()).await;
            let text = invalid_text(&result).unwrap_or_else(|| panic!("{result:?}"));
            assert!(!text.contains("injected"), "{text}");
            assert!(!text.contains("bad name"), "{text}");
        }
        assert!(server.seen().is_empty(), "a refused request made a connection");
    }

    #[test]
    fn too_many_or_too_large_headers_are_refused() {
        let policy = policy("p", &["127.0.0.1"]);

        let many: Vec<(String, String)> = (0..=MAX_REQUEST_HEADERS)
            .map(|n| (format!("x-h{n}"), "v".to_string()))
            .collect();
        let request = types::Request {
            headers: many,
            ..get("http://127.0.0.1/")
        };
        let result = policy.check(&request);
        assert!(invalid_text(&result).is_some(), "101 headers: {result:?}");

        // One header whose name and value total the limit plus one byte.
        let value = "v".repeat(MAX_REQUEST_HEADER_BYTES - "x-big".len() + 1);
        let result = policy.check(&with_headers("http://127.0.0.1/", &[("x-big", &value)]));
        assert!(invalid_text(&result).is_some(), "over the byte limit: {result:?}");
    }

    #[tokio::test]
    async fn an_oversized_request_body_is_refused() {
        let server = TestHttpServer::start(|_| Reply::ok("x")).await;
        let policy = policy_for(&["127.0.0.1"], Duration::from_secs(5));
        let request = types::Request {
            method: types::Method::Post,
            body: Some(vec![0_u8; MAX_REQUEST_BODY_BYTES + 1]),
            ..get(&server.url("/b"))
        };

        let result = Egress::new()
            .execute(&policy, request, in_five_seconds())
            .await;

        let text = invalid_text(&result).unwrap_or_else(|| panic!("{result:?}"));
        assert!(text.contains("1048576"), "{text}");
        assert!(server.seen().is_empty(), "a refused request made a connection");
    }

    /// `http://127.0.0.1/?q=aaa...` padded to exactly `length` bytes.
    fn url_of_length(length: usize) -> String {
        let base = "http://127.0.0.1/?q=";
        format!("{base}{}", "a".repeat(length - base.len()))
    }

    #[test]
    fn an_oversized_url_is_refused() {
        let policy = policy("p", &["127.0.0.1"]);

        let result = policy.check(&request(&url_of_length(MAX_URL_BYTES + 1)));

        assert!(invalid_text(&result).is_some(), "{result:?}");
    }

    #[test]
    fn a_request_at_the_limits_is_accepted() {
        let policy = policy("p", &["127.0.0.1"]);

        let body = types::Request {
            method: types::Method::Post,
            body: Some(vec![0_u8; MAX_REQUEST_BODY_BYTES]),
            ..get("http://127.0.0.1/")
        };
        assert!(policy.check(&body).is_ok(), "a 1 MiB body");

        let headers = types::Request {
            headers: (0..MAX_REQUEST_HEADERS)
                .map(|n| (format!("x-h{n}"), "v".to_string()))
                .collect(),
            ..get("http://127.0.0.1/")
        };
        assert!(policy.check(&headers).is_ok(), "100 headers");

        let value = "v".repeat(MAX_REQUEST_HEADER_BYTES - "x-big".len());
        let big = with_headers("http://127.0.0.1/", &[("x-big", &value)]);
        assert!(policy.check(&big).is_ok(), "exactly 64 KiB of header text");

        let url = request(&url_of_length(MAX_URL_BYTES));
        assert!(policy.check(&url).is_ok(), "an 8 KiB url");
    }

    // ── Policy Tests (no wasm) ────────────────────────────────────────────────

    fn policy(consumer: &str, hosts: &[&str]) -> FetchPolicy {
        FetchPolicy::new(consumer, Allowlist::new(hosts), Duration::from_secs(30))
    }

    fn denied(result: &Result<url::Url, types::FetchError>) -> bool {
        matches!(result, Err(types::FetchError::Denied(_)))
    }

    fn invalid(result: &Result<url::Url, types::FetchError>) -> bool {
        matches!(result, Err(types::FetchError::InvalidRequest(_)))
    }

    #[test]
    fn allowlist_matches_the_exact_host_only() {
        let policy = policy("p", &["reddit.com"]);
        assert!(policy.check(&request("https://reddit.com/")).is_ok());
        for url in [
            "https://www.reddit.com/",
            "https://evilreddit.com/",
            "https://reddit.com.evil.com/",
            "https://notreddit.com/",
            "https://evil.com/reddit.com",
            "https://evil.com/?h=reddit.com",
        ] {
            let result = policy.check(&request(url));
            assert!(denied(&result), "{url}: {result:?}");
        }
    }

    #[test]
    fn allowlist_ignores_case_and_one_trailing_dot() {
        let policy = policy("p", &["reddit.com"]);
        assert!(policy.check(&request("https://REDDIT.com/x")).is_ok());
        assert!(policy.check(&request("https://reddit.com./")).is_ok());

        let dotted = self::policy("p", &["Reddit.COM."]);
        assert!(dotted.check(&request("https://reddit.com/")).is_ok());
    }

    #[test]
    fn allowlist_entries_that_are_not_bare_hosts_never_match() {
        let entries = [
            "https://reddit.com",
            "reddit.com:443",
            "reddit.com/x",
            "*.reddit.com",
            "",
            "   ",
            "red dit.com",
        ];
        let allowlist = Allowlist::new(entries);
        assert_eq!(
            allowlist.rejected(),
            [
                "https://reddit.com",
                "reddit.com:443",
                "reddit.com/x",
                "*.reddit.com",
                "red dit.com"
            ]
        );

        let policy = policy("p", &entries);
        for url in [
            "https://www.reddit.com/",
            "https://reddit.com/",
            "https://example.com/",
        ] {
            let result = policy.check(&request(url));
            assert!(denied(&result), "{url}: {result:?}");
        }
    }

    #[test]
    fn an_empty_allowlist_denies_every_host() {
        let policy = FetchPolicy::new(
            "p",
            Allowlist::new(Vec::<String>::new()),
            Duration::from_secs(30),
        );
        for url in ["https://example.com/", "http://127.0.0.1/"] {
            let result = policy.check(&request(url));
            assert!(denied(&result), "{url}: {result:?}");
        }
    }

    #[test]
    fn a_url_with_userinfo_is_judged_by_its_real_host() {
        let policy = policy("p", &["reddit.com"]);
        let result = policy.check(&request("https://reddit.com@evil.com/"));
        assert!(denied(&result), "{result:?}");
        assert!(policy.check(&request("https://evil.com@reddit.com/")).is_ok());
    }

    #[test]
    fn any_port_on_an_allowed_host_is_allowed() {
        let policy = policy("p", &["reddit.com"]);
        assert!(policy.check(&request("https://reddit.com:8443/x")).is_ok());
        assert!(policy.check(&request("http://reddit.com:80/")).is_ok());
    }

    #[test]
    fn an_ip_literal_host_matches_an_ip_literal_entry() {
        let policy = policy("p", &["127.0.0.1", "[::1]"]);
        assert!(policy.check(&request("http://127.0.0.1:9/")).is_ok());
        assert!(policy.check(&request("http://[::1]:9/")).is_ok());
        let result = policy.check(&request("http://127.0.0.2/"));
        assert!(denied(&result), "{result:?}");
    }

    #[test]
    fn only_http_and_https_urls_pass_the_policy() {
        let policy = policy("p", &["reddit.com", "etc"]);
        for url in [
            "file:///etc/passwd",
            "ftp://reddit.com/",
            "gopher://reddit.com/",
            "data:text/plain,hi",
        ] {
            let result = policy.check(&request(url));
            assert!(invalid(&result), "{url}: {result:?}");
        }
    }

    #[test]
    fn an_empty_or_unparsable_url_is_an_invalid_request() {
        let policy = policy("p", &["reddit.com"]);
        for url in ["", "   ", "not a url", "http://"] {
            let result = policy.check(&request(url));
            assert!(invalid(&result), "{url:?}: {result:?}");
        }
    }

    #[test]
    fn a_denial_names_the_host_and_the_consumer_but_not_the_path_or_query() {
        let policy = policy("reddit-src", &["reddit.com"]);
        let result = policy.check(&request("https://evil.example/secret/path?token=SECRET123"));
        let Err(types::FetchError::Denied(text)) = result else {
            panic!("expected a denial, got {result:?}");
        };
        assert!(text.contains("evil.example"), "{text}");
        assert!(text.contains("reddit-src"), "{text}");
        assert!(!text.contains("SECRET123"), "{text}");
        assert!(!text.contains("/secret/path"), "{text}");
    }

    #[test]
    fn the_source_policy_reads_timeout_and_allowlist_from_the_host_config() {
        use crate::plugins::config::BackoffConfig;

        let host = HostConfig {
            poll_interval: Duration::from_mins(1),
            timeout: Duration::from_secs(7),
            allowlist: vec!["www.reddit.com".to_string()],
            backoff: BackoffConfig {
                initial: Duration::from_secs(1),
                max: Duration::from_mins(1),
                multiplier: 2.0,
            },
        };
        let policy = FetchPolicy::for_source("reddit", &host);
        assert_eq!(policy.timeout(), Duration::from_secs(7));
        assert_eq!(policy.consumer(), "reddit");
        assert!(policy.check(&request("https://www.reddit.com/")).is_ok());
        let result = policy.check(&request("https://reddit.com/"));
        assert!(denied(&result), "{result:?}");
    }
}
