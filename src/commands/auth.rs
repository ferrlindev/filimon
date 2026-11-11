// ─────────────────────────────────────────────────────────────────────────────
// commands/auth.rs - `fili auth` subcommands
//
// `login <name> [--no-browser]` logs the user in to a source plugin's platform
// through OAuth 2.0 PKCE. It checks the plugin first (a registered, enabled
// source that declares `[host.oauth]`), and only then opens the loopback port.
// It prints the authorize URL, also tries the default browser unless told not
// to, waits for the callback and stores the login as
// `<state-dir>/auth/<alias>.json`.
//
// Login never takes the state-directory lock (`fili run` holds it for its whole
// life, and login must work beside it), never creates anything before the code
// exchange succeeded, and prints no secret: the only value shown is the
// authorize URL, which holds public parameters only (D-06).
//
// Everything that came from a file or the network is untrusted at print time
// and passes through `sanitize_for_terminal` or `clean_guest_text`.
// ─────────────────────────────────────────────────────────────────────────────

use std::io::Write;
use std::path::Path;

use miette::IntoDiagnostic;

use crate::cli::AuthCommand;
use crate::commands::plugins::state_dir_for;
use crate::output::sanitize_for_terminal;
use crate::plugins::auth::login::{LoginHooks, LoginOptions, LoginSummary, login};
use crate::plugins::auth::store::TokenStore;
use crate::plugins::config::{
    ListedPlugin, OAuthConfig, OAuthDeclared, OAuthProbeReport, RegistryListing, list_registry,
    load_plugin, probe_oauth_sources,
};
use crate::plugins::host::engine::clean_guest_text;

/// Longest plugin name echoed back in an error, in characters.
const NAME_ECHO_CLIP: usize = 64;

/// Longest io error text shown for a failed browser launch, in characters.
const OPEN_ERROR_CLIP: usize = 120;

/// How a browser launch is attempted; a function so tests can stand in.
type Opener<'a> = &'a dyn Fn(&str) -> std::io::Result<()>;

/// `fili auth <subcommand>`.
///
/// # Errors
/// Returns whatever the subcommand returns.
pub async fn dispatch(
    command: &AuthCommand,
    registry: &Path,
    state_dir_flag: Option<&Path>,
) -> miette::Result<()> {
    match command {
        AuthCommand::Login { plugin, no_browser } => {
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            let stderr = std::io::stderr();
            let mut err = stderr.lock();
            login_command(
                registry,
                state_dir_flag,
                plugin,
                *no_browser,
                &mut out,
                &mut err,
            )
            .await
        }
        AuthCommand::Status { json } => {
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            let stderr = std::io::stderr();
            let mut err = stderr.lock();
            status(registry, state_dir_flag, *json, &mut out, &mut err)
        }
    }
}

// ── Login ─────────────────────────────────────────────────────────────────────

/// The source called `name`, or the reason `login` refuses it. Every refusal
/// comes before any file is read or socket opened.
fn find_login_source<'a>(
    listing: &'a RegistryListing,
    name: &str,
) -> miette::Result<&'a ListedPlugin> {
    let shown = clean_guest_text(name, NAME_ECHO_CLIP);
    if let Some(plugin) = listing.sources.iter().find(|plugin| plugin.name == name) {
        if !plugin.enabled {
            return Err(miette::miette!(
                "plugin `{shown}` is disabled in filimon.toml; set `enabled = true` to log in"
            ));
        }
        return Ok(plugin);
    }
    if listing.fetchers.iter().any(|plugin| plugin.name == name) {
        return Err(miette::miette!(
            "`{shown}` is a fetcher; login applies to sources"
        ));
    }
    let registered = if listing.sources.is_empty() {
        "none".to_string()
    } else {
        listing
            .sources
            .iter()
            .map(|plugin| plugin.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    };
    Err(miette::miette!(
        "`{shown}` is not a registered source; registered sources: {registered}"
    ))
}

