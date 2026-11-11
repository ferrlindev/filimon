// ─────────────────────────────────────────────────────────────────────────────
// plugins/auth/provider.rs - What a plugin's `poll` gets for an access token
//
// `TokenProvider::access_token` is consulted immediately before every poll, by
// the scheduler loop and by `fili plugins run-once` alike. It re-reads the
// token file each time (D-08), so a login made, replaced or revoked while
// `fili run` is up takes effect on the next poll with no restart.
//
// The decision table:
//
//   no file                      -> NotLoggedIn   (poll with none, hint once)
//   file flagged `needs_login`   -> NeedsLogin    (poll with none, hint once)
//   at least 60 s of life left   -> Token         (D-07)
//   under 60 s of life left      -> refresh first (below)
//   file unusable                -> FileProblem   (disables only that plugin)
//
// The refresh is lazy: it happens here, just before a poll, and never on a
// timer (D-07). It runs under an in-process gate and the per-alias file lock,
// re-reads the file after taking the lock, and saves the new login before its
// access token is returned (D-08):
//
//   refresh ok                   -> Token         (new login on disk first)
//   save failed                  -> RefreshFailed (no token is handed out)
//   invalid_grant                -> NeedsLogin    (file kept, flag set; D-09)
//   transient / rejected         -> Token(old) while it is still valid,
//                                   else RefreshFailed; nothing is marked
//   no refresh token, expired    -> NeedsLogin
//
// The `client_credentials` grant (D-03) has no login and no file. The host
// fetches the app token itself, holds it in memory only and serves every poll
// from that cache:
//
//   nothing cached               -> fetch first      (single flight)
//   at least 60 s of life left   -> Token            (cached, no request)
//   under 60 s of life left      -> fetch again first
//   fetch ok                     -> Token           (cached; no expires_in
//                                                    means 3600 s)
//   fetch failed, cached token
//   still valid                  -> Token(cached)   (a warning is logged)
//   fetch failed, nothing valid  -> RefreshFailed   (a backed-off cycle)
//
// It never answers NotLoggedIn, NeedsLogin or FileProblem, and a stale login
// file of the same alias is ignored. A new process fetches a fresh token.
//
// Expiry is judged by an injectable wall-clock function. Tokio's paused time
// does not move the wall clock, so a test that pauses time can neither make a
// token look fresh nor stale by accident.
// ─────────────────────────────────────────────────────────────────────────────

use std::path::Path;
use std::sync::{Arc, OnceLock};

use super::client::{DEFAULT_EXPIRES_IN_SECS, RefreshFailure, TokenClient, TokenSet};
use super::error::AuthError;
use super::store::{AccessToken, AuthLock, RefreshToken, StoredTokens, TokenStore};
use crate::plugins::config::{OAuthConfig, OAuthGrant};
use crate::plugins::host::engine::clean_guest_text;

/// Longest error text kept in an outcome, in characters.
const ERROR_TEXT_CLIP: usize = 240;

/// A token with fewer seconds than this left is due for a refresh (D-07).
pub const REFRESH_MARGIN_SECS: i64 = 60;

/// The wall clock, as Unix seconds.
pub type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

/// The real wall clock.
#[must_use]
pub fn system_clock() -> Clock {
    Arc::new(|| chrono::Utc::now().timestamp())
}

/// What the provider decided for one poll.
#[derive(Debug)]
pub enum TokenOutcome {
    /// Poll with this access token.
    Token(AccessToken),
    /// No token file: poll with none.
    NotLoggedIn,
    /// The stored login was flagged as needing a new login: poll with none.
    NeedsLogin,
    /// A transient problem (cleaned text): no token is available this poll.
    RefreshFailed(String),
    /// The token file cannot be used.
    FileProblem(AuthError),
}

/// Reads one plugin's stored login and decides what its poll receives.
pub struct TokenProvider {
    plugin: String,
    oauth: OAuthConfig,
    store: TokenStore,
    clock: Clock,
    /// One refresh in flight per plugin inside this process (D-08); the
    /// per-alias file lock covers other processes.
    gate: tokio::sync::Mutex<()>,
    /// Built on first use; a build failure is kept as cleaned text.
    client: OnceLock<Result<TokenClient, String>>,
    /// The app-only token of the `client_credentials` grant. Memory only: it
    /// is never written to disk. Held across the request (single flight).
    app_token: tokio::sync::Mutex<Option<AppToken>>,
}

/// A fetched app-only access token and when it expires (Unix seconds).
struct AppToken {
    token: AccessToken,
    expires_at: i64,
}

impl std::fmt::Debug for TokenProvider {
    /// The alias only: never the config, a token or the clock.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenProvider")
            .field("plugin", &self.plugin)
            .finish_non_exhaustive()
    }
}

impl TokenProvider {
    /// A provider for `plugin` over `<state_dir>/auth`, on the system clock.
    #[must_use]
    pub fn new(plugin: &str, oauth: OAuthConfig, state_dir: &Path) -> Self {
        Self {
            plugin: plugin.to_string(),
            oauth,
            store: TokenStore::new(state_dir, plugin),
            clock: system_clock(),
            gate: tokio::sync::Mutex::new(()),
            client: OnceLock::new(),
            app_token: tokio::sync::Mutex::new(None),
        }
    }

