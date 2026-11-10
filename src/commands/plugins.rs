// ─────────────────────────────────────────────────────────────────────────────
// commands/plugins.rs - `fili plugins` subcommands
//
// `list` shows every registered plugin and, for a source, the outcome of its
// last poll as the scheduler recorded it in `<state_dir>/<alias>.status.json`.
// It reads `filimon.toml` and those status files only: it never loads a plugin
// file, a secret or a component, never takes the state directory lock and
// never creates or writes anything, so it is safe while `fili run` is polling.
//
// `check <name>` validates one plugin without polling it: its component goes
// through the host's own preflight (`check_component`) and its TOML and
// secrets through the plugin-scoped config loader. It never calls guest
// `init` or `poll`, never takes the state-directory lock and never creates the
// state directory. Only alias names are printed for secrets, never values.
//
// `run-once <name>` polls one source once (instantiate, `init`, `poll`, map)
// and prints what came back. Without `--produce` it is a read-only look: the
// saved cursor is read without the lock, nothing is sent, and the state
// directory is neither created nor modified. With `--produce` the order is
// fixed: poll, print, send the batch, then write the cursor (D-07).
//
// Everything that came from a file or a guest is untrusted at print time and
// passes through `sanitize_for_terminal` or `clean_guest_text`.
// ─────────────────────────────────────────────────────────────────────────────

use std::io::Write;
use std::path::{Path, PathBuf};

use miette::IntoDiagnostic;

use crate::cli::PluginsCommand;
use crate::output::sanitize_for_terminal;
use crate::plugins::config::{
    ListedPlugin, RegistryListing, list_registry, load_plugin, load_plugin_with_env,
};
use crate::plugins::error::ConfigErrors;
use crate::plugins::host::HostOptions;
use crate::plugins::host::cursor::{
    CursorStore, PollStatus, StatusRead, read_cursor_file, read_status, require_state_dir,
};
use crate::plugins::host::engine::{HostEngine, clean_guest_text};
use crate::plugins::host::error::HostError;
use crate::plugins::host::instance::{InstanceFactory, WasmtimeFactory};
use crate::plugins::host::loader::{PluginKind, check_component, load_source};
use crate::plugins::host::once::{PolledOnce, poll_once};
use crate::plugins::host::scheduler::{CycleOutcome, PluginSpec, describe};
use crate::plugins::host::sink::{FluvioSink, ItemSink, SinkRecord};

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

// ── Check ─────────────────────────────────────────────────────────────────────

/// Longest plugin name echoed back in an error, in characters.
const NAME_ECHO_CLIP: usize = 64;

/// Longest engine error kept in a diagnostic, in characters.
const ENGINE_REASON_CLIP: usize = 512;

/// Whether the component passed the host's preflight.
#[derive(Debug)]
pub enum ComponentCheck {
    Ok,
    Failed(HostError),
}

/// What the check says about a plugin's own TOML and secrets.
#[derive(Debug)]
pub enum ConfigCheck {
    /// The TOML and every declared secret are valid.
    Ok {
        poll_interval_secs: u64,
        /// Alias names only, never values.
        secrets: Vec<String>,
    },
    Failed(ConfigErrors),
    /// A disabled source: its config is not validated.
    Skipped,
    /// A fetcher has no config file.
    NoConfigFile,
}

/// The outcome of checking one plugin.
#[derive(Debug)]
pub struct CheckReport {
    pub name: String,
    pub kind: PluginKind,
    pub component: ComponentCheck,
    pub config: ConfigCheck,
}

impl CheckReport {
    /// `true` only when no part failed. A skipped or absent config is not a
    /// failure, and it is printed as such.
    #[must_use]
    pub fn passed(&self) -> bool {
        matches!(self.component, ComponentCheck::Ok)
            && !matches!(self.config, ConfigCheck::Failed(_))
    }
}

/// `sources: a, b; fetchers: f`, with `none` for an empty list.
fn registered_plugins(listing: &RegistryListing) -> String {
    let join = |names: Vec<&str>| {
        if names.is_empty() {
            "none".to_string()
        } else {
            names.join(", ")
        }
    };
    format!(
        "sources: {}; fetchers: {}",
        join(listing.sources.iter().map(|p| p.name.as_str()).collect()),
        join(listing.fetchers.iter().map(|p| p.name.as_str()).collect()),
    )
}

