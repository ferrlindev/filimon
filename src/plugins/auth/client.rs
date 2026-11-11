// ─────────────────────────────────────────────────────────────────────────────
// plugins/auth/client.rs - The host's token endpoint client
//
// Three form POSTs on the async `reqwest::Client`: exchange an authorization
// code (with its PKCE verifier), refresh an access token and revoke a token.
// It is the host's own client, outside the plugin fetch allowlist and its
// egress policy, so a plugin's allowlist need not list login hosts.
//
// Hardening (D-13): certificate validation stays at the library default with
// no override, redirects are never followed (a redirect would re-send the
// client secret to a third party), the request times out after 30 s (10 s to
// connect) and the answer is capped at 64 KiB while it is read. Only the
// async client is used: the blocking one panics inside the tokio runtime
// `fili` runs in.
//
// Text safety (D-06): provider text is attacker-influenced. Only the OAuth
// `error` code and `error_description`, each cleaned and clipped, are ever
// shown. A body, a request url, a code, a verifier, a token or a secret never
// appears in an error.
// ─────────────────────────────────────────────────────────────────────────────

use std::error::Error as _;
use std::time::Duration;

use super::callback::AuthCode;
use super::error::AuthError;
use super::pkce::Verifier;
use super::store::RefreshToken;
use crate::plugins::config::OAuthConfig;
use crate::plugins::host::engine::clean_guest_text;

/// The lifetime a caller assumes when an answer carries no `expires_in`.
pub const DEFAULT_EXPIRES_IN_SECS: u64 = 3600;

/// The whole request, connection to last body byte.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// Establishing the connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

/// The most response body bytes read before the answer is refused.
const MAX_RESPONSE_BYTES: usize = 64 * 1024;

/// Longest provider-supplied error text kept, in characters.
const ERROR_TEXT_CLIP: usize = 160;

/// Longest transport error text kept, in characters.
const TRANSPORT_TEXT_CLIP: usize = 240;

/// What a successful token answer holds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TokenSet {
    /// The access token. Redacted in no way here: callers wrap it in an
    /// `AccessToken` straight away.
    pub access_token: String,
    /// A new refresh token. `None` when the answer carried none: a refresh
    /// then keeps the stored one.
    pub refresh_token: Option<String>,
    /// The lifetime in seconds; `None` when the answer carried none, for the
    /// caller to default to [`DEFAULT_EXPIRES_IN_SECS`].
    pub expires_in: Option<u64>,
    pub scope: Option<String>,
    pub token_type: Option<String>,
}

/// Which kind of token a revocation names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenKind {
    Access,
    Refresh,
}

/// Why a refresh did not produce a token (D-09).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RefreshFailure {
    /// The refresh token is no longer valid (`invalid_grant`): the user must
    /// log in again.
    Permanent(String),
    /// A network, server or unreadable-answer problem: retry later and keep
    /// the login.
    Transient(String),
    /// The provider refused for a reason that is not the user's login (a bad
    /// client, a bad scope, a redirect): surfaced, but it never marks the
    /// login as needing renewal.
    Rejected(String),
}

impl RefreshFailure {
    /// The cleaned description: an OAuth code, `HTTP <status>` or a transport
    /// description, never a body.
    #[must_use]
    pub fn text(&self) -> &str {
        match self {
            Self::Permanent(text) | Self::Transient(text) | Self::Rejected(text) => text,
        }
    }
}

/// The host's client for one provider's token and revocation endpoints.
#[derive(Debug)]
pub struct TokenClient {
    http: reqwest::Client,
    oauth: OAuthConfig,
}

impl TokenClient {
    /// A client for `oauth`'s endpoints with the hardening described in the
    /// module header.
    ///
    /// # Errors
    /// [`AuthError::TokenRequest`] when the HTTP client cannot be built.
    pub fn new(oauth: &OAuthConfig) -> Result<Self, AuthError> {
        Self::build(oauth, REQUEST_TIMEOUT)
    }

    /// Like [`TokenClient::new`] with a shorter overall timeout.
    #[cfg(test)]
    pub(crate) fn with_timeout(oauth: &OAuthConfig, timeout: Duration) -> Result<Self, AuthError> {
        Self::build(oauth, timeout)
    }

