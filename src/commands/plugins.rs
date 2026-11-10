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
/// when `out` cannot be written.
pub fn list(
    registry: &Path,
    state_dir_flag: Option<&Path>,
    out: &mut impl Write,
) -> miette::Result<()> {
    let listing = list_registry(registry)?;
    let state_dir = state_dir_for(&listing.base_dir, state_dir_flag)?;
    let rows = collect_list(&listing, &state_dir);
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
        PluginsCommand::List => {
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            list(registry, state_dir_flag, &mut out)
        }
    }
}

#[cfg(test)]
mod tests {
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
        let error = list(&registry, Some(Path::new("")), &mut out)
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
        let pattern =
            regex::Regex::new(r"^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z ok \(1 item\)$")
                .expect("regex");
        assert!(pattern.is_match(last_poll), "{last_poll:?}");
    }
}
