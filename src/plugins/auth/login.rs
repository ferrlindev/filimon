// ─────────────────────────────────────────────────────────────────────────────
// plugins/auth/login.rs - The interactive PKCE login flow
//
// `login` composes the loopback listener, PKCE, the token client and the token
// store, in this order (D-04, D-05, D-06):
//
//   1. bind the loopback port         (a busy port fails here, before any output)
//   2. build the authorize URL        (fresh verifier, challenge and state)
//   3. hand the URL to the hooks      (print it; optionally open the browser)
//   4. wait for the callback          (state checked; the port is released)
//   5. exchange the code              (with the verifier from step 2)
//   6. lock, then write the file      (the only filesystem change)
//
// Nothing is locked, written or created before step 6, so a state mismatch, a
// denied login, a timeout, an interruption or a failed exchange leaves an
// existing login byte for byte as it was (D-06).
//
// The hooks are the only seam: the CLI prints and opens a browser, tests drive
// the callback themselves. The only text a hook ever receives is the authorize
// URL, which holds public parameters only. The flow logs nothing.
// ─────────────────────────────────────────────────────────────────────────────

use std::time::Duration;

use super::callback::{CallbackListener, LOGIN_TIMEOUT};
use super::client::{DEFAULT_EXPIRES_IN_SECS, TokenClient};
use super::error::AuthError;
use super::pkce::{build_authorize_url, generate_state, generate_verifier, s256_challenge};
use super::store::{AccessToken, RefreshToken, StoredTokens, TokenStore};
use crate::plugins::config::OAuthConfig;

/// How one login behaves.
#[derive(Debug, Clone, Copy)]
pub struct LoginOptions {
    /// Whether to ask the hooks to open the default browser (`--no-browser`
    /// turns this off; the URL is shown either way).
    pub open_browser: bool,
    /// How long to wait for the provider's callback.
    pub timeout: Duration,
}

impl LoginOptions {
    /// The options of an interactive `fili auth login`: the standard
    /// five-minute wait.
    #[must_use]
    pub fn interactive(open_browser: bool) -> Self {
        Self {
            open_browser,
            timeout: LOGIN_TIMEOUT,
        }
    }
}

/// What the login flow tells its caller while it runs. Both methods are
/// infallible: an implementation reports its own problems (for example a
/// browser that fails to launch) itself.
pub trait LoginHooks {
    /// Called once, with the authorize URL, before the wait begins.
    fn authorize_url(&mut self, url: &str);
    /// Called after `authorize_url`, and only when the options ask for the
    /// browser to be opened.
    fn open_browser(&mut self, url: &str);
}

/// What a successful login reports. It carries no token text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoginSummary {
    /// Unix seconds when the access token expires.
    pub expires_at: i64,
    /// Whether the provider issued a refresh token. Without one the login
    /// cannot be renewed and ends at `expires_at`.
    pub has_refresh_token: bool,
    /// The scope the provider granted, when it said.
    pub scope: Option<String>,
}