/// Finds the plugin called `name`, sources first. `name` is compared exactly,
/// as bytes, against the registry's names and is never used to build a path.
///
/// # Errors
/// An error that lists the registered plugins when `name` is not one of them.
/// The echoed name is sanitized and clipped.
fn find_plugin<'a>(
    listing: &'a RegistryListing,
    name: &str,
) -> miette::Result<(PluginKind, &'a ListedPlugin)> {
    let found = listing
        .sources
        .iter()
        .find(|plugin| plugin.name == name)
        .map(|plugin| (PluginKind::Source, plugin))
        .or_else(|| {
            listing
                .fetchers
                .iter()
                .find(|plugin| plugin.name == name)
                .map(|plugin| (PluginKind::Fetcher, plugin))
        });
    let Some(found) = found else {
        let shown = clean_guest_text(name, NAME_ECHO_CLIP);
        let path = sanitize_for_terminal(&listing.path.display().to_string());
        if listing.sources.is_empty() && listing.fetchers.is_empty() {
            return Err(miette::miette!(
                "no plugin named `{shown}`: no plugins are registered in {path}"
            ));
        }
        return Err(miette::miette!(
            "no plugin named `{shown}` in {path} (registered: {})",
            registered_plugins(listing)
        ));
    };
    Ok(found)
}

/// Checks the plugin called `name`: its component against its world, and for a
/// source its TOML and secrets. `name` is compared exactly, as bytes, against
/// the registry's names (sources first) and is never used to build a path. The
/// two halves are independent: a failing component does not stop the config
/// result from being computed.
///
/// # Errors
/// Returns the registry's diagnostics when it cannot be read, or an error that
/// lists the registered plugins when `name` is not one of them.
pub fn run_check(
    registry: &Path,
    name: &str,
    env: &dyn Fn(&str) -> Option<String>,
    engine: &HostEngine,
) -> miette::Result<CheckReport> {
    let listing = list_registry(registry)?;
    let (kind, plugin) = find_plugin(&listing, name)?;

    let component = match check_component(kind, name, &plugin.component, engine) {
        Ok(()) => ComponentCheck::Ok,
        Err(error) => ComponentCheck::Failed(error),
    };

    let config = match kind {
        PluginKind::Fetcher => ConfigCheck::NoConfigFile,
        PluginKind::Source => match load_plugin_with_env(registry, name, env) {
            Err(errors) => ConfigCheck::Failed(errors),
            Ok(_) if !plugin.enabled => ConfigCheck::Skipped,
            Ok(loaded) => match loaded.sources.get(name).and_then(|s| s.plugin.as_ref()) {
                Some(config) => ConfigCheck::Ok {
                    poll_interval_secs: config.host.poll_interval.as_secs(),
                    secrets: config.secrets.keys().cloned().collect(),
                },
                None => ConfigCheck::Skipped,
            },
        },
    };

    Ok(CheckReport {
        name: name.to_string(),
        kind,
        component,
        config,
    })
}

/// Writes the check summary: one line per part and a final verdict. Only alias
/// names are printed for secrets.
///
/// # Errors
/// Returns the write error of `out`.
pub fn render_check(report: &CheckReport, out: &mut impl Write) -> std::io::Result<()> {
    let world = report.kind.world();
    writeln!(
        out,
        "plugin `{}` ({world})",
        sanitize_for_terminal(&report.name)
    )?;
    match report.component {
        ComponentCheck::Ok => writeln!(
            out,
            "  component: ok (compiles, imports only filimon:plugin/*, fits the `{world}` world)"
        )?,
        ComponentCheck::Failed(_) => writeln!(out, "  component: FAILED")?,
    }
    match &report.config {
        ConfigCheck::Ok {
            poll_interval_secs,
            secrets,
        } => {
            let aliases = if secrets.is_empty() {
                "none".to_string()
            } else {
                secrets
                    .iter()
                    .map(|alias| sanitize_for_terminal(alias))
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            writeln!(
                out,
                "  config: ok (poll interval {poll_interval_secs}s, secrets: {aliases})"
            )?;
        }
        ConfigCheck::Failed(_) => writeln!(out, "  config: FAILED")?,
        ConfigCheck::Skipped => writeln!(out, "  config: skipped (plugin is disabled)")?,
        ConfigCheck::NoConfigFile => {
            writeln!(out, "  config: none (fetchers have no config file)")?;
        }
    }
    writeln!(
        out,
        "{}",
        if report.passed() {
            "check passed"
        } else {
            "check failed"
        }
    )
}

/// Writes each failing part's diagnostic to `err` in miette's report form.
/// The diagnostics carry plugin names, paths, field paths, aliases and env var
/// names only.
fn write_diagnostics(report: CheckReport, err: &mut impl Write) {
    if let ComponentCheck::Failed(error) = report.component {
        writeln!(err, "{:?}", miette::Report::new(error)).ok();
    }
    if let ConfigCheck::Failed(errors) = report.config {
        writeln!(err, "{:?}", miette::Report::new(errors)).ok();
    }
}

/// `fili plugins check <name>`: the summary goes to `out`, each failing part's
/// diagnostic to `err`.
///
/// # Errors
/// Returns an error when the registry cannot be read, `name` is not
/// registered, the engine cannot start, or any part of the check failed, so
/// that the process exits non-zero.
pub fn check(
    registry: &Path,
    name: &str,
    out: &mut impl Write,
    err: &mut impl Write,
) -> miette::Result<()> {
    let engine = HostEngine::new().map_err(|e| HostError::Engine {
        reason: clean_guest_text(&format!("{e:#}"), ENGINE_REASON_CLIP),
    })?;
    let report = run_check(registry, name, &|key| std::env::var(key).ok(), &engine)?;
    render_check(&report, out).into_diagnostic()?;
    if report.passed() {
        return Ok(());
    }
    let shown = sanitize_for_terminal(&report.name);
    write_diagnostics(report, err);
    Err(miette::miette!("plugin `{shown}` failed its check"))
}

// ── Run once ──────────────────────────────────────────────────────────────────

/// Longest field value printed in the human output, in characters.
const ITEM_FIELD_CLIP: usize = 200;

/// The notice printed after a dry run.
const DRY_RUN_NOTICE: &str = "dry run: nothing was produced and the cursor was not saved (use --produce to produce and save the cursor)";

/// Whether `run-once` sends what it polled.
pub enum Produce<'a, S: ItemSink> {
    /// A read-only look: no sink, no cursor store, no status write.
    No,
    /// Send the batch, then save the cursor. The store holds the state
    /// directory lock.
    Yes {
        sink: &'a S,
        cursors: &'a CursorStore,
    },
}

