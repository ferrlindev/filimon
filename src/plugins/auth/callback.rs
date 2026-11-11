// ─────────────────────────────────────────────────────────────────────────────
// plugins/auth/callback.rs - One-shot loopback listener for the login redirect
//
// The provider redirects the user's browser to `http://127.0.0.1:<port>/callback`
// with `?code=..&state=..` (or `?error=..`). This listener receives exactly that
// one request. It is hand-written on tokio's `TcpListener`: it needs one GET
// with strict size and time caps, so no HTTP framework is added.
//
// - Binds `127.0.0.1` only (RFC 8252 section 8.3, D-04), never `localhost`.
// - Open only while `wait_for_code` runs: the call consumes the listener, so the
//   port is free again when it returns.
// - One task per connection, each under a read timeout and a size cap, so a
//   silent or hostile client cannot stall or end the wait.
// - Only `/callback` is meaningful; anything else gets a fixed 404 or 400.
// - The reply page is a constant: no request value is ever echoed (D-06).
// - Nothing here logs, and no error or Debug text carries a request value
//   except the cleaned `error` code of a denied login.
// ─────────────────────────────────────────────────────────────────────────────

use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;

use super::error::AuthError;
use crate::plugins::host::engine::clean_guest_text;

/// The path the provider redirects to.
pub const CALLBACK_PATH: &str = "/callback";

/// How long a login waits for the redirect (D-06).
pub const LOGIN_TIMEOUT: Duration = Duration::from_mins(5);

/// Largest request head accepted, in bytes.
const MAX_REQUEST_BYTES: usize = 8 * 1024;

/// How long one connection may take to send its request head.
const CONNECTION_READ_TIMEOUT: Duration = Duration::from_secs(2);

/// Longest `error` code carried into a [`AuthError::CallbackDenied`], in
/// characters.
const MAX_ERROR_CHARS: usize = 64;

/// The page every `/callback` request gets, whatever its outcome. It holds no
/// request value.
const CALLBACK_PAGE: &str = "<!doctype html><html><head><meta charset=\"utf-8\">\
<title>fili</title></head><body><p>The login finished. You can close this tab.</p>\
</body></html>";

const BAD_REQUEST_PAGE: &str = "Bad request";
const NOT_FOUND_PAGE: &str = "Not found";

/// An authorization code. Debug prints `***`, there is no `Display`, and the
/// text is only reachable through `expose`.
pub struct AuthCode(String);

impl AuthCode {
    /// The code text, for the token request.
    #[must_use]
    pub fn expose(&self) -> &str {
        &self.0
    }

    /// A code with this text, for tests of the token request.
    #[cfg(test)]
    pub(crate) fn for_test(value: &str) -> Self {
        Self(value.to_string())
    }
}

impl std::fmt::Debug for AuthCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AuthCode(***)")
    }
}

/// What one connection decided about the login.
type Decision = Result<AuthCode, AuthError>;

/// A bound loopback listener, not yet waiting.
pub struct CallbackListener {
    listener: TcpListener,
    port: u16,
}

impl CallbackListener {
    /// Binds `127.0.0.1:<port>`. Port 0 takes an ephemeral port (tests);
    /// [`Self::port`] reports the one bound.
    ///
    /// # Errors
    ///
    /// [`AuthError::RedirectPortInUse`] when the bind fails, for example
    /// because another login or program holds the port.
    pub async fn bind(port: u16) -> Result<Self, AuthError> {
        let listener = TcpListener::bind(("127.0.0.1", port)).await.map_err(|e| {
            AuthError::RedirectPortInUse {
                port,
                reason: clean_guest_text(&e.to_string(), 200),
            }
        })?;
        let bound = listener
            .local_addr()
            .map_err(|e| AuthError::RedirectPortInUse {
                port,
                reason: clean_guest_text(&e.to_string(), 200),
            })?
            .port();
        Ok(Self {
            listener,
            port: bound,
        })
    }