    fn build(oauth: &OAuthConfig, timeout: Duration) -> Result<Self, AuthError> {
        let http = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(timeout)
            .connect_timeout(CONNECT_TIMEOUT)
            .user_agent(concat!("fili/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|error| AuthError::TokenRequest {
                reason: describe_transport_error(error),
            })?;
        Ok(Self {
            http,
            oauth: oauth.clone(),
        })
    }

    /// Exchanges an authorization code and its PKCE verifier for tokens.
    ///
    /// # Errors
    /// `TokenRequest` for a transport failure, `TokenResponse` for an answer
    /// that is too large or has no usable access token, and `TokenRejected`
    /// for a non-success status.
    pub async fn exchange_code(
        &self,
        code: &AuthCode,
        verifier: &Verifier,
        redirect_uri: &str,
    ) -> Result<TokenSet, AuthError> {
        let form = vec![
            ("grant_type", "authorization_code"),
            ("code", code.expose()),
            ("redirect_uri", redirect_uri),
            ("code_verifier", verifier.expose()),
        ];
        let (status, body) = match self.post_form(&self.oauth.token_url, form).await {
            Ok(answer) => answer,
            Err(SendFailure::Transport(reason)) => return Err(AuthError::TokenRequest { reason }),
            Err(SendFailure::TooLarge) => {
                return Err(AuthError::TokenResponse {
                    reason: too_large_text(),
                });
            }
        };
        if !status.is_success() {
            return Err(AuthError::TokenRejected {
                error: parse_oauth_error(&body)
                    .unwrap_or_else(|| format!("HTTP {}", status.as_u16())),
            });
        }
        parse_token_set(&body).map_err(|reason| AuthError::TokenResponse {
            reason: reason.to_string(),
        })
    }

    /// Refreshes an access token with `refresh_token`.
    ///
    /// # Errors
    /// A [`RefreshFailure`] classifying why no token was obtained.
    pub async fn refresh(&self, refresh_token: &RefreshToken) -> Result<TokenSet, RefreshFailure> {
        let _ = refresh_token;
        Err(RefreshFailure::Transient("not implemented".to_string()))
    }

    /// Revokes `token` at the provider's revocation endpoint. `Ok(false)` when
    /// no `revocation_url` is configured (no request is made).
    ///
    /// # Errors
    /// `RevokeFailed` for a transport failure or a non-2xx status. The
    /// provider's answer text is not carried, only the status.
    pub async fn revoke(&self, token: &str, kind: TokenKind) -> Result<bool, AuthError> {
        let Some(url) = &self.oauth.revocation_url else {
            return Ok(false);
        };
        let hint = match kind {
            TokenKind::Access => "access_token",
            TokenKind::Refresh => "refresh_token",
        };
        match self
            .post_form(url, vec![("token", token), ("token_type_hint", hint)])
            .await
        {
            Ok((status, _)) if status.is_success() => Ok(true),
            Ok((status, _)) => Err(AuthError::RevokeFailed {
                reason: format!("HTTP {}", status.as_u16()),
            }),
            Err(SendFailure::Transport(reason)) => Err(AuthError::RevokeFailed { reason }),
            Err(SendFailure::TooLarge) => Err(AuthError::RevokeFailed {
                reason: too_large_text(),
            }),
        }
    }

    /// Posts `form` to `url` with the client's authentication and reads the
    /// answer, never more than [`MAX_RESPONSE_BYTES`] of it.
    ///
    /// A client with a secret authenticates with HTTP Basic and the secret
    /// stays out of the form; a public client sends `client_id` in the form.
    async fn post_form<'a>(
        &'a self,
        url: &url::Url,
        mut form: Vec<(&'a str, &'a str)>,
    ) -> Result<(reqwest::StatusCode, Vec<u8>), SendFailure> {
        let mut request = self
            .http
            .post(url.clone())
            .header(reqwest::header::ACCEPT, "application/json");
        if let Some(secret) = &self.oauth.client_secret {
            request = request.basic_auth(&self.oauth.client_id, Some(secret.expose()));
        } else {
            form.push(("client_id", self.oauth.client_id.as_str()));
        }
        // One `.form` call: a second one replaces the body.
        let mut response = request
            .form(&form)
            .send()
            .await
            .map_err(|error| SendFailure::Transport(describe_transport_error(error)))?;
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|length| length > MAX_RESPONSE_BYTES as u64)
        {
            return Err(SendFailure::TooLarge);
        }
        let mut body = Vec::new();
        loop {
            match response.chunk().await {
                Ok(Some(chunk)) => {
                    if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
                        return Err(SendFailure::TooLarge);
                    }
                    body.extend_from_slice(&chunk);
                }
                Ok(None) => break,
                Err(error) => {
                    return Err(SendFailure::Transport(describe_transport_error(error)));
                }
            }
        }
        Ok((status, body))
    }
}

