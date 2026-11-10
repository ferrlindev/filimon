// ─────────────────────────────────────────────────────────────────────────────
// commands/plugins.rs - `fili plugins` subcommands
//
// `list` shows every registered plugin and, for a source, the outcome of its
// last poll as the scheduler recorded it in `<state_dir>/<alias>.status.json`.
// It reads `filimon.toml` and those status files only: it never loads a plugin
// file, a secret or a component, never takes the state directory lock and
// never creates or writes anything, so it is safe while `fili run` is polling.
//
// Everything that came from a file is untrusted at print time and passes
// through `sanitize_for_terminal` or `clean_guest_text`.
// ─────────────────────────────────────────────────────────────────────────────

use std::io::Write;
use std::path::{Path, PathBuf};

use miette::IntoDiagnostic;

use crate::cli::PluginsCommand;
use crate::output::sanitize_for_terminal;
use crate::plugins::config::{RegistryListing, list_registry};
use crate::plugins::host::HostOptions;
use crate::plugins::host::cursor::{PollStatus, StatusRead, read_status, require_state_dir};
use crate::plugins::host::engine::clean_guest_text;

/// Longest error text shown in the human table, in characters.
const TABLE_ERROR_CLIP: usize = 80;

/// Longest error text kept in the JSON output, in characters.
const JSON_ERROR_CLIP: usize = 512;

/// Longest timestamp text kept in the JSON output, in characters.
const JSON_AT_CLIP: usize = 64;

/// The table's column headings, in order.
const HEADINGS: [&str; 5] = ["NAME", "KIND", "ENABLED", "FETCHER", "LAST POLL"];

// ── Collecting ────────────────────────────────────────────────────────────────

/// What `list` knows about a plugin's last poll.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LastPoll {
    /// Fetchers are not polled.
    NotApplicable,
    /// A source with no status file.
    Never,
    Status(PollStatus),
    /// A status file that cannot be used, with the fixed reason.
    Unreadable(String),
}

/// One table row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListRow {
    /// `"source"` or `"fetcher"`.
    pub kind: &'static str,
    pub name: String,
    pub enabled: bool,
    pub fetcher: Option<String>,
    pub last_poll: LastPoll,
}

/// The rows for `listing`: sources first, then fetchers, each in name order.
/// A source's status is read from `state_dir` without locking or creating
/// anything; a fetcher's status file is never read.
#[must_use]
pub fn collect_list(listing: &RegistryListing, state_dir: &Path) -> Vec<ListRow> {
    let sources = listing.sources.iter().map(|plugin| ListRow {
        kind: "source",
        name: plugin.name.clone(),
        enabled: plugin.enabled,
        fetcher: plugin.fetcher.clone(),
        last_poll: match read_status(state_dir, &plugin.name) {
            StatusRead::Missing => LastPoll::Never,
            StatusRead::Found(status) => LastPoll::Status(status),
            StatusRead::Unreadable(reason) => LastPoll::Unreadable(reason),
        },
    });
    let fetchers = listing.fetchers.iter().map(|plugin| ListRow {
        kind: "fetcher",
        name: plugin.name.clone(),
        enabled: plugin.enabled,
        fetcher: None,
        last_poll: LastPoll::NotApplicable,
    });
    sources.chain(fetchers).collect()
}

// ── Rendering ─────────────────────────────────────────────────────────────────

/// The LAST POLL cell.
fn last_poll_cell(last_poll: &LastPoll) -> String {
    match last_poll {
        LastPoll::NotApplicable => "n/a".to_string(),
        LastPoll::Never => "-".to_string(),
        LastPoll::Unreadable(reason) => {
            format!("unreadable ({})", clean_guest_text(reason, 160))
        }
        LastPoll::Status(status) => {
            let at = clean_guest_text(&status.at, 40);
            if status.ok {
                match status.items {
                    None => format!("{at} ok"),
                    Some(1) => format!("{at} ok (1 item)"),
                    Some(n) => format!("{at} ok ({n} items)"),
                }
            } else {
                let error = status.error.as_deref().unwrap_or("no message");
                format!("{at} error: {}", clean_guest_text(error, TABLE_ERROR_CLIP))
            }
        }
    }
}