    /// The port actually bound.
    #[must_use]
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The redirect URI to register and to send in the authorize URL. It uses
    /// the literal loopback address, never `localhost`.
    #[must_use]
    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}{CALLBACK_PATH}", self.port)
    }

    /// Waits for the provider's redirect and returns its authorization code.
    /// Consumes the listener, so the port is released when this returns.
    ///
    /// # Errors
    ///
    /// [`AuthError::StateMismatch`] when the callback's `state` is missing or
    /// different, [`AuthError::CallbackDenied`] for an `error=` callback,
    /// [`AuthError::CallbackInvalid`] for a callback without a code and
    /// [`AuthError::CallbackTimeout`] when nothing arrives within `timeout`.
    pub async fn wait_for_code(
        self,
        expected_state: &str,
        timeout: Duration,
    ) -> Result<AuthCode, AuthError> {
        let expected: Arc<str> = Arc::from(expected_state);
        let listener = self.listener;
        let wait = async move {
            let (decisions, mut inbox) = mpsc::channel::<Decision>(8);
            loop {
                tokio::select! {
                    accepted = listener.accept() => match accepted {
                        Ok((stream, _peer)) => {
                            tokio::spawn(handle_connection(
                                stream,
                                Arc::clone(&expected),
                                decisions.clone(),
                            ));
                        }
                        // A transient accept failure (for example no free file
                        // descriptor) must not spin the loop.
                        Err(_) => tokio::time::sleep(Duration::from_millis(50)).await,
                    },
                    decision = inbox.recv() => {
                        if let Some(decision) = decision {
                            return decision;
                        }
                    }
                }
            }
        };
        tokio::time::timeout(timeout, wait)
            .await
            .unwrap_or(Err(AuthError::CallbackTimeout {
                secs: timeout.as_secs(),
            }))
    }
}

/// What the request head turned out to be.
enum Head {
    /// A well-formed `GET` request line; the target is unresolved.
    Request(String),
    /// Not a usable HTTP request, or larger than the cap.
    Malformed,
    /// The client sent nothing usable and went away.
    Closed,
}

/// What to answer, and whether the login is decided.
enum Reply {
    BadRequest,
    NotFound,
    Callback(Decision),
}

/// Serves one connection: read the head under the timeout, judge it, answer,
/// and report a `/callback` decision to the wait loop.
async fn handle_connection(
    mut stream: TcpStream,
    expected_state: Arc<str>,
    decisions: mpsc::Sender<Decision>,
) {
    let head = match tokio::time::timeout(CONNECTION_READ_TIMEOUT, read_head(&mut stream)).await {
        Ok(head) => head,
        // A client that connects and goes quiet gets dropped, nothing more.
        Err(_elapsed) => return,
    };
    let reply = match head {
        Head::Closed => return,
        Head::Malformed => Reply::BadRequest,
        Head::Request(target) => judge(&target, &expected_state),
    };
    let (status, page) = match &reply {
        Reply::BadRequest => ("400 Bad Request", BAD_REQUEST_PAGE),
        Reply::NotFound => ("404 Not Found", NOT_FOUND_PAGE),
        Reply::Callback(_) => ("200 OK", CALLBACK_PAGE),
    };
    // The reply goes out first and its errors are ignored: a lost reply must
    // not fail a login whose code is already in hand.
    let _ = stream
        .write_all(http_response(status, page).as_bytes())
        .await;
    let _ = stream.shutdown().await;
    if let Reply::Callback(decision) = reply {
        let _ = decisions.send(decision).await;
    }
}

/// A complete fixed HTTP response. No request value is ever part of it.
fn http_response(status: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {status}\r\n\
Content-Type: text/html; charset=utf-8\r\n\
Cache-Control: no-store\r\n\
Connection: close\r\n\
Content-Length: {}\r\n\
\r\n\
{body}",
        body.len()
    )
}

/// Reads until the blank line that ends the head, never more than
/// `MAX_REQUEST_BYTES`. A request line that is not a plain `GET` is rejected
/// as soon as it is complete, without waiting for the rest.
async fn read_head(stream: &mut TcpStream) -> Head {
    let mut buf: Vec<u8> = Vec::with_capacity(1024);
    let mut chunk = [0_u8; 1024];
    loop {
        if let Some(line_end) = buf.iter().position(|&b| b == b'\n') {
            let Some(target) = parse_request_line(&buf[..line_end]) else {
                return Head::Malformed;
            };
            if let Some(end) = head_end(&buf) {
                return if end <= MAX_REQUEST_BYTES {
                    Head::Request(target)
                } else {
                    Head::Malformed
                };
            }
        }
        if buf.len() >= MAX_REQUEST_BYTES {
            return Head::Malformed;
        }
        match stream.read(&mut chunk).await {
            Ok(0) | Err(_) => return Head::Closed,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
        }
    }
}