/// `polled N items (D dropped)`.
fn polled_line(count: usize, dropped: usize) -> String {
    let noun = if count == 1 { "item" } else { "items" };
    format!("polled {count} {noun} ({dropped} dropped)")
}

/// One field line: the value cleaned and clipped by characters, `--` when
/// absent or empty.
fn field_line(
    out: &mut impl Write,
    label: &str,
    value: Option<&str>,
) -> std::io::Result<()> {
    let shown = value
        .map(|text| clean_guest_text(text, ITEM_FIELD_CLIP))
        .filter(|text| !text.is_empty())
        .unwrap_or_else(|| "--".to_string());
    writeln!(out, "    {label}: {shown}")
}

/// Writes one block per record in guest order: `[i/n] <key>`, then the url,
/// title, author, published and a text preview. Every value is guest text, so
/// each one is cleaned and clipped; nothing is ever sliced by bytes.
///
/// # Errors
/// Returns the write error of `out`.
pub fn render_items(records: &[SinkRecord], out: &mut impl Write) -> std::io::Result<()> {
    let total = records.len();
    for (index, record) in records.iter().enumerate() {
        writeln!(
            out,
            "[{}/{total}] {}",
            index + 1,
            clean_guest_text(&record.key, ITEM_FIELD_CLIP)
        )?;
        let item = &record.item;
        field_line(out, "url", Some(&item.url))?;
        field_line(out, "title", item.title.as_deref())?;
        field_line(out, "author", item.author.as_deref())?;
        field_line(out, "published", item.published.as_deref())?;
        field_line(out, "text", Some(&item.text))?;
    }
    Ok(())
}

/// Prints a poll's result and, with `Produce::Yes`, commits it. This is the
/// part of `run-once` that is never interrupted: a stop must not split the send
/// from the cursor write.
async fn finish_run_once<S: ItemSink>(
    spec: &PluginSpec,
    polled: Result<PolledOnce, CycleOutcome>,
    produce: Produce<'_, S>,
    json: bool,
    out: &mut impl Write,
    err: &mut impl Write,
) -> miette::Result<()> {
    let polled = match polled {
        Ok(polled) => polled,
        Err(outcome) => return Err(miette::miette!("{}", describe(&outcome))),
    };
    if json {
        return Err(miette::miette!("--json is not available yet"));
    }
    render_items(&polled.records, out).into_diagnostic()?;
    writeln!(out, "{}", polled_line(polled.records.len(), polled.dropped)).into_diagnostic()?;
    match produce {
        Produce::No => writeln!(out, "{DRY_RUN_NOTICE}").into_diagnostic(),
        Produce::Yes { .. } => {
            let _ = (spec, err);
            Err(miette::miette!("--produce is not available yet"))
        }
    }
}

/// Polls `spec` once from `cursor` and prints the result. This is the seam the
/// tests use; `run_once` splits the same two steps around Ctrl-C.
///
/// # Errors
/// The cleaned `describe` text of a failed cycle, or a write error.
pub async fn execute_run_once<F: InstanceFactory, S: ItemSink>(
    spec: &PluginSpec,
    factory: &F,
    cursor: Option<&[u8]>,
    produce: Produce<'_, S>,
    json: bool,
    out: &mut impl Write,
    err: &mut impl Write,
) -> miette::Result<()> {
    let polled = poll_once(spec, factory, cursor).await;
    finish_run_once(spec, polled, produce, json, out, err).await
}