/// `fili auth login <name>`.
///
/// # Errors
/// Returns the registry's or the plugin's diagnostics, a refusal naming the
/// plugin (unknown, a fetcher, disabled, or no `[host.oauth]`), a state
/// directory error, the login flow's diagnostics, or an io error when `out`
/// cannot be written.
pub async fn login_command(
    registry: &Path,
    state_dir_flag: Option<&Path>,
    name: &str,
    no_browser: bool,
    out: &mut impl Write,
    err: &mut impl Write,
) -> miette::Result<()> {
    let listing = list_registry(registry)?;
    find_login_source(&listing, name)?;

    // A scoped load: only this plugin's own `.env` secret is needed.
    let loaded = load_plugin(registry, name)?;
    let oauth = loaded
        .sources
        .get(name)
        .and_then(|source| source.plugin.as_ref())
        .and_then(|plugin| plugin.oauth.clone())
        .ok_or_else(|| {
            miette::miette!(
                "plugin `{}` declares no [host.oauth] table, so there is nothing to log in to",
                clean_guest_text(name, NAME_ECHO_CLIP)
            )
        })?;

    let state_dir = state_dir_for(&listing.base_dir, state_dir_flag)?;
    let store = TokenStore::new(&state_dir, name);
    perform_login(
        name,
        &oauth,
        &store,
        no_browser,
        &|url| open::that_detached(url),
        out,
        err,
    )
    .await
}

/// Prints for the login flow and launches the browser.
struct CliHooks<'a> {
    name: &'a str,
    no_browser: bool,
    opener: Opener<'a>,
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

impl LoginHooks for CliHooks<'_> {
    fn authorize_url(&mut self, url: &str) {
        // Write failures are ignored: a closed terminal must not abort a login.
        let _ = writeln!(
            self.out,
            "Open this URL in your browser to log in to `{}`:",
            self.name
        );
        let _ = writeln!(self.out, "  {url}");
        if self.no_browser {
            let _ = writeln!(
                self.out,
                "Waiting up to 5 minutes for you to finish the login in your browser..."
            );
        }
        let _ = self.out.flush();
    }

    fn open_browser(&mut self, url: &str) {
        match (self.opener)(url) {
            Ok(()) => {
                let _ = writeln!(
                    self.out,
                    "Opening your browser; waiting up to 5 minutes for the login to finish..."
                );
            }
            Err(error) => {
                let _ = writeln!(
                    self.err,
                    "could not open a browser ({}); open the URL above yourself",
                    clean_guest_text(&error.to_string(), OPEN_ERROR_CLIP)
                );
                let _ = writeln!(
                    self.out,
                    "Waiting up to 5 minutes for you to finish the login in your browser..."
                );
            }
        }
        let _ = self.out.flush();
        let _ = self.err.flush();
    }
}

/// An RFC 3339 UTC time for `unix_seconds`, or a fixed phrase when it cannot
/// be represented.
fn rfc3339(unix_seconds: i64) -> String {
    chrono::DateTime::from_timestamp(unix_seconds, 0).map_or_else(
        || "an unknown time".to_string(),
        |time| time.to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    )
}

/// Runs the login for `name` over an already loaded `oauth` and prints the
/// outcome. The seam the tests use: they pass a fake token endpoint in
/// `oauth` and an `opener` that "visits" the URL.
///
/// # Errors
/// The login flow's diagnostics, as a miette report.
async fn perform_login(
    name: &str,
    oauth: &OAuthConfig,
    store: &TokenStore,
    no_browser: bool,
    opener: Opener<'_>,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> miette::Result<()> {
    let options = LoginOptions::interactive(!no_browser);
    let summary: LoginSummary = {
        let mut hooks = CliHooks {
            name,
            no_browser,
            opener,
            out: &mut *out,
            err: &mut *err,
        };
        login(oauth, store, &options, &mut hooks)
            .await
            .map_err(miette::Report::new)?
    };

    let expires = rfc3339(summary.expires_at);
    writeln!(
        out,
        "Logged in to `{name}`. The access token expires at {expires}. A running `fili run` picks the new login up on its next poll, after any backoff."
    )
    .map_err(|e| miette::miette!("cannot write to stdout: {e}"))?;
    if !summary.has_refresh_token {
        writeln!(
            err,
            "warning: the provider issued no refresh token, so this login cannot be renewed and ends at {expires}; run `fili auth login {name}` again then. Some providers need an authorize parameter such as `duration` in `host.oauth.authorize_params`."
        )
        .map_err(|e| miette::miette!("cannot write to stderr: {e}"))?;
    }
    Ok(())
}

// ── Status ────────────────────────────────────────────────────────────────────

