// ─────────────────────────────────────────────────────────────────────────────
// tests/auth_cli.rs - `fili auth` through the real binary
//
// These tests drive the compiled `fili` executable. They only ever run the
// refusals of `login`, `status` and `logout`, with token files prepared on
// disk; nothing here opens a browser, reaches a network or needs a Fluvio
// cluster. Every sentinel token is checked for on both output streams.
// ─────────────────────────────────────────────────────────────────────────────

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

/// `[host.oauth]` over example endpoints: enough for `auth` to treat the
/// plugin as one that logs in. `auth` never opens the component.
const OAUTH_PLUGIN: &str = "[host]\npoll_interval_secs = 60\n\n[host.oauth]\n\
    authorize_url = \"https://auth.example/authorize\"\n\
    token_url = \"https://auth.example/token\"\n\
    scopes = [\"read\"]\nclient_id = \"c\"\n";

/// A plugin that declares no `[host.oauth]`.
const PLAIN_PLUGIN: &str = "[host]\npoll_interval_secs = 60\n";

const ACCESS_SENTINEL: &str = "AT-SENTINEL-B";
const REFRESH_SENTINEL: &str = "RT-SENTINEL-B";

/// A `fili` invocation with the environment that could redirect it removed.
fn fili() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_fili"));
    command
        .env_remove("FILI_REGISTRY")
        .env_remove("FILI_STATE_DIR")
        .env_remove("RUST_LOG");
    command
}

/// `fili --registry <registry> --state-dir <state> auth <args>`, run.
fn run_auth(registry: &Path, state: &Path, args: &[&str]) -> Output {
    fili()
        .arg("--registry")
        .arg(registry)
        .arg("--state-dir")
        .arg(state)
        .arg("auth")
        .args(args)
        .output()
        .expect("run fili")
}

fn stdout(output: &Output) -> String {
    String::from_utf8_lossy(&output.stdout).into_owned()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

/// `text` with whitespace and miette's gutter character removed, so a long
/// sentence that miette wrapped can still be searched for.
fn squash(text: &str) -> String {
    text.chars()
        .filter(|c| !c.is_whitespace() && *c != '│')
        .collect()
}

/// Writes a registry of one source per `(name, plugin toml)`, and, with
/// `fetcher`, a fetcher `f`. Returns the registry path.
fn write_registry(dir: &Path, sources: &[(&str, &str)], fetcher: bool) -> PathBuf {
    let mut text = String::new();
    for (name, plugin_toml) in sources {
        write!(
            text,
            "[sources.{name}]\ncomponent = \"{name}.wasm\"\nconfig = \"{name}.toml\"\n\n"
        )
        .expect("write to a string");
        std::fs::write(dir.join(format!("{name}.toml")), plugin_toml).expect("write plugin toml");
    }
    if fetcher {
        text.push_str("[fetchers.f]\ncomponent = \"f.wasm\"\n");
    }
    let path = dir.join("filimon.toml");
    std::fs::write(&path, text).expect("write registry");
    path
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("after the epoch")
        .as_secs()
        .try_into()
        .expect("fits i64")
}

/// Writes `bytes` as `<state>/auth/<alias>.json` with `mode`.
fn put_raw(state: &Path, alias: &str, bytes: &[u8], mode: u32) -> PathBuf {
    let dir = state.join("auth");
    std::fs::create_dir_all(&dir).expect("auth dir");
    let path = dir.join(format!("{alias}.json"));
    std::fs::write(&path, bytes).expect("write the token file");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode))
            .expect("chmod the token file");
    }
    #[cfg(not(unix))]
    let _ = mode;
    path
}

/// Writes a valid version-1 token file for `alias`, mode 0600, with the
/// sentinel tokens.
fn put_login(
    state: &Path,
    alias: &str,
    refresh: Option<&str>,
    expires_at: i64,
    needs_login: bool,
) -> PathBuf {
    let body = serde_json::json!({
        "version": 1,
        "plugin": alias,
        "access_token": ACCESS_SENTINEL,
        "refresh_token": refresh,
        "expires_at": expires_at,
        "token_type": "Bearer",
        "scope": "read",
        "needs_login": needs_login,
        "obtained_at": expires_at - 3600,
    });
    put_raw(state, alias, body.to_string().as_bytes(), 0o600)
}