/// One row's five cells, every string cleaned.
fn cells(row: &ListRow) -> [String; 5] {
    [
        sanitize_for_terminal(&row.name),
        row.kind.to_string(),
        if row.enabled { "yes" } else { "no" }.to_string(),
        row.fetcher
            .as_deref()
            .map_or_else(|| "-".to_string(), |name| clean_guest_text(name, 64)),
        last_poll_cell(&row.last_poll),
    ]
}

/// Writes the human table: a header line, then one line per row. Columns are
/// left-aligned and padded to the widest cell with two spaces between them;
/// the last column is not padded, so no line ends in a space.
///
/// # Errors
/// Returns the write error of `out`.
pub fn render_list(rows: &[ListRow], out: &mut impl Write) -> std::io::Result<()> {
    let table: Vec<[String; 5]> = rows.iter().map(cells).collect();
    let mut widths = HEADINGS.map(|heading| heading.chars().count());
    for row in &table {
        for (width, cell) in widths.iter_mut().zip(row) {
            *width = (*width).max(cell.chars().count());
        }
    }

    let line = |row: [&str; 5]| {
        let mut text = String::new();
        for (index, cell) in row.iter().enumerate() {
            text.push_str(cell);
            if index + 1 < row.len() {
                let padding = widths[index] - cell.chars().count();
                text.push_str(&" ".repeat(padding + 2));
            }
        }
        text
    };

    writeln!(out, "{}", line(HEADINGS))?;
    for row in &table {
        let refs = [
            row[0].as_str(),
            row[1].as_str(),
            row[2].as_str(),
            row[3].as_str(),
            row[4].as_str(),
        ];
        writeln!(out, "{}", line(refs))?;
    }
    Ok(())
}

/// The `last_poll` object of one JSON row. Every string that came from a file
/// is cleaned and bounded before it is put into the value.
fn last_poll_json(last_poll: &LastPoll) -> serde_json::Value {
    let object = |state: &str, at: Option<&str>, items: Option<u64>, error: Option<&str>| {
        serde_json::json!({
            "state": state,
            "at": at.map(|text| clean_guest_text(text, JSON_AT_CLIP)),
            "items": items,
            "error": error.map(|text| clean_guest_text(text, JSON_ERROR_CLIP)),
        })
    };
    match last_poll {
        LastPoll::Never => object("never", None, None, None),
        LastPoll::NotApplicable => object("not-applicable", None, None, None),
        LastPoll::Unreadable(reason) => object("unreadable", None, None, Some(reason)),
        LastPoll::Status(status) => object(
            if status.ok { "ok" } else { "error" },
            Some(&status.at),
            status.items,
            status.error.as_deref(),
        ),
    }
}

/// Writes the rows as one compact JSON array followed by a single newline.
/// Nothing else is ever written to `out` in this mode.
///
/// # Errors
/// Returns the write error of `out`.
pub fn render_list_json(rows: &[ListRow], out: &mut impl Write) -> std::io::Result<()> {
    let array: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            serde_json::json!({
                "name": sanitize_for_terminal(&row.name),
                "kind": row.kind,
                "enabled": row.enabled,
                "fetcher": row.fetcher.as_deref().map(|name| clean_guest_text(name, 64)),
                "last_poll": last_poll_json(&row.last_poll),
            })
        })
        .collect();
    let text = serde_json::Value::Array(array).to_string();
    writeln!(out, "{text}")
}

// ── Commands ──────────────────────────────────────────────────────────────────

/// Resolves the state directory and refuses an empty or blank one, so no
/// state file can land in the working directory (WR-07). Every command that
/// touches the state directory resolves it through here.
fn state_dir_for(base_dir: &Path, flag: Option<&Path>) -> miette::Result<PathBuf> {
    let dir = HostOptions::resolve_state_dir(base_dir, flag);
    require_state_dir(&dir)?;
    Ok(dir)
}

