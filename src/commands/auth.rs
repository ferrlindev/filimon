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
use crate::plugins::auth::client::{TokenClient, TokenKind};
use crate::plugins::auth::error::AuthError;
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
        AuthCommand::Logout { plugin } => {
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            let stderr = std::io::stderr();
            let mut err = stderr.lock();
            logout_command(registry, state_dir_flag, plugin, &mut out, &mut err).await
        }
    }
}

// ── Login ─────────────────────────────────────────────────────────────────────

/// The enabled source called `name`, or the reason `login` refuses it.
fn find_login_source<'a>(
    listing: &'a RegistryListing,
    name: &str,
) -> miette::Result<&'a ListedPlugin> {
    let plugin = find_source(listing, name, "login")?;
    if !plugin.enabled {
        return Err(miette::miette!(
            "plugin `{}` is disabled in filimon.toml; set `enabled = true` to log in",
            clean_guest_text(name, NAME_ECHO_CLIP)
        ));
    }
    Ok(plugin)
}

/// The source called `name`, or the reason `verb` refuses it: an unknown name
/// lists the registered sources and a fetcher is named as one. A disabled
/// source is returned; each command decides what that means. Every refusal
/// comes before any file is read or socket opened.
fn find_source<'a>(
    listing: &'a RegistryListing,
    name: &str,
    verb: &str,
) -> miette::Result<&'a ListedPlugin> {
    let shown = clean_guest_text(name, NAME_ECHO_CLIP);
    if let Some(plugin) = listing.sources.iter().find(|plugin| plugin.name == name) {
        return Ok(plugin);
    }
    if listing.fetchers.iter().any(|plugin| plugin.name == name) {
        return Err(miette::miette!(
            "`{shown}` is a fetcher; {verb} applies to sources"
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
/// plugin (unknown, a fetcher, disabled, no `[host.oauth]`, or the
/// `client_credentials` grant, which has no login), a state
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

    // An app-only plugin has nothing to log in to. Say so before `load_plugin`
    // resolves the client secret, so an operator who has not set it yet still
    // gets the right explanation, and before any port, file or directory.
    let probe = probe_oauth_sources(registry)?;
    if probe
        .sources
        .iter()
        .any(|source| source.name == name && source.declares == OAuthDeclared::AppOnly)
    {
        return Err(miette::miette!(
            "plugin `{}` uses the client_credentials grant, so there is nothing to log in to; the host fetches its app token itself",
            clean_guest_text(name, NAME_ECHO_CLIP)
        ));
    }

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
    /// The plugin uses the `client_credentials` grant: the host fetches its own
    /// app token, so there is no login and no stored file to report.
    AppOnly,
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

impl StatusState {
    /// The label shown in the STATE column and the JSON `state` key.
    fn label(&self) -> &'static str {
        match self {
            Self::LoggedIn => "logged-in",
            Self::Expired => "expired",
            Self::NeedsLogin => "needs-login",
            Self::NotLoggedIn => "not-logged-in",
            Self::AppOnly => "app-only",
            Self::Unreadable(_) => "unreadable",
        }
    }
}

/// The table's column headings, in order.
const STATUS_HEADINGS: [&str; 4] = ["PLUGIN", "STATE", "EXPIRES", "REFRESH"];

/// Longest unreadable-file detail kept, in characters.
const DETAIL_CLIP: usize = 240;

/// The rows `status` shows, in registry name order. Each plugin's login is
/// read with `TokenStore::read`: no lock is taken and nothing is created, so
/// this works while `fili run` holds the state directory.
///
/// A row exists when the plugin declares `[host.oauth]`, or has a token file
/// whatever its config says. A plugin whose config could not be read and that
/// has no token file is skipped with a note on `err`.
#[must_use]
pub fn collect_status(
    report: &OAuthProbeReport,
    state_dir: &Path,
    now: i64,
    err: &mut impl Write,
) -> Vec<StatusRow> {
    let mut rows = Vec::new();
    for probe in &report.sources {
        let store = TokenStore::new(state_dir, &probe.name);
        let row = |state, expires_at, has_refresh_token| StatusRow {
            plugin: probe.name.clone(),
            state,
            expires_at,
            has_refresh_token,
        };
        // An app-only plugin has no login: its row never reads the token store,
        // so a stale file of the same alias is neither consulted nor shown.
        if probe.declares == OAuthDeclared::AppOnly {
            rows.push(row(StatusState::AppOnly, None, None));
            continue;
        }
        match store.read() {
            Ok(None) => match probe.declares {
                OAuthDeclared::Yes => rows.push(row(StatusState::NotLoggedIn, None, None)),
                // `AppOnly` was answered above, before the token store was read.
                OAuthDeclared::No | OAuthDeclared::AppOnly => {}
                OAuthDeclared::Unknown(_) => {
                    // A note is best effort: an unwritable stderr must not fail status.
                    writeln!(
                        err,
                        "skipped plugin `{}`: its config could not be read",
                        clean_guest_text(&probe.name, NAME_ECHO_CLIP)
                    )
                    .ok();
                }
            },
            Ok(Some(tokens)) => {
                let state = if tokens.needs_login {
                    StatusState::NeedsLogin
                } else if tokens.expires_at > now {
                    StatusState::LoggedIn
                } else {
                    StatusState::Expired
                };
                rows.push(row(
                    state,
                    Some(tokens.expires_at),
                    Some(tokens.refresh_token.is_some()),
                ));
            }
            Err(error) => rows.push(row(
                StatusState::Unreadable(clean_guest_text(&error.to_string(), DETAIL_CLIP)),
                None,
                None,
            )),
        }
    }
    rows
}

/// A relative description of `expires_at` against `now`: `in 2h 5m`, `now` or
/// `3m 0s ago`. The two largest units are shown, zero components included.
#[must_use]
pub fn describe_expiry(expires_at: i64, now: i64) -> String {
    let delta = expires_at.saturating_sub(now);
    let span = |seconds: u64| {
        let (days, hours, minutes) = (
            seconds / 86_400,
            seconds % 86_400 / 3_600,
            seconds % 3_600 / 60,
        );
        if seconds >= 86_400 {
            format!("{days}d {hours}h")
        } else if seconds >= 3_600 {
            format!("{hours}h {minutes}m")
        } else if seconds >= 60 {
            format!("{minutes}m {}s", seconds % 60)
        } else {
            format!("{seconds}s")
        }
    };
    match delta.cmp(&0) {
        std::cmp::Ordering::Equal => "now".to_string(),
        std::cmp::Ordering::Greater => format!("in {}", span(delta.unsigned_abs())),
        std::cmp::Ordering::Less => format!("{} ago", span(delta.unsigned_abs())),
    }
}

/// An RFC 3339 UTC time for `unix_seconds`, or `None` when it cannot be
/// represented.
fn rfc3339_opt(unix_seconds: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(unix_seconds, 0)
        .map(|time| time.to_rfc3339_opts(chrono::SecondsFormat::Secs, true))
}

/// One row's four cells.
fn status_cells(row: &StatusRow, now: i64) -> [String; 4] {
    let expires = row.expires_at.and_then(|at| {
        rfc3339_opt(at).map(|text| format!("{} ({text})", describe_expiry(at, now)))
    });
    [
        sanitize_for_terminal(&row.plugin),
        row.state.label().to_string(),
        expires.unwrap_or_else(|| "-".to_string()),
        match row.has_refresh_token {
            Some(true) => "yes",
            Some(false) => "no",
            None => "-",
        }
        .to_string(),
    ]
}

/// Writes the human table, then one line per row that needs the user's
/// attention. Columns are left-aligned and padded to the widest cell with two
/// spaces between them; the last column is not padded, so no line ends in a
/// space. Nothing here is built from a token.
///
/// # Errors
/// Returns the write error of `out`.
pub fn render_status(rows: &[StatusRow], now: i64, out: &mut impl Write) -> std::io::Result<()> {
    let table: Vec<[String; 4]> = rows.iter().map(|row| status_cells(row, now)).collect();
    let mut widths = STATUS_HEADINGS.map(|heading| heading.chars().count());
    for row in &table {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }
    let line = |cells: [&str; 4]| {
        let mut text = String::new();
        for (index, cell) in cells.iter().enumerate() {
            text.push_str(cell);
            if index + 1 < cells.len() {
                text.push_str(&" ".repeat(widths[index] - cell.chars().count() + 2));
            }
        }
        text
    };

    writeln!(out, "{}", line(STATUS_HEADINGS))?;
    for cells in &table {
        writeln!(
            out,
            "{}",
            line([
                cells[0].as_str(),
                cells[1].as_str(),
                cells[2].as_str(),
                cells[3].as_str()
            ])
        )?;
    }

    for row in rows {
        let plugin = sanitize_for_terminal(&row.plugin);
        match &row.state {
            StatusState::NeedsLogin => {
                writeln!(
                    out,
                    "`{plugin}`: run `fili auth login {plugin}` to log in again"
                )?;
            }
            StatusState::Expired if row.has_refresh_token == Some(false) => {
                writeln!(
                    out,
                    "`{plugin}`: run `fili auth login {plugin}` to log in again"
                )?;
            }
            StatusState::Unreadable(detail) => {
                writeln!(out, "`{plugin}`: {}", sanitize_for_terminal(detail))?;
            }
            _ => {}
        }
    }
    Ok(())
}

/// Writes the rows as one compact JSON array and a newline, and nothing else.
/// Every object has exactly the keys `plugin`, `state`, `expires_at`,
/// `has_refresh_token` and `detail`; no key can hold a token.
///
/// # Errors
/// Returns the write error of `out`.
pub fn render_status_json(rows: &[StatusRow], out: &mut impl Write) -> std::io::Result<()> {
    let array: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "plugin": sanitize_for_terminal(&row.plugin),
                "state": row.state.label(),
                "expires_at": row.expires_at.and_then(rfc3339_opt),
                "has_refresh_token": row.has_refresh_token,
                "detail": match &row.state {
                    StatusState::Unreadable(detail) => Some(sanitize_for_terminal(detail)),
                    _ => None,
                },
            })
        })
        .collect();
    writeln!(out, "{}", serde_json::Value::Array(array))
}