/// The plugin `name` as a pollable source, or the reason `run-once` refuses it.
fn find_source<'a>(
    listing: &'a RegistryListing,
    name: &str,
) -> miette::Result<&'a ListedPlugin> {
    let (kind, plugin) = find_plugin(listing, name)?;
    let shown = clean_guest_text(name, NAME_ECHO_CLIP);
    if kind == PluginKind::Fetcher {
        return Err(miette::miette!(
            "`{shown}` is a fetcher; run-once polls sources"
        ));
    }
    if !plugin.enabled {
        return Err(miette::miette!(
            "plugin `{shown}` is disabled in filimon.toml; set `enabled = true` to run it"
        ));
    }
    Ok(plugin)
}

/// Opens the state directory and takes its lock, as `--produce` must: a second
/// producer on the same directory fails here, before any poll or connection.
///
/// # Errors
/// `StateDirLocked` when another process holds the lock, `StateDir` otherwise.
fn open_state_store(state_dir: &Path) -> Result<CursorStore, HostError> {
    CursorStore::open(state_dir)
}

/// The poll step of `run-once`, raced against Ctrl-C so a spinning guest can be
/// interrupted. An interrupt is an error (exit 1).
async fn poll_interruptible<F: InstanceFactory>(
    spec: &PluginSpec,
    factory: &F,
    cursor: Option<&[u8]>,
) -> miette::Result<Result<PolledOnce, CycleOutcome>> {
    tokio::select! {
        polled = poll_once(spec, factory, cursor) => Ok(polled),
        _ = tokio::signal::ctrl_c() => Err(miette::miette!(
            "interrupted: nothing was sent and the cursor was not saved"
        )),
    }
}

/// `fili plugins run-once <name>`.
///
/// # Errors
/// An error for an unknown name, a fetcher or a disabled source (before any
/// component is loaded), a config or component problem, a corrupt cursor file,
/// a held state-directory lock (`--produce`), a failed cycle, or an interrupt.
pub async fn run_once(
    registry: &Path,
    state_dir_flag: Option<&Path>,
    name: &str,
    produce: bool,
    json: bool,
    out: &mut impl Write,
    err: &mut impl Write,
) -> miette::Result<()> {
    if produce {
        return Err(miette::miette!("--produce is not available yet"));
    }
    let listing = list_registry(registry)?;
    let planned = find_source(&listing, name)?;
    let state_dir = state_dir_for(&listing.base_dir, state_dir_flag)?;

    // Only this plugin's own config is loaded (a scoped load).
    let loaded = load_plugin(registry, name).map_err(miette::Report::new)?;
    let spec = loaded
        .sources
        .get(name)
        .and_then(PluginSpec::from_loaded)
        .ok_or_else(|| miette::miette!("plugin `{}` could not be prepared", planned.name))?;

    let engine = HostEngine::new().map_err(|e| HostError::Engine {
        reason: clean_guest_text(&format!("{e:#}"), ENGINE_REASON_CLIP),
    })?;
    let pre = load_source(&spec.name, &planned.component, &engine)?;
    let factory = WasmtimeFactory::new(engine.engine().clone(), pre);

    if produce {
        // The lock first, so a second producer fails before any poll or
        // network connection.
        let store = open_state_store(&state_dir)?;
        let cursor = store.read(&spec.name)?;
        let sink = FluvioSink::connect_social().await?;
        let polled = poll_interruptible(&spec, &factory, cursor.as_deref()).await?;
        let produce = Produce::Yes {
            sink: &sink,
            cursors: &store,
        };
        finish_run_once(&spec, polled, produce, json, out, err).await
    } else {
        // Lock-free read: works while `fili run` is up and creates nothing.
        let cursor = read_cursor_file(&state_dir, &spec.name)?;
        let polled = poll_interruptible(&spec, &factory, cursor.as_deref()).await?;
        finish_run_once(&spec, polled, Produce::<FluvioSink>::No, json, out, err).await
    }
}