    /// The same provider on `clock`, so a test decides what time it is.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn with_clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }

    /// Re-reads the token file and decides what the next poll receives,
    /// refreshing the token first when it is due.
    pub async fn access_token(&self) -> TokenOutcome {
        // An app-only plugin has no login: its token is fetched by the host
        // and held in memory, so the token file is never read, created or
        // locked for this grant.
        if self.oauth.grant == OAuthGrant::ClientCredentials {
            return self.app_token().await;
        }
        let tokens = match self.store.read() {
            Ok(Some(tokens)) => tokens,
            Ok(None) => return TokenOutcome::NotLoggedIn,
            Err(error) => return TokenOutcome::FileProblem(error),
        };
        if tokens.needs_login {
            return TokenOutcome::NeedsLogin;
        }
        let now = (self.clock)();
        if tokens.expires_at.saturating_sub(now) >= REFRESH_MARGIN_SECS {
            return TokenOutcome::Token(tokens.access_token);
        }
        self.renew(tokens, now).await
    }

    /// The app-only token of the `client_credentials` grant (D-03): served
    /// from memory while at least [`REFRESH_MARGIN_SECS`] seconds of life are
    /// left, fetched otherwise. The tokio mutex is held across the request, so
    /// a concurrent caller waits and then finds the fresh cache (single
    /// flight). There is no retry here: one request per due poll, the
    /// scheduler's backoff does the waiting.
    async fn app_token(&self) -> TokenOutcome {
        let mut cache = self.app_token.lock().await;
        let now = (self.clock)();
        if let Some(cached) = cache.as_ref()
            && cached.expires_at.saturating_sub(now) >= REFRESH_MARGIN_SECS
        {
            return TokenOutcome::Token(cached.token.clone());
        }
        let fetched = match self.client() {
            Ok(client) => client.client_credentials().await,
            Err(text) => Err(RefreshFailure::Transient(format!(
                "http client unavailable: {text}"
            ))),
        };
        match fetched {
            Ok(set) => {
                let lifetime = i64::try_from(set.expires_in.unwrap_or(DEFAULT_EXPIRES_IN_SECS))
                    .unwrap_or(i64::MAX);
                let token = AccessToken::new(set.access_token);
                *cache = Some(AppToken {
                    token: token.clone(),
                    expires_at: (self.clock)().saturating_add(lifetime),
                });
                TokenOutcome::Token(token)
            }
            Err(
                RefreshFailure::Transient(text)
                | RefreshFailure::Rejected(text)
                | RefreshFailure::Permanent(text),
            ) => {
                let text = clean_guest_text(&text, ERROR_TEXT_CLIP);
                if let Some(cached) = cache.as_ref()
                    && cached.expires_at > (self.clock)()
                {
                    tracing::warn!(
                        plugin = %self.plugin,
                        "the app token request failed, using the current token: {text}"
                    );
                    return TokenOutcome::Token(cached.token.clone());
                }
                TokenOutcome::RefreshFailed(format!("the app token request failed: {text}"))
            }
        }
    }

    /// The token is under the margin. With no refresh token to use and some
    /// life left it is used as it is; otherwise the refresh (or the marking)
    /// runs under the in-process gate and the per-alias lock.
    async fn renew(&self, stored: StoredTokens, now: i64) -> TokenOutcome {
        if stored.refresh_token.is_none() && stored.expires_at > now {
            return TokenOutcome::Token(stored.access_token);
        }
        // One refresh in flight per plugin, then one writer across processes.
        let _gate = self.gate.lock().await;
        let lock = match self.store.lock().await {
            Ok(lock) => lock,
            Err(error) => return TokenOutcome::RefreshFailed(cleaned(&error)),
        };

        // A refresh or login that landed while this call waited is used as it
        // is: decide again from what is on disk now.
        let current = match self.store.read() {
            Ok(Some(tokens)) => tokens,
            Ok(None) => return TokenOutcome::NotLoggedIn,
            Err(error) => return TokenOutcome::FileProblem(error),
        };
        if current.needs_login {
            return TokenOutcome::NeedsLogin;
        }
        let now = (self.clock)();
        if current.expires_at.saturating_sub(now) >= REFRESH_MARGIN_SECS {
            return TokenOutcome::Token(current.access_token);
        }
        let Some(refresh_token) = current.refresh_token.clone() else {
            if current.expires_at > now {
                return TokenOutcome::Token(current.access_token);
            }
            self.mark_needs_login(&lock);
            return TokenOutcome::NeedsLogin;
        };

        let refreshed = match self.client() {
            Ok(client) => client.refresh(&refresh_token).await,
            Err(text) => Err(RefreshFailure::Transient(format!(
                "http client unavailable: {text}"
            ))),
        };
        match refreshed {
            Ok(set) => self.persist(&current, set),
            Err(RefreshFailure::Permanent(_)) => {
                self.mark_needs_login(&lock);
                tracing::warn!(
                    plugin = %self.plugin,
                    "the provider refused the stored login; run: fili auth login {}",
                    self.plugin
                );
                TokenOutcome::NeedsLogin
            }
            Err(RefreshFailure::Transient(text) | RefreshFailure::Rejected(text)) => {
                // D-09: nothing is marked and nothing is deleted.
                if current.expires_at > (self.clock)() {
                    tracing::warn!(
                        plugin = %self.plugin,
                        "token refresh failed, using the current token: {text}"
                    );
                    TokenOutcome::Token(current.access_token)
                } else {
                    TokenOutcome::RefreshFailed(text)
                }
            }
        }
    }