/// `fili auth status`: every source that declares `[host.oauth]` or has a
/// stored login, with its login state and expiry. It never takes the state
/// directory lock, never creates anything and never reads `.env` or a
/// secret, so it works beside a running `fili run`.
///
/// # Errors
/// The registry's diagnostics, a state directory error for an empty
/// `--state-dir`, or an io error when `out` cannot be written.
pub fn status(
    registry: &Path,
    state_dir_flag: Option<&Path>,
    json: bool,
    out: &mut impl Write,
    err: &mut impl Write,
) -> miette::Result<()> {
    let report = probe_oauth_sources(registry)?;
    let state_dir = state_dir_for(&report.base_dir, state_dir_flag)?;
    let now = chrono::Utc::now().timestamp();
    let rows = collect_status(&report, &state_dir, now, err);
    if json {
        return render_status_json(&rows, out).into_diagnostic();
    }
    if rows.is_empty() {
        writeln!(
            out,
            "No registered plugin declares [host.oauth] or has a stored login in {}.",
            sanitize_for_terminal(&state_dir.display().to_string())
        )
        .into_diagnostic()?;
        return Ok(());
    }
    render_status(&rows, now, out).into_diagnostic()
}

// ── Logout ────────────────────────────────────────────────────────────────────

/// Longest provider or io text echoed in a logout note, in characters.
const LOGOUT_TEXT_CLIP: usize = 200;

