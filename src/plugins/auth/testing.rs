// ─────────────────────────────────────────────────────────────────────────────
// plugins/auth/testing.rs - Shared test helpers (cfg(test) only)
// ─────────────────────────────────────────────────────────────────────────────

use std::path::Path;
use std::sync::Arc;

use super::provider::TokenProvider;
use super::store::{AccessToken, StoredTokens, TokenStore};
use crate::plugins::config::OAuthConfig;

/// An OAuth config over example https endpoints.
pub(crate) fn oauth_config() -> OAuthConfig {
    OAuthConfig {
        authorize_url: url::Url::parse("https://auth.example/authorize").expect("url"),
        token_url: url::Url::parse("https://auth.example/token").expect("url"),
        scopes: vec!["read".to_string()],
        client_id: "client-1".to_string(),
        client_secret: None,
        revocation_url: None,
        redirect_port: 0,
        authorize_params: vec![],
    }
}

/// The real wall clock, as Unix seconds.
pub(crate) fn unix_now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// Stores a login with `access` and no refresh token, expiring at `expires_at`.
pub(crate) fn write_tokens(state_dir: &Path, alias: &str, access: &str, expires_at: i64) {
    TokenStore::new(state_dir, alias)
        .write(&StoredTokens {
            access_token: AccessToken::new(access.to_string()),
            refresh_token: None,
            expires_at,
            token_type: None,
            scope: None,
            needs_login: false,
            obtained_at: expires_at - 3600,
        })
        .expect("write the token file");
}

/// A provider for `alias` over `state_dir` on the system clock.
pub(crate) fn provider_for(state_dir: &Path, alias: &str) -> Arc<TokenProvider> {
    Arc::new(TokenProvider::new(alias, oauth_config(), state_dir))
}