    /// Builds the new login from a refresh answer and saves it before its
    /// access token is handed to anyone (D-08). A save that fails hands out no
    /// token: using a rotated token that was never persisted could lose the
    /// only valid refresh token.
    fn persist(&self, current: &StoredTokens, set: TokenSet) -> TokenOutcome {
        let obtained_at = (self.clock)();
        let lifetime = i64::try_from(set.expires_in.unwrap_or(DEFAULT_EXPIRES_IN_SECS))
            .unwrap_or(i64::MAX);
        let tokens = StoredTokens {
            access_token: AccessToken::new(set.access_token),
            // An answer without one keeps the stored refresh token.
            refresh_token: set
                .refresh_token
                .map(RefreshToken::new)
                .or_else(|| current.refresh_token.clone()),
            expires_at: obtained_at.saturating_add(lifetime),
            token_type: set.token_type.or_else(|| current.token_type.clone()),
            scope: set.scope.or_else(|| current.scope.clone()),
            needs_login: false,
            obtained_at,
        };
        match self.store.write(&tokens) {
            Ok(()) => TokenOutcome::Token(tokens.access_token),
            Err(error) => TokenOutcome::RefreshFailed(format!(
                "the refreshed login could not be saved: {}",
                cleaned(&error)
            )),
        }
    }

    /// Flags the stored login as needing a new login. The file is kept (D-09).
    /// A failure to flag is logged once; the poll still runs without a token.
    fn mark_needs_login(&self, lock: &AuthLock) {
        if let Err(error) = self.store.mark_needs_login(lock) {
            tracing::warn!(
                plugin = %self.plugin,
                "could not mark the login as needing renewal: {}",
                cleaned(&error)
            );
        }
    }

    /// The token endpoint client, built on first use.
    fn client(&self) -> Result<&TokenClient, &str> {
        self.client
            .get_or_init(|| TokenClient::new(&self.oauth).map_err(|error| cleaned(&error)))
            .as_ref()
            .map_err(String::as_str)
    }
}