/// Runs one interactive login and stores its tokens.
///
/// # Errors
///
/// `RedirectPortInUse` for a busy port (before anything is shown), the
/// callback errors (`StateMismatch`, `CallbackDenied`, `CallbackInvalid`,
/// `CallbackTimeout`), the token client's errors (`TokenRequest`,
/// `TokenRejected`, `TokenResponse`) and the store's (`TokenWrite`). Any error
/// before the final write leaves the filesystem untouched.
pub async fn login(
    oauth: &OAuthConfig,
    store: &TokenStore,
    options: &LoginOptions,
    hooks: &mut dyn LoginHooks,
) -> Result<LoginSummary, AuthError> {
    // 1. A busy port fails here, before anything is shown.
    let listener = CallbackListener::bind(oauth.redirect_port).await?;
    let redirect_uri = listener.redirect_uri();

    // 2. Fresh PKCE values and the authorize URL.
    let verifier = generate_verifier()?;
    let challenge = s256_challenge(verifier.expose());
    let state = generate_state()?;
    let Some(url) = build_authorize_url(oauth, &redirect_uri, &state, &challenge) else {
        return Err(AuthError::CallbackInvalid {
            reason: "no authorize url".to_string(),
        });
    };

    // 3. Show the URL; open the browser only when asked.
    hooks.authorize_url(url.as_str());
    if options.open_browser {
        hooks.open_browser(url.as_str());
    }

    // 4. Wait for the callback. The listener is consumed: the port is free
    // again when this returns.
    let code = listener.wait_for_code(&state, options.timeout).await?;

    // 5. Exchange the code with the verifier whose challenge was sent.
    let answer = TokenClient::new(oauth)?
        .exchange_code(&code, &verifier, &redirect_uri)
        .await?;

    // 6. Only now touch the filesystem. An older refresh token is never
    // carried over: it belongs to another grant.
    let now = chrono::Utc::now().timestamp();
    let lifetime =
        i64::try_from(answer.expires_in.unwrap_or(DEFAULT_EXPIRES_IN_SECS)).unwrap_or(i64::MAX);
    let tokens = StoredTokens {
        access_token: AccessToken::new(answer.access_token),
        refresh_token: answer.refresh_token.map(RefreshToken::new),
        expires_at: now.saturating_add(lifetime),
        token_type: answer.token_type,
        scope: answer.scope,
        needs_login: false,
        obtained_at: now,
    };
    let summary = LoginSummary {
        expires_at: tokens.expires_at,
        has_refresh_token: tokens.refresh_token.is_some(),
        scope: tokens.scope.clone(),
    };
    let lock = store.lock().await?;
    store.write(&tokens)?;
    drop(lock);

    Ok(summary)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::auth::testing::{
        client_credentials_config_for, form_pairs, oauth_config_for, token_body, unix_now,
        write_tokens,
    };
    use crate::plugins::host::testing::{Reply, TestHttpServer};
    use tempfile::TempDir;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpStream;

    const ALIAS: &str = "stub";

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum Visit {
        /// A callback with the code `CODE-1` and the right state.
        Good,
        /// A callback with the code but a wrong state.
        WrongState,
        /// A callback with `error=access_denied` and the right state.
        Denied,
        /// Nobody visits.
        Nobody,
    }

    /// Records what it is given and "visits" the authorize URL the way a
    /// browser would: a raw GET to the redirect URI.
    struct TestHooks {
        visit: Visit,
        visit_from_authorize_url: bool,
        urls: Vec<String>,
        opener_calls: usize,
    }

    impl TestHooks {
        fn new(visit: Visit) -> Self {
            Self {
                visit,
                visit_from_authorize_url: false,
                urls: Vec::new(),
                opener_calls: 0,
            }
        }

        fn perform_visit(&self, url: &str) {
            let parsed = url::Url::parse(url).expect("authorize url");
            let query = |name: &str| {
                parsed
                    .query_pairs()
                    .find(|(key, _)| key == name)
                    .map(|(_, value)| value.into_owned())
                    .unwrap_or_default()
            };
            let state = query("state");
            let redirect = url::Url::parse(&query("redirect_uri")).expect("redirect uri");
            let port = redirect.port().expect("redirect port");
            let target = match self.visit {
                Visit::Nobody => return,
                Visit::Good => format!("/callback?code=CODE-1&state={state}"),
                Visit::WrongState => "/callback?code=CODE-1&state=WRONG".to_string(),
                Visit::Denied => format!("/callback?error=access_denied&state={state}"),
            };
            tokio::spawn(async move {
                let mut stream = TcpStream::connect(("127.0.0.1", port))
                    .await
                    .expect("connect to the listener");
                let request = format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n");
                stream.write_all(request.as_bytes()).await.expect("write");
                // The wait ends on the request; the reply page is not needed.
            });
        }
    }

    impl LoginHooks for TestHooks {
        fn authorize_url(&mut self, url: &str) {
            self.urls.push(url.to_string());
            if self.visit_from_authorize_url {
                self.perform_visit(url);
            }
        }

        fn open_browser(&mut self, url: &str) {
            self.opener_calls += 1;
            if !self.visit_from_authorize_url {
                self.perform_visit(url);
            }
        }
    }

    fn token_endpoint(reply: Reply) -> impl Future<Output = TestHttpServer> {
        TestHttpServer::start(move |_| reply.clone())
    }

    fn good_endpoint() -> impl Future<Output = TestHttpServer> {
        token_endpoint(Reply::json(
            200,
            &token_body("AT-1", Some("RT-1"), Some(3600)),
        ))
    }

    fn query_value(url: &str, name: &str) -> String {
        url::Url::parse(url)
            .expect("url")
            .query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
            .unwrap_or_default()
    }

    fn token_path(state: &TempDir) -> std::path::PathBuf {
        state.path().join("auth").join(format!("{ALIAS}.json"))
    }

    fn options() -> LoginOptions {
        LoginOptions {
            open_browser: true,
            timeout: Duration::from_secs(10),
        }
    }

    #[tokio::test]
    async fn login_stores_the_tokens_0600_after_a_matching_callback() {
        let server = good_endpoint().await;
        let state = TempDir::new().expect("tempdir");
        let oauth = oauth_config_for(&server.url("/token"), None);
        let store = TokenStore::new(state.path(), ALIAS);
        let mut hooks = TestHooks::new(Visit::Good);

        let before = unix_now();
        let summary = login(&oauth, &store, &options(), &mut hooks)
            .await
            .expect("login");

        assert!(summary.has_refresh_token);
        let stored = store.read().expect("read").expect("a stored login");
        assert_eq!(stored.access_token.expose(), "AT-1");
        assert_eq!(stored.refresh_token.expect("refresh").expose(), "RT-1");
        assert!(!stored.needs_login);
        assert!((stored.expires_at - (before + 3600)).abs() <= 5);
        assert_eq!(summary.expires_at, stored.expires_at);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(token_path(&state))
                .expect("metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }

    #[tokio::test]
    async fn the_exchange_carries_the_verifier_that_matches_the_authorize_challenge() {
        let server = good_endpoint().await;
        let state = TempDir::new().expect("tempdir");
        let oauth = oauth_config_for(&server.url("/token"), None);
        let store = TokenStore::new(state.path(), ALIAS);
        let mut hooks = TestHooks::new(Visit::Good);

        login(&oauth, &store, &options(), &mut hooks)
            .await
            .expect("login");

        let url = hooks.urls.first().expect("the url was shown");
        let seen = server.seen();
        assert_eq!(seen.len(), 1);
        let form = form_pairs(&seen[0].body);
        let field = |name: &str| {
            form.iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
                .unwrap_or_default()
        };
        assert_eq!(
            s256_challenge(&field("code_verifier")),
            query_value(url, "code_challenge")
        );
        assert_eq!(field("redirect_uri"), query_value(url, "redirect_uri"));
        assert_eq!(field("code"), "CODE-1");
        assert_eq!(field("client_id"), "client-1");
    }

    #[tokio::test]
    async fn the_authorize_url_carries_authorize_params() {
        let server = good_endpoint().await;
        let state = TempDir::new().expect("tempdir");
        let mut oauth = oauth_config_for(&server.url("/token"), None);
        oauth.authorize_params = vec![("duration".to_string(), "permanent".to_string())];
        let store = TokenStore::new(state.path(), ALIAS);
        let mut hooks = TestHooks::new(Visit::Good);

        login(&oauth, &store, &options(), &mut hooks)
            .await
            .expect("login");

        let url = hooks.urls.first().expect("the url was shown");
        assert!(url.ends_with("duration=permanent"), "url: {url}");
        assert!(url.contains("code_challenge_method=S256&duration=permanent"));
    }

    /// Writes an existing login and returns its bytes.
    fn existing_login(state: &TempDir) -> Vec<u8> {
        write_tokens(state.path(), ALIAS, "OLD-AT", unix_now() + 1000);
        std::fs::read(token_path(state)).expect("read the existing login")
    }

    #[tokio::test]
    async fn a_state_mismatch_writes_nothing_and_leaves_an_existing_login_alone() {
        let server = good_endpoint().await;
        let oauth = oauth_config_for(&server.url("/token"), None);

        // With an existing login: byte-identical afterwards.
        let state = TempDir::new().expect("tempdir");
        let before = existing_login(&state);
        let store = TokenStore::new(state.path(), ALIAS);
        let result = login(
            &oauth,
            &store,
            &options(),
            &mut TestHooks::new(Visit::WrongState),
        )
        .await;
        assert!(
            matches!(result, Err(AuthError::StateMismatch)),
            "{result:?}"
        );
        assert_eq!(std::fs::read(token_path(&state)).expect("read"), before);

        // Without one: no file and no `auth` directory.
        let empty = TempDir::new().expect("tempdir");
        let store = TokenStore::new(empty.path(), ALIAS);
        let result = login(
            &oauth,
            &store,
            &options(),
            &mut TestHooks::new(Visit::WrongState),
        )
        .await;
        assert!(
            matches!(result, Err(AuthError::StateMismatch)),
            "{result:?}"
        );
        assert!(!empty.path().join("auth").exists());
        assert_eq!(server.seen().len(), 0, "no token request was made");
    }

    #[tokio::test]
    async fn a_denied_login_writes_nothing() {
        let server = good_endpoint().await;
        let oauth = oauth_config_for(&server.url("/token"), None);
        let state = TempDir::new().expect("tempdir");
        let store = TokenStore::new(state.path(), ALIAS);

        let result = login(
            &oauth,
            &store,
            &options(),
            &mut TestHooks::new(Visit::Denied),
        )
        .await;

        assert!(
            matches!(result, Err(AuthError::CallbackDenied { .. })),
            "{result:?}"
        );
        assert!(!state.path().join("auth").exists());
        assert_eq!(server.seen().len(), 0);
    }

    #[tokio::test]
    async fn a_timeout_writes_nothing() {
        let server = good_endpoint().await;
        let oauth = oauth_config_for(&server.url("/token"), None);
        let state = TempDir::new().expect("tempdir");
        let before = existing_login(&state);
        let store = TokenStore::new(state.path(), ALIAS);
        let short = LoginOptions {
            open_browser: true,
            timeout: Duration::from_millis(100),
        };

        let result = login(&oauth, &store, &short, &mut TestHooks::new(Visit::Nobody)).await;

        assert!(
            matches!(result, Err(AuthError::CallbackTimeout { .. })),
            "{result:?}"
        );
        assert_eq!(std::fs::read(token_path(&state)).expect("read"), before);

        let empty = TempDir::new().expect("tempdir");
        let store = TokenStore::new(empty.path(), ALIAS);
        let result = login(&oauth, &store, &short, &mut TestHooks::new(Visit::Nobody)).await;
        assert!(matches!(result, Err(AuthError::CallbackTimeout { .. })));
        assert!(!empty.path().join("auth").exists());
    }

    #[tokio::test]
    async fn a_failed_exchange_writes_nothing() {
        let server = token_endpoint(Reply::json(400, r#"{"error":"invalid_grant"}"#)).await;
        let oauth = oauth_config_for(&server.url("/token"), None);
        let state = TempDir::new().expect("tempdir");
        let before = existing_login(&state);
        let store = TokenStore::new(state.path(), ALIAS);

        let result = login(&oauth, &store, &options(), &mut TestHooks::new(Visit::Good)).await;

        assert!(
            matches!(result, Err(AuthError::TokenRejected { .. })),
            "{result:?}"
        );
        assert_eq!(server.seen().len(), 1, "the exchange was attempted");
        assert_eq!(std::fs::read(token_path(&state)).expect("read"), before);

        let empty = TempDir::new().expect("tempdir");
        let store = TokenStore::new(empty.path(), ALIAS);
        let result = login(&oauth, &store, &options(), &mut TestHooks::new(Visit::Good)).await;
        assert!(matches!(result, Err(AuthError::TokenRejected { .. })));
        assert!(!empty.path().join("auth").exists());
    }

    #[tokio::test]
    async fn no_browser_never_calls_the_opener() {
        let server = good_endpoint().await;
        let oauth = oauth_config_for(&server.url("/token"), None);
        let state = TempDir::new().expect("tempdir");
        let store = TokenStore::new(state.path(), ALIAS);
        let mut hooks = TestHooks::new(Visit::Good);
        hooks.visit_from_authorize_url = true;
        let no_browser = LoginOptions {
            open_browser: false,
            timeout: Duration::from_secs(10),
        };

        login(&oauth, &store, &no_browser, &mut hooks)
            .await
            .expect("login");

        assert_eq!(hooks.opener_calls, 0);
        assert_eq!(hooks.urls.len(), 1);
        assert!(token_path(&state).exists());
    }

    #[tokio::test]
    async fn login_refuses_a_client_credentials_config_before_binding_the_port() {
        let server = good_endpoint().await;
        // A held port would make a bind attempt fail with `RedirectPortInUse`,
        // so `GrantHasNoLogin` proves the refusal came first.
        let holder = CallbackListener::bind(0).await.expect("hold a port");
        let mut oauth = client_credentials_config_for(&server.url("/token"), "SECRET-1");
        oauth.redirect_port = holder.port();
        let state = TempDir::new().expect("tempdir");
        let store = TokenStore::new(state.path(), ALIAS);
        let mut hooks = TestHooks::new(Visit::Good);

        let result = login(&oauth, &store, &options(), &mut hooks).await;

        assert!(
            matches!(result, Err(AuthError::GrantHasNoLogin)),
            "expected GrantHasNoLogin, got {result:?}"
        );
        assert!(hooks.urls.is_empty(), "no authorize URL was shown");
        assert_eq!(hooks.opener_calls, 0);
        assert_eq!(server.seen().len(), 0);
        assert!(!state.path().join("auth").exists(), "no token file");
    }

    #[tokio::test]
    async fn a_second_login_on_the_same_port_fails_naming_the_key() {
        use miette::Diagnostic as _;

        let server = good_endpoint().await;
        let holder = CallbackListener::bind(0).await.expect("hold a port");
        let mut oauth = oauth_config_for(&server.url("/token"), None);
        oauth.redirect_port = holder.port();
        let state = TempDir::new().expect("tempdir");
        let store = TokenStore::new(state.path(), ALIAS);
        let mut hooks = TestHooks::new(Visit::Good);

        let result = login(&oauth, &store, &options(), &mut hooks).await;

        let Err(error @ AuthError::RedirectPortInUse { .. }) = result else {
            panic!("expected RedirectPortInUse, got {result:?}");
        };
        let rendered = format!(
            "{error} {}",
            error.help().map(|h| h.to_string()).unwrap_or_default()
        );
        assert!(rendered.contains("host.oauth.redirect_port"), "{rendered}");
        assert!(hooks.urls.is_empty(), "no URL was shown");
        assert_eq!(hooks.opener_calls, 0);
        assert_eq!(server.seen().len(), 0);
        assert!(!state.path().join("auth").exists());
    }

    #[tokio::test]
    async fn a_login_without_a_refresh_token_is_reported() {
        let server = token_endpoint(Reply::json(200, &token_body("AT-1", None, Some(3600)))).await;
        let oauth = oauth_config_for(&server.url("/token"), None);
        let state = TempDir::new().expect("tempdir");
        let store = TokenStore::new(state.path(), ALIAS);

        let summary = login(&oauth, &store, &options(), &mut TestHooks::new(Visit::Good))
            .await
            .expect("login");

        assert!(!summary.has_refresh_token);
        let stored = store.read().expect("read").expect("stored");
        assert!(stored.refresh_token.is_none());
    }

    #[tokio::test]
    async fn login_uses_the_default_lifetime_when_expires_in_is_absent() {
        let server =
            token_endpoint(Reply::json(200, &token_body("AT-1", Some("RT-1"), None))).await;
        let oauth = oauth_config_for(&server.url("/token"), None);
        let state = TempDir::new().expect("tempdir");
        let store = TokenStore::new(state.path(), ALIAS);

        let before = unix_now();
        login(&oauth, &store, &options(), &mut TestHooks::new(Visit::Good))
            .await
            .expect("login");

        let stored = store.read().expect("read").expect("stored");
        assert!((stored.expires_at - (before + 3600)).abs() <= 5);
    }

    #[tokio::test]
    async fn nothing_secret_reaches_the_hooks_or_the_summary() {
        let server = good_endpoint().await;
        let oauth = oauth_config_for(&server.url("/token"), Some("CS-SENTINEL"));
        let state = TempDir::new().expect("tempdir");
        let store = TokenStore::new(state.path(), ALIAS);
        let mut hooks = TestHooks::new(Visit::Good);

        let summary = login(&oauth, &store, &options(), &mut hooks)
            .await
            .expect("login");

        let seen = server.seen();
        let form = form_pairs(&seen[0].body);
        let verifier = form
            .iter()
            .find(|(key, _)| key == "code_verifier")
            .map(|(_, value)| value.clone())
            .expect("the verifier was sent");
        let url = hooks.urls.first().expect("url");
        let shown = format!("{} {summary:?}", hooks.urls.join(" "));
        for secret in ["CODE-1", verifier.as_str(), "AT-1", "RT-1", "CS-SENTINEL"] {
            assert!(!shown.contains(secret), "{secret} leaked into: {shown}");
        }
        assert!(url.contains("client_id="));
        assert!(url.contains("state="));
        assert!(url.contains("code_challenge="));
        assert!(
            !url::Url::parse(url)
                .expect("url")
                .query_pairs()
                .any(|(key, _)| key == "code_verifier")
        );
    }
}