/// `fili plugins list`.
///
/// # Errors
/// Returns the registry's diagnostics when `registry` cannot be read or
/// parsed, a state directory error for an empty `--state-dir`, or an io error
/// when `out` cannot be written. `err` receives one note per registry
/// name that was skipped because it breaks the name rule.
pub fn list(
    registry: &Path,
    state_dir_flag: Option<&Path>,
    json: bool,
    out: &mut impl Write,
    err: &mut impl Write,
) -> miette::Result<()> {
    let listing = list_registry(registry)?;
    let state_dir = state_dir_for(&listing.base_dir, state_dir_flag)?;
    for skipped in &listing.skipped {
        // A note is best effort: an unwritable stderr must not fail the listing.
        writeln!(
            err,
            "skipped [{}.{}]: plugin names may only contain a-z, 0-9, `_` and `-`",
            skipped.table,
            clean_guest_text(&skipped.name, 64)
        )
        .ok();
    }
    let rows = collect_list(&listing, &state_dir);
    if json {
        return render_list_json(&rows, out).into_diagnostic();
    }
    if rows.is_empty() {
        writeln!(
            out,
            "No plugins are registered in {}.",
            sanitize_for_terminal(&listing.path.display().to_string())
        )
        .into_diagnostic()?;
        return Ok(());
    }
    render_list(&rows, out).into_diagnostic()
}