/// A stored login's state as `status` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatusState {
    LoggedIn,
    Expired,
    NeedsLogin,
    NotLoggedIn,
    /// The token file cannot be used; the cleaned text names the plugin, the
    /// path and a fixed reason, never the file's content (D-02).
    Unreadable(String),
}

/// One `status` row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusRow {
    pub plugin: String,
    pub state: StatusState,
    /// Unix seconds; `None` when no file could be read.
    pub expires_at: Option<i64>,
    /// `None` when no file could be read.
    pub has_refresh_token: Option<bool>,
}

/// The rows `status` shows.
#[must_use]
pub fn collect_status(
    report: &OAuthProbeReport,
    state_dir: &Path,
    now: i64,
    err: &mut impl Write,
) -> Vec<StatusRow> {
    let _ = (report, state_dir, now, err);
    todo!("RED stub")
}

/// Relative expiry text.
#[must_use]
pub fn describe_expiry(expires_at: i64, now: i64) -> String {
    let _ = (expires_at, now);
    todo!("RED stub")
}

/// The human table.
///
/// # Errors
/// Returns the write error of `out`.
pub fn render_status(rows: &[StatusRow], now: i64, out: &mut impl Write) -> std::io::Result<()> {
    let _ = (rows, now, out);
    todo!("RED stub")
}

/// The JSON array.
///
/// # Errors
/// Returns the write error of `out`.
pub fn render_status_json(rows: &[StatusRow], out: &mut impl Write) -> std::io::Result<()> {
    let _ = (rows, out);
    todo!("RED stub")
}

