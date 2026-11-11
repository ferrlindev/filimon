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
//   under 60 s of life left      -> RefreshDue    (the refresh seam)
//   file unusable                -> FileProblem   (disables only that plugin)
//
// Expiry is judged by an injectable wall-clock function. Tokio's paused time
// does not move the wall clock, so a test that pauses time can neither make a
// token look fresh nor stale by accident.
// ─────────────────────────────────────────────────────────────────────────────

use std::path::Path;
use std::sync::Arc;

use super::error::AuthError;
use super::store::{AccessToken, TokenStore};
use crate::plugins::config::OAuthConfig;

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
    /// The token has less than `REFRESH_MARGIN_SECS` left. This is the refresh
    /// seam: the refresh itself fills this branch in a later plan.
    RefreshDue,
    /// The token file cannot be used.
    FileProblem(AuthError),
}

/// Reads one plugin's stored login and decides what its poll receives.
pub struct TokenProvider {
    plugin: String,
    oauth: OAuthConfig,
    store: TokenStore,
    clock: Clock,
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
        }
    }

    /// The same provider on `clock`, so a test decides what time it is.
    #[cfg(test)]
    #[must_use]
    pub(crate) fn with_clock(mut self, clock: Clock) -> Self {
        self.clock = clock;
        self
    }

    /// Re-reads the token file and decides what the next poll receives.
    #[allow(
        clippy::unused_async,
        reason = "the refresh path of plan 06-06 awaits the token endpoint here"
    )]
    pub async fn access_token(&self) -> TokenOutcome {
        let tokens = match self.store.read() {
            Ok(Some(tokens)) => tokens,
            Ok(None) => return TokenOutcome::NotLoggedIn,
            Err(error) => return TokenOutcome::FileProblem(error),
        };
        if tokens.needs_login {
            return TokenOutcome::NeedsLogin;
        }
        let left = tokens.expires_at.saturating_sub((self.clock)());
        if left >= REFRESH_MARGIN_SECS {
            TokenOutcome::Token(tokens.access_token)
        } else {
            TokenOutcome::RefreshDue
        }
    }
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
    async fn a_token_one_second_under_the_margin_is_refresh_due() {
        // Clock 1_000; expires at 1_059: 59 seconds left.
        let (_dir, provider) = provider(1_000, 1_000 + REFRESH_MARGIN_SECS - 1);

        assert!(matches!(
            provider.access_token().await,
            TokenOutcome::RefreshDue
        ));
    }

    #[tokio::test]
    async fn the_injected_clock_decides_expiry() {
        let dir = TempDir::new().expect("tempdir");
        write_tokens(dir.path(), "stub", "AT-1", 10_000);

        // Clock 1_000: 9_000 seconds left.
        let early = TokenProvider::new("stub", oauth_config(), dir.path()).with_clock(fixed(1_000));
        assert!(matches!(early.access_token().await, TokenOutcome::Token(_)));

        // Clock 9_990: 10 seconds left. Same file, no sleeping.
        let late = TokenProvider::new("stub", oauth_config(), dir.path()).with_clock(fixed(9_990));
        assert!(matches!(
            late.access_token().await,
            TokenOutcome::RefreshDue
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
}