/// Runs one `fili plugins` subcommand, writing to stdout.
///
/// # Errors
/// Returns whatever the subcommand returns.
pub async fn dispatch(
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
        PluginsCommand::Check { name } => {
            let stdout = std::io::stdout();
            let mut out = stdout.lock();
            let stderr = std::io::stderr();
            let mut err = stderr.lock();
            check(registry, name, &mut out, &mut err)
        }
        PluginsCommand::RunOnce {
            name,
            produce,
            json,
        } => {
            let mut out = std::io::stdout();
            let mut err = std::io::stderr();
            run_once(
                registry,
                state_dir_flag,
                name,
                *produce,
                *json,
                &mut out,
                &mut err,
            )
            .await
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
    use crate::plugins::host::sink::{ItemSink, VecSink};
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

    // ── check ────────────────────────────────────────────────────────────────

    use crate::plugins::error::ConfigError;
    use crate::plugins::host::testing::{WASI_IMPORT_COMPONENT, fixture_wasm};

    const SECRET_VALUE: &str = "hunter2-distinctive";
    const GARBAGE: &[u8] = b"this is not a wasm component";

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn engine() -> HostEngine {
        HostEngine::new().expect("engine")
    }

    /// Runs `check` on `name` with `env` and no secret from the process.
    fn check_with(
        registry: &Path,
        name: &str,
        env: &dyn Fn(&str) -> Option<String>,
    ) -> CheckReport {
        run_check(registry, name, env, &engine()).expect("the plugin is registered")
    }

    /// The summary `render_check` prints.
    fn summary(report: &CheckReport) -> String {
        let mut out = Vec::new();
        render_check(report, &mut out).expect("render");
        String::from_utf8(out).expect("utf8")
    }

    /// Everything `check` would print to stderr for `report`, with whitespace
    /// and miette's gutter removed so wrapped lines can be searched.
    fn diagnostics(report: CheckReport) -> String {
        let mut err = Vec::new();
        write_diagnostics(report, &mut err);
        squash(&String::from_utf8(err).expect("utf8"))
    }

    fn squash(text: &str) -> String {
        text.chars()
            .filter(|c| !c.is_whitespace() && *c != '│')
            .collect()
    }

    /// A one-source registry: `stub` with `component_bytes` and `plugin_toml`.
    fn stub_registry(dir: &Path, component_bytes: &[u8], plugin_toml: &str) -> PathBuf {
        std::fs::write(dir.join("stub.wasm"), component_bytes).expect("component");
        std::fs::write(dir.join("stub.toml"), plugin_toml).expect("plugin toml");
        write_registry_text(dir, &source_table("stub", true))
    }

    const MINIMAL_TOML: &str = "[host]\npoll_interval_secs = 60\n";

    fn component_error(report: &CheckReport) -> &HostError {
        match &report.component {
            ComponentCheck::Failed(error) => error,
            ComponentCheck::Ok => panic!("expected a component failure: {report:?}"),
        }
    }

    fn config_errors(report: &CheckReport) -> &ConfigErrors {
        match &report.config {
            ConfigCheck::Failed(errors) => errors,
            other => panic!("expected a config failure, got {other:?}"),
        }
    }

    #[test]
    fn check_unknown_name_lists_registered_plugins() {
        let dir = TempDir::new().expect("tempdir");
        let mut text = source_table("a", true);
        text.push_str(&source_table("b", true));
        text.push_str("[fetchers.f]\ncomponent = \"f.wasm\"\n");
        let registry = write_registry_text(dir.path(), &text);

        let error = run_check(&registry, "nope", &no_env, &engine()).expect_err("unknown name");
        let text = error.to_string();
        assert!(text.contains("`nope`"), "{text}");
        assert!(text.contains("sources: a, b"), "{text}");
        assert!(text.contains("fetchers: f"), "{text}");

        let empty = TempDir::new().expect("tempdir");
        let registry = write_registry_text(empty.path(), "");
        let error = run_check(&registry, "nope", &no_env, &engine()).expect_err("unknown name");
        assert!(
            error.to_string().contains("no plugins are registered"),
            "{error}"
        );
    }

    #[test]
    fn check_empty_name_is_unknown_and_never_a_path() {
        let dir = TempDir::new().expect("tempdir");
        // A name joined into a path would find these.
        std::fs::write(dir.path().join(".wasm"), GARBAGE).expect("decoy component");
        std::fs::write(dir.path().join(".toml"), MINIMAL_TOML).expect("decoy toml");
        let registry = stub_registry(dir.path(), GARBAGE, MINIMAL_TOML);

        let error = run_check(&registry, "", &no_env, &engine()).expect_err("empty is unknown");

        let text = error.to_string();
        assert!(text.contains("no plugin named ``"), "{text}");
        assert!(text.contains("sources: stub"), "{text}");
    }

    #[test]
    fn check_name_is_compared_exactly_and_echoed_sanitized() {
        let dir = TempDir::new().expect("tempdir");
        let registry = stub_registry(dir.path(), GARBAGE, MINIMAL_TOML);

        for name in ["Stub", "../stub", "stüb", "stub ", "stub\u{1b}[31m"] {
            let error = run_check(&registry, name, &no_env, &engine())
                .expect_err("only the exact registry key matches");
            let text = error.to_string();
            assert!(text.contains("no plugin named"), "{name:?}: {text}");
            assert!(!text.chars().any(char::is_control), "{name:?}: {text:?}");
        }

        let long = "x".repeat(200);
        let text = run_check(&registry, &long, &no_env, &engine())
            .expect_err("unknown")
            .to_string();
        assert!(
            text.contains(&format!("`{}…`", "x".repeat(64))),
            "clipped to 64 characters: {text}"
        );
        assert!(!text.contains(&"x".repeat(65)), "{text}");
    }

    #[test]
    fn check_garbage_component_names_plugin_and_path() {
        let dir = TempDir::new().expect("tempdir");
        let registry = stub_registry(dir.path(), GARBAGE, MINIMAL_TOML);

        let report = check_with(&registry, "stub", &no_env);

        assert!(!report.passed());
        let error = component_error(&report);
        assert!(
            matches!(error, HostError::ComponentLoad { .. }),
            "{error:?}"
        );
        let text = error.to_string();
        assert!(text.contains("`stub`"), "{text}");
        assert!(
            text.contains(&dir.path().join("stub.wasm").display().to_string()),
            "{text}"
        );
        assert!(
            matches!(report.config, ConfigCheck::Ok { .. }),
            "{report:?}"
        );
        assert!(summary(&report).contains("  component: FAILED"));
        assert!(summary(&report).ends_with("check failed\n"));
    }

    #[test]
    fn check_zero_byte_component_is_a_component_load_failure() {
        let dir = TempDir::new().expect("tempdir");
        let registry = stub_registry(dir.path(), b"", MINIMAL_TOML);

        let report = check_with(&registry, "stub", &no_env);

        let error = component_error(&report);
        assert!(
            matches!(error, HostError::ComponentLoad { .. }),
            "{error:?}"
        );
        let text = error.to_string();
        assert!(text.contains("`stub`"), "{text}");
        assert!(text.contains("stub.wasm"), "{text}");
    }

    #[test]
    fn check_wasi_import_component_names_the_import() {
        let dir = TempDir::new().expect("tempdir");
        let registry = stub_registry(dir.path(), WASI_IMPORT_COMPONENT, MINIMAL_TOML);

        let report = check_with(&registry, "stub", &no_env);

        let HostError::ForeignImports { imports, .. } = component_error(&report) else {
            panic!("expected ForeignImports: {report:?}");
        };
        assert_eq!(imports, &vec!["wasi:cli/environment@0.2.0".to_string()]);
    }

    #[test]
    fn check_bad_toml_and_bad_host_value_name_plugin_and_field() {
        let dir = TempDir::new().expect("tempdir");

        let registry = stub_registry(dir.path(), GARBAGE, "[host\npoll_interval_secs = ");
        let report = check_with(&registry, "stub", &no_env);
        let errors = config_errors(&report);
        assert!(
            matches!(
                &errors.problems[0],
                ConfigError::PluginFileMalformed { plugin, path, .. }
                    if plugin == "stub" && path.ends_with("stub.toml")
            ),
            "{errors:?}"
        );
        assert!(summary(&report).contains("  config: FAILED"));

        let registry = stub_registry(dir.path(), GARBAGE, "[host]\npoll_interval_secs = 0\n");
        let report = check_with(&registry, "stub", &no_env);
        let text = diagnostics(report);
        assert!(text.contains("stub"), "{text}");
        assert!(text.contains("poll_interval_secs"), "{text}");

        let registry = stub_registry(
            dir.path(),
            GARBAGE,
            "[host]\npoll_interval_secs = 60\nallowlist = [\"https://x\"]\n",
        );
        let report = check_with(&registry, "stub", &no_env);
        let errors = config_errors(&report);
        assert!(
            matches!(
                &errors.problems[0],
                ConfigError::InvalidValue { plugin, field, .. }
                    if plugin == "stub" && field == "host.allowlist[0]"
            ),
            "{errors:?}"
        );
    }

    #[test]
    fn check_missing_secret_names_plugin_alias_and_env_var() {
        let dir = TempDir::new().expect("tempdir");
        let registry = stub_registry(
            dir.path(),
            GARBAGE,
            "[host]\npoll_interval_secs = 60\n\n[host.secrets]\ntoken = \"STUB_TOKEN\"\n",
        );

        let report = check_with(&registry, "stub", &no_env);

        let errors = config_errors(&report);
        assert_eq!(errors.problems.len(), 1, "{errors:?}");
        assert!(
            matches!(
                &errors.problems[0],
                ConfigError::MissingSecret { plugin, alias, env_var, .. }
                    if plugin == "stub" && alias == "token" && env_var == "STUB_TOKEN"
            ),
            "{errors:?}"
        );
    }

    #[test]
    fn check_reports_an_unknown_fetcher_reference() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::write(dir.path().join("stub.wasm"), GARBAGE).expect("component");
        std::fs::write(dir.path().join("stub.toml"), MINIMAL_TOML).expect("plugin toml");
        let registry = write_registry_text(
            dir.path(),
            "[sources.stub]\ncomponent = \"stub.wasm\"\nconfig = \"stub.toml\"\nfetcher = \"nope\"\n",
        );

        let report = check_with(&registry, "stub", &no_env);

        let errors = config_errors(&report);
        assert!(
            matches!(
                &errors.problems[0],
                ConfigError::UnknownFetcher { plugin, fetcher, .. }
                    if plugin == "stub" && fetcher == "nope"
            ),
            "{errors:?}"
        );
    }

    #[test]
    fn check_ignores_other_plugins_broken_config() {
        let dir = TempDir::new().expect("tempdir");
        let mut text = source_table("good", true);
        text.push_str(&source_table("broken", true));
        text.push_str(&source_table("needy", true));
        text.push_str("[sources.\"Bad Name\"]\ncomponent = \"x.wasm\"\nconfig = \"x.toml\"\n");
        let registry = write_registry_text(dir.path(), &text);
        std::fs::write(dir.path().join("good.toml"), MINIMAL_TOML).expect("good toml");
        std::fs::write(dir.path().join("broken.toml"), "[host\nnope = ").expect("broken toml");
        std::fs::write(
            dir.path().join("needy.toml"),
            "[host]\npoll_interval_secs = 60\n\n[host.secrets]\ntoken = \"NEEDY_TOKEN\"\n",
        )
        .expect("needy toml");

        let report = check_with(&registry, "good", &no_env);

        assert!(
            matches!(
                report.config,
                ConfigCheck::Ok {
                    poll_interval_secs: 60,
                    ..
                }
            ),
            "{report:?}"
        );
    }

    #[test]
    fn check_reports_a_dotenv_problem_only_when_the_plugin_needs_it() {
        let dir = TempDir::new().expect("tempdir");
        let mut text = source_table("plain", true);
        text.push_str(&source_table("needy", true));
        let registry = write_registry_text(dir.path(), &text);
        std::fs::write(dir.path().join("plain.toml"), MINIMAL_TOML).expect("plain toml");
        std::fs::write(
            dir.path().join("needy.toml"),
            "[host]\npoll_interval_secs = 60\n\n[host.secrets]\ntoken = \"NEEDY_TOKEN\"\n",
        )
        .expect("needy toml");
        std::fs::write(dir.path().join(".env"), "NEEDY_TOKEN=has a space\n").expect(".env");

        let plain = check_with(&registry, "plain", &no_env);
        assert!(matches!(plain.config, ConfigCheck::Ok { .. }), "{plain:?}");

        let needy = check_with(&registry, "needy", &no_env);
        let errors = config_errors(&needy);
        assert!(
            errors
                .problems
                .iter()
                .any(|p| matches!(p, ConfigError::DotenvInvalid { line: Some(1), .. })),
            "{errors:?}"
        );
        let text = diagnostics(needy);
        assert!(text.contains(".env"), "{text}");
        assert!(
            !text.contains("hasaspace"),
            "the .env content leaked: {text}"
        );
    }

    #[test]
    fn check_disabled_source_validates_component_only_and_says_config_skipped() {
        let dir = TempDir::new().expect("tempdir");
        std::fs::write(dir.path().join("off.wasm"), GARBAGE).expect("component");
        std::fs::write(dir.path().join("f.wasm"), GARBAGE).expect("fetcher component");
        // `off.toml` does not exist: a disabled source's config is never read.
        let registry = write_registry_text(
            dir.path(),
            &format!(
                "{}[fetchers.f]\ncomponent = \"f.wasm\"\n",
                source_table("off", false)
            ),
        );

        let off = check_with(&registry, "off", &no_env);
        assert!(matches!(off.config, ConfigCheck::Skipped), "{off:?}");
        assert!(
            matches!(off.component, ComponentCheck::Failed(_)),
            "{off:?}"
        );
        let text = summary(&off);
        assert!(
            text.contains("  config: skipped (plugin is disabled)\n"),
            "{text}"
        );
        assert!(text.contains("  component: FAILED\n"), "{text}");

        let fetcher = check_with(&registry, "f", &no_env);
        assert!(
            matches!(fetcher.config, ConfigCheck::NoConfigFile),
            "{fetcher:?}"
        );
        let text = summary(&fetcher);
        assert!(text.starts_with("plugin `f` (fetcher)\n"), "{text}");
        assert!(
            text.contains("  config: none (fetchers have no config file)\n"),
            "{text}"
        );
    }

    #[test]
    fn check_does_not_print_secret_values() {
        let dir = TempDir::new().expect("tempdir");
        let registry = stub_registry(
            dir.path(),
            GARBAGE,
            "[host]\npoll_interval_secs = 60\n\n[host.secrets]\ntoken = \"STUB_TOKEN\"\nmissing = \"STUB_MISSING\"\n",
        );
        let env = |key: &str| (key == "STUB_TOKEN").then(|| SECRET_VALUE.to_string());

        // One secret is set and one is not: the config fails, the component
        // fails, and both diagnostics are rendered.
        let report = check_with(&registry, "stub", &env);
        let shown = summary(&report);
        let debug = format!("{report:?}");
        let rendered = diagnostics(report);

        // A passing config with the secret set.
        let registry = stub_registry(
            dir.path(),
            GARBAGE,
            "[host]\npoll_interval_secs = 60\n\n[host.secrets]\ntoken = \"STUB_TOKEN\"\n",
        );
        let passing = check_with(&registry, "stub", &env);
        let passing_summary = summary(&passing);
        assert!(
            passing_summary.contains("secrets: token"),
            "{passing_summary}"
        );
        let passing_debug = format!("{passing:?}");

        for text in [&shown, &debug, &rendered, &passing_summary, &passing_debug] {
            for leaked in [SECRET_VALUE, "hunter2", "distinctive"] {
                assert!(!text.contains(leaked), "leaked {leaked:?} in: {text}");
            }
        }
        assert!(rendered.contains("STUB_MISSING"), "env var names are shown");
    }

    // ── run-once ─────────────────────────────────────────────────────────────

    use crate::plugins::host::engine::WitItem;

    /// A scripted poll answer with `items` and `next_cursor`.
    fn poll_step(items: Vec<WitItem>, next_cursor: Option<Vec<u8>>) -> FakeStep {
        FakeStep::PollOk {
            items,
            next_cursor,
            delay: Duration::ZERO,
        }
    }

    /// Runs one cycle with the fakes and returns `(result, stdout, stderr)`.
    async fn run_fake<S: ItemSink>(
        factory: &FakeFactory,
        cursor: Option<&[u8]>,
        produce: Produce<'_, S>,
        json: bool,
    ) -> (miette::Result<()>, String, String) {
        let mut out = Vec::new();
        let mut err = Vec::new();
        let result = execute_run_once(
            &spec("fake", 60, 60, 3600, 2.0),
            factory,
            cursor,
            produce,
            json,
            &mut out,
            &mut err,
        )
        .await;
        (
            result,
            String::from_utf8(out).expect("utf8 stdout"),
            String::from_utf8(err).expect("utf8 stderr"),
        )
    }

    #[tokio::test]
    async fn run_once_dry_run_prints_items_and_leaves_the_state_dir_absent() {
        let dir = TempDir::new().expect("tempdir");
        let state = dir.path().join("state");
        let factory = FakeFactory::new(vec![
            FakeStep::InitOk,
            poll_step(vec![wit_item("a"), wit_item("b")], Some(vec![7])),
        ]);

        // The dry-run path of `run_once`: a lock-free cursor read, no store.
        let cursor = read_cursor_file(&state, "fake").expect("a missing directory is a first poll");
        let (result, out, err) =
            run_fake(&factory, cursor.as_deref(), Produce::<VecSink>::No, false).await;

        result.expect("dry run succeeds");
        assert!(out.contains("[1/2] fake:a"), "{out}");
        assert!(out.contains("[2/2] fake:b"), "{out}");
        assert!(
            out.contains("https://example.invalid/fake/a"),
            "the url is shown: {out}"
        );
        assert!(out.contains("polled 2 items (0 dropped)"), "{out}");
        assert!(out.contains(DRY_RUN_NOTICE), "{out}");
        assert!(err.is_empty(), "{err}");
        assert!(!state.exists(), "a dry run must not create the state directory");
    }

    #[test]
    fn run_once_notice_and_counts_use_the_documented_wording() {
        assert_eq!(polled_line(1, 0), "polled 1 item (0 dropped)");
        assert_eq!(polled_line(0, 0), "polled 0 items (0 dropped)");
        assert_eq!(polled_line(2, 1), "polled 2 items (1 dropped)");
        assert_eq!(
            DRY_RUN_NOTICE,
            "dry run: nothing was produced and the cursor was not saved (use --produce to produce and save the cursor)"
        );
    }

    #[test]
    #[ignore = "needs plugin fixtures: see plugins/README.md"]
    fn wasm_check_a_fetcher_component_registered_as_a_source_is_a_wrong_world() {
        let dir = TempDir::new().expect("tempdir");
        let fetcher = fixture_wasm("stub-fetcher", "stub_fetcher.wasm");
        let bytes = std::fs::read(fetcher).expect("read fetcher component");
        let registry = stub_registry(dir.path(), &bytes, MINIMAL_TOML);

        let report = check_with(&registry, "stub", &no_env);

        let error = component_error(&report);
        assert!(
            matches!(error, HostError::WrongWorld { kind: "source", .. }),
            "{error:?}"
        );
        assert!(error.to_string().contains("`source` world"), "{error}");
        assert!(!report.passed());
    }
}