/// `fili auth logout <name>`: deletes the plugin's stored login, then tries to
/// revoke it at the provider when the plugin declares a `revocation_url`.
///
/// The plugin's config is loaded only to learn the revocation endpoint (and the
/// client secret that endpoint needs). A plugin that is disabled, or whose
/// config or secret cannot be loaded, still has its login deleted; the login
/// is then simply not revoked, and a note says so.
///
/// # Errors
/// The registry's diagnostics, a refusal naming the plugin (unknown or a
/// fetcher), a state directory error, or a failure to delete the local file.
pub async fn logout_command(
    registry: &Path,
    state_dir_flag: Option<&Path>,
    name: &str,
    out: &mut impl Write,
    err: &mut impl Write,
) -> miette::Result<()> {
    let listing = list_registry(registry)?;
    let source = find_source(&listing, name, "logout")?;
    let state_dir = state_dir_for(&listing.base_dir, state_dir_flag)?;
    let store = TokenStore::new(&state_dir, name);

    let shown = clean_guest_text(name, NAME_ECHO_CLIP);
    let (oauth, note) = if source.enabled {
        match load_plugin(registry, name) {
            Ok(loaded) => {
                let oauth = loaded
                    .sources
                    .get(name)
                    .and_then(|loaded| loaded.plugin.as_ref())
                    .and_then(|plugin| plugin.oauth.clone());
                let note = oauth.is_none().then(|| {
                    format!(
                        "plugin `{shown}` declares no [host.oauth] table, so the login was not revoked at the provider"
                    )
                });
                (oauth, note)
            }
            Err(_) => (
                None,
                Some(format!(
                    "could not load [host.oauth] for `{shown}` (its plugin file or a secret it needs is not usable), so the login was not revoked at the provider"
                )),
            ),
        }
    } else {
        (
            None,
            Some(format!(
                "plugin `{shown}` is disabled, so the login was not revoked at the provider"
            )),
        )
    };
    perform_logout(name, oauth.as_ref(), note.as_deref(), &store, out, err).await
}

/// The text of a failed revocation: the transport or status reason, cleaned.
fn revoke_failure_text(error: &AuthError) -> String {
    match error {
        AuthError::RevokeFailed { reason } => clean_guest_text(reason, LOGOUT_TEXT_CLIP),
        other => clean_guest_text(&other.to_string(), LOGOUT_TEXT_CLIP),
    }
}

