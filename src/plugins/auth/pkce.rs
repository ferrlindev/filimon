// ─────────────────────────────────────────────────────────────────────────────
// plugins/auth/pkce.rs - PKCE verifier, challenge, state and the authorize URL
//
// The login is an authorization-code flow with PKCE (RFC 7636) using the S256
// method only; `plain` is never used. The verifier and the `state` are each
// 32 bytes from the operating system's CSPRNG, written as 43 URL-safe base64
// characters without padding (inside RFC 7636's 43 to 128 range).
//
// - `Verifier`: the secret half of the pair; it prints `***` in Debug.
// - `generate_verifier`, `generate_state`: CSPRNG draws.
// - `s256_challenge`: base64url(SHA-256(verifier)).
// - `build_authorize_url`: the host-built URL, with the plugin's extra
//   `authorize_params` after the standard ones (D-14).
//
// Nothing in this module logs; a verifier or a state never leaves it except
// through `Verifier::expose` and the returned `state` string.
// ─────────────────────────────────────────────────────────────────────────────

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use sha2::{Digest, Sha256};

use super::error::AuthError;
use crate::plugins::config::OAuthConfig;
use crate::plugins::host::engine::clean_guest_text;

/// Random bytes behind a verifier and a state: 32 bytes is 43 base64url
/// characters.
const RANDOM_BYTES: usize = 32;

/// A PKCE code verifier. Debug prints `***`, there is no `Display`, and the
/// text is only reachable through `expose`.
#[derive(Clone, PartialEq, Eq)]
pub struct Verifier(String);

impl Verifier {
    /// Wraps verifier text.
    #[must_use]
    pub fn new(value: String) -> Self {
        Self(value)
    }

    /// The verifier text, for the token request.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Verifier {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Verifier(***)")
    }
}

/// 32 CSPRNG bytes as 43 URL-safe base64 characters without padding.
fn random_token() -> Result<String, AuthError> {
    let mut bytes = [0_u8; RANDOM_BYTES];
    getrandom::fill(&mut bytes).map_err(|e| AuthError::Random {
        reason: clean_guest_text(&e.to_string(), 200),
    })?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// A fresh PKCE code verifier.
///
/// # Errors
///
/// [`AuthError::Random`] when the operating system cannot supply random bytes.
pub fn generate_verifier() -> Result<Verifier, AuthError> {
    random_token().map(Verifier::new)
}

/// A fresh `state` value for one login.
///
/// # Errors
///
/// [`AuthError::Random`] when the operating system cannot supply random bytes.
pub fn generate_state() -> Result<String, AuthError> {
    random_token()
}

/// The S256 challenge of `verifier`: base64url(SHA-256(verifier)).
#[must_use]
pub fn s256_challenge(verifier: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()))
}

/// The provider's authorize URL for one login.
///
/// Any query already on the endpoint is kept. The standard parameters come
/// first, in a fixed order, then the plugin's `authorize_params` in the order
/// stored (config loading guarantees none collides with a standard key).
///
/// `None` when the config has no authorize URL, which is the case for the
/// `client_credentials` grant.
#[must_use]
pub fn build_authorize_url(
    oauth: &OAuthConfig,
    redirect_uri: &str,
    state: &str,
    challenge: &str,
) -> Option<url::Url> {
    let mut url = oauth.authorize_url.clone()?;
    {
        let mut query = url.query_pairs_mut();
        query.append_pair("response_type", "code");
        query.append_pair("client_id", &oauth.client_id);
        query.append_pair("redirect_uri", redirect_uri);
        if !oauth.scopes.is_empty() {
            query.append_pair("scope", &oauth.scopes.join(" "));
        }
        query.append_pair("state", state);
        query.append_pair("code_challenge", challenge);
        query.append_pair("code_challenge_method", "S256");
        for (key, value) in &oauth.authorize_params {
            query.append_pair(key, value);
        }
    }
    Some(url)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::auth::testing::oauth_config;

    const REDIRECT: &str = "http://127.0.0.1:17420/callback";

    fn pairs(url: &url::Url) -> Vec<(String, String)> {
        url.query_pairs()
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect()
    }

    fn url_safe(text: &str) -> bool {
        text.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    }

    #[test]
    fn rfc7636_appendix_b_vector() {
        assert_eq!(
            s256_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
    }

    #[test]
    fn verifier_is_43_url_safe_characters() {
        let verifier = generate_verifier().expect("verifier");
        assert_eq!(verifier.expose().len(), 43);
        assert!(url_safe(verifier.expose()));
    }

    #[test]
    fn state_is_43_url_safe_characters() {
        let state = generate_state().expect("state");
        assert_eq!(state.len(), 43);
        assert!(url_safe(&state));
    }

    #[test]
    fn two_verifiers_differ() {
        let a = generate_verifier().expect("verifier");
        let b = generate_verifier().expect("verifier");
        assert_ne!(a, b);
    }

    #[test]
    fn verifier_never_prints_its_value() {
        let verifier = Verifier::new("secret-verifier-text".to_string());
        let debug = format!("{verifier:?}");
        assert!(debug.contains("***"));
        assert!(!debug.contains("secret-verifier-text"));
    }

    #[test]
    fn authorize_url_carries_every_standard_parameter() {
        let mut oauth = oauth_config();
        oauth.scopes = vec!["read".to_string(), "identity".to_string()];
        let url = build_authorize_url(&oauth, REDIRECT, "S", "C").expect("an authorize url");
        let got = pairs(&url);
        for expected in [
            ("response_type", "code"),
            ("client_id", "client-1"),
            ("redirect_uri", REDIRECT),
            ("scope", "read identity"),
            ("state", "S"),
            ("code_challenge", "C"),
            ("code_challenge_method", "S256"),
        ] {
            assert!(
                got.contains(&(expected.0.to_string(), expected.1.to_string())),
                "missing {expected:?} in {got:?}"
            );
        }
        let raw = url.as_str();
        assert!(raw.contains("scope=read+identity") || raw.contains("scope=read%20identity"));
        assert!(raw.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A17420%2Fcallback"));
    }

    #[test]
    fn authorize_url_appends_authorize_params_after_the_standard_ones() {
        let mut oauth = oauth_config();
        oauth.authorize_params = vec![("duration".to_string(), "permanent".to_string())];
        let url = build_authorize_url(&oauth, REDIRECT, "S", "C").expect("an authorize url");
        let got = pairs(&url);
        assert_eq!(
            got.last(),
            Some(&("duration".to_string(), "permanent".to_string()))
        );
        let method_at = got
            .iter()
            .position(|(k, _)| k == "code_challenge_method")
            .expect("method");
        assert_eq!(method_at, got.len() - 2);
    }

    #[test]
    fn authorize_url_keeps_an_existing_query_of_the_endpoint() {
        let mut oauth = oauth_config();
        oauth.authorize_url =
            Some(url::Url::parse("https://auth.example/authorize?tenant=t1").expect("url"));
        let url = build_authorize_url(&oauth, REDIRECT, "S", "C").expect("an authorize url");
        let got = pairs(&url);
        assert_eq!(got.first(), Some(&("tenant".to_string(), "t1".to_string())));
        assert!(got.contains(&("state".to_string(), "S".to_string())));
    }

    #[test]
    fn authorize_url_omits_scope_when_there_are_no_scopes() {
        let mut oauth = oauth_config();
        oauth.scopes = vec![];
        let url = build_authorize_url(&oauth, REDIRECT, "S", "C").expect("an authorize url");
        assert!(pairs(&url).iter().all(|(k, _)| k != "scope"));
    }
}