/// The offset just past the blank line that ends the head, if present.
fn head_end(buf: &[u8]) -> Option<usize> {
    let crlf = buf.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4);
    let lf = buf.windows(2).position(|w| w == b"\n\n").map(|i| i + 2);
    match (crlf, lf) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    }
}

/// The request target of a `GET <origin-form target> HTTP/1.x` line, or `None`
/// for anything else.
fn parse_request_line(line: &[u8]) -> Option<String> {
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let line = std::str::from_utf8(line).ok()?;
    let mut parts = line.split(' ');
    let method = parts.next()?;
    let target = parts.next()?;
    let version = parts.next()?;
    if parts.next().is_some() || method != "GET" || !version.starts_with("HTTP/1.") {
        return None;
    }
    // Origin-form only: a `//host` target would be read as another authority.
    if !target.starts_with('/') || target.starts_with("//") {
        return None;
    }
    Some(target.to_string())
}

/// Judges a request target. For `/callback`, in this order: a `state` that is
/// missing or different, an `error` parameter, a missing code, else the code.
fn judge(target: &str, expected_state: &str) -> Reply {
    let Ok(base) = url::Url::parse("http://127.0.0.1") else {
        return Reply::BadRequest;
    };
    let Ok(resolved) = base.join(target) else {
        return Reply::BadRequest;
    };
    if resolved.path() != CALLBACK_PATH {
        return Reply::NotFound;
    }
    let first = |name: &str| {
        resolved
            .query_pairs()
            .find(|(key, _)| key == name)
            .map(|(_, value)| value.into_owned())
    };
    if first("state").as_deref() != Some(expected_state) {
        return Reply::Callback(Err(AuthError::StateMismatch));
    }
    if let Some(error) = first("error").filter(|error| !error.is_empty()) {
        return Reply::Callback(Err(AuthError::CallbackDenied {
            error: clean_guest_text(&error, MAX_ERROR_CHARS),
        }));
    }
    match first("code").filter(|code| !code.is_empty()) {
        Some(code) => Reply::Callback(Ok(AuthCode(code))),
        None => Reply::Callback(Err(AuthError::CallbackInvalid {
            reason: "no authorization code".to_string(),
        })),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use miette::Diagnostic as _;
    use tokio::task::JoinHandle;

    type Waiting = JoinHandle<Result<AuthCode, AuthError>>;

    /// Binds an ephemeral listener and starts waiting on it.
    async fn start(state: &str, timeout: Duration) -> (u16, Waiting) {
        let listener = CallbackListener::bind(0).await.expect("bind");
        let port = listener.port();
        let state = state.to_string();
        let handle = tokio::spawn(async move { listener.wait_for_code(&state, timeout).await });
        (port, handle)
    }

    /// Sends `raw` to the port and returns the whole response text.
    async fn send(port: u16, raw: &[u8]) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("connect");
        stream.write_all(raw).await.expect("write");
        let mut out = Vec::new();
        let _ = tokio::time::timeout(Duration::from_secs(3), stream.read_to_end(&mut out)).await;
        String::from_utf8_lossy(&out).into_owned()
    }

    fn get(target: &str) -> Vec<u8> {
        format!("GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n").into_bytes()
    }

    async fn finished(handle: Waiting) -> Result<AuthCode, AuthError> {
        tokio::time::timeout(Duration::from_secs(3), handle)
            .await
            .expect("the wait finished")
            .expect("task")
    }

    /// Drives one callback and returns what the wait decided.
    async fn decide(target: &str) -> Result<AuthCode, AuthError> {
        let (port, handle) = start("S1", Duration::from_secs(5)).await;
        let reply = send(port, &get(target)).await;
        assert!(reply.starts_with("HTTP/1.1 200"), "reply: {reply}");
        finished(handle).await
    }

    fn texts(error: &AuthError) -> String {
        format!("{error} {error:?}")
    }

    #[tokio::test]
    async fn callback_returns_the_code_for_a_matching_state() {
        let code = decide("/callback?code=abc&state=S1").await.expect("code");
        assert_eq!(code.expose(), "abc");
    }

    #[tokio::test]
    async fn callback_page_is_static_and_reflects_nothing() {
        let (port, handle) = start("S1", Duration::from_secs(5)).await;
        let reply = send(
            port,
            &get("/callback?code=abc&state=S1&x=<script>SENTINEL</script>"),
        )
        .await;
        let lower = reply.to_ascii_lowercase();
        let (head, body) = reply.split_once("\r\n\r\n").expect("head and body");
        let head = head.to_ascii_lowercase();
        assert!(head.starts_with("http/1.1 200"));
        assert!(head.contains("cache-control: no-store"));
        assert!(head.contains("connection: close"));
        assert!(head.contains("content-type: text/html"));
        assert!(head.contains(&format!("content-length: {}", body.len())));
        for forbidden in ["sentinel", "script", "abc", "s1"] {
            assert!(!lower.contains(forbidden), "page reflects {forbidden}");
        }
        finished(handle).await.expect("code");
    }

    #[tokio::test]
    async fn a_denied_callback_is_a_named_error() {
        match decide("/callback?error=access_denied&state=S1").await {
            Err(AuthError::CallbackDenied { error }) => assert_eq!(error, "access_denied"),
            other => panic!("expected CallbackDenied, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_state_mismatch_is_a_named_error() {
        match decide("/callback?code=abc&state=WRONG").await {
            Err(error @ AuthError::StateMismatch) => {
                let text = texts(&error);
                for secret in ["WRONG", "S1", "abc"] {
                    assert!(!text.contains(secret), "{text} carries {secret}");
                }
            }
            other => panic!("expected StateMismatch, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_missing_state_is_a_state_mismatch() {
        assert!(matches!(
            decide("/callback?code=abc").await,
            Err(AuthError::StateMismatch)
        ));
    }

    #[tokio::test]
    async fn an_error_callback_with_a_wrong_state_cannot_print_provider_text() {
        assert!(matches!(
            decide("/callback?error=injected&state=WRONG").await,
            Err(AuthError::StateMismatch)
        ));
    }

    #[tokio::test]
    async fn a_matching_state_without_a_code_is_an_invalid_callback() {
        assert!(matches!(
            decide("/callback?state=S1").await,
            Err(AuthError::CallbackInvalid { .. })
        ));
    }

    #[tokio::test]
    async fn a_wrong_path_gets_404_and_the_wait_continues() {
        let (port, handle) = start("S1", Duration::from_secs(5)).await;
        let reply = send(port, &get("/favicon.ico")).await;
        assert!(reply.starts_with("HTTP/1.1 404"), "reply: {reply}");
        assert!(!handle.is_finished());
        send(port, &get("/callback?code=abc&state=S1")).await;
        assert_eq!(finished(handle).await.expect("code").expose(), "abc");
    }

    #[tokio::test]
    async fn garbage_and_oversize_requests_get_400_and_the_wait_continues() {
        let (port, handle) = start("S1", Duration::from_secs(5)).await;

        let garbage = send(port, b"\x00\x01\xfe\xff garbage\r\n\r\n").await;
        assert!(
            garbage.is_empty() || garbage.starts_with("HTTP/1.1 400"),
            "reply: {garbage}"
        );

        let long = "a".repeat(9 * 1024);
        let oversize = send(port, &get(&format!("/callback?x={long}"))).await;
        assert!(
            oversize.is_empty() || oversize.starts_with("HTTP/1.1 400"),
            "reply: {oversize}"
        );

        let unterminated = send(port, long.as_bytes()).await;
        assert!(
            unterminated.is_empty() || unterminated.starts_with("HTTP/1.1 400"),
            "reply: {unterminated}"
        );

        assert!(!handle.is_finished());
        send(port, &get("/callback?code=abc&state=S1")).await;
        assert_eq!(finished(handle).await.expect("code").expose(), "abc");
    }

    #[tokio::test]
    async fn a_non_get_method_gets_400_and_the_wait_continues() {
        let (port, handle) = start("S1", Duration::from_secs(5)).await;
        let reply = send(
            port,
            b"POST /callback?code=abc&state=S1 HTTP/1.1\r\nContent-Length: 0\r\n\r\n",
        )
        .await;
        assert!(reply.starts_with("HTTP/1.1 400"), "reply: {reply}");
        assert!(!handle.is_finished());
        send(port, &get("/callback?code=real&state=S1")).await;
        assert_eq!(finished(handle).await.expect("code").expose(), "real");
    }

    #[tokio::test]
    async fn a_silent_client_cannot_stall_the_wait() {
        let (port, handle) = start("S1", Duration::from_secs(10)).await;
        let _silent = TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("connect");
        send(port, &get("/callback?code=abc&state=S1")).await;
        let outcome = tokio::time::timeout(Duration::from_secs(1), handle)
            .await
            .expect("the silent client delayed the wait")
            .expect("task");
        assert_eq!(outcome.expect("code").expose(), "abc");
    }

    #[tokio::test]
    async fn bind_fails_when_the_port_is_in_use_naming_the_config_key() {
        let first = CallbackListener::bind(0).await.expect("bind");
        let port = first.port();
        let error = CallbackListener::bind(port)
            .await
            .err()
            .expect("a second bind on the same port must fail");
        assert!(matches!(error, AuthError::RedirectPortInUse { .. }));
        assert!(error.to_string().contains(&port.to_string()));
        let help = error.help().expect("help text").to_string();
        assert!(help.contains("host.oauth.redirect_port"), "help: {help}");
        assert_eq!(
            error.code().expect("code").to_string(),
            "fili::auth::redirect_port_in_use"
        );
    }

    #[tokio::test]
    async fn the_wait_times_out_with_a_named_error() {
        let listener = CallbackListener::bind(0).await.expect("bind");
        let outcome = listener
            .wait_for_code("S1", Duration::from_millis(100))
            .await;
        assert!(matches!(outcome, Err(AuthError::CallbackTimeout { .. })));
    }

    #[tokio::test]
    async fn the_listener_binds_loopback_only() {
        let listener = CallbackListener::bind(0).await.expect("bind");
        let addr = listener.listener.local_addr().expect("addr");
        assert_eq!(addr.ip(), std::net::Ipv4Addr::LOCALHOST);
        assert!(addr.ip().is_loopback());
    }

    #[tokio::test]
    async fn redirect_uri_uses_the_literal_loopback_address() {
        let listener = CallbackListener::bind(0).await.expect("bind");
        let uri = listener.redirect_uri();
        assert_eq!(
            uri,
            format!("http://127.0.0.1:{}/callback", listener.port())
        );
        assert!(!uri.contains("localhost"));
    }

    #[tokio::test]
    async fn the_listener_is_closed_after_the_wait_ends() {
        let (port, handle) = start("S1", Duration::from_secs(5)).await;
        send(port, &get("/callback?code=abc&state=S1")).await;
        finished(handle).await.expect("code");
        CallbackListener::bind(port)
            .await
            .expect("the port is free after the wait");
    }

    #[tokio::test]
    async fn codes_never_appear_in_debug_or_error_text() {
        let (port, handle) = start("EXPECTED-STATE", Duration::from_secs(5)).await;
        send(
            port,
            &get("/callback?code=SECRET-CODE&state=EXPECTED-STATE"),
        )
        .await;
        let code = finished(handle).await.expect("code");
        let debug = format!("{code:?}");
        assert!(debug.contains("***"));
        assert!(!debug.contains("SECRET-CODE"));

        let mut errors = Vec::new();
        for target in [
            "/callback?code=SECRET-CODE&state=RECEIVED-STATE",
            "/callback?code=SECRET-CODE",
            "/callback?error=access_denied&state=EXPECTED-STATE&code=SECRET-CODE",
            "/callback?state=EXPECTED-STATE",
        ] {
            let (port, handle) = start("EXPECTED-STATE", Duration::from_secs(5)).await;
            send(port, &get(target)).await;
            errors.push(finished(handle).await.expect_err("an error"));
        }
        assert_eq!(errors.len(), 4);
        for error in &errors {
            let text = texts(error);
            for secret in ["SECRET-CODE", "RECEIVED-STATE", "EXPECTED-STATE"] {
                assert!(!text.contains(secret), "{text} carries {secret}");
            }
        }
    }
}