/// `fili auth status`.
///
/// # Errors
/// The registry's diagnostics, a state directory error, or an io error.
pub fn status(
    registry: &Path,
    state_dir_flag: Option<&Path>,
    json: bool,
    out: &mut impl Write,
    err: &mut impl Write,
) -> miette::Result<()> {
    let _ = (registry, state_dir_flag, json, out, err);
    todo!("RED stub")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::auth::store::{AccessToken, RefreshToken, StoredTokens};
    use crate::plugins::auth::testing::{oauth_config_for, token_body, unix_now};
    use crate::plugins::host::testing::{Reply, TestHttpServer};
    use std::cell::Cell;
    use std::path::PathBuf;
    use tempfile::TempDir;
    use tokio::io::AsyncWriteExt;
    use tokio::net::TcpStream;

    const MINIMAL_TOML: &str = "[host]\npoll_interval_secs = 60\n";

    /// A registry with the given tables and a `stub` plugin file.
    fn registry_with(dir: &Path, text: &str, plugin_toml: &str) -> PathBuf {
        std::fs::write(dir.join("stub.toml"), plugin_toml).expect("plugin toml");
        let path = dir.join("filimon.toml");
        std::fs::write(&path, text).expect("registry");
        path
    }

    fn source_table(name: &str, enabled: bool) -> String {
        format!(
            "[sources.{name}]\ncomponent = \"{name}.wasm\"\nconfig = \"{name}.toml\"\nenabled = {enabled}\n\n"
        )
    }

    /// Runs `login_command` against `registry` with a state directory that
    /// does not exist yet, and returns the error text plus whether the state
    /// directory exists afterwards.
    async fn refused(registry: &Path, state: &Path, name: &str) -> (String, bool) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let error = login_command(registry, Some(state), name, true, &mut out, &mut err)
            .await
            .expect_err("login is refused");
        assert!(out.is_empty(), "nothing was printed on stdout");
        (format!("{error} {error:?}"), state.exists())
    }

    #[tokio::test]
    async fn login_refuses_an_unknown_plugin_listing_the_registered_ones() {
        let dir = TempDir::new().expect("tempdir");
        let mut text = source_table("a", true);
        text.push_str(&source_table("b", true));
        let registry = registry_with(dir.path(), &text, MINIMAL_TOML);
        let state = dir.path().join("state");

        let (text, created) = refused(&registry, &state, "nope").await;

        assert!(text.contains("`nope`"), "{text}");
        assert!(text.contains("registered sources: a, b"), "{text}");
        assert!(!created, "no state directory was created");
    }

    #[tokio::test]
    async fn login_refuses_a_fetcher() {
        let dir = TempDir::new().expect("tempdir");
        let mut text = source_table("stub", true);
        text.push_str("[fetchers.f]\ncomponent = \"f.wasm\"\n");
        let registry = registry_with(dir.path(), &text, MINIMAL_TOML);
        let state = dir.path().join("state");

        let (text, created) = refused(&registry, &state, "f").await;

        assert!(text.contains("`f` is a fetcher"), "{text}");
        assert!(text.contains("login applies to sources"), "{text}");
        assert!(!created);
    }

    #[tokio::test]
    async fn login_refuses_a_disabled_source() {
        let dir = TempDir::new().expect("tempdir");
        let registry = registry_with(dir.path(), &source_table("stub", false), MINIMAL_TOML);
        let state = dir.path().join("state");

        let (text, created) = refused(&registry, &state, "stub").await;

        assert!(text.contains("plugin `stub`"), "{text}");
        assert!(text.contains("enabled = true"), "{text}");
        assert!(!created);
    }

    #[tokio::test]
    async fn login_refuses_a_plugin_without_host_oauth() {
        let dir = TempDir::new().expect("tempdir");
        let registry = registry_with(dir.path(), &source_table("stub", true), MINIMAL_TOML);
        let state = dir.path().join("state");

        let (text, created) = refused(&registry, &state, "stub").await;

        assert!(text.contains("plugin `stub`"), "{text}");
        assert!(text.contains("declares no [host.oauth]"), "{text}");
        assert!(!created, "no state directory was created");
        assert!(!dir.path().join(".fili-state").exists());
    }

    // ── perform_login ────────────────────────────────────────────────────────

    /// "Visits" an authorize URL the way a browser would: a raw GET of
    /// `/callback` with the right state and the code `CODE-1`.
    fn visit(url: &str) {
        let parsed = url::Url::parse(url).expect("authorize url");
        let query = |name: &str| {
            parsed
                .query_pairs()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.into_owned())
                .unwrap_or_default()
        };
        let state = query("state");
        let port = url::Url::parse(&query("redirect_uri"))
            .expect("redirect uri")
            .port()
            .expect("redirect port");
        tokio::spawn(async move {
            let mut stream = TcpStream::connect(("127.0.0.1", port))
                .await
                .expect("connect to the listener");
            let request = format!(
                "GET /callback?code=CODE-1&state={state} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n"
            );
            stream.write_all(request.as_bytes()).await.expect("write");
        });
    }

    struct Ran {
        out: String,
        err: String,
        opener_calls: usize,
        token_file: PathBuf,
        server: TestHttpServer,
        _state: TempDir,
    }

    /// Runs `perform_login` against a fake token endpoint answering `body`,
    /// with an opener that visits the URL and then fails when `opener_fails`.
    async fn run_login(body: String, opener_fails: bool) -> Ran {
        let server = TestHttpServer::start(move |_| Reply::json(200, &body)).await;
        let oauth = oauth_config_for(&server.url("/token"), Some("CS-SENTINEL"));
        let state = TempDir::new().expect("tempdir");
        let store = TokenStore::new(state.path(), "stub");
        let calls = Cell::new(0_usize);
        let opener = |url: &str| -> std::io::Result<()> {
            calls.set(calls.get() + 1);
            visit(url);
            if opener_fails {
                Err(std::io::Error::other("no browser here"))
            } else {
                Ok(())
            }
        };
        let mut out = Vec::new();
        let mut err = Vec::new();
        perform_login("stub", &oauth, &store, false, &opener, &mut out, &mut err)
            .await
            .expect("login succeeds");
        Ran {
            out: String::from_utf8(out).expect("utf8"),
            err: String::from_utf8(err).expect("utf8"),
            opener_calls: calls.get(),
            token_file: state.path().join("auth").join("stub.json"),
            server,
            _state: state,
        }
    }

    #[tokio::test]
    async fn login_prints_the_url_and_a_success_note_and_stores_the_file() {
        let ran = run_login(token_body("AT-1", Some("RT-1"), Some(3600)), false).await;

        assert!(
            ran.out
                .contains("Open this URL in your browser to log in to `stub`:\n  https://"),
            "{}",
            ran.out
        );
        assert!(ran.out.contains("Logged in to `stub`"), "{}", ran.out);
        let expiry = ran
            .out
            .split("expires at ")
            .nth(1)
            .and_then(|rest| rest.split(". ").next())
            .expect("an expiry in the note");
        assert!(
            chrono::DateTime::parse_from_rfc3339(expiry).is_ok(),
            "not RFC 3339: {expiry}"
        );
        assert!(ran.err.is_empty(), "{}", ran.err);
        assert!(ran.token_file.exists());
        assert_eq!(ran.opener_calls, 1);
    }

    #[tokio::test]
    async fn a_login_without_a_refresh_token_warns_on_stderr() {
        let ran = run_login(token_body("AT-1", None, Some(3600)), false).await;

        assert!(ran.err.contains("warning:"), "{}", ran.err);
        assert!(ran.err.contains("cannot be renewed"), "{}", ran.err);
        assert!(ran.err.contains(" ends at 20"), "{}", ran.err);
        assert!(ran.out.contains("Logged in to `stub`"), "{}", ran.out);
    }

    #[tokio::test]
    async fn no_browser_never_calls_the_opener() {
        // The test is the browser: it reads the URL from the shared buffer and
        // visits it once printed.
        let server = TestHttpServer::start(move |_| {
            Reply::json(200, &token_body("AT-1", Some("RT-1"), Some(3600)))
        })
        .await;
        let oauth = oauth_config_for(&server.url("/token"), None);
        let state = TempDir::new().expect("tempdir");
        let store = TokenStore::new(state.path(), "stub");
        let calls = Cell::new(0_usize);
        let opener = |_: &str| -> std::io::Result<()> {
            calls.set(calls.get() + 1);
            Ok(())
        };
        let shared = SharedBuf::default();
        let mut out = shared.clone();
        let mut err = Vec::new();
        let watcher = {
            let shared = shared.clone();
            tokio::spawn(async move {
                loop {
                    let text = shared.text();
                    if let Some(url) = text.lines().find_map(|l| l.trim().strip_prefix("https://"))
                    {
                        visit(&format!("https://{url}"));
                        return;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            })
        };

        perform_login("stub", &oauth, &store, true, &opener, &mut out, &mut err)
            .await
            .expect("login succeeds");
        watcher.await.expect("watcher");

        assert_eq!(calls.get(), 0, "the opener was never called");
        let printed = shared.text();
        assert!(printed.contains("Open this URL"), "{printed}");
        assert!(printed.contains("https://"), "{printed}");
        assert!(printed.contains("Waiting up to 5 minutes"), "{printed}");
        assert!(!printed.contains("Opening your browser"), "{printed}");
        assert!(state.path().join("auth").join("stub.json").exists());
    }

    #[tokio::test]
    async fn a_failed_browser_launch_prints_a_note_and_the_login_still_completes() {
        let ran = run_login(token_body("AT-1", Some("RT-1"), Some(3600)), true).await;

        assert!(
            ran.err
                .contains("could not open a browser (no browser here)"),
            "{}",
            ran.err
        );
        assert!(
            ran.err.contains("open the URL above yourself"),
            "{}",
            ran.err
        );
        assert!(ran.out.contains("Logged in to `stub`"), "{}", ran.out);
        assert!(ran.token_file.exists());
    }

    #[tokio::test]
    async fn nothing_secret_is_printed() {
        let ran = run_login(token_body("AT-1", Some("RT-1"), Some(3600)), false).await;

        let seen = ran.server.seen();
        let verifier = crate::plugins::auth::testing::form_pairs(&seen[0].body)
            .into_iter()
            .find(|(key, _)| key == "code_verifier")
            .map(|(_, value)| value)
            .expect("the verifier was sent");
        let everything = format!("{} {}", ran.out, ran.err);
        for secret in ["CODE-1", verifier.as_str(), "AT-1", "RT-1", "CS-SENTINEL"] {
            assert!(
                !everything.contains(secret),
                "{secret} leaked: {everything}"
            );
        }

        // A failed login renders no secret either.
        let server = TestHttpServer::start(|_| {
            Reply::json(
                400,
                r#"{"error":"invalid_grant","error_description":"CODE-1 RT-1"}"#,
            )
        })
        .await;
        let oauth = oauth_config_for(&server.url("/token"), Some("CS-SENTINEL"));
        let state = TempDir::new().expect("tempdir");
        let store = TokenStore::new(state.path(), "stub");
        let opener = |url: &str| -> std::io::Result<()> {
            visit(url);
            Ok(())
        };
        let mut out = Vec::new();
        let mut err = Vec::new();
        let error = perform_login("stub", &oauth, &store, false, &opener, &mut out, &mut err)
            .await
            .expect_err("the exchange is rejected");
        let shown = format!(
            "{error} {error:?} {} {}",
            String::from_utf8_lossy(&out),
            String::from_utf8_lossy(&err)
        );
        for secret in ["AT-1", "CS-SENTINEL"] {
            assert!(!shown.contains(secret), "{secret} leaked: {shown}");
        }
        assert!(!state.path().join("auth").exists());
    }

    // ── status ───────────────────────────────────────────────────────────────

    const OAUTH_PLUGIN_TOML: &str = "[host]\npoll_interval_secs = 60\n\n[host.oauth]\nauthorize_url = \"https://auth.example/authorize\"\ntoken_url = \"https://auth.example/token\"\nclient_id = \"c\"\n";

    /// A registry with one source per `(name, plugin toml)`; a `None` plugin
    /// file is not written, so that source's file is missing.
    fn registry_of(dir: &Path, sources: &[(&str, Option<&str>)]) -> PathBuf {
        let mut text = String::new();
        for (name, plugin_toml) in sources {
            text.push_str(&source_table(name, true));
            if let Some(plugin_toml) = plugin_toml {
                std::fs::write(dir.join(format!("{name}.toml")), plugin_toml)
                    .expect("plugin toml");
            }
        }
        let path = dir.join("filimon.toml");
        std::fs::write(&path, text).expect("registry");
        path
    }

    /// Stores a login for `alias`.
    fn put_login(
        state: &Path,
        alias: &str,
        access: &str,
        refresh: Option<&str>,
        expires_at: i64,
        needs_login: bool,
    ) {
        TokenStore::new(state, alias)
            .write(&StoredTokens {
                access_token: AccessToken::new(access.to_string()),
                refresh_token: refresh.map(|text| RefreshToken::new(text.to_string())),
                expires_at,
                token_type: None,
                scope: None,
                needs_login,
                obtained_at: expires_at - 3600,
            })
            .expect("write the login");
    }

    /// The whitespace-separated cells of the table line that starts with `plugin`.
    fn row_cells(out: &str, plugin: &str) -> Vec<String> {
        out.lines()
            .find(|line| line.split_whitespace().next() == Some(plugin))
            .unwrap_or_else(|| panic!("no row for {plugin}: {out}"))
            .split_whitespace()
            .map(str::to_string)
            .collect()
    }

    /// Runs `status` and returns (stdout, stderr).
    fn run_status(registry: &Path, state: &Path, json: bool) -> (String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        status(registry, Some(state), json, &mut out, &mut err).expect("status succeeds");
        (
            String::from_utf8(out).expect("utf8"),
            String::from_utf8(err).expect("utf8"),
        )
    }

    /// Five plugins in the five states, with sentinel tokens.
    fn five_states(dir: &Path) -> (PathBuf, PathBuf) {
        let registry = registry_of(
            dir,
            &[
                ("broken", Some(OAUTH_PLUGIN_TOML)),
                ("fresh", Some(OAUTH_PLUGIN_TOML)),
                ("nologin", Some(OAUTH_PLUGIN_TOML)),
                ("reauth", Some(OAUTH_PLUGIN_TOML)),
                ("stale", Some(OAUTH_PLUGIN_TOML)),
            ],
        );
        let state = dir.join("state");
        let now = unix_now();
        put_login(&state, "fresh", "AT-SENTINEL-S", Some("RT-SENTINEL-S"), now + 3600, false);
        put_login(&state, "stale", "AT-SENTINEL-S", Some("RT-SENTINEL-S"), now - 180, false);
        put_login(&state, "reauth", "AT-SENTINEL-S", Some("RT-SENTINEL-S"), now + 3600, true);
        std::fs::create_dir_all(state.join("auth")).expect("auth dir");
        std::fs::write(
            state.join("auth").join("broken.json"),
            b"{ not json AT-SENTINEL-S",
        )
        .expect("corrupt file");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(
                state.join("auth").join("broken.json"),
                std::fs::Permissions::from_mode(0o600),
            )
            .expect("chmod");
        }
        (registry, state)
    }

    #[test]
    fn status_lists_each_state_without_token_values() {
        let dir = TempDir::new().expect("tempdir");
        let (registry, state) = five_states(dir.path());

        let (out, err) = run_status(&registry, &state, false);

        let state_of = |plugin: &str| row_cells(&out, plugin)[1].clone();
        assert_eq!(state_of("broken"), "unreadable", "{out}");
        assert_eq!(state_of("fresh"), "logged-in", "{out}");
        assert_eq!(state_of("nologin"), "not-logged-in", "{out}");
        assert_eq!(state_of("reauth"), "needs-login", "{out}");
        assert_eq!(state_of("stale"), "expired", "{out}");
        for secret in ["AT-SENTINEL-S", "RT-SENTINEL-S"] {
            assert!(!out.contains(secret) && !err.contains(secret), "{out}{err}");
        }
    }

    #[test]
    fn status_json_is_an_array_with_no_token_fields() {
        let dir = TempDir::new().expect("tempdir");
        let (registry, state) = five_states(dir.path());

        let (out, _) = run_status(&registry, &state, true);

        let value: serde_json::Value = serde_json::from_str(&out).expect("one JSON value");
        let rows = value.as_array().expect("an array");
        assert_eq!(rows.len(), 5, "{out}");
        for row in rows {
            let mut keys: Vec<&str> = row
                .as_object()
                .expect("an object")
                .keys()
                .map(String::as_str)
                .collect();
            keys.sort_unstable();
            assert_eq!(
                keys,
                ["detail", "expires_at", "has_refresh_token", "plugin", "state"]
            );
        }
        let fresh = rows.iter().find(|row| row["plugin"] == "fresh").expect("fresh");
        assert_eq!(fresh["state"], "logged-in");
        assert_eq!(fresh["has_refresh_token"], true);
        assert!(
            chrono::DateTime::parse_from_rfc3339(fresh["expires_at"].as_str().expect("text"))
                .is_ok()
        );
        let nologin = rows.iter().find(|row| row["plugin"] == "nologin").expect("row");
        assert!(nologin["expires_at"].is_null() && nologin["has_refresh_token"].is_null());
        for secret in ["AT-SENTINEL-S", "RT-SENTINEL-S"] {
            assert!(!out.contains(secret), "{out}");
        }
    }

    #[test]
    fn status_says_to_log_in_again_for_a_needs_login_row() {
        let dir = TempDir::new().expect("tempdir");
        let (registry, state) = five_states(dir.path());

        let (out, _) = run_status(&registry, &state, false);

        assert!(
            out.contains("`reauth`: run `fili auth login reauth` to log in again"),
            "{out}"
        );
        assert!(!out.contains("fili auth login fresh"), "{out}");
    }

    #[test]
    fn status_shows_an_expired_row_with_its_refresh_flag() {
        let dir = TempDir::new().expect("tempdir");
        let registry = registry_of(
            dir.path(),
            &[
                ("renewable", Some(OAUTH_PLUGIN_TOML)),
                ("terminal", Some(OAUTH_PLUGIN_TOML)),
            ],
        );
        let state = dir.path().join("state");
        let now = unix_now();
        put_login(&state, "renewable", "AT-SENTINEL-S", Some("RT-SENTINEL-S"), now - 600, false);
        put_login(&state, "terminal", "AT-SENTINEL-S", None, now - 600, false);

        let (out, _) = run_status(&registry, &state, false);

        let renewable = row_cells(&out, "renewable");
        assert_eq!(renewable[1], "expired");
        assert_eq!(renewable.last().map(String::as_str), Some("yes"));
        let terminal = row_cells(&out, "terminal");
        assert_eq!(terminal[1], "expired");
        assert_eq!(terminal.last().map(String::as_str), Some("no"));
        assert!(out.contains("fili auth login terminal"), "{out}");
        assert!(!out.contains("fili auth login renewable"), "{out}");
    }

    #[test]
    fn status_works_while_the_state_lock_is_held() {
        let dir = TempDir::new().expect("tempdir");
        let (registry, state) = five_states(dir.path());
        let _held = crate::plugins::host::cursor::CursorStore::open(&state)
            .expect("hold the state directory lock");

        let (out, _) = run_status(&registry, &state, false);

        assert_eq!(row_cells(&out, "fresh")[1], "logged-in", "{out}");
    }

    #[test]
    fn status_creates_nothing() {
        let dir = TempDir::new().expect("tempdir");
        let registry = registry_of(dir.path(), &[("stub", Some(OAUTH_PLUGIN_TOML))]);
        let state = dir.path().join("state");

        let (out, _) = run_status(&registry, &state, false);

        assert_eq!(row_cells(&out, "stub")[1], "not-logged-in", "{out}");
        assert!(!state.exists(), "status created the state directory");
    }

    #[test]
    fn status_skips_plugins_that_declare_no_oauth_and_have_no_login() {
        let dir = TempDir::new().expect("tempdir");
        let registry = registry_of(
            dir.path(),
            &[
                ("plain", Some(MINIMAL_TOML)),
                ("withauth", Some(OAUTH_PLUGIN_TOML)),
            ],
        );
        let state = dir.path().join("state");

        let (out, err) = run_status(&registry, &state, false);

        assert!(!out.contains("plain"), "{out}");
        assert!(!err.contains("plain"), "{err}");
        assert_eq!(row_cells(&out, "withauth")[1], "not-logged-in");

        // Nothing declares OAuth and nothing is stored: the empty-state line.
        let registry = registry_of(dir.path(), &[("plain", Some(MINIMAL_TOML))]);
        let (out, _) = run_status(&registry, &state, false);
        assert!(
            out.contains("No registered plugin declares [host.oauth] or has a stored login in "),
            "{out}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn status_reports_an_unreadable_token_file_naming_plugin_and_path_without_content() {
        use std::os::unix::fs::PermissionsExt;
        let dir = TempDir::new().expect("tempdir");
        let registry = registry_of(dir.path(), &[("loose", Some(OAUTH_PLUGIN_TOML))]);
        let state = dir.path().join("state");
        put_login(&state, "loose", "AT-SENTINEL-S", None, unix_now() + 3600, false);
        let file = state.join("auth").join("loose.json");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).expect("chmod");

        let (out, err) = run_status(&registry, &state, false);

        assert_eq!(row_cells(&out, "loose")[1], "unreadable", "{out}");
        let detail = out
            .lines()
            .find(|line| line.starts_with("`loose`: "))
            .unwrap_or_else(|| panic!("no detail line: {out}"));
        assert!(detail.contains("loose"), "{detail}");
        assert!(
            detail.contains(&file.display().to_string()),
            "path missing: {detail}"
        );
        assert!(detail.contains("permissions"), "{detail}");
        assert!(!out.contains("AT-SENTINEL-S") && !err.contains("AT-SENTINEL-S"));
        assert_eq!(
            std::fs::metadata(&file).expect("meta").permissions().mode() & 0o777,
            0o644,
            "status never repairs the file"
        );
    }

    #[test]
    fn status_notes_a_plugin_whose_config_cannot_be_read() {
        let dir = TempDir::new().expect("tempdir");
        let registry = registry_of(
            dir.path(),
            &[
                ("garbled", Some("this = = broken")),
                ("gone", None),
                ("garbled-with-login", Some("this = = broken")),
            ],
        );
        let state = dir.path().join("state");
        put_login(
            &state,
            "garbled-with-login",
            "AT-SENTINEL-S",
            None,
            unix_now() + 3600,
            false,
        );

        let (out, err) = run_status(&registry, &state, false);

        assert!(!out.lines().any(|l| l.starts_with("garbled ")), "{out}");
        assert!(!out.lines().any(|l| l.starts_with("gone ")), "{out}");
        assert!(
            err.contains("skipped plugin `garbled`: its config could not be read"),
            "{err}"
        );
        assert!(
            err.contains("skipped plugin `gone`: its config could not be read"),
            "{err}"
        );
        assert!(!err.contains("garbled-with-login"), "{err}");
        assert_eq!(row_cells(&out, "garbled-with-login")[1], "logged-in", "{out}");
    }

    #[test]
    fn describe_expiry_formats_each_range() {
        let now = 1_000_000;
        assert_eq!(describe_expiry(now + 30, now), "in 30s");
        assert_eq!(describe_expiry(now + 3570, now), "in 59m 30s");
        assert_eq!(describe_expiry(now + 7500, now), "in 2h 5m");
        assert_eq!(describe_expiry(now + 90_000, now), "in 1d 1h");
        assert_eq!(describe_expiry(now, now), "now");
        assert_eq!(describe_expiry(now - 180, now), "3m 0s ago");
    }

    /// A cloneable in-memory writer, for a test that reads the output while the
    /// login is still running.
    #[derive(Clone, Default)]
    struct SharedBuf(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl SharedBuf {
        fn text(&self) -> String {
            String::from_utf8_lossy(&self.0.lock().expect("buffer")).into_owned()
        }
    }

    impl Write for SharedBuf {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().expect("buffer").extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
}
