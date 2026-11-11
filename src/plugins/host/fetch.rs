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
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use super::engine::source_world::filimon::plugin::types;
use super::engine::{FetcherPre, HostState, clean_guest_text, new_guest_store};
use crate::plugins::config::HostConfig;

/// Longest text kept from a fetcher failure, in characters.
const FAILURE_TEXT_CLIP: usize = 512;

// ── Policy ────────────────────────────────────────────────────────────────────

/// The hosts a consumer may fetch from.
#[derive(Debug, Clone, Default)]
pub struct Allowlist {
    hosts: BTreeSet<String>,
    rejected: Vec<String>,
}

impl Allowlist {
    /// Builds an allowlist from the configured host names.
    pub fn new<I>(entries: I) -> Self
    where
        I: IntoIterator,
        I::Item: AsRef<str>,
    {
        let hosts = entries
            .into_iter()
            .map(|entry| entry.as_ref().trim().to_ascii_lowercase())
            .filter(|entry| !entry.is_empty())
            .collect();
        Self {
            hosts,
            rejected: Vec::new(),
        }
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
        self.hosts.contains(&host.to_string())
    }
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
        let url = url::Url::parse(&request.url)
            .map_err(|_| types::FetchError::InvalidRequest("url cannot be parsed".to_string()))?;
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
        Ok(url)
    }
}

// ── Route ─────────────────────────────────────────────────────────────────────

/// Answers one consumer's `fetch` calls with its assigned fetcher component.
pub struct FetchRoute {
    fetcher: Arc<str>,
    pre: Arc<FetcherPre<HostState>>,
    engine: wasmtime::Engine,
    policy: Arc<FetchPolicy>,
    calls: AtomicU64,
}

impl FetchRoute {
    #[must_use]
    pub fn new(
        fetcher: &str,
        pre: Arc<FetcherPre<HostState>>,
        engine: wasmtime::Engine,
        policy: Arc<FetchPolicy>,
    ) -> Self {
        Self {
            fetcher: Arc::from(fetcher),
            pre,
            engine,
            policy,
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
        if let Ok(result) = tokio::time::timeout_at(deadline, self.call_fetcher(&request)).await {
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
    ) -> Result<types::Response, types::FetchError> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let mut store = new_guest_store(&self.engine, HostState::new(&self.fetcher));
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
    use crate::plugins::host::testing::{self, LogCapture, fixture_wasm};

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
            poll_interval: Duration::from_secs(60),
            timeout: Duration::from_secs(7),
            allowlist: vec!["www.reddit.com".to_string()],
            backoff: BackoffConfig {
                initial: Duration::from_secs(1),
                max: Duration::from_secs(60),
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