/// Neither stream carries a token value.
fn assert_no_tokens(output: &Output) {
    let (out, err) = (stdout(output), stderr(output));
    for secret in [ACCESS_SENTINEL, REFRESH_SENTINEL] {
        assert!(
            !out.contains(secret) && !err.contains(secret),
            "{secret} was printed:\nstdout: {out}\nstderr: {err}"
        );
    }
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

/// A registry of five OAuth plugins and a state directory holding them in the
/// five states `status` reports. Returns both paths.
fn five_states(dir: &Path) -> (PathBuf, PathBuf) {
    let registry = write_registry(
        dir,
        &[
            ("broken", OAUTH_PLUGIN),
            ("fresh", OAUTH_PLUGIN),
            ("nologin", OAUTH_PLUGIN),
            ("reauth", OAUTH_PLUGIN),
            ("stale", OAUTH_PLUGIN),
        ],
        false,
    );
    let state = dir.join("state");
    put_login(&state, "fresh", Some(REFRESH_SENTINEL), now() + 3600, false);
    put_login(&state, "stale", Some(REFRESH_SENTINEL), now() - 180, false);
    put_login(&state, "reauth", Some(REFRESH_SENTINEL), now() + 3600, true);
    put_raw(
        &state,
        "broken",
        format!("{{ not json {ACCESS_SENTINEL}").as_bytes(),
        0o600,
    );
    (registry, state)
}

// ── Usage and refusals ────────────────────────────────────────────────────────

#[test]
fn auth_without_a_subcommand_is_a_usage_error() {
    let output = fili().arg("auth").output().expect("run fili");

    assert_eq!(output.status.code(), Some(2), "{}", stderr(&output));
    assert!(stderr(&output).contains("Usage"), "{}", stderr(&output));
    assert!(stdout(&output).is_empty(), "{}", stdout(&output));
}

#[test]
fn login_for_an_unknown_plugin_exits_nonzero_and_lists_the_registered_ones() {
    let dir = TempDir::new().expect("tempdir");
    let registry = write_registry(
        dir.path(),
        &[("alpha", OAUTH_PLUGIN), ("beta", OAUTH_PLUGIN)],
        false,
    );
    let state = dir.path().join("state");

    let output = run_auth(&registry, &state, &["login", "nope"]);

    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let text = squash(&stderr(&output));
    assert!(text.contains("`nope`"), "{text}");
    assert!(text.contains("registeredsources:alpha,beta"), "{text}");
    assert!(stdout(&output).is_empty(), "{}", stdout(&output));
    assert!(!state.exists(), "login must not create the state directory");
}

#[test]
fn login_for_a_plugin_without_host_oauth_exits_nonzero_naming_the_plugin() {
    let dir = TempDir::new().expect("tempdir");
    let registry = write_registry(dir.path(), &[("plain", PLAIN_PLUGIN)], false);
    let state = dir.path().join("state");

    // `output()` returns only when the command does: a listener that waited
    // for a browser would hang here until the test runner's timeout.
    let output = run_auth(&registry, &state, &["login", "plain", "--no-browser"]);

    assert_eq!(output.status.code(), Some(1), "{}", stderr(&output));
    let text = squash(&stderr(&output));
    assert!(text.contains("plugin`plain`"), "{text}");
    assert!(text.contains("declaresno[host.oauth]"), "{text}");
    assert!(stdout(&output).is_empty(), "{}", stdout(&output));
    assert!(!state.exists(), "login must not create the state directory");
}

// ── Status ────────────────────────────────────────────────────────────────────

#[test]
fn status_prints_each_state_without_token_values_and_no_banner() {
    let dir = TempDir::new().expect("tempdir");
    let (registry, state) = five_states(dir.path());

    let output = run_auth(&registry, &state, &["status"]);

    assert!(
        output.status.success(),
        "exit {:?}, stderr: {}",
        output.status.code(),
        stderr(&output)
    );
    let out = stdout(&output);
    let header = out.lines().next().expect("a header line");
    assert_eq!(
        header.split_whitespace().collect::<Vec<_>>(),
        ["PLUGIN", "STATE", "EXPIRES", "REFRESH"],
        "{out}"
    );
    let state_of = |plugin: &str| row_cells(&out, plugin)[1].clone();
    assert_eq!(state_of("broken"), "unreadable", "{out}");
    assert_eq!(state_of("fresh"), "logged-in", "{out}");
    assert_eq!(state_of("nologin"), "not-logged-in", "{out}");
    assert_eq!(state_of("reauth"), "needs-login", "{out}");
    assert_eq!(state_of("stale"), "expired", "{out}");
    assert!(!out.contains('╔'), "the banner leaked into stdout: {out}");
    assert!(
        !stderr(&output).contains('╔'),
        "the banner leaked into stderr: {}",
        stderr(&output)
    );
    assert_no_tokens(&output);
}

#[test]
fn status_json_stdout_parses_as_an_array() {
    let dir = TempDir::new().expect("tempdir");
    let (registry, state) = five_states(dir.path());

    let output = run_auth(&registry, &state, &["status", "--json"]);

    assert!(
        output.status.success(),
        "exit {:?}, stderr: {}",
        output.status.code(),
        stderr(&output)
    );
    assert_eq!(output.stdout.first(), Some(&b'['), "{}", stdout(&output));
    let rows: Vec<serde_json::Value> =
        serde_json::from_slice(&output.stdout).expect("stdout is one JSON array");
    assert_eq!(rows.len(), 5, "{rows:?}");
    for row in &rows {
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
            ],
            "{row}"
        );
    }
    let fresh = rows
        .iter()
        .find(|row| row["plugin"] == "fresh")
        .expect("a fresh row");
    assert_eq!(fresh["state"], "logged-in");
    assert_eq!(fresh["has_refresh_token"], true);
    assert_no_tokens(&output);
}