/// Why a request produced no answer.
enum SendFailure {
    /// A connection, DNS, TLS, timeout or body-read failure, as cleaned text.
    Transport(String),
    /// The answer is larger than [`MAX_RESPONSE_BYTES`].
    TooLarge,
}

fn too_large_text() -> String {
    format!("answer is larger than {MAX_RESPONSE_BYTES} bytes")
}

/// Reads a success answer. The `Err` text is fixed and carries nothing from
/// the body.
fn parse_token_set(body: &[u8]) -> Result<TokenSet, &'static str> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| "the answer is not JSON")?;
    let text = |key: &str| {
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    };
    let access_token = text("access_token").ok_or("no access_token in the answer")?;
    Ok(TokenSet {
        access_token,
        refresh_token: text("refresh_token"),
        expires_in: value.get("expires_in").and_then(parse_expires_in),
        scope: text("scope"),
        token_type: text("token_type"),
    })
}

/// `expires_in` as a JSON number (truncated to whole seconds) or a numeric
/// string; anything else, and a negative number, is `None`.
fn parse_expires_in(value: &serde_json::Value) -> Option<u64> {
    match value {
        serde_json::Value::Number(number) => number.as_u64().or_else(|| {
            number
                .as_f64()
                .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
                .map(|seconds| {
                    #[allow(
                        clippy::cast_possible_truncation,
                        clippy::cast_sign_loss,
                        reason = "checked finite and non-negative; the cast saturates"
                    )]
                    let whole = seconds.trunc() as u64;
                    whole
                })
        }),
        serde_json::Value::String(text) => text.trim().parse().ok(),
        _ => None,
    }
}