/// Logs `name` out over an already loaded `oauth` (`None` when its config
/// could not be used, with `config_note` saying why). The seam the tests use.
///
/// The order is fixed (D-03): read what is stored, delete the local file under
/// the per-alias lock, and only then make the best-effort revoke call. A
/// failed or slow revocation therefore never keeps the local login, and a
/// refresh that was waiting on the lock finds no file and stops. Output is
/// built from the plugin name, fixed phrases and cleaned error text; a token
/// value is never printed (D-06).
///
/// # Errors
/// Only a failure to delete the local file; every other problem is a note.
async fn perform_logout(
    name: &str,
    oauth: Option<&OAuthConfig>,
    config_note: Option<&str>,
    store: &TokenStore,
    out: &mut dyn Write,
    err: &mut dyn Write,
) -> miette::Result<()> {
    // Notes are best effort: a closed terminal must not undo a logout.
    let tokens = match store.read() {
        Ok(None) => {
            writeln!(out, "No stored login for `{name}`; nothing to do.").ok();
            return Ok(());
        }
        Ok(Some(tokens)) => Some(tokens),
        Err(error) => {
            writeln!(
                err,
                "the token file could not be read ({}); it is deleted without revoking the login at the provider",
                clean_guest_text(&error.to_string(), LOGOUT_TEXT_CLIP)
            )
            .ok();
            None
        }
    };

    let lock = store.lock().await.map_err(miette::Report::new)?;
    store.delete(&lock).map_err(miette::Report::new)?;
    drop(lock);
    writeln!(out, "Logged out of `{name}`: the local login was deleted.").ok();

    let Some(tokens) = tokens else {
        return Ok(());
    };
    let Some(oauth) = oauth else {
        if let Some(note) = config_note {
            writeln!(err, "{note}").ok();
        }
        return Ok(());
    };
    if oauth.revocation_url.is_none() {
        writeln!(
            out,
            "This plugin declares no revocation_url, so the login was not revoked at the provider."
        )
        .ok();
        return Ok(());
    }

    let (token, kind) = match &tokens.refresh_token {
        Some(refresh) => (refresh.expose(), TokenKind::Refresh),
        None => (tokens.access_token.expose(), TokenKind::Access),
    };
    let revoked = match TokenClient::new(oauth) {
        Ok(client) => client.revoke(token, kind).await,
        Err(error) => Err(error),
    };
    match revoked {
        Ok(_) => {
            writeln!(out, "The login was revoked at the provider.").ok();
        }
        Err(error) => {
            writeln!(
                err,
                "warning: revoking the login at the provider failed ({}); it is deleted locally but may still be valid at the provider",
                revoke_failure_text(&error)
            )
            .ok();
        }
    }
    Ok(())
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

    /// A plugin file for the app-only grant; its secret variable is unset
    /// everywhere, so a strict load of it fails.
    const APP_ONLY_PLUGIN_TOML: &str = "[host]\npoll_interval_secs = 60\n\n[host.oauth]\ngrant = \"client_credentials\"\ntoken_url = \"https://auth.example/token\"\nclient_id = \"c\"\nclient_secret_env = \"FILI_AUTH_TEST_APP_ONLY_SECRET_UNSET\"\n";

    #[tokio::test]
    async fn login_refuses_a_client_credentials_plugin_before_loading_secrets() {
        let dir = TempDir::new().expect("tempdir");
        let registry = registry_with(
            dir.path(),
            &source_table("stub", true),
            APP_ONLY_PLUGIN_TOML,
        );
        let state = dir.path().join("state");

        let (text, created) = refused(&registry, &state, "stub").await;

        assert!(text.contains("plugin `stub`"), "{text}");
        assert!(text.contains("client_credentials"), "{text}");
        assert!(text.contains("nothing to log in to"), "{text}");
        assert!(
            !text.contains("FILI_AUTH_TEST_APP_ONLY_SECRET_UNSET"),
            "the missing-secret diagnostic must not show: {text}"
        );
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
                std::fs::write(dir.join(format!("{name}.toml")), plugin_toml).expect("plugin toml");
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
        put_login(
            &state,
            "fresh",
            "AT-SENTINEL-S",
            Some("RT-SENTINEL-S"),
            now + 3600,
            false,
        );
        put_login(
            &state,
            "stale",
            "AT-SENTINEL-S",
            Some("RT-SENTINEL-S"),
            now - 180,
            false,
        );
        put_login(
            &state,
            "reauth",
            "AT-SENTINEL-S",
            Some("RT-SENTINEL-S"),
            now + 3600,
            true,
        );
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
                [
                    "detail",
                    "expires_at",
                    "has_refresh_token",
                    "plugin",
                    "state"
                ]
            );
        }
        let fresh = rows
            .iter()
            .find(|row| row["plugin"] == "fresh")
            .expect("fresh");
        assert_eq!(fresh["state"], "logged-in");
        assert_eq!(fresh["has_refresh_token"], true);
        assert!(
            chrono::DateTime::parse_from_rfc3339(fresh["expires_at"].as_str().expect("text"))
                .is_ok()
        );
        let nologin = rows
            .iter()
            .find(|row| row["plugin"] == "nologin")
            .expect("row");
        assert!(nologin["expires_at"].is_null() && nologin["has_refresh_token"].is_null());
        for secret in ["AT-SENTINEL-S", "RT-SENTINEL-S"] {
            assert!(!out.contains(secret), "{out}");
        }
    }

    /// A registry with one app-only plugin and one code-flow plugin, plus a
    /// stale token file of the app-only alias holding sentinel tokens.
    fn app_only_registry(dir: &Path) -> (PathBuf, PathBuf) {
        let registry = registry_of(
            dir,
            &[
                ("appy", Some(APP_ONLY_PLUGIN_TOML)),
                ("coder", Some(OAUTH_PLUGIN_TOML)),
            ],
        );
        let state = dir.join("state");
        put_login(
            &state,
            "appy",
            "AT-SENTINEL-A",
            Some("RT-SENTINEL-A"),
            unix_now() + 3600,
            false,
        );
        (registry, state)
    }

    #[test]
    fn status_lists_an_app_only_plugin_without_reading_a_token_file() {
        let dir = TempDir::new().expect("tempdir");
        let (registry, state) = app_only_registry(dir.path());

        let (out, err) = run_status(&registry, &state, false);

        let cells = row_cells(&out, "appy");
        assert_eq!(cells, ["appy", "app-only", "-", "-"], "{out}");
        assert_eq!(row_cells(&out, "coder")[1], "not-logged-in", "{out}");
        assert!(
            !out.lines().any(|line| line.contains("fili auth login appy")),
            "no attention hint for an app-only row: {out}"
        );
        for secret in ["AT-SENTINEL-A", "RT-SENTINEL-A"] {
            assert!(!out.contains(secret) && !err.contains(secret), "{out}{err}");
        }
    }

    #[test]
    fn status_json_shows_app_only_with_null_expiry_and_refresh() {
        let dir = TempDir::new().expect("tempdir");
        let (registry, state) = app_only_registry(dir.path());

        let (out, err) = run_status(&registry, &state, true);

        let value: serde_json::Value = serde_json::from_str(&out).expect("one JSON value");
        let rows = value.as_array().expect("an array");
        let row = rows
            .iter()
            .find(|row| row["plugin"] == "appy")
            .expect("the app-only row");
        assert_eq!(row["state"], "app-only", "{out}");
        assert!(row["expires_at"].is_null(), "{out}");
        assert!(row["has_refresh_token"].is_null(), "{out}");
        assert!(row["detail"].is_null(), "{out}");
        let mut keys: Vec<&str> = row
            .as_object()
            .expect("an object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            [
                "detail",
                "expires_at",
                "has_refresh_token",
                "plugin",
                "state"
            ]
        );
        for secret in ["AT-SENTINEL-A", "RT-SENTINEL-A"] {
            assert!(!out.contains(secret) && !err.contains(secret), "{out}{err}");
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
        put_login(
            &state,
            "renewable",
            "AT-SENTINEL-S",
            Some("RT-SENTINEL-S"),
            now - 600,
            false,
        );
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
        put_login(
            &state,
            "loose",
            "AT-SENTINEL-S",
            None,
            unix_now() + 3600,
            false,
        );
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
        assert_eq!(
            row_cells(&out, "garbled-with-login")[1],
            "logged-in",
            "{out}"
        );
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

    // ── logout ───────────────────────────────────────────────────────────────

    const SENTINELS: [&str; 2] = ["AT-SENTINEL-L", "RT-SENTINEL-L"];

    /// An `OAuthConfig` whose `revocation_url` is `/revoke` on `server`.
    fn revoking_oauth(server: &TestHttpServer) -> OAuthConfig {
        OAuthConfig {
            revocation_url: Some(url::Url::parse(&server.url("/revoke")).expect("revoke url")),
            ..oauth_config_for(&server.url("/token"), None)
        }
    }

    /// The token file's path for `alias`.
    fn token_file(state: &Path, alias: &str) -> PathBuf {
        state.join("auth").join(format!("{alias}.json"))
    }

    /// Runs `perform_logout`, returning (result, stdout, stderr).
    async fn run_logout(
        name: &str,
        oauth: Option<&OAuthConfig>,
        note: Option<&str>,
        state: &Path,
    ) -> (miette::Result<()>, String, String) {
        let store = TokenStore::new(state, name);
        let mut out = Vec::new();
        let mut err = Vec::new();
        let result = perform_logout(name, oauth, note, &store, &mut out, &mut err).await;
        (
            result,
            String::from_utf8(out).expect("utf8"),
            String::from_utf8(err).expect("utf8"),
        )
    }

    /// A fake endpoint that answers 200 to everything.
    async fn quiet_server() -> TestHttpServer {
        TestHttpServer::start(|_| Reply::json(200, "{}")).await
    }

    #[tokio::test]
    async fn logout_deletes_the_token_file() {
        let state = TempDir::new().expect("tempdir");
        put_login(
            state.path(),
            "stub",
            "AT-SENTINEL-L",
            Some("RT-SENTINEL-L"),
            unix_now() + 3600,
            false,
        );
        let server = quiet_server().await;
        let oauth = oauth_config_for(&server.url("/token"), None);

        let (result, out, err) = run_logout("stub", Some(&oauth), None, state.path()).await;

        result.expect("logout succeeds");
        assert!(!token_file(state.path(), "stub").exists());
        assert!(
            out.contains("Logged out of `stub`: the local login was deleted."),
            "{out}"
        );
        assert!(
            server.seen().is_empty(),
            "no request without a revocation_url"
        );
        assert!(err.is_empty(), "{err}");
    }

    #[tokio::test]
    async fn logout_without_a_stored_login_succeeds_with_a_note_and_creates_nothing() {
        let dir = TempDir::new().expect("tempdir");
        let state = dir.path().join("state");

        let (result, out, err) = run_logout("stub", None, None, &state).await;

        result.expect("logout succeeds");
        assert!(
            out.contains("No stored login for `stub`; nothing to do."),
            "{out}"
        );
        assert!(err.is_empty(), "{err}");
        assert!(!state.exists(), "logout created the state directory");
    }

    #[tokio::test]
    async fn logout_posts_the_refresh_token_to_the_revocation_url_and_deletes_first() {
        let state = TempDir::new().expect("tempdir");
        put_login(
            state.path(),
            "stub",
            "AT-SENTINEL-L",
            Some("RT-SENTINEL-L"),
            unix_now() + 3600,
            false,
        );
        let file = token_file(state.path(), "stub");
        let existed_at_request = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let server = {
            let file = file.clone();
            let flag = std::sync::Arc::clone(&existed_at_request);
            TestHttpServer::start(move |_| {
                flag.store(file.exists(), std::sync::atomic::Ordering::SeqCst);
                Reply::json(200, "{}")
            })
            .await
        };
        let oauth = revoking_oauth(&server);

        let (result, out, err) = run_logout("stub", Some(&oauth), None, state.path()).await;

        result.expect("logout succeeds");
        let seen = server.seen();
        assert_eq!(seen.len(), 1, "{seen:?}");
        assert_eq!(seen[0].method, "POST");
        assert!(seen[0].target.starts_with("/revoke"), "{}", seen[0].target);
        let form = crate::plugins::auth::testing::form_pairs(&seen[0].body);
        let value = |key: &str| {
            form.iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.as_str())
        };
        assert_eq!(value("token"), Some("RT-SENTINEL-L"));
        assert_eq!(value("token_type_hint"), Some("refresh_token"));
        assert!(
            !existed_at_request.load(std::sync::atomic::Ordering::SeqCst),
            "the token file was still there when the revoke request arrived"
        );
        assert!(
            out.contains("The login was revoked at the provider."),
            "{out}"
        );
        assert!(err.is_empty(), "{err}");
    }

    #[tokio::test]
    async fn logout_revokes_the_access_token_when_there_is_no_refresh_token() {
        let state = TempDir::new().expect("tempdir");
        put_login(
            state.path(),
            "stub",
            "AT-SENTINEL-L",
            None,
            unix_now() + 3600,
            false,
        );
        let server = quiet_server().await;
        let oauth = revoking_oauth(&server);

        let (result, _, _) = run_logout("stub", Some(&oauth), None, state.path()).await;

        result.expect("logout succeeds");
        let seen = server.seen();
        assert_eq!(seen.len(), 1, "{seen:?}");
        let form = crate::plugins::auth::testing::form_pairs(&seen[0].body);
        assert!(
            form.contains(&("token".to_string(), "AT-SENTINEL-L".to_string())),
            "{form:?}"
        );
        assert!(
            form.contains(&("token_type_hint".to_string(), "access_token".to_string())),
            "{form:?}"
        );
    }

    #[tokio::test]
    async fn a_failed_revocation_warns_and_still_succeeds_with_the_file_gone() {
        let state = TempDir::new().expect("tempdir");
        put_login(
            state.path(),
            "stub",
            "AT-SENTINEL-L",
            Some("RT-SENTINEL-L"),
            unix_now() + 3600,
            false,
        );
        let server = TestHttpServer::start(|_| {
            Reply::json(
                500,
                r#"{"error":"server_error","error_description":"RT-SENTINEL-L"}"#,
            )
        })
        .await;
        let oauth = revoking_oauth(&server);

        let (result, out, err) = run_logout("stub", Some(&oauth), None, state.path()).await;

        result.expect("a failed revocation is not an error");
        assert!(!token_file(state.path(), "stub").exists());
        assert!(
            err.contains("warning: revoking the login at the provider failed"),
            "{err}"
        );
        assert!(err.contains("HTTP 500"), "{err}");
        assert!(err.contains("may still be valid at the provider"), "{err}");
        for sentinel in SENTINELS {
            assert!(
                !out.contains(sentinel) && !err.contains(sentinel),
                "{out}{err}"
            );
        }
    }

    #[tokio::test]
    async fn logout_without_a_revocation_url_makes_no_network_call() {
        let state = TempDir::new().expect("tempdir");
        put_login(
            state.path(),
            "stub",
            "AT-SENTINEL-L",
            Some("RT-SENTINEL-L"),
            unix_now() + 3600,
            false,
        );
        let server = quiet_server().await;
        let oauth = oauth_config_for(&server.url("/token"), None);

        let (result, out, _) = run_logout("stub", Some(&oauth), None, state.path()).await;

        result.expect("logout succeeds");
        assert!(server.seen().is_empty());
        assert!(out.contains("not revoked at the provider"), "{out}");
        assert!(!token_file(state.path(), "stub").exists());
    }

    #[tokio::test]
    async fn logout_without_a_loadable_config_says_why_and_deletes() {
        let state = TempDir::new().expect("tempdir");
        put_login(
            state.path(),
            "stub",
            "AT-SENTINEL-L",
            None,
            unix_now() + 3600,
            false,
        );

        let (result, _, err) = run_logout(
            "stub",
            None,
            Some("plugin `stub` is disabled, so the login was not revoked at the provider"),
            state.path(),
        )
        .await;

        result.expect("logout succeeds");
        assert!(
            err.contains("plugin `stub` is disabled, so the login was not revoked"),
            "{err}"
        );
        assert!(!token_file(state.path(), "stub").exists());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn logout_with_a_loose_permission_file_deletes_it_without_revoking() {
        use std::os::unix::fs::PermissionsExt;
        let state = TempDir::new().expect("tempdir");
        put_login(
            state.path(),
            "stub",
            "AT-SENTINEL-L",
            Some("RT-SENTINEL-L"),
            unix_now() + 3600,
            false,
        );
        let file = token_file(state.path(), "stub");
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        let server = quiet_server().await;
        let oauth = revoking_oauth(&server);

        let (result, out, err) = run_logout("stub", Some(&oauth), None, state.path()).await;

        result.expect("logout succeeds");
        assert!(!file.exists(), "the loose file is deleted");
        assert!(err.contains("could not be read"), "{err}");
        assert!(err.contains("without revoking"), "{err}");
        assert!(server.seen().is_empty(), "nothing is revoked");
        for sentinel in SENTINELS {
            assert!(
                !out.contains(sentinel) && !err.contains(sentinel),
                "{out}{err}"
            );
        }
    }

    /// `logout_command` against `registry`, expecting a refusal; returns the
    /// error text and whether the state directory exists afterwards.
    async fn logout_refused(registry: &Path, state: &Path, name: &str) -> (String, bool) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let error = logout_command(registry, Some(state), name, &mut out, &mut err)
            .await
            .expect_err("logout is refused");
        assert!(out.is_empty(), "nothing was printed on stdout");
        (format!("{error} {error:?}"), state.exists())
    }

    #[tokio::test]
    async fn logout_refuses_a_fetcher_and_an_unknown_name() {
        let dir = TempDir::new().expect("tempdir");
        let mut text = source_table("a", true);
        text.push_str(&source_table("b", true));
        text.push_str("[fetchers.f]\ncomponent = \"f.wasm\"\n");
        let registry = registry_with(dir.path(), &text, MINIMAL_TOML);
        let state = dir.path().join("state");

        let (fetcher, created) = logout_refused(&registry, &state, "f").await;
        assert!(fetcher.contains("`f` is a fetcher"), "{fetcher}");
        assert!(fetcher.contains("logout applies to sources"), "{fetcher}");
        assert!(!created);

        let (unknown, created) = logout_refused(&registry, &state, "nope").await;
        assert!(unknown.contains("`nope`"), "{unknown}");
        assert!(unknown.contains("registered sources: a, b"), "{unknown}");
        assert!(!created);
    }

    #[tokio::test]
    async fn logout_command_deletes_the_login_of_a_disabled_source() {
        let dir = TempDir::new().expect("tempdir");
        let registry = registry_with(dir.path(), &source_table("stub", false), MINIMAL_TOML);
        let state = dir.path().join("state");
        put_login(
            &state,
            "stub",
            "AT-SENTINEL-L",
            None,
            unix_now() + 3600,
            false,
        );
        let mut out = Vec::new();
        let mut err = Vec::new();

        logout_command(&registry, Some(&state), "stub", &mut out, &mut err)
            .await
            .expect("a disabled source can still log out");

        assert!(!token_file(&state, "stub").exists());
        let err = String::from_utf8(err).expect("utf8");
        assert!(err.contains("plugin `stub` is disabled"), "{err}");
        assert!(err.contains("not revoked at the provider"), "{err}");
    }

    #[tokio::test]
    async fn later_polls_run_without_a_token_after_logout() {
        use crate::plugins::auth::provider::{TokenOutcome, TokenProvider};
        let state = TempDir::new().expect("tempdir");
        put_login(
            state.path(),
            "stub",
            "AT-SENTINEL-L",
            Some("RT-SENTINEL-L"),
            unix_now() + 3600,
            false,
        );
        let provider = TokenProvider::new(
            "stub",
            crate::plugins::auth::testing::oauth_config(),
            state.path(),
        );
        assert!(matches!(
            provider.access_token().await,
            TokenOutcome::Token(_)
        ));

        let (result, _, _) = run_logout("stub", None, None, state.path()).await;

        result.expect("logout succeeds");
        assert!(matches!(
            provider.access_token().await,
            TokenOutcome::NotLoggedIn
        ));
    }

    #[tokio::test]
    async fn a_refresh_never_resurrects_a_logged_out_plugin() {
        use crate::plugins::auth::provider::{TokenOutcome, TokenProvider};
        let state = TempDir::new().expect("tempdir");
        // Due for a refresh: under the 60 s margin, with a refresh token.
        put_login(
            state.path(),
            "stub",
            "AT-SENTINEL-L",
            Some("RT-SENTINEL-L"),
            unix_now() + 10,
            false,
        );
        let refresh_server = TestHttpServer::start(|_| {
            Reply::json(200, &token_body("AT-NEW", Some("RT-NEW"), Some(3600)))
        })
        .await;
        let provider = std::sync::Arc::new(TokenProvider::new(
            "stub",
            oauth_config_for(&refresh_server.url("/token"), None),
            state.path(),
        ));
        let store = TokenStore::new(state.path(), "stub");

        // The logout holds the lock; the poll starts and waits on it.
        let lock = store.lock().await.expect("logout takes the lock");
        let poll = {
            let provider = std::sync::Arc::clone(&provider);
            tokio::spawn(async move { provider.access_token().await })
        };
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;
        assert!(!poll.is_finished(), "the refresh is waiting on the lock");
        store.delete(&lock).expect("delete under the lock");
        drop(lock);

        let outcome = poll.await.expect("the poll finishes");

        assert!(matches!(outcome, TokenOutcome::NotLoggedIn), "{outcome:?}");
        assert!(
            !token_file(state.path(), "stub").exists(),
            "the login was resurrected"
        );
        assert!(
            refresh_server.seen().is_empty(),
            "the provider was contacted"
        );
    }

    #[tokio::test]
    async fn logout_leaves_other_plugins_logins_alone() {
        let state = TempDir::new().expect("tempdir");
        let now = unix_now();
        put_login(
            state.path(),
            "a",
            "AT-SENTINEL-L",
            Some("RT-SENTINEL-L"),
            now + 3600,
            false,
        );
        put_login(state.path(), "b", "AT-SENTINEL-L", None, now + 3600, false);
        let before = std::fs::read(token_file(state.path(), "b")).expect("b's file");

        let (result, _, _) = run_logout("a", None, None, state.path()).await;

        result.expect("logout succeeds");
        assert!(!token_file(state.path(), "a").exists());
        assert_eq!(
            std::fs::read(token_file(state.path(), "b")).expect("b's file"),
            before
        );
    }

    #[tokio::test]
    async fn logout_never_prints_the_tokens() {
        let mut shown: Vec<String> = Vec::new();

        // A revocation that works, one the provider fails and one that finds
        // a file it cannot read.
        for status in [200, 500] {
            let state = TempDir::new().expect("tempdir");
            put_login(
                state.path(),
                "stub",
                "AT-SENTINEL-L",
                Some("RT-SENTINEL-L"),
                unix_now() + 3600,
                false,
            );
            let server = TestHttpServer::start(move |_| {
                Reply::json(
                    status,
                    r#"{"error":"x","error_description":"AT-SENTINEL-L RT-SENTINEL-L"}"#,
                )
            })
            .await;
            let oauth = revoking_oauth(&server);
            let (result, out, err) = run_logout("stub", Some(&oauth), None, state.path()).await;
            shown.push(format!("{result:?} {out} {err}"));
        }
        let state = TempDir::new().expect("tempdir");
        std::fs::create_dir_all(state.path().join("auth")).expect("auth dir");
        std::fs::write(
            token_file(state.path(), "stub"),
            b"{ not json AT-SENTINEL-L RT-SENTINEL-L",
        )
        .expect("corrupt file");
        let (result, out, err) = run_logout("stub", None, None, state.path()).await;
        shown.push(format!("{result:?} {out} {err}"));

        let joined = shown.join("\n");
        for sentinel in SENTINELS {
            assert!(!joined.contains(sentinel), "{sentinel} leaked: {joined}");
        }
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