/// Runs one `fili plugins` subcommand, writing to stdout.
///
/// # Errors
/// Returns whatever the subcommand returns.
pub fn dispatch(
    command: &PluginsCommand,
    registry: &Path,
    state_dir_flag: Option<&Path>,
) -> miette::Result<()> {
    match command {
        PluginsCommand::List { json } => {
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            let stderr = std::io::stderr();
            let mut err = stderr.lock();
            list(registry, state_dir_flag, *json, &mut out, &mut err)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fmt::Write as _;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use tempfile::TempDir;

    use super::*;
    use crate::plugins::host::cursor::CursorStore;
    use crate::plugins::host::sink::VecSink;
    use crate::plugins::host::testing::{
        FakeFactory, FakeStep, TestSource, run_for, spec, wit_item, write_registry,
    };

    fn rendered(rows: &[ListRow]) -> String {
        let mut out = Vec::new();
        render_list(rows, &mut out).expect("render");
        String::from_utf8(out).expect("utf8")
    }

    #[test]
    fn list_rejects_an_empty_state_dir() {
        let dir = TempDir::new().expect("tempdir");
        let registry = write_registry(dir.path(), &[]);
        let mut out = Vec::new();
        let error = list(
            &registry,
            Some(Path::new("")),
            false,
            &mut out,
            &mut Vec::new(),
        )
        .expect_err("an empty state directory is refused");
        let text = error.to_string();
        assert!(text.contains("state directory must not be empty"), "{text}");
        assert!(out.is_empty(), "nothing is printed before the refusal");
    }

    /// The tracer: a polled plugin's last poll shows up in the list.
    #[tokio::test(start_paused = true)]
    async fn tracer_polled_status_appears_in_the_list() {
        let dir = TempDir::new().expect("tempdir");
        let registry = write_registry(
            dir.path(),
            &[TestSource {
                name: "stub".to_string(),
                component: PathBuf::from("stub.wasm"),
                enabled: true,
                interval_secs: 60,
                backoff: None,
                settings: String::new(),
            }],
        );
        let state = dir.path().join("state");
        let cursors = Arc::new(CursorStore::open(&state).expect("cursor store"));
        let factory = Arc::new(FakeFactory::new(vec![
            FakeStep::InitOk,
            FakeStep::PollOk {
                items: vec![wit_item("a")],
                next_cursor: Some(vec![1]),
                delay: Duration::ZERO,
            },
        ]));
        run_for(
            spec("stub", 60, 60, 3600, 2.0),
            factory,
            Arc::new(VecSink::default()),
            Arc::clone(&cursors),
            Duration::from_secs(1),
        )
        .await;

        let listing = list_registry(&registry).expect("listing");
        let text = rendered(&collect_list(&listing, &state));
        let mut lines = text.lines();
        let header = lines.next().expect("header");
        assert_eq!(
            header.split_whitespace().collect::<Vec<_>>(),
            ["NAME", "KIND", "ENABLED", "FETCHER", "LAST", "POLL"]
        );
        let row = lines.next().expect("stub row");
        assert!(lines.next().is_none(), "one row only: {text}");

        let cells: Vec<&str> = row.split("  ").filter(|c| !c.trim().is_empty()).collect();
        assert_eq!(&cells[..4], ["stub", "source", "yes", "-"], "{row:?}");
        let last_poll = cells[4].trim();
        let pattern = regex::Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z ok \(1 item\)$")
            .expect("regex");
        assert!(pattern.is_match(last_poll), "{last_poll:?}");
    }

    /// A registry with source `stub` (polled once by the fakes) and fetcher `f`.
    #[tokio::test(start_paused = true)]
    async fn list_json_prints_one_array_with_a_row_per_plugin() {
        let dir = TempDir::new().expect("tempdir");
        let registry = write_registry(
            dir.path(),
            &[TestSource {
                name: "stub".to_string(),
                component: PathBuf::from("stub.wasm"),
                enabled: true,
                interval_secs: 60,
                backoff: None,
                settings: String::new(),
            }],
        );
        let mut text = std::fs::read_to_string(&registry).expect("read registry");
        text.push_str("[fetchers.f]\ncomponent = \"f.wasm\"\n");
        std::fs::write(&registry, text).expect("write registry");

        let state = dir.path().join("state");
        let cursors = Arc::new(CursorStore::open(&state).expect("cursor store"));
        let factory = Arc::new(FakeFactory::new(vec![
            FakeStep::InitOk,
            FakeStep::PollOk {
                items: vec![wit_item("a")],
                next_cursor: Some(vec![1]),
                delay: Duration::ZERO,
            },
        ]));
        run_for(
            spec("stub", 60, 60, 3600, 2.0),
            factory,
            Arc::new(VecSink::default()),
            Arc::clone(&cursors),
            Duration::from_secs(1),
        )
        .await;
        let at = match read_status(&state, "stub") {
            StatusRead::Found(status) => status.at,
            other => panic!("expected a status, got {other:?}"),
        };

        let listing = list_registry(&registry).expect("listing");
        let mut out = Vec::new();
        render_list_json(&collect_list(&listing, &state), &mut out).expect("render");
        let text = String::from_utf8(out).expect("utf8");
        assert!(text.ends_with("}]\n"), "one line, one newline: {text:?}");
        assert_eq!(text.matches('\n').count(), 1, "{text:?}");

        let value: serde_json::Value = serde_json::from_str(&text).expect("valid json");
        assert_eq!(
            value,
            serde_json::json!([
                {
                    "name": "stub",
                    "kind": "source",
                    "enabled": true,
                    "fetcher": null,
                    "last_poll": {"state": "ok", "at": at, "items": 1, "error": null}
                },
                {
                    "name": "f",
                    "kind": "fetcher",
                    "enabled": true,
                    "fetcher": null,
                    "last_poll": {
                        "state": "not-applicable", "at": null, "items": null, "error": null
                    }
                }
            ])
        );
    }

    // ── Helpers for the list coverage tests ──────────────────────────────────

    /// Writes `text` as `filimon.toml` inside `dir` and returns its path.
    fn write_registry_text(dir: &Path, text: &str) -> PathBuf {
        let path = dir.join("filimon.toml");
        std::fs::write(&path, text).expect("write registry");
        path
    }

    /// A one-line source table whose files are never opened by `list`.
    fn source_table(name: &str, enabled: bool) -> String {
        format!(
            "[sources.{name}]\ncomponent = \"{name}.wasm\"\nconfig = \"{name}.toml\"\nenabled = {enabled}\n\n"
        )
    }

    /// Writes `<alias>.status.json` into `state`, creating the directory.
    fn write_status_file(state: &Path, alias: &str, body: &str) {
        std::fs::create_dir_all(state).expect("state dir");
        std::fs::write(state.join(format!("{alias}.status.json")), body)
            .expect("write status file");
    }

    /// A well-formed status file body.
    fn status_body(
        alias: &str,
        at: &str,
        ok: bool,
        error: Option<&str>,
        items: Option<u64>,
    ) -> String {
        serde_json::json!({
            "version": 1,
            "plugin": alias,
            "at": at,
            "ok": ok,
            "error": error,
            "items": items,
        })
        .to_string()
    }

    /// The listing's rows for `registry` against `state`.
    fn rows_for(registry: &Path, state: &Path) -> Vec<ListRow> {
        let listing = list_registry(registry).expect("listing");
        collect_list(&listing, state)
    }

    /// Runs `list` and returns (stdout, stderr).
    fn run_list(registry: &Path, state: &Path, json: bool) -> (String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        list(registry, Some(state), json, &mut out, &mut err).expect("list succeeds");
        (
            String::from_utf8(out).expect("utf8 stdout"),
            String::from_utf8(err).expect("utf8 stderr"),
        )
    }

    /// The first token of every table row after the header.
    fn table_names(table: &str) -> Vec<&str> {
        table
            .lines()
            .skip(1)
            .filter_map(|line| line.split_whitespace().next())
            .collect()
    }

    // ── Row coverage ─────────────────────────────────────────────────────────

    #[test]
    fn list_rows_cover_every_status_shape() {
        let dir = TempDir::new().expect("tempdir");
        let state = dir.path().join("state");
        let mut text = String::new();
        for (name, enabled) in [
            ("a_never", true),
            ("b_off", false),
            ("c_ok", true),
            ("d_err", true),
            ("e_garbage", true),
            ("f_empty", true),
        ] {
            text.push_str(&source_table(name, enabled));
        }
        text.push_str("[fetchers.fetch]\ncomponent = \"fetch.wasm\"\n");
        let registry = write_registry_text(dir.path(), &text);
        write_status_file(
            &state,
            "c_ok",
            &status_body("c_ok", "2026-01-02T03:04:05Z", true, None, Some(3)),
        );
        write_status_file(
            &state,
            "d_err",
            &status_body("d_err", "2026-01-02T03:04:06Z", false, Some("boom"), None),
        );
        write_status_file(&state, "e_garbage", "this is not json");
        write_status_file(&state, "f_empty", "");

        let rows = rows_for(&registry, &state);
        let cell = |name: &str| {
            let row = rows
                .iter()
                .find(|row| row.name == name)
                .unwrap_or_else(|| panic!("row {name}"));
            cells(row)
        };

        assert_eq!(cell("a_never")[4], "-");
        assert_eq!(cell("b_off")[2], "no");
        assert_eq!(cell("b_off")[4], "-");
        assert_eq!(cell("c_ok")[2], "yes");
        assert_eq!(cell("c_ok")[4], "2026-01-02T03:04:05Z ok (3 items)");
        assert_eq!(cell("d_err")[4], "2026-01-02T03:04:06Z error: boom");
        assert_eq!(cell("e_garbage")[4], "unreadable (not a valid status file)");
        assert_eq!(cell("f_empty")[4], "unreadable (not a valid status file)");
        assert_eq!(cell("fetch")[1], "fetcher");
        assert_eq!(cell("fetch")[4], "n/a");
        assert_eq!(rows.len(), 7, "every plugin has a row");
    }

    #[test]
    fn list_orders_sources_then_fetchers_by_name() {
        let dir = TempDir::new().expect("tempdir");
        let state = dir.path().join("state");
        let mut text = String::new();
        for name in ["zeta", "alpha", "mid"] {
            text.push_str(&source_table(name, true));
        }
        for name in ["b", "a"] {
            writeln!(text, "[fetchers.{name}]\ncomponent = \"{name}.wasm\"").expect("append");
        }
        let registry = write_registry_text(dir.path(), &text);
        let expected = ["alpha", "mid", "zeta", "a", "b"];

        let (table, _) = run_list(&registry, &state, false);
        assert_eq!(table_names(&table), expected, "{table}");

        let (json, _) = run_list(&registry, &state, true);
        let value: serde_json::Value = serde_json::from_str(&json).expect("json");
        let json_names: Vec<&str> = value
            .as_array()
            .expect("array")
            .iter()
            .map(|row| row["name"].as_str().expect("name"))
            .collect();
        assert_eq!(json_names, expected);
        let (again, _) = run_list(&registry, &state, true);
        assert_eq!(again, json, "identical across runs");
    }

    #[test]
    fn list_keeps_same_named_source_and_fetcher_as_two_rows() {
        let dir = TempDir::new().expect("tempdir");
        let state = dir.path().join("state");
        let mut text = source_table("shared", true);
        text.push_str("[fetchers.shared]\ncomponent = \"shared.wasm\"\n");
        let registry = write_registry_text(dir.path(), &text);

        let rows = rows_for(&registry, &state);
        let kinds: Vec<(&str, &str)> = rows.iter().map(|r| (r.kind, r.name.as_str())).collect();
        assert_eq!(kinds, [("source", "shared"), ("fetcher", "shared")]);
    }

    #[test]
    fn list_never_reads_a_status_file_for_a_fetcher() {
        let dir = TempDir::new().expect("tempdir");
        let state = dir.path().join("state");
        let mut text = source_table("shared", true);
        text.push_str("[fetchers.shared]\ncomponent = \"shared.wasm\"\n");
        let registry = write_registry_text(dir.path(), &text);
        write_status_file(
            &state,
            "shared",
            &status_body("shared", "2026-01-02T03:04:05Z", true, None, Some(2)),
        );

        let rows = rows_for(&registry, &state);
        assert_eq!(cells(&rows[0])[4], "2026-01-02T03:04:05Z ok (2 items)");
        assert_eq!(rows[1].kind, "fetcher");
        assert_eq!(rows[1].last_poll, LastPoll::NotApplicable);
        assert_eq!(cells(&rows[1])[4], "n/a");
    }

    #[test]
    fn list_empty_registry_prints_a_no_plugins_line_and_json_empty_array() {
        let dir = TempDir::new().expect("tempdir");
        let state = dir.path().join("state");
        let registry = write_registry_text(dir.path(), "");

        let (table, _) = run_list(&registry, &state, false);
        assert_eq!(
            table,
            format!("No plugins are registered in {}.\n", registry.display())
        );
        let (json, _) = run_list(&registry, &state, true);
        assert_eq!(json, "[]\n");
    }

    // ── Tolerance of broken things ───────────────────────────────────────────

    #[test]
    fn list_survives_a_broken_plugin_toml_and_a_missing_component() {
        let dir = TempDir::new().expect("tempdir");
        let state = dir.path().join("state");
        let mut text = source_table("bad", true);
        text.push_str(&source_table("good", true));
        let registry = write_registry_text(dir.path(), &text);
        std::fs::write(dir.path().join("bad.toml"), "[host\npoll_interval = ").expect("bad toml");
        // `bad.wasm` and `good.toml` do not exist at all.

        let (table, _) = run_list(&registry, &state, false);
        assert_eq!(table_names(&table), ["bad", "good"], "{table}");
    }

    #[test]
    fn list_works_while_the_state_dir_lock_is_held() {
        let dir = TempDir::new().expect("tempdir");
        let state = dir.path().join("state");
        let registry = write_registry_text(dir.path(), &source_table("held", true));
        let store = CursorStore::open(&state).expect("hold the lock");
        write_status_file(
            &state,
            "held",
            &status_body("held", "2026-01-02T03:04:05Z", true, None, Some(3)),
        );

        let (table, _) = run_list(&registry, &state, false);
        assert!(table.contains("ok (3 items)"), "{table}");
        drop(store);
    }

    #[test]
    fn list_creates_nothing() {
        let dir = TempDir::new().expect("tempdir");
        let state = dir.path().join("state");
        let registry = write_registry_text(dir.path(), &source_table("quiet", true));
        let entries = |path: &Path| {
            let mut names: Vec<_> = std::fs::read_dir(path)
                .expect("read dir")
                .map(|e| e.expect("entry").file_name())
                .collect();
            names.sort();
            names
        };
        let before = entries(dir.path());

        run_list(&registry, &state, false);
        run_list(&registry, &state, true);

        assert!(!state.exists(), "state directory must not be created");
        let after = entries(dir.path());
        assert_eq!(before, after, "registry directory is unchanged");
        for name in &after {
            let name = name.to_string_lossy();
            assert!(
                name != ".lock" && name != ".env" && !name.ends_with(".status.json"),
                "unexpected file {name}"
            );
        }
    }

    // ── Sanitizing and clipping ──────────────────────────────────────────────

    #[test]
    fn list_sanitizes_status_text_from_files() {
        let dir = TempDir::new().expect("tempdir");
        let state = dir.path().join("state");
        let registry = write_registry_text(dir.path(), &source_table("evil", true));
        write_status_file(
            &state,
            "evil",
            &status_body(
                "evil",
                "2026-01-02\nT03:04:05Z",
                false,
                Some("\u{1b}[31mred\nforged row"),
                None,
            ),
        );

        let (table, _) = run_list(&registry, &state, false);
        assert!(
            !table.contains('\u{1b}'),
            "ESC reached the table: {table:?}"
        );
        assert_eq!(
            table.lines().count(),
            2,
            "one header and one row: {table:?}"
        );
        assert!(table.contains("red"), "{table:?}");

        let (json, _) = run_list(&registry, &state, true);
        assert!(!json.contains('\u{1b}'), "ESC reached the JSON: {json:?}");
        assert_eq!(json.matches('\n').count(), 1, "{json:?}");
        let value: serde_json::Value = serde_json::from_str(&json).expect("json");
        let poll = &value[0]["last_poll"];
        assert_eq!(poll["state"], "error");
        for key in ["at", "error"] {
            let text = poll[key].as_str().expect("string");
            assert!(
                !text.chars().any(char::is_control),
                "{key} holds a control character: {text:?}"
            );
        }
    }

    #[test]
    fn list_clips_the_error_in_the_table_but_not_in_json() {
        let dir = TempDir::new().expect("tempdir");
        let state = dir.path().join("state");
        let registry = write_registry_text(dir.path(), &source_table("long", true));
        let long = "x".repeat(300);
        write_status_file(
            &state,
            "long",
            &status_body("long", "2026-01-02T03:04:05Z", false, Some(&long), None),
        );

        let (table, _) = run_list(&registry, &state, false);
        let row = table.lines().nth(1).expect("row");
        assert!(
            row.ends_with(&format!("error: {}…", "x".repeat(80))),
            "{row:?}"
        );

        let (json, _) = run_list(&registry, &state, true);
        let value: serde_json::Value = serde_json::from_str(&json).expect("json");
        let error = value[0]["last_poll"]["error"].as_str().expect("error");
        assert_eq!(error.chars().count(), 300);
    }

    #[test]
    fn list_reports_skipped_invalid_names_and_never_uses_them_as_paths() {
        let dir = TempDir::new().expect("tempdir");
        let state = dir.path().join("state");
        let mut text = source_table("fine", true);
        for key in ["Bad Name", "../evil", "esc\\u001bseq"] {
            writeln!(
                text,
                "[sources.\"{key}\"]\ncomponent = \"x.wasm\"\nconfig = \"x.toml\"\n"
            )
            .expect("append");
        }
        text.push_str("[fetchers.\"Bad/Fetcher\"]\ncomponent = \"f.wasm\"\n");
        let registry = write_registry_text(dir.path(), &text);
        // A status file under an invalid name must never be picked up.
        write_status_file(
            &state,
            "Bad Name",
            &status_body("Bad Name", "2026-01-02T03:04:05Z", true, None, Some(9)),
        );

        let (table, err) = run_list(&registry, &state, false);
        assert_eq!(table_names(&table), ["fine"], "{table}");
        assert!(!table.contains("9 items"), "{table}");

        assert!(err.contains("sources"), "{err:?}");
        assert!(err.contains("Bad Name"), "{err:?}");
        assert!(err.contains("../evil"), "{err:?}");
        assert!(err.contains("fetchers"), "{err:?}");
        assert!(err.contains("Bad/Fetcher"), "{err:?}");
        assert!(!err.contains('\u{1b}'), "ESC reached stderr: {err:?}");
        assert_eq!(err.lines().count(), 4, "one note per skipped name: {err:?}");
        assert!(!table.contains("Bad"), "notes never go to stdout: {table}");
    }
}