/// The OAuth error of a JSON error body as `error` or `error: description`,
/// each part cleaned and clipped. `None` when the body is not a JSON object
/// with a string `error`; a body that is not JSON is never echoed.
fn parse_oauth_error(body: &[u8]) -> Option<String> {
    let value: serde_json::Value = serde_json::from_slice(body).ok()?;
    let code = value.get("error")?.as_str()?;
    let code = clean_guest_text(code, ERROR_TEXT_CLIP);
    if code.is_empty() {
        return None;
    }
    let description = value
        .get("error_description")
        .and_then(serde_json::Value::as_str)
        .map(|text| clean_guest_text(text, ERROR_TEXT_CLIP))
        .filter(|text| !text.is_empty());
    Some(match description {
        Some(description) => format!("{code}: {description}"),
        None => code,
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::auth::testing::{form_pairs, oauth_config_for, token_body};
    use crate::plugins::host::testing::{Reply, SeenRequest, TestHttpServer};
    use base64::Engine as _;

    const REDIRECT: &str = "http://127.0.0.1:17420/callback";
    const BODY_SENTINEL: &str = "SECRET-BODY-SENTINEL";

    fn code() -> AuthCode {
        AuthCode::for_test("CODE-1")
    }

    fn verifier() -> Verifier {
        Verifier::new("VERIFIER-1".to_string())
    }

    async fn serve(reply: Reply) -> TestHttpServer {
        TestHttpServer::start(move |_| reply.clone()).await
    }

    fn has_pair(pairs: &[(String, String)], key: &str, value: &str) -> bool {
        pairs.iter().any(|(k, v)| k == key && v == value)
    }

    fn basic(user: &str, password: &str) -> String {
        format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("{user}:{password}"))
        )
    }

    /// Runs one code exchange against a server answering `reply`.
    async fn exchange(
        reply: Reply,
        secret: Option<&str>,
    ) -> (Result<TokenSet, AuthError>, Vec<SeenRequest>) {
        let server = serve(reply).await;
        let config = oauth_config_for(&server.url("/token"), secret);
        let client = TokenClient::new(&config).expect("client");
        let result = client.exchange_code(&code(), &verifier(), REDIRECT).await;
        (result, server.seen())
    }

    /// Every text an error shows: `Display` and `Debug`.
    fn texts(error: &AuthError) -> String {
        format!("{error} {error:?}")
    }

    fn assert_no_credentials(text: &str) {
        for secret in ["CODE-1", "VERIFIER-1", "CS-SENTINEL", BODY_SENTINEL] {
            assert!(!text.contains(secret), "`{secret}` leaked into: {text}");
        }
        assert!(!text.contains('\u{1b}'), "escape character in: {text}");
    }

    #[tokio::test]
    async fn exchange_sends_the_pkce_form_fields() {
        let (result, seen) = exchange(
            Reply::json(200, &token_body("AT-1", None, Some(3600))),
            None,
        )
        .await;
        result.expect("exchange");
        assert_eq!(seen.len(), 1);
        let request = &seen[0];
        assert_eq!(request.method, "POST");
        let pairs = form_pairs(&request.body);
        assert!(
            has_pair(&pairs, "grant_type", "authorization_code"),
            "{pairs:?}"
        );
        assert!(has_pair(&pairs, "code", "CODE-1"), "{pairs:?}");
        assert!(has_pair(&pairs, "redirect_uri", REDIRECT), "{pairs:?}");
        assert!(has_pair(&pairs, "code_verifier", "VERIFIER-1"), "{pairs:?}");
        assert!(has_pair(&pairs, "client_id", "client-1"), "{pairs:?}");
        assert_eq!(request.header("authorization"), None);
    }

    #[tokio::test]
    async fn exchange_with_a_secret_uses_basic_auth_and_no_body_secret() {
        let (result, seen) = exchange(
            Reply::json(200, &token_body("AT-1", None, Some(3600))),
            Some("CS-SENTINEL"),
        )
        .await;
        result.expect("exchange");
        let request = &seen[0];
        assert_eq!(
            request.header("authorization"),
            Some(basic("client-1", "CS-SENTINEL").as_str())
        );
        let body = String::from_utf8_lossy(&request.body).into_owned();
        assert!(!body.contains("CS-SENTINEL"), "{body}");
        assert!(
            form_pairs(&request.body)
                .iter()
                .all(|(k, _)| k != "client_secret"),
            "{body}"
        );
    }

    #[tokio::test]
    async fn exchange_parses_numeric_and_string_expires_in() {
        let (numeric, _) = exchange(
            Reply::json(200, r#"{"access_token":"AT-1","expires_in":3600}"#),
            None,
        )
        .await;
        assert_eq!(numeric.expect("numeric").expires_in, Some(3600));
        let (string, _) = exchange(
            Reply::json(200, r#"{"access_token":"AT-1","expires_in":"3600"}"#),
            None,
        )
        .await;
        assert_eq!(string.expect("string").expires_in, Some(3600));
    }

    #[tokio::test]
    async fn exchange_without_expires_in_leaves_it_none() {
        let (bare, _) = exchange(Reply::json(200, r#"{"access_token":"AT-1"}"#), None).await;
        let bare = bare.expect("bare");
        assert_eq!(bare.expires_in, None);
        assert_eq!(bare.refresh_token, None);
        let (full, _) = exchange(
            Reply::json(
                200,
                r#"{"access_token":"AT-1","refresh_token":"RT-1","scope":"read write","token_type":"Bearer"}"#,
            ),
            None,
        )
        .await;
        let full = full.expect("full");
        assert_eq!(full.access_token, "AT-1");
        assert_eq!(full.refresh_token.as_deref(), Some("RT-1"));
        assert_eq!(full.scope.as_deref(), Some("read write"));
        assert_eq!(full.token_type.as_deref(), Some("Bearer"));
        assert_eq!(full.expires_in, None);
    }

    #[tokio::test]
    async fn an_access_token_that_is_missing_or_empty_is_a_response_error() {
        for body in ["{}", r#"{"access_token":""}"#] {
            let (result, _) = exchange(Reply::json(200, body), None).await;
            assert!(
                matches!(result, Err(AuthError::TokenResponse { .. })),
                "{body}: {result:?}"
            );
        }
    }

    #[tokio::test]
    async fn an_oauth_error_body_is_a_named_error_with_cleaned_text() {
        let (result, _) = exchange(
            Reply::json(
                400,
                r#"{"error":"invalid_grant","error_description":"bad\u001b[31m code"}"#,
            ),
            None,
        )
        .await;
        let error = result.expect_err("rejected");
        assert!(
            matches!(error, AuthError::TokenRejected { .. }),
            "{error:?}"
        );
        let text = error.to_string();
        assert!(text.contains("invalid_grant"), "{text}");
        assert!(!text.contains('\u{1b}'), "{text:?}");
    }

    #[tokio::test]
    async fn a_non_json_success_body_is_a_response_error_that_does_not_echo_it() {
        let (result, _) = exchange(Reply::ok(BODY_SENTINEL), None).await;
        let error = result.expect_err("not json");
        assert!(
            matches!(error, AuthError::TokenResponse { .. }),
            "{error:?}"
        );
        assert!(!error.to_string().contains(BODY_SENTINEL));
        assert!(!format!("{error:?}").contains(BODY_SENTINEL));
    }

    #[tokio::test]
    async fn an_oversized_body_is_refused() {
        let (result, _) = exchange(Reply::ok(vec![b'a'; MAX_RESPONSE_BYTES + 1]), None).await;
        let error = result.expect_err("too large");
        assert!(
            matches!(error, AuthError::TokenResponse { .. }),
            "{error:?}"
        );
        assert!(error.to_string().contains("65536"), "{error}");
    }

    #[tokio::test]
    async fn a_redirect_is_not_followed_and_the_secret_is_not_resent() {
        let target = serve(Reply::json(200, &token_body("AT-1", None, None))).await;
        let location = target.url("/token");
        let first = serve(Reply::redirect(307, &location)).await;
        let config = oauth_config_for(&first.url("/token"), Some("CS-SENTINEL"));
        let client = TokenClient::new(&config).expect("client");
        let result = client.exchange_code(&code(), &verifier(), REDIRECT).await;
        assert!(
            target.seen().is_empty(),
            "the redirect target was contacted"
        );
        let error = result.expect_err("a redirect is a rejection");
        assert!(
            matches!(error, AuthError::TokenRejected { .. }),
            "{error:?}"
        );
        assert!(error.to_string().contains("307"), "{error}");
    }

    #[tokio::test]
    async fn the_request_carries_accept_json_and_a_fili_user_agent() {
        let (result, seen) =
            exchange(Reply::json(200, &token_body("AT-1", None, None)), None).await;
        result.expect("exchange");
        assert_eq!(seen[0].header("accept"), Some("application/json"));
        assert!(
            seen[0]
                .header("user-agent")
                .is_some_and(|ua| ua.starts_with("fili/")),
            "{:?}",
            seen[0].header("user-agent")
        );
    }

    #[tokio::test]
    async fn a_hung_endpoint_times_out() {
        let server = serve(
            Reply::json(200, &token_body("AT-1", None, None)).delayed(Duration::from_secs(2)),
        )
        .await;
        let config = oauth_config_for(&server.url("/token"), None);
        let client =
            TokenClient::with_timeout(&config, Duration::from_millis(200)).expect("client");
        let result = client.exchange_code(&code(), &verifier(), REDIRECT).await;
        assert!(
            matches!(result, Err(AuthError::TokenRequest { .. })),
            "{result:?}"
        );
    }

    /// A loopback url whose port nothing listens on.
    async fn closed_port_url() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let port = listener.local_addr().expect("address").port();
        drop(listener);
        format!("http://127.0.0.1:{port}/token")
    }

    #[tokio::test]
    async fn an_unreachable_endpoint_is_a_request_error_that_omits_the_url() {
        let config = oauth_config_for(&closed_port_url().await, None);
        let client = TokenClient::new(&config).expect("client");
        let error = client
            .exchange_code(&code(), &verifier(), REDIRECT)
            .await
            .expect_err("unreachable");
        assert!(matches!(error, AuthError::TokenRequest { .. }), "{error:?}");
        assert!(!error.to_string().contains("/token"), "{error}");
    }

    #[tokio::test]
    async fn no_credential_appears_in_any_error_text() {
        let mut errors = Vec::new();
        for reply in [
            Reply::json(
                400,
                r#"{"error":"invalid_grant","error_description":"expired"}"#,
            ),
            Reply::ok(BODY_SENTINEL),
            Reply::json(200, "{}"),
            Reply::status(500),
            Reply::ok(vec![b'a'; MAX_RESPONSE_BYTES + 1]),
        ] {
            let (result, _) = exchange(reply, Some("CS-SENTINEL")).await;
            errors.push(result.expect_err("an error case"));
        }
        let config = oauth_config_for(&closed_port_url().await, Some("CS-SENTINEL"));
        let client = TokenClient::new(&config).expect("client");
        errors.push(
            client
                .exchange_code(&code(), &verifier(), REDIRECT)
                .await
                .expect_err("unreachable"),
        );
        assert_eq!(errors.len(), 6);
        for error in &errors {
            assert_no_credentials(&texts(error));
        }
        let debug = format!("{client:?}");
        assert!(!debug.contains("CS-SENTINEL"), "{debug}");
    }

    /// A server that answers a revocation and a config pointing at it.
    async fn revoke_against(
        reply: Reply,
        secret: Option<&str>,
        token: &str,
        kind: TokenKind,
    ) -> (Result<bool, AuthError>, Vec<SeenRequest>) {
        let server = serve(reply).await;
        let mut config = oauth_config_for(&server.url("/token"), secret);
        config.revocation_url = Some(url::Url::parse(&server.url("/revoke")).expect("url"));
        let client = TokenClient::new(&config).expect("client");
        (client.revoke(token, kind).await, server.seen())
    }

    #[tokio::test]
    async fn revoke_posts_the_token_and_hint_with_client_auth() {
        for status in [200, 204] {
            let (result, seen) =
                revoke_against(Reply::status(status), None, "RT-1", TokenKind::Refresh).await;
            assert!(result.expect("revoke"), "status {status}");
            assert_eq!(seen.len(), 1);
            assert_eq!(seen[0].method, "POST");
            assert_eq!(seen[0].target, "/revoke");
            let pairs = form_pairs(&seen[0].body);
            assert!(has_pair(&pairs, "token", "RT-1"), "{pairs:?}");
            assert!(
                has_pair(&pairs, "token_type_hint", "refresh_token"),
                "{pairs:?}"
            );
            assert!(has_pair(&pairs, "client_id", "client-1"), "{pairs:?}");
            assert_eq!(seen[0].header("authorization"), None);
        }
        let (result, seen) = revoke_against(
            Reply::status(200),
            Some("CS-SENTINEL"),
            "AT-1",
            TokenKind::Access,
        )
        .await;
        assert!(result.expect("revoke"));
        let pairs = form_pairs(&seen[0].body);
        assert!(
            has_pair(&pairs, "token_type_hint", "access_token"),
            "{pairs:?}"
        );
        assert_eq!(
            seen[0].header("authorization"),
            Some(basic("client-1", "CS-SENTINEL").as_str())
        );
        assert!(!String::from_utf8_lossy(&seen[0].body).contains("CS-SENTINEL"));
    }

    #[tokio::test]
    async fn revoke_without_a_revocation_url_makes_no_request() {
        let server = serve(Reply::status(200)).await;
        let config = oauth_config_for(&server.url("/token"), None);
        let client = TokenClient::new(&config).expect("client");
        let result = client.revoke("RT-1", TokenKind::Refresh).await;
        assert!(!result.expect("no revocation url"));
        assert!(server.seen().is_empty());
    }

    #[tokio::test]
    async fn a_failed_revocation_is_a_named_error_with_cleaned_text() {
        let (result, _) = revoke_against(
            Reply::json(
                500,
                r#"{"error":"server_error","error_description":"RT-1 broke\u001b[0m"}"#,
            ),
            None,
            "RT-1",
            TokenKind::Refresh,
        )
        .await;
        let error = result.expect_err("a 500 fails");
        assert!(matches!(error, AuthError::RevokeFailed { .. }), "{error:?}");
        let text = error.to_string();
        assert!(text.contains("500"), "{text}");
        assert!(!text.contains("RT-1"), "{text}");
        assert!(!text.contains('\u{1b}'), "{text:?}");
    }

    // ── Refresh ───────────────────────────────────────────────────────────────

    fn refresh_token() -> RefreshToken {
        RefreshToken::new("RT-1".to_string())
    }

    /// Runs one refresh against a server answering `reply`.
    async fn refresh_against(
        reply: Reply,
        secret: Option<&str>,
    ) -> (Result<TokenSet, RefreshFailure>, Vec<SeenRequest>) {
        let server = serve(reply).await;
        let config = oauth_config_for(&server.url("/token"), secret);
        let client = TokenClient::new(&config).expect("client");
        (client.refresh(&refresh_token()).await, server.seen())
    }

    /// The failure of a refresh that must fail.
    async fn failure_of(reply: Reply) -> RefreshFailure {
        refresh_against(reply, None)
            .await
            .0
            .expect_err("the refresh must fail")
    }

    fn oauth_error(status: u16, code: &str) -> Reply {
        Reply::json(status, &format!(r#"{{"error":"{code}"}}"#))
    }

    #[tokio::test]
    async fn refresh_posts_the_refresh_token_grant() {
        let (result, seen) = refresh_against(
            Reply::json(200, &token_body("AT-2", None, Some(3600))),
            None,
        )
        .await;
        result.expect("refresh");
        assert_eq!(seen[0].method, "POST");
        let pairs = form_pairs(&seen[0].body);
        assert!(has_pair(&pairs, "grant_type", "refresh_token"), "{pairs:?}");
        assert!(has_pair(&pairs, "refresh_token", "RT-1"), "{pairs:?}");
        assert!(has_pair(&pairs, "client_id", "client-1"), "{pairs:?}");
        assert_eq!(seen[0].header("authorization"), None);

        let (result, seen) = refresh_against(
            Reply::json(200, &token_body("AT-2", None, Some(3600))),
            Some("CS-SENTINEL"),
        )
        .await;
        result.expect("refresh with a secret");
        assert_eq!(
            seen[0].header("authorization"),
            Some(basic("client-1", "CS-SENTINEL").as_str())
        );
        let body = String::from_utf8_lossy(&seen[0].body).into_owned();
        assert!(!body.contains("CS-SENTINEL"), "{body}");
        assert!(
            form_pairs(&seen[0].body)
                .iter()
                .all(|(key, _)| key != "client_secret")
        );
    }

    #[tokio::test]
    async fn a_refresh_answer_without_a_refresh_token_returns_none_for_it() {
        let (result, _) = refresh_against(
            Reply::json(200, &token_body("AT-2", None, Some(1800))),
            None,
        )
        .await;
        let tokens = result.expect("refresh");
        assert_eq!(tokens.access_token, "AT-2");
        assert_eq!(tokens.refresh_token, None);
        assert_eq!(tokens.expires_in, Some(1800));
    }

    #[tokio::test]
    async fn a_refresh_answer_with_a_rotated_token_returns_it() {
        let (result, _) = refresh_against(
            Reply::json(200, &token_body("AT-2", Some("RT-2"), Some(3600))),
            None,
        )
        .await;
        assert_eq!(
            result.expect("refresh").refresh_token.as_deref(),
            Some("RT-2")
        );
    }

    #[tokio::test]
    async fn invalid_grant_is_permanent() {
        for status in [400, 401] {
            let failure = failure_of(oauth_error(status, "invalid_grant")).await;
            assert!(
                matches!(&failure, RefreshFailure::Permanent(text) if text.contains("invalid_grant")),
                "{status}: {failure:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_5xx_is_transient() {
        for reply in [
            Reply::status(500),
            Reply::status(503),
            oauth_error(500, "server_down"),
            oauth_error(503, "invalid_grant"),
        ] {
            let failure = failure_of(reply).await;
            assert!(
                matches!(failure, RefreshFailure::Transient(_)),
                "{failure:?}"
            );
        }
    }

    #[tokio::test]
    async fn status_408_and_429_are_transient() {
        for status in [408, 429] {
            let failure = failure_of(Reply::status(status)).await;
            assert!(
                matches!(failure, RefreshFailure::Transient(_)),
                "{status}: {failure:?}"
            );
        }
    }

    #[tokio::test]
    async fn temporarily_unavailable_and_server_error_are_transient() {
        for code in ["temporarily_unavailable", "server_error"] {
            let failure = failure_of(oauth_error(400, code)).await;
            assert!(
                matches!(&failure, RefreshFailure::Transient(text) if text.contains(code)),
                "{code}: {failure:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_connection_error_is_transient() {
        let config = oauth_config_for(&closed_port_url().await, None);
        let client = TokenClient::new(&config).expect("client");
        let failure = client
            .refresh(&refresh_token())
            .await
            .expect_err("unreachable");
        assert!(
            matches!(failure, RefreshFailure::Transient(_)),
            "{failure:?}"
        );
        assert!(!failure.text().contains("/token"), "{}", failure.text());
    }

    #[tokio::test]
    async fn an_unparseable_5xx_body_is_transient() {
        let failure = failure_of(Reply::json(502, &format!("<html>{BODY_SENTINEL}</html>"))).await;
        assert!(
            matches!(&failure, RefreshFailure::Transient(text) if text == "HTTP 502"),
            "{failure:?}"
        );
        assert!(!failure.text().contains(BODY_SENTINEL));
    }

    #[tokio::test]
    async fn a_success_without_an_access_token_is_transient() {
        let failure = failure_of(Reply::json(200, "{}")).await;
        assert!(
            matches!(failure, RefreshFailure::Transient(_)),
            "{failure:?}"
        );
    }

    #[tokio::test]
    async fn invalid_client_is_rejected_not_permanent() {
        let failure = failure_of(oauth_error(401, "invalid_client")).await;
        assert!(
            matches!(&failure, RefreshFailure::Rejected(text) if text.contains("invalid_client")),
            "{failure:?}"
        );
    }

    #[tokio::test]
    async fn other_4xx_oauth_errors_are_rejected() {
        for code in [
            "unsupported_grant_type",
            "invalid_scope",
            "invalid_request",
            "unauthorized_client",
        ] {
            let failure = failure_of(oauth_error(400, code)).await;
            assert!(
                matches!(&failure, RefreshFailure::Rejected(text) if text.contains(code)),
                "{code}: {failure:?}"
            );
        }
    }

    #[tokio::test]
    async fn an_unparseable_4xx_body_is_rejected() {
        let failure = failure_of(Reply::json(400, BODY_SENTINEL)).await;
        assert!(
            matches!(&failure, RefreshFailure::Rejected(text) if text == "HTTP 400"),
            "{failure:?}"
        );
    }

    #[tokio::test]
    async fn a_redirect_answer_is_rejected() {
        let target = serve(Reply::json(200, &token_body("AT-2", None, None))).await;
        let first = serve(Reply::redirect(307, &target.url("/token"))).await;
        let config = oauth_config_for(&first.url("/token"), Some("CS-SENTINEL"));
        let client = TokenClient::new(&config).expect("client");
        let failure = client
            .refresh(&refresh_token())
            .await
            .expect_err("a redirect is a rejection");
        assert!(
            matches!(&failure, RefreshFailure::Rejected(text) if text == "HTTP 307"),
            "{failure:?}"
        );
        assert!(
            target.seen().is_empty(),
            "the redirect target was contacted"
        );
    }

    #[tokio::test]
    async fn refresh_failure_text_never_contains_the_tokens_or_the_body() {
        let mut failures = vec![];
        for reply in [
            Reply::json(
                400,
                r#"{"error":"invalid_grant","error_description":"gone\u001b[31m"}"#,
            ),
            Reply::json(
                401,
                r#"{"error":"invalid_client","error_description":"nope\u001b[0m"}"#,
            ),
            Reply::json(502, &format!("<html>{BODY_SENTINEL}</html>")),
            Reply::json(400, BODY_SENTINEL),
            Reply::ok(BODY_SENTINEL),
            Reply::json(200, "{}"),
            Reply::status(503),
            Reply::redirect(307, "http://127.0.0.1:9/RT-2"),
        ] {
            let (result, _) = refresh_against(reply, Some("CS-SENTINEL")).await;
            failures.push(result.expect_err("a failing answer"));
        }
        let config = oauth_config_for(&closed_port_url().await, Some("CS-SENTINEL"));
        let client = TokenClient::new(&config).expect("client");
        failures.push(
            client
                .refresh(&refresh_token())
                .await
                .expect_err("unreachable"),
        );
        for failure in &failures {
            let text = format!("{} {failure:?}", failure.text());
            for secret in ["RT-1", "RT-2", "CS-SENTINEL", BODY_SENTINEL] {
                assert!(!text.contains(secret), "`{secret}` leaked into: {text}");
            }
            assert!(!failure.text().contains('\u{1b}'), "{:?}", failure.text());
        }
    }
}