#[test]
fn status_works_while_the_state_directory_lock_is_held() {
    let dir = TempDir::new().expect("tempdir");
    let (registry, state) = five_states(dir.path());
    // The lock `fili run` holds on its state directory.
    let lock = std::fs::File::create(state.join(".lock")).expect("create the lock file");
    lock.lock().expect("hold the state directory lock");

    let output = run_auth(&registry, &state, &["status"]);

    lock.unlock().expect("release the lock");
    assert!(
        output.status.success(),
        "exit {:?}, stderr: {}",
        output.status.code(),
        stderr(&output)
    );
    let out = stdout(&output);
    assert_eq!(row_cells(&out, "fresh")[1], "logged-in", "{out}");
    assert_eq!(row_cells(&out, "nologin")[1], "not-logged-in", "{out}");
}

#[test]
fn status_does_not_create_the_state_dir() {
    let dir = TempDir::new().expect("tempdir");
    let registry = write_registry(dir.path(), &[("alpha", OAUTH_PLUGIN)], false);
    let state = dir.path().join("state");

    let output = run_auth(&registry, &state, &["status"]);

    assert!(
        output.status.success(),
        "exit {:?}, stderr: {}",
        output.status.code(),
        stderr(&output)
    );
    assert_eq!(
        row_cells(&stdout(&output), "alpha")[1],
        "not-logged-in",
        "{}",
        stdout(&output)
    );
    assert!(!state.exists(), "status must not create the state directory");
}

#[cfg(unix)]
#[test]
fn status_never_prints_a_token_even_for_a_corrupt_file() {
    use std::os::unix::fs::PermissionsExt;

    let dir = TempDir::new().expect("tempdir");
    let registry = write_registry(
        dir.path(),
        &[("garbled", OAUTH_PLUGIN), ("loose", OAUTH_PLUGIN)],
        false,
    );
    let state = dir.path().join("state");
    // Not JSON, with a token in it.
    put_raw(
        &state,
        "garbled",
        format!("{{ not json {ACCESS_SENTINEL} {REFRESH_SENTINEL}").as_bytes(),
        0o600,
    );
    // A valid file that anyone may read.
    let loose = put_login(&state, "loose", Some(REFRESH_SENTINEL), now() + 3600, false);
    std::fs::set_permissions(&loose, std::fs::Permissions::from_mode(0o644)).expect("chmod 644");

    for args in [["status"].as_slice(), ["status", "--json"].as_slice()] {
        let output = run_auth(&registry, &state, args);

        assert!(
            output.status.success(),
            "exit {:?}, stderr: {}",
            output.status.code(),
            stderr(&output)
        );
        let out = stdout(&output);
        assert!(out.contains("unreadable"), "{out}");
        assert!(out.contains("loose"), "{out}");
        assert!(
            squash(&out).contains(&squash(&loose.display().to_string())),
            "the path of the loose file is missing: {out}"
        );
        assert_no_tokens(&output);
    }
    assert_eq!(
        std::fs::metadata(&loose)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777,
        0o644,
        "status never repairs the file"
    );
}

// ── Logout ────────────────────────────────────────────────────────────────────

#[test]
fn logout_deletes_the_token_file_and_a_second_logout_says_there_is_none() {
    let dir = TempDir::new().expect("tempdir");
    let registry = write_registry(dir.path(), &[("alpha", OAUTH_PLUGIN)], false);
    let state = dir.path().join("state");
    let file = put_login(&state, "alpha", Some(REFRESH_SENTINEL), now() + 3600, false);

    let first = run_auth(&registry, &state, &["logout", "alpha"]);

    assert!(
        first.status.success(),
        "exit {:?}, stderr: {}",
        first.status.code(),
        stderr(&first)
    );
    assert!(
        stdout(&first).contains("Logged out of `alpha`"),
        "{}",
        stdout(&first)
    );
    assert!(!file.exists(), "the token file was deleted");
    assert_no_tokens(&first);

    let second = run_auth(&registry, &state, &["logout", "alpha"]);

    assert!(
        second.status.success(),
        "exit {:?}, stderr: {}",
        second.status.code(),
        stderr(&second)
    );
    assert!(
        stdout(&second).contains("No stored login for `alpha`"),
        "{}",
        stdout(&second)
    );
    assert_no_tokens(&second);
}

#[test]
fn logout_refuses_a_fetcher_and_an_unknown_name() {
    let dir = TempDir::new().expect("tempdir");
    let registry = write_registry(dir.path(), &[("alpha", OAUTH_PLUGIN)], true);
    let state = dir.path().join("state");

    let fetcher = run_auth(&registry, &state, &["logout", "f"]);
    assert_eq!(fetcher.status.code(), Some(1), "{}", stderr(&fetcher));
    let text = squash(&stderr(&fetcher));
    assert!(text.contains("`f`isafetcher"), "{text}");

    let unknown = run_auth(&registry, &state, &["logout", "nope"]);
    assert_eq!(unknown.status.code(), Some(1), "{}", stderr(&unknown));
    let text = squash(&stderr(&unknown));
    assert!(text.contains("`nope`"), "{text}");
    assert!(text.contains("registeredsources:alpha"), "{text}");

    assert!(
        !state.exists(),
        "a refused logout must not create the state directory"
    );
}