/// An error's text with control characters removed and its length capped.
fn cleaned(error: &AuthError) -> String {
    clean_guest_text(&error.to_string(), ERROR_TEXT_CLIP)
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use super::*;
    use crate::plugins::auth::testing::{oauth_config, write_tokens};

    /// A clock that always reads `now`.
    fn fixed(now: i64) -> Clock {
        Arc::new(move || now)
    }

    use crate::plugins::auth::store::{AccessToken, StoredTokens, TokenStore};

    /// A provider over a fresh state directory with a token expiring at
    /// `expires_at` and a clock reading `now`.
    fn provider(now: i64, expires_at: i64) -> (TempDir, TokenProvider) {
        let dir = TempDir::new().expect("tempdir");
        write_tokens(dir.path(), "stub", "AT-1", expires_at);
        let provider =
            TokenProvider::new("stub", oauth_config(), dir.path()).with_clock(fixed(now));
        (dir, provider)
    }

    #[tokio::test]
    async fn a_token_with_exactly_the_margin_left_is_used() {
        // Clock 1_000; expires at 1_060: exactly 60 seconds left (D-07).
        let (_dir, provider) = provider(1_000, 1_000 + REFRESH_MARGIN_SECS);

        match provider.access_token().await {
            TokenOutcome::Token(token) => assert_eq!(token.expose(), "AT-1"),
            other => panic!("expected a token, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn the_injected_clock_decides_expiry() {
        let dir = TempDir::new().expect("tempdir");
        write_tokens(dir.path(), "stub", "AT-1", 10_000);

        // Clock 1_000: 9_000 seconds left.
        let early = TokenProvider::new("stub", oauth_config(), dir.path()).with_clock(fixed(1_000));
        assert!(matches!(early.access_token().await, TokenOutcome::Token(_)));

        // Clock 10_010: expired, and the login holds no refresh token. Same
        // file, no sleeping.
        let late = TokenProvider::new("stub", oauth_config(), dir.path()).with_clock(fixed(10_010));
        assert!(matches!(
            late.access_token().await,
            TokenOutcome::NeedsLogin
        ));
    }

    #[tokio::test]
    async fn needs_login_wins_over_a_fresh_expiry() {
        let dir = TempDir::new().expect("tempdir");
        TokenStore::new(dir.path(), "stub")
            .write(&StoredTokens {
                access_token: AccessToken::new("AT-1".to_string()),
                refresh_token: None,
                expires_at: 1_000 + 3600,
                token_type: None,
                scope: None,
                needs_login: true,
                obtained_at: 1_000,
            })
            .expect("write");
        // Clock 1_000: an hour left, yet the flag decides.
        let provider =
            TokenProvider::new("stub", oauth_config(), dir.path()).with_clock(fixed(1_000));

        assert!(matches!(
            provider.access_token().await,
            TokenOutcome::NeedsLogin
        ));
    }

    #[tokio::test]
    async fn a_missing_file_is_not_logged_in() {
        let dir = TempDir::new().expect("tempdir");
        let provider = TokenProvider::new("stub", oauth_config(), dir.path());

        assert!(matches!(
            provider.access_token().await,
            TokenOutcome::NotLoggedIn
        ));
        assert!(!dir.path().join("auth").exists(), "nothing is created");
    }

    #[tokio::test]
    async fn a_corrupt_file_is_a_file_problem_naming_plugin_and_path() {
        let dir = TempDir::new().expect("tempdir");
        let path = dir.path().join("auth").join("stub.json");
        std::fs::create_dir_all(dir.path().join("auth")).expect("auth dir");
        std::fs::write(&path, "NOT-JSON-SENTINEL").expect("write the corrupt file");
        let provider = TokenProvider::new("stub", oauth_config(), dir.path());

        let TokenOutcome::FileProblem(error) = provider.access_token().await else {
            panic!("expected a file problem");
        };

        let text = error.to_string();
        assert!(text.contains("`stub`"), "{text}");
        assert!(text.contains(&path.display().to_string()), "{text}");
        assert!(!text.contains("NOT-JSON-SENTINEL"), "{text}");
        assert_eq!(
            std::fs::read(&path).expect("bytes"),
            b"NOT-JSON-SENTINEL",
            "the file is left as found"
        );
    }

    #[tokio::test]
    async fn a_token_with_the_margin_left_is_used() {
        let dir = TempDir::new().expect("tempdir");
        // Clock 1_000; the token expires at 4_600: 3600 seconds left.
        write_tokens(dir.path(), "stub", "AT-1", 4_600);
        let provider =
            TokenProvider::new("stub", oauth_config(), dir.path()).with_clock(fixed(1_000));

        match provider.access_token().await {
            TokenOutcome::Token(token) => assert_eq!(token.expose(), "AT-1"),
            other => panic!("expected a token, got {other:?}"),
        }
    }

    // ── The refresh path (D-07, D-08, D-09) ───────────────────────────────────

    use std::collections::VecDeque;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicI64, Ordering};
    use std::time::Duration;

    use crate::plugins::auth::store::RefreshToken;
    use crate::plugins::auth::testing::{form_pairs, oauth_config_for, token_body};
    use crate::plugins::host::testing::{Reply, TestHttpServer};

    /// A fake token endpoint that answers from a script, a temporary state
    /// directory and a clock the test owns.
    struct Rig {
        dir: TempDir,
        server: TestHttpServer,
        now: Arc<AtomicI64>,
    }

    impl Rig {
        async fn new(replies: Vec<Reply>) -> Self {
            let script = Arc::new(Mutex::new(VecDeque::from(replies)));
            let server = TestHttpServer::start(move |_| {
                script
                    .lock()
                    .expect("script")
                    .pop_front()
                    .unwrap_or_else(|| Reply::json(500, r#"{"error":"script exhausted"}"#))
            })
            .await;
            Self {
                dir: TempDir::new().expect("tempdir"),
                server,
                now: Arc::new(AtomicI64::new(1_000)),
            }
        }

        fn store(&self) -> TokenStore {
            TokenStore::new(self.dir.path(), "stub")
        }

        fn provider(&self) -> TokenProvider {
            let now = Arc::clone(&self.now);
            TokenProvider::new(
                "stub",
                oauth_config_for(&self.server.url("/token"), None),
                self.dir.path(),
            )
            .with_clock(Arc::new(move || now.load(Ordering::SeqCst)))
        }

        /// Stores a login; `refresh` is its refresh token.
        fn login(&self, access: &str, refresh: Option<&str>, expires_at: i64) {
            self.store()
                .write(&StoredTokens {
                    access_token: AccessToken::new(access.to_string()),
                    refresh_token: refresh.map(|r| RefreshToken::new(r.to_string())),
                    expires_at,
                    token_type: Some("Bearer".to_string()),
                    scope: Some("read".to_string()),
                    needs_login: false,
                    obtained_at: 1,
                })
                .expect("write the login");
        }

        fn stored(&self) -> StoredTokens {
            self.store().read().expect("read").expect("a login")
        }

        fn requests(&self) -> usize {
            self.server.seen().len()
        }

        fn file_bytes(&self) -> Vec<u8> {
            std::fs::read(self.dir.path().join("auth").join("stub.json")).expect("bytes")
        }
    }

    fn good(access: &str, refresh: Option<&str>, expires_in: Option<u64>) -> Reply {
        Reply::json(200, &token_body(access, refresh, expires_in))
    }

    /// The access token of a `Token` outcome; panics on any other outcome.
    fn token_text(outcome: TokenOutcome) -> String {
        match outcome {
            TokenOutcome::Token(token) => token.expose().to_string(),
            other => panic!("expected a token, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_token_with_60_seconds_left_is_not_refreshed() {
        let rig = Rig::new(vec![good("AT-NEW", None, Some(3600))]).await;
        rig.login("AT-OLD", Some("RT-OLD"), 1_000 + 60);

        assert_eq!(token_text(rig.provider().access_token().await), "AT-OLD");
        assert_eq!(rig.requests(), 0);
    }

    #[tokio::test]
    async fn a_token_with_59_seconds_left_is_refreshed_and_persisted() {
        let rig = Rig::new(vec![good("AT-NEW", None, Some(7200))]).await;
        rig.login("AT-OLD", Some("RT-OLD"), 1_000 + 59);

        assert_eq!(token_text(rig.provider().access_token().await), "AT-NEW");

        let stored = rig.stored();
        assert_eq!(stored.access_token.expose(), "AT-NEW");
        assert_eq!(stored.expires_at, 8_200);
        assert_eq!(
            stored.refresh_token.expect("kept").expose(),
            "RT-OLD",
            "the old refresh token is kept"
        );
        assert!(!stored.needs_login);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(rig.dir.path().join("auth").join("stub.json"))
                .expect("metadata")
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let seen = rig.server.seen();
        assert_eq!(seen.len(), 1);
        let form = form_pairs(&seen[0].body);
        assert!(form.contains(&("grant_type".to_string(), "refresh_token".to_string())));
        assert!(form.contains(&("refresh_token".to_string(), "RT-OLD".to_string())));
    }

    #[tokio::test]
    async fn an_expired_token_is_refreshed() {
        let rig = Rig::new(vec![good("AT-NEW", None, Some(7200))]).await;
        rig.login("AT-OLD", Some("RT-OLD"), 900);

        assert_eq!(token_text(rig.provider().access_token().await), "AT-NEW");
        assert_eq!(rig.stored().access_token.expose(), "AT-NEW");
        assert_eq!(rig.stored().expires_at, 8_200);
        assert_eq!(rig.requests(), 1);
    }

    #[tokio::test]
    async fn an_omitted_refresh_token_keeps_the_stored_one() {
        let rig = Rig::new(vec![good("AT-NEW", None, Some(3600))]).await;
        rig.login("AT-OLD", Some("RT-OLD"), 900);

        rig.provider().access_token().await;

        assert_eq!(
            rig.stored().refresh_token.expect("kept").expose(),
            "RT-OLD"
        );
    }

    #[tokio::test]
    async fn a_rotated_refresh_token_replaces_the_stored_one() {
        let rig = Rig::new(vec![good("AT-NEW", Some("RT-NEW"), Some(3600))]).await;
        rig.login("AT-OLD", Some("RT-OLD"), 900);

        rig.provider().access_token().await;

        assert_eq!(
            rig.stored().refresh_token.expect("rotated").expose(),
            "RT-NEW"
        );
    }

    #[tokio::test]
    async fn the_default_lifetime_is_3600_when_expires_in_is_absent() {
        let rig = Rig::new(vec![good("AT-NEW", None, None)]).await;
        rig.login("AT-OLD", Some("RT-OLD"), 900);

        rig.provider().access_token().await;

        assert_eq!(rig.stored().expires_at, 1_000 + 3_600);
    }

    #[tokio::test]
    async fn invalid_grant_marks_needs_login_and_keeps_the_file() {
        let rig = Rig::new(vec![Reply::json(400, r#"{"error":"invalid_grant"}"#)]).await;
        rig.login("AT-OLD", Some("RT-OLD"), 900);
        let before = rig.stored();
        let provider = rig.provider();

        assert!(matches!(
            provider.access_token().await,
            TokenOutcome::NeedsLogin
        ));

        let after = rig.stored();
        assert!(after.needs_login);
        assert_eq!(
            after,
            StoredTokens {
                needs_login: true,
                ..before
            }
        );
        assert!(matches!(
            provider.access_token().await,
            TokenOutcome::NeedsLogin
        ));
        assert_eq!(rig.requests(), 1, "no further request after the mark");
    }

    #[tokio::test]
    async fn a_transient_failure_with_a_valid_token_hands_the_old_token_and_does_not_mark_needs_login()
     {
        let rig = Rig::new(vec![Reply::status(503)]).await;
        rig.login("AT-OLD", Some("RT-OLD"), 1_000 + 30);

        assert_eq!(token_text(rig.provider().access_token().await), "AT-OLD");

        assert!(!rig.stored().needs_login);
        assert_eq!(rig.requests(), 1);
    }

    #[tokio::test]
    async fn a_transient_failure_with_an_expired_token_is_refresh_failed_without_marking() {
        let rig = Rig::new(vec![Reply::status(503)]).await;
        rig.login("AT-OLD", Some("RT-OLD"), 900);
        let before = rig.file_bytes();

        match rig.provider().access_token().await {
            TokenOutcome::RefreshFailed(text) => assert!(!text.is_empty()),
            other => panic!("expected a refresh failure, got {other:?}"),
        }

        assert_eq!(rig.requests(), 1);
        assert_eq!(rig.file_bytes(), before, "the file is byte-identical");
    }

    #[tokio::test]
    async fn a_rejected_failure_does_not_mark_needs_login() {
        let rig = Rig::new(vec![Reply::json(401, r#"{"error":"invalid_client"}"#)]).await;
        rig.login("AT-OLD", Some("RT-OLD"), 900);

        match rig.provider().access_token().await {
            TokenOutcome::RefreshFailed(text) => assert!(text.contains("invalid_client"), "{text}"),
            other => panic!("expected a refresh failure, got {other:?}"),
        }

        assert!(!rig.stored().needs_login);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_failure_to_save_the_refreshed_login_hands_out_no_token() {
        use std::os::unix::fs::PermissionsExt;

        let rig = Rig::new(vec![good("AT-NEW", Some("RT-NEW"), Some(3600))]).await;
        rig.login("AT-OLD", Some("RT-OLD"), 900);
        // Make the lock file exist first: only the write must fail.
        drop(rig.store().lock().await.expect("lock"));
        let auth_dir = rig.dir.path().join("auth");
        std::fs::set_permissions(&auth_dir, std::fs::Permissions::from_mode(0o500))
            .expect("chmod 500");
        if std::fs::File::create(auth_dir.join("probe")).is_ok() {
            // A process that ignores directory permissions (root) cannot
            // provoke the failure.
            std::fs::remove_file(auth_dir.join("probe")).expect("remove probe");
            std::fs::set_permissions(&auth_dir, std::fs::Permissions::from_mode(0o700))
                .expect("chmod 700");
            return;
        }

        let outcome = rig.provider().access_token().await;
        std::fs::set_permissions(&auth_dir, std::fs::Permissions::from_mode(0o700))
            .expect("chmod 700");

        match outcome {
            TokenOutcome::RefreshFailed(text) => {
                assert!(text.contains("could not be saved"), "{text}");
                assert!(!text.contains("AT-NEW"), "{text}");
                assert!(!text.contains("RT-NEW"), "{text}");
            }
            other => panic!("expected a refresh failure, got {other:?}"),
        }
        assert_eq!(rig.requests(), 1);
        assert_eq!(
            rig.stored().access_token.expose(),
            "AT-OLD",
            "the old login is untouched"
        );
    }

    #[tokio::test]
    async fn an_expired_token_without_a_refresh_token_marks_needs_login() {
        let rig = Rig::new(vec![]).await;
        rig.login("AT-OLD", None, 900);

        assert!(matches!(
            rig.provider().access_token().await,
            TokenOutcome::NeedsLogin
        ));

        assert!(rig.stored().needs_login);
        assert_eq!(rig.requests(), 0);
    }

    #[tokio::test]
    async fn a_token_under_the_margin_without_a_refresh_token_is_still_used_while_valid() {
        let rig = Rig::new(vec![]).await;
        rig.login("AT-OLD", None, 1_000 + 30);
        let before = rig.file_bytes();

        assert_eq!(token_text(rig.provider().access_token().await), "AT-OLD");

        assert_eq!(rig.requests(), 0);
        assert_eq!(rig.file_bytes(), before);
    }

    #[tokio::test]
    async fn a_login_flagged_needs_login_is_not_refreshed() {
        let rig = Rig::new(vec![good("AT-NEW", None, Some(3600))]).await;
        rig.store()
            .write(&StoredTokens {
                access_token: AccessToken::new("AT-OLD".to_string()),
                refresh_token: Some(RefreshToken::new("RT-OLD".to_string())),
                expires_at: 900,
                token_type: None,
                scope: None,
                needs_login: true,
                obtained_at: 1,
            })
            .expect("write");

        assert!(matches!(
            rig.provider().access_token().await,
            TokenOutcome::NeedsLogin
        ));
        assert_eq!(rig.requests(), 0);
    }

    #[tokio::test]
    async fn refresh_re_reads_after_taking_the_lock_and_skips_when_another_process_already_refreshed()
     {
        let rig = Rig::new(vec![good("AT-NEW", None, Some(3600))]).await;
        rig.login("AT-OLD", Some("RT-OLD"), 900);
        let held = rig.store().lock().await.expect("the test holds the lock");
        let provider = Arc::new(rig.provider());
        let task = {
            let provider = Arc::clone(&provider);
            tokio::spawn(async move { provider.access_token().await })
        };

        // The call decided the token is due and now waits for the lock.
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(!task.is_finished());
        rig.login("AT-OTHER", Some("RT-OTHER"), 1_000 + 3_600);
        drop(held);

        let outcome = task.await.expect("task");
        assert_eq!(token_text(outcome), "AT-OTHER");
        assert_eq!(rig.requests(), 0, "someone else already refreshed");
    }

    #[tokio::test]
    async fn a_login_during_a_refresh_is_not_overwritten() {
        let rig = Rig::new(vec![
            good("AT-NEW", Some("RT-NEW"), Some(3600)).delayed(Duration::from_millis(300)),
        ])
        .await;
        rig.login("AT-OLD", Some("RT-OLD"), 900);
        let provider = Arc::new(rig.provider());
        let refresh = {
            let provider = Arc::clone(&provider);
            tokio::spawn(async move { provider.access_token().await })
        };
        // Wait until the endpoint has the request, then log in meanwhile.
        for _ in 0..500 {
            if rig.requests() == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(rig.requests(), 1);
        let store = rig.store();
        let login = tokio::spawn(async move {
            let _lock = store.lock().await.expect("login lock");
            store
                .write(&StoredTokens {
                    access_token: AccessToken::new("AT-LOGIN".to_string()),
                    refresh_token: Some(RefreshToken::new("RT-LOGIN".to_string())),
                    expires_at: 1_000 + 3_600,
                    token_type: None,
                    scope: None,
                    needs_login: false,
                    obtained_at: 1_000,
                })
                .expect("login write");
        });

        assert_eq!(token_text(refresh.await.expect("refresh")), "AT-NEW");
        login.await.expect("login");

        let stored = rig.stored();
        assert_eq!(stored.access_token.expose(), "AT-LOGIN");
        assert_eq!(stored.refresh_token.expect("rt").expose(), "RT-LOGIN");
    }

    #[tokio::test]
    async fn two_concurrent_calls_make_one_refresh_request() {
        let rig = Rig::new(vec![
            good("AT-NEW", None, Some(3600)).delayed(Duration::from_millis(100)),
        ])
        .await;
        rig.login("AT-OLD", Some("RT-OLD"), 900);
        let provider = rig.provider();

        let (first, second) = tokio::join!(provider.access_token(), provider.access_token());

        assert_eq!(token_text(first), "AT-NEW");
        assert_eq!(token_text(second), "AT-NEW");
        assert_eq!(rig.requests(), 1);
    }

    #[tokio::test]
    async fn refresh_failure_text_never_contains_the_tokens() {
        let rig = Rig::new(vec![Reply::json(503, "BODY-SENTINEL AT-NEW RT-OLD")]).await;
        rig.login("AT-OLD", Some("RT-OLD"), 900);
        let provider = rig.provider();

        let outcome = provider.access_token().await;

        let text = match outcome {
            TokenOutcome::RefreshFailed(text) => text,
            other => panic!("expected a refresh failure, got {other:?}"),
        };
        for hidden in ["AT-OLD", "RT-OLD", "AT-NEW", "BODY-SENTINEL"] {
            assert!(!text.contains(hidden), "{text}");
            assert!(!format!("{provider:?}").contains(hidden));
        }
    }

    // ── The app-only token: the client_credentials grant (D-03) ───────────────

    use crate::plugins::auth::testing::client_credentials_config_for;
    use base64::Engine as _;

    impl Rig {
        /// A provider for the app-only grant against the rig's endpoint, on
        /// the rig's clock.
        fn app_provider(&self) -> TokenProvider {
            let now = Arc::clone(&self.now);
            TokenProvider::new(
                "stub",
                client_credentials_config_for(&self.server.url("/token"), "CS-SENTINEL"),
                self.dir.path(),
            )
            .with_clock(Arc::new(move || now.load(Ordering::SeqCst)))
        }

        fn set_clock(&self, now: i64) {
            self.now.store(now, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn client_credentials_fetches_a_token_on_first_use_and_caches_it_in_memory() {
        let rig = Rig::new(vec![
            good("AT-APP-1", None, Some(3600)),
            good("AT-APP-2", None, Some(3600)),
        ])
        .await;
        let provider = rig.app_provider();

        assert_eq!(token_text(provider.access_token().await), "AT-APP-1");
        assert_eq!(rig.requests(), 1);
        assert_eq!(token_text(provider.access_token().await), "AT-APP-1");
        assert_eq!(rig.requests(), 1, "the second call is served from memory");

        let seen = rig.server.seen();
        let expected = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode("client-1:CS-SENTINEL")
        );
        assert_eq!(seen[0].header("authorization"), Some(expected.as_str()));
        assert_eq!(
            form_pairs(&seen[0].body),
            vec![("grant_type".to_string(), "client_credentials".to_string())]
        );
    }

    #[tokio::test]
    async fn a_cached_app_token_with_exactly_the_margin_left_is_reused_and_one_second_less_refetches()
     {
        let rig = Rig::new(vec![
            good("AT-APP-1", None, Some(3600)),
            good("AT-APP-2", None, Some(3600)),
        ])
        .await;
        let provider = rig.app_provider();
        // Clock 1_000, expires_in 3_600: the token expires at 4_600.
        assert_eq!(token_text(provider.access_token().await), "AT-APP-1");

        rig.set_clock(4_600 - REFRESH_MARGIN_SECS);
        assert_eq!(token_text(provider.access_token().await), "AT-APP-1");
        assert_eq!(rig.requests(), 1, "60 seconds left: reused");

        rig.set_clock(4_600 - REFRESH_MARGIN_SECS + 1);
        assert_eq!(token_text(provider.access_token().await), "AT-APP-2");
        assert_eq!(rig.requests(), 2, "59 seconds left: fetched again");
    }

    #[tokio::test]
    async fn a_missing_expires_in_defaults_to_3600_seconds() {
        let rig = Rig::new(vec![
            good("AT-APP-1", None, None),
            good("AT-APP-2", None, None),
        ])
        .await;
        let provider = rig.app_provider();
        assert_eq!(token_text(provider.access_token().await), "AT-APP-1");

        rig.set_clock(1_000 + 3_540);
        assert_eq!(token_text(provider.access_token().await), "AT-APP-1");
        assert_eq!(rig.requests(), 1);

        rig.set_clock(1_000 + 3_541);
        assert_eq!(token_text(provider.access_token().await), "AT-APP-2");
        assert_eq!(rig.requests(), 2);
    }

    #[tokio::test]
    async fn two_concurrent_app_token_calls_make_one_request() {
        let rig = Rig::new(vec![
            good("AT-APP-1", None, Some(3600)).delayed(Duration::from_millis(100)),
        ])
        .await;
        let provider = rig.app_provider();

        let (first, second) = tokio::join!(provider.access_token(), provider.access_token());

        assert_eq!(token_text(first), "AT-APP-1");
        assert_eq!(token_text(second), "AT-APP-1");
        assert_eq!(rig.requests(), 1);
    }

    #[tokio::test]
    async fn a_failed_first_fetch_is_refresh_failed_with_cleaned_text() {
        let rig = Rig::new(vec![Reply::json(
            503,
            "BODY-SENTINEL AT-APP-1 CS-SENTINEL",
        )])
        .await;
        let provider = rig.app_provider();

        let text = match provider.access_token().await {
            TokenOutcome::RefreshFailed(text) => text,
            other => panic!("expected a refresh failure, got {other:?}"),
        };

        assert!(text.contains("app token"), "{text}");
        for hidden in ["BODY-SENTINEL", "AT-APP-1", "CS-SENTINEL"] {
            assert!(!text.contains(hidden), "{text}");
            assert!(!format!("{provider:?}").contains(hidden));
        }
        assert_eq!(rig.requests(), 1, "one request, no retry loop");
    }

    #[tokio::test]
    async fn a_rejected_client_is_refresh_failed_and_never_needs_login() {
        let rig = Rig::new(vec![
            Reply::json(401, r#"{"message":"Unauthorized","error":401}"#),
            Reply::json(200, r#"{"error":"unsupported_grant_type"}"#),
        ])
        .await;
        let provider = rig.app_provider();

        match provider.access_token().await {
            TokenOutcome::RefreshFailed(text) => assert!(text.contains("HTTP 401"), "{text}"),
            other => panic!("expected a refresh failure, got {other:?}"),
        }
        match provider.access_token().await {
            TokenOutcome::RefreshFailed(text) => {
                assert!(text.contains("unsupported_grant_type"), "{text}");
            }
            other => panic!("expected a refresh failure, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_failed_refetch_keeps_using_a_still_valid_cached_token() {
        let rig = Rig::new(vec![
            good("AT-APP-1", None, Some(100)),
            Reply::status(503),
            Reply::status(503),
        ])
        .await;
        let provider = rig.app_provider();
        // Clock 1_000, expires_in 100: expires at 1_100.
        assert_eq!(token_text(provider.access_token().await), "AT-APP-1");

        // 50 seconds left: inside the margin, so a refetch is tried; it fails
        // and the token that is still valid keeps being used.
        rig.set_clock(1_000 + 50);
        assert_eq!(token_text(provider.access_token().await), "AT-APP-1");
        assert_eq!(rig.requests(), 2);

        // Past its expiry nothing is left to use.
        rig.set_clock(1_000 + 101);
        assert!(matches!(
            provider.access_token().await,
            TokenOutcome::RefreshFailed(_)
        ));
        assert_eq!(rig.requests(), 3);
    }

    #[tokio::test]
    async fn the_app_token_is_never_written_to_disk() {
        let rig = Rig::new(vec![good("AT-APP-1", None, Some(3600)), Reply::status(503)]).await;
        let provider = rig.app_provider();
        assert_eq!(token_text(provider.access_token().await), "AT-APP-1");
        assert!(!rig.dir.path().join("auth").exists(), "after a success");

        rig.set_clock(1_000 + 3_600);
        assert!(matches!(
            provider.access_token().await,
            TokenOutcome::RefreshFailed(_)
        ));
        assert!(!rig.dir.path().join("auth").exists(), "after a failure");
    }

    #[tokio::test]
    async fn a_stale_token_file_for_the_same_alias_is_ignored() {
        let rig = Rig::new(vec![good("AT-APP-1", None, Some(3600))]).await;
        rig.login("AT-STALE-LOGIN", Some("RT-STALE-LOGIN"), 1_000 + 7_200);
        let before = rig.file_bytes();
        let provider = rig.app_provider();

        assert_eq!(token_text(provider.access_token().await), "AT-APP-1");

        assert_eq!(rig.requests(), 1);
        assert_eq!(rig.file_bytes(), before, "the login file is never touched");
    }

    #[tokio::test]
    async fn a_new_provider_fetches_again_because_nothing_is_persisted() {
        let rig = Rig::new(vec![
            good("AT-APP-1", None, Some(3600)),
            good("AT-APP-2", None, Some(3600)),
        ])
        .await;
        assert_eq!(token_text(rig.app_provider().access_token().await), "AT-APP-1");
        assert_eq!(token_text(rig.app_provider().access_token().await), "AT-APP-2");
        assert_eq!(rig.requests(), 2);
    }
}
