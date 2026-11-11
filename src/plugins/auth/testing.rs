// ─────────────────────────────────────────────────────────────────────────────
// plugins/auth/testing.rs - Shared test helpers (cfg(test) only)
// ─────────────────────────────────────────────────────────────────────────────

use std::path::Path;
use std::sync::Arc;

use super::provider::TokenProvider;
use super::store::{AccessToken, StoredTokens, TokenStore};
use crate::plugins::config::{OAuthConfig, OAuthGrant};
use crate::plugins::secrets::Secret;

/// An OAuth config over example https endpoints.
pub(crate) fn oauth_config() -> OAuthConfig {
    OAuthConfig {
        grant: OAuthGrant::AuthorizationCode,
        authorize_url: Some(url::Url::parse("https://auth.example/authorize").expect("url")),
        token_url: url::Url::parse("https://auth.example/token").expect("url"),
        scopes: vec!["read".to_string()],
        client_id: "client-1".to_string(),
        client_secret: None,
        revocation_url: None,
        redirect_port: 0,
        authorize_params: vec![],
        user_agent: None,
    }
}

/// `oauth_config()` with `token_url` set to `token_url` (a loopback test
/// server) and, when given, a client secret.
pub(crate) fn oauth_config_for(token_url: &str, secret: Option<&str>) -> OAuthConfig {
    OAuthConfig {
        token_url: url::Url::parse(token_url).expect("token url"),
        client_secret: secret.map(Secret::for_test),
        ..oauth_config()
    }
}

/// An app-only (`client_credentials`) config whose token endpoint is
/// `token_url` (a loopback test server) and whose client secret is `secret`.
pub(crate) fn client_credentials_config_for(token_url: &str, secret: &str) -> OAuthConfig {
    OAuthConfig {
        grant: OAuthGrant::ClientCredentials,
        authorize_url: None,
        token_url: url::Url::parse(token_url).expect("token url"),
        client_secret: Some(Secret::for_test(secret)),
        ..oauth_config()
    }
}

/// A token endpoint answer as JSON text.
pub(crate) fn token_body(access: &str, refresh: Option<&str>, expires_in: Option<u64>) -> String {
    let mut body = serde_json::json!({ "access_token": access });
    if let Some(refresh) = refresh {
        body["refresh_token"] = serde_json::json!(refresh);
    }
    if let Some(expires_in) = expires_in {
        body["expires_in"] = serde_json::json!(expires_in);
    }
    body.to_string()
}

/// The key and value pairs of a form-encoded request body.
pub(crate) fn form_pairs(body: &[u8]) -> Vec<(String, String)> {
    url::form_urlencoded::parse(body)
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect()
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
