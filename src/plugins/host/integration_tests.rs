// ─────────────────────────────────────────────────────────────────────────────
// plugins/host/integration_tests.rs - Replay end to end (cfg(test) only)
//
// The compiled Reddit and YouTube components run inside the real host with
// their `fetch` calls answered by the replay-fetcher component (assigned with
// `fetcher = "replay"`, under the source's allowlist), so no request leaves
// the machine. The records land in a scratch Fluvio topic and are read back.
//
// Why in-crate: the crate has only a binary target and the production sink is
// bound to one topic, so a `tests/` file could only run the binary and would
// write to the real topic. Every sink here is built for a unique
// `filimon-test-replay-` scratch topic.
//
// Skips are runtime decisions, not ignore attributes (D-02): with no reachable
// Fluvio cluster a cluster-backed test prints a `skipped:` line and returns.
// Set `FILI_TEST_REQUIRE_FLUVIO=1` to make that skip a failure, so a
// verification run cannot pass on a skip. Run with `-- --nocapture` to see it.
//
// Tests use the real clock with a plain `#[tokio::test]`, never the paused
// clock: Fluvio I/O must not see an auto-advancing clock.
// ─────────────────────────────────────────────────────────────────────────────

use std::collections::HashMap;
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use fluvio::Offset;
use futures_util::{FutureExt, StreamExt};
use serde_json::Value;
use tokio_util::sync::CancellationToken;

use super::sink::live_support::{delete_topic, ready_sink, scratch_topic};
use super::sink::{FluvioSink, ItemSink, SinkError, SinkRecord, VecSink};
use super::testing::{ExtSource, ensure_built, is_stale, repo_root, write_registry_ext};
use super::{Host, HostOptions};
use crate::models::{SourceItem, SourceType};
use crate::plugins::auth::testing::{unix_now, write_tokens};
use crate::plugins::config::{LoadedRegistry, load_registry_with_env};
use crate::streaming::FluvioStream;

/// The environment variable that turns a missing cluster into a failure.
const REQUIRE_FLUVIO: &str = "FILI_TEST_REQUIRE_FLUVIO";

// ── Cluster Probe ─────────────────────────────────────────────────────────────

/// True when a Fluvio cluster is reachable. Without one the test prints the
/// skip line and the caller returns; with `FILI_TEST_REQUIRE_FLUVIO=1` that is
/// a panic instead. Only a failed connect skips: every later failure is a
/// test failure.
async fn fluvio_or_skip() -> bool {
    if let Ok(Ok(_)) = tokio::time::timeout(Duration::from_secs(5), fluvio::Fluvio::connect()).await
    {
        return true;
    }
    assert!(
        std::env::var(REQUIRE_FLUVIO).as_deref() != Ok("1"),
        "{REQUIRE_FLUVIO}=1 but no Fluvio cluster is reachable \
         (start one with `fluvio cluster start`)"
    );
    eprintln!("skipped: no reachable Fluvio cluster (start one with `fluvio cluster start`)");
    false
}

// ── Credential Gating ─────────────────────────────────────────────────────────

/// The environment variable that points the credential lookup at another
/// dotenv file (a verification run can force the skip path with it).
const ENV_FILE_VAR: &str = "FILI_TEST_ENV_FILE";

/// The credential keys the live tests read.
const LIVE_KEYS: [&str; 4] = [
    "REDDIT_CLIENT_ID",
    "REDDIT_CLIENT_SECRET",
    "REDDIT_USER_AGENT_CONTACT",
    "YOUTUBE_API_KEY",
];

/// The first of `keys` whose value is absent or the empty string.
fn missing_key<'a>(keys: &[&'a str], env: &dyn Fn(&str) -> Option<String>) -> Option<&'a str> {
    keys.iter()
        .copied()
        .find(|key| env(key).is_none_or(|value| value.is_empty()))
}

/// The line a credentialed test prints when it skips. It names the key only.
fn skip_line(key: &str) -> String {
    format!("skipped: missing {key}")
}

/// The non-empty process value of `key`, else the non-empty dotenv value.
fn lookup_env(
    key: &str,
    process: &dyn Fn(&str) -> Option<String>,
    dotenv: &HashMap<String, String>,
) -> Option<String> {
    process(key)
        .filter(|value| !value.is_empty())
        .or_else(|| dotenv.get(key).filter(|value| !value.is_empty()).cloned())
}

/// The live credentials: the process environment over the dotenv file at
/// `path`. The file is read by the repository's literal parser; a file it
/// rejects counts as absent and only a fixed note is printed, because the
/// parser's diagnostics name lines.
fn load_live_env_from(path: &Path) -> HashMap<String, String> {
    let dotenv = crate::plugins::secrets::read_dotenv(path).unwrap_or_else(|_| {
        eprintln!("note: the dotenv file could not be read; using the process environment only");
        HashMap::new()
    });
    let process = |key: &str| std::env::var(key).ok();
    LIVE_KEYS
        .iter()
        .filter_map(|key| {
            lookup_env(key, &process, &dotenv).map(|value| ((*key).to_string(), value))
        })
        .collect()
}

/// `load_live_env_from` over `FILI_TEST_ENV_FILE`, else `<repo root>/.env`.
fn load_live_env() -> HashMap<String, String> {
    let path = match std::env::var(ENV_FILE_VAR) {
        Ok(value) if !value.is_empty() => PathBuf::from(value),
        _ => repo_root().join(".env"),
    };
    load_live_env_from(&path)
}

// ── Platforms And Backends ────────────────────────────────────────────────────

/// A source platform under test.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Platform {
    Reddit,
    YouTube,
}

impl Platform {
    /// The `platform` value of the items it produces.
    fn name(self) -> &'static str {
        match self {
            Self::Reddit => "reddit",
            Self::YouTube => "youtube",
        }
    }
}

/// Where a source's HTTP is answered from. It selects the fetcher component,
/// the token provisioning and the env closure.
enum Backend {
    /// Frozen fixtures served by the replay-fetcher component.
    Replay,
    /// The real APIs, reached through the http fetcher component. Carries the
    /// live credentials (see `load_live_env`).
    Live(HashMap<String, String>),
}

impl Backend {
    /// The scratch-topic label of this backend.
    fn label(&self) -> &'static str {
        match self {
            Self::Replay => "replay",
            Self::Live(_) => "live",
        }
    }
}

/// The contact the live Reddit source puts in its User-Agent by default.
const DEFAULT_CONTACT: &str = "u/filimon-integration-test";

/// The Reddit token endpoint the host calls for the app-only grant.
const REDDIT_TOKEN_URL: &str = "https://www.reddit.com/api/v1/access_token";

/// A YouTube channel the live poll reads.
const LIVE_YOUTUBE_CHANNEL: &str = "UC_x5XG1OV2P6uZZ5FSM9Ttw";

fn reddit_fixture() -> PathBuf {
    repo_root().join("plugins/reddit/tests/fixtures/listing_new.json")
}

fn youtube_fixture() -> PathBuf {
    repo_root().join("plugins/youtube/tests/fixtures/playlist_page.json")
}

/// The API key the `YouTube` source receives through the env closure. It is
/// fake and never written to a file.
const FAKE_YOUTUBE_KEY: &str = "replay-fake-key";

/// Writes the registry for `platforms` into `dir` and returns its path and the
/// environment the loader reads secrets from (so no secret file is written).
fn build_registry(
    backend: &Backend,
    platforms: &[Platform],
    dir: &Path,
) -> (PathBuf, HashMap<String, String>) {
    if let Backend::Live(live) = backend {
        return build_live_registry(live, platforms, dir);
    }
    let mut env = HashMap::new();

    // Both fixtures are inputs: editing either one rebuilds the component.
    let replay = ensure_built(
        "replay-fetcher",
        "replay_fetcher.wasm",
        &[reddit_fixture(), youtube_fixture()],
    );
    let mut sources = Vec::new();
    for platform in platforms {
        match platform {
            Platform::Reddit => sources.push(ExtSource {
                name: "reddit".to_string(),
                component: ensure_built("reddit", "reddit_source.wasm", &[]),
                interval_secs: 3600,
                fetcher: Some("replay".to_string()),
                allowlist: vec!["oauth.reddit.com".to_string()],
                secrets: vec![],
                // An authorization-code login makes the host read a stored
                // token from disk; a client-credentials grant would make it
                // call the real token endpoint.
                oauth: Some(
                    "authorize_url = \"https://auth.example/authorize\"\n\
                     token_url = \"https://auth.example/token\"\n\
                     scopes = [\"read\"]\n\
                     client_id = \"replay\""
                        .to_string(),
                ),
                settings: "subreddits = [\"rust\"]\n\
                           listings = [\"new\"]\n\
                           user_agent_contact = \"u/tester\""
                    .to_string(),
            }),
            Platform::YouTube => {
                env.insert("YOUTUBE_API_KEY".to_string(), FAKE_YOUTUBE_KEY.to_string());
                sources.push(ExtSource {
                    name: "youtube".to_string(),
                    component: ensure_built("youtube", "youtube_source.wasm", &[]),
                    interval_secs: 3600,
                    fetcher: Some("replay".to_string()),
                    allowlist: vec!["www.googleapis.com".to_string()],
                    secrets: vec![("api_key".to_string(), "YOUTUBE_API_KEY".to_string())],
                    oauth: None,
                    settings: "channels = [\"UC_x5XG1OV2P6uZZ5FSM9Ttw\"]".to_string(),
                });
            }
        }
    }
    let path = write_registry_ext(dir, &sources, &[("replay".to_string(), replay)]);
    (path, env)
}

/// The live registry: the http fetcher component, one app-only source per
/// platform. The public client id is the only credential-derived value written
/// to a file; the secrets travel only in the returned env map, which the
/// loader reads through its closure. No stored token is written: the host
/// fetches the app token itself.
fn build_live_registry(
    live: &HashMap<String, String>,
    platforms: &[Platform],
    dir: &Path,
) -> (PathBuf, HashMap<String, String>) {
    let mut env = HashMap::new();
    let http = ensure_built("http-fetcher", "http_fetcher.wasm", &[]);
    let mut sources = Vec::new();
    for platform in platforms {
        match platform {
            Platform::Reddit => {
                let client_id = live.get("REDDIT_CLIENT_ID").cloned().unwrap_or_default();
                let contact = live
                    .get("REDDIT_USER_AGENT_CONTACT")
                    .filter(|value| !value.is_empty())
                    .map_or(DEFAULT_CONTACT, String::as_str);
                if let Some(secret) = live.get("REDDIT_CLIENT_SECRET") {
                    env.insert("REDDIT_CLIENT_SECRET".to_string(), secret.clone());
                }
                sources.push(ExtSource {
                    name: "reddit".to_string(),
                    component: ensure_built("reddit", "reddit_source.wasm", &[]),
                    interval_secs: 3600,
                    fetcher: Some("http".to_string()),
                    allowlist: vec!["oauth.reddit.com".to_string()],
                    secrets: vec![],
                    oauth: Some(format!(
                        "grant = \"client_credentials\"\n\
                         token_url = \"{REDDIT_TOKEN_URL}\"\n\
                         client_id = {client_id:?}\n\
                         client_secret_env = \"REDDIT_CLIENT_SECRET\""
                    )),
                    settings: format!(
                        "subreddits = [\"rust\"]\n\
                         listings = [\"new\"]\n\
                         user_agent_contact = {contact:?}"
                    ),
                });
            }
            Platform::YouTube => {
                if let Some(key) = live.get("YOUTUBE_API_KEY") {
                    env.insert("YOUTUBE_API_KEY".to_string(), key.clone());
                }
                sources.push(ExtSource {
                    name: "youtube".to_string(),
                    component: ensure_built("youtube", "youtube_source.wasm", &[]),
                    interval_secs: 3600,
                    fetcher: Some("http".to_string()),
                    allowlist: vec!["www.googleapis.com".to_string()],
                    secrets: vec![("api_key".to_string(), "YOUTUBE_API_KEY".to_string())],
                    oauth: None,
                    settings: format!("channels = [\"{LIVE_YOUTUBE_CHANNEL}\"]"),
                });
            }
        }
    }
    let path = write_registry_ext(dir, &sources, &[("http".to_string(), http)]);
    (path, env)
}

/// Pre-writes the stored tokens the replayed sources are handed.
fn provision_tokens(backend: &Backend, platforms: &[Platform], state_dir: &Path) {
    if matches!(backend, Backend::Live(_)) {
        return;
    }
    for platform in platforms {
        match platform {
            Platform::Reddit => {
                write_tokens(state_dir, "reddit", "replay-token", unix_now() + 86_400);
            }
            // The API-key instance needs no login.
            Platform::YouTube => {}
        }
    }
}

fn load_registry(backend: &Backend, path: &Path, env: &HashMap<String, String>) -> LoadedRegistry {
    load_registry_with_env(path, &|key| env.get(key).cloned()).unwrap_or_else(|e| {
        // A live registry carries credentials: its failure text is fixed.
        assert!(
            !matches!(backend, Backend::Live(_)),
            "the live registry failed to load"
        );
        panic!(
            "replay registry failed to load: {:?}",
            miette::Report::new(e)
        );
    })
}

/// The number of records a run waits for: the whole fixture for a replay, the
/// first page (at least one record) for a live poll.
fn want_count(backend: &Backend, platforms: &[Platform]) -> usize {
    match backend {
        Backend::Replay => expected_count(platforms),
        Backend::Live(_) => 1,
    }
}

// ── Host Run ──────────────────────────────────────────────────────────────────

/// Delegates to a `FluvioSink` and counts the records it delivered.
struct CountingSink {
    inner: FluvioSink,
    delivered: AtomicUsize,
}

impl CountingSink {
    fn new(inner: FluvioSink) -> Self {
        Self {
            inner,
            delivered: AtomicUsize::new(0),
        }
    }

    fn delivered(&self) -> usize {
        self.delivered.load(Ordering::SeqCst)
    }
}

impl ItemSink for CountingSink {
    async fn send_batch(&self, plugin: &str, records: Vec<SinkRecord>) -> Result<(), SinkError> {
        let count = records.len();
        self.inner.send_batch(plugin, records).await?;
        self.delivered.fetch_add(count, Ordering::SeqCst);
        Ok(())
    }
}

/// Runs a host over `registry` into `sink` until `delivered()` reaches
/// `at_least` (30 s limit), then cancels and joins. Every plugin must have
/// stopped without being disabled. Returns the final count.
async fn run_host_until<S: ItemSink>(
    registry: &LoadedRegistry,
    state_dir: &Path,
    sink: Arc<S>,
    delivered: impl Fn() -> usize,
    at_least: usize,
) -> miette::Result<usize> {
    let host = Host::load(
        registry,
        HostOptions {
            state_dir: state_dir.to_path_buf(),
        },
    )
    .map_err(|e| miette::miette!("Host::load: {e}"))?;
    let token = CancellationToken::new();
    let handle = tokio::spawn(host.run(sink, token.clone()));

    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    while delivered() < at_least {
        if tokio::time::Instant::now() >= deadline {
            token.cancel();
            let _ = handle.await;
            return Err(miette::miette!(
                "only {} of {at_least} records were produced within 30 s",
                delivered()
            ));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    token.cancel();
    let report = handle
        .await
        .map_err(|e| miette::miette!("host task: {e}"))?;
    for (name, plugin) in &report.plugins {
        if let Some(reason) = &plugin.disabled {
            return Err(miette::miette!("{name} was disabled: {reason}"));
        }
    }
    Ok(delivered())
}

/// Reads `count` records of `topic` from the beginning as `(key, item)` pairs.
async fn read_back(topic: &str, count: usize) -> miette::Result<Vec<(String, SourceItem)>> {
    let stream = FluvioStream::connect().await?;
    let mut records = stream
        .topic_stream(topic, Offset::beginning(), Vec::new())
        .await?;
    let mut out = Vec::new();
    for _ in 0..count {
        let got = tokio::time::timeout(Duration::from_secs(15), records.next())
            .await
            .map_err(|_| miette::miette!("timed out reading back {count} records"))?
            .ok_or_else(|| miette::miette!("stream ended early"))?
            .map_err(|e| miette::miette!("stream error: {e:?}"))?;
        let key = got
            .key()
            .map(|k| String::from_utf8_lossy(k).into_owned())
            .ok_or_else(|| miette::miette!("a record has no key"))?;
        let item: SourceItem = serde_json::from_slice(got.value())
            .map_err(|e| miette::miette!("a record value is not a SourceItem: {e}"))?;
        out.push((key, item));
    }
    Ok(out)
}

/// The number of items the fixtures hold for `platforms`.
fn expected_count(platforms: &[Platform]) -> usize {
    platforms
        .iter()
        .map(|platform| match platform {
            Platform::Reddit => reddit_expected().len(),
            Platform::YouTube => youtube_expected().len(),
        })
        .sum()
}

/// A loaded replay registry in a temp directory, with its state directory.
struct Prepared {
    _tmp: tempfile::TempDir,
    registry: LoadedRegistry,
    state_dir: PathBuf,
}

/// Builds the components, writes the registry and the stored tokens, loads it.
fn prepare(backend: &Backend, platforms: &[Platform]) -> miette::Result<Prepared> {
    let tmp = tempfile::TempDir::new().map_err(|e| miette::miette!("tempdir: {e}"))?;
    let (path, env) = build_registry(backend, platforms, tmp.path());
    let state_dir = tmp.path().join(".fili-state");
    provision_tokens(backend, platforms, &state_dir);
    let registry = load_registry(backend, &path, &env);
    Ok(Prepared {
        _tmp: tmp,
        registry,
        state_dir,
    })
}

/// Runs the host over `platforms` into a unique scratch Fluvio topic, reads
/// the topic back from the beginning and always deletes the topic, also when
/// the body panics.
async fn drive_fluvio(
    backend: &Backend,
    platforms: &[Platform],
) -> miette::Result<Vec<(String, SourceItem)>> {
    let topic = scratch_topic(backend.label()).await?;
    let body = AssertUnwindSafe(async {
        let prepared = prepare(backend, platforms)?;
        let want = want_count(backend, platforms);
        let sink = Arc::new(CountingSink::new(ready_sink(&topic).await?));
        let counted = Arc::clone(&sink);
        let delivered = run_host_until(
            &prepared.registry,
            &prepared.state_dir,
            sink,
            move || counted.delivered(),
            want,
        )
        .await?;
        read_back(&topic, delivered).await
    })
    .catch_unwind()
    .await;
    let deleted = delete_topic(&topic).await;
    match body {
        Ok(outcome) => {
            let records = outcome?;
            deleted?;
            Ok(records)
        }
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// Like `drive_fluvio`, into an in-memory sink: no cluster, no topic. It runs
/// the same host loop and the same per-plugin `disabled` check.
async fn drive_memory(
    backend: &Backend,
    platforms: &[Platform],
) -> miette::Result<Vec<(String, SourceItem)>> {
    let prepared = prepare(backend, platforms)?;
    let want = expected_count(platforms);
    let sink = Arc::new(VecSink::default());
    let counted = Arc::clone(&sink);
    run_host_until(
        &prepared.registry,
        &prepared.state_dir,
        Arc::clone(&sink),
        move || counted.records().len(),
        want,
    )
    .await?;
    Ok(sink
        .records()
        .into_iter()
        .map(|record| (record.key, record.item))
        .collect())
}

// ── Expected Items, Computed From The Fixtures ───────────────────────────────

/// What one landed item must equal.
struct Want {
    id: String,
    url: String,
    title: Option<String>,
    author: Option<String>,
    published: String,
    text: String,
    raw: Value,
}

fn read_json(path: &Path) -> Value {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_slice(&bytes).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

/// The Reddit items the listing fixture must produce, oldest first. Computed
/// at test time from the fixture: expected values are never hand-typed.
fn reddit_expected() -> Vec<Want> {
    let listing = read_json(&reddit_fixture());
    let mut posts: Vec<Value> = listing["data"]["children"]
        .as_array()
        .expect("children")
        .iter()
        .filter(|child| child["kind"] == "t3")
        .map(|child| child["data"].clone())
        .collect();
    posts.sort_by(|a, b| {
        a["created_utc"]
            .as_f64()
            .partial_cmp(&b["created_utc"].as_f64())
            .expect("comparable timestamps")
    });
    posts
        .iter()
        .map(|post| {
            // The plugin maps `created_utc * 1000` to whole milliseconds.
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let created_ms = (post["created_utc"].as_f64().expect("created_utc") * 1000.0) as u64;
            let text = if post["is_self"] == true {
                post["selftext"].as_str().expect("selftext")
            } else {
                post["title"].as_str().expect("title")
            };
            Want {
                id: post["name"].as_str().expect("name").to_string(),
                url: format!(
                    "https://www.reddit.com{}",
                    post["permalink"].as_str().expect("permalink")
                ),
                title: post["title"].as_str().map(str::to_string),
                author: post["author"].as_str().map(str::to_string),
                published: crate::models::epoch_ms_to_rfc3339(created_ms)
                    .expect("fixture time formats"),
                text: text.to_string(),
                raw: serde_json::json!({ "name": post["name"] }),
            }
        })
        .collect()
}

/// The `YouTube` items the playlist fixture must produce, oldest first by
/// publication. Computed at test time from the fixture. The published value
/// prefers `contentDetails.videoPublishedAt` over `snippet.publishedAt`.
fn youtube_expected() -> Vec<Want> {
    let page = read_json(&youtube_fixture());
    let mut wanted: Vec<Want> = page["items"]
        .as_array()
        .expect("items")
        .iter()
        .map(|entry| {
            let snippet = &entry["snippet"];
            let id = entry["contentDetails"]["videoId"]
                .as_str()
                .expect("videoId")
                .to_string();
            let published = entry["contentDetails"]["videoPublishedAt"]
                .as_str()
                .or_else(|| snippet["publishedAt"].as_str())
                .expect("a publication time");
            Want {
                url: format!("https://www.youtube.com/watch?v={id}"),
                id,
                title: snippet["title"].as_str().map(str::to_string),
                author: snippet["videoOwnerChannelTitle"]
                    .as_str()
                    .or_else(|| snippet["channelTitle"].as_str())
                    .map(str::to_string),
                published: crate::models::rfc3339_to_utc(published)
                    .expect("fixture time is RFC 3339"),
                text: snippet["description"]
                    .as_str()
                    .expect("description")
                    .to_string(),
                raw: snippet.clone(),
            }
        })
        .collect();
    // RFC 3339 UTC with fixed millisecond precision sorts as text.
    wanted.sort_by(|a, b| a.published.cmp(&b.published));
    wanted
}

/// Checks one landed record against `want` for `platform`.
fn assert_item(platform: Platform, key: &str, item: &SourceItem, want: &Want) {
    let id = &want.id;
    assert_eq!(key, format!("{}:{id}", platform.name()), "key of {id}");
    assert_eq!(item.id, *id);
    assert_eq!(item.platform, platform.name(), "platform of {id}");
    assert_eq!(item.source_type, SourceType::Social, "source type of {id}");
    assert_eq!(item.url, want.url, "url of {id}");
    assert_eq!(item.title, want.title, "title of {id}");
    assert_eq!(item.author, want.author, "author of {id}");
    assert_eq!(
        item.published.as_deref(),
        Some(want.published.as_str()),
        "published of {id}"
    );
    assert_eq!(item.text, want.text, "text of {id}");
    let raw = item
        .raw
        .as_ref()
        .unwrap_or_else(|| panic!("{id} has no raw"));
    match platform {
        Platform::Reddit => assert_eq!(raw["name"], want.raw["name"], "raw name of {id}"),
        Platform::YouTube => assert_eq!(raw, &want.raw, "raw snippet of {id}"),
    }
}

/// The shared assertion body: `records` hold exactly the fixtures' items for
/// `platforms`. Per platform the items equal the values computed from the
/// fixtures and are oldest first; every key is `<platform>:<id>`; keys are
/// distinct. The order across platforms is not asserted (plugins/README.md:
/// the order across plugins is unspecified).
fn assert_replay_items(records: &[(String, SourceItem)], platforms: &[Platform]) {
    assert_eq!(
        records.len(),
        expected_count(platforms),
        "record count: {:?}",
        records.iter().map(|(key, _)| key).collect::<Vec<_>>()
    );
    let mut keys: Vec<&str> = records.iter().map(|(key, _)| key.as_str()).collect();
    keys.sort_unstable();
    keys.dedup();
    assert_eq!(keys.len(), records.len(), "every key is distinct");

    for platform in platforms {
        let landed: Vec<&(String, SourceItem)> = records
            .iter()
            .filter(|(_, item)| item.platform == platform.name())
            .collect();
        let wanted = match platform {
            Platform::Reddit => reddit_expected(),
            Platform::YouTube => youtube_expected(),
        };
        assert_eq!(landed.len(), wanted.len(), "{} item count", platform.name());
        // Oldest first: `wanted` is sorted that way and landing order must match.
        for ((key, item), want) in landed.into_iter().zip(&wanted) {
            assert_item(*platform, key, item, want);
        }
    }
}

// ── Live Structure Check ──────────────────────────────────────────────────────

/// The earliest `published` a live item may carry (Reddit's launch).
const PUBLISHED_FLOOR: &str = "2005-06-23T00:00:00Z";

/// How far past now a `published` value may lie (clock skew allowance).
const PUBLISHED_CEILING_HOURS: i64 = 6;

/// The structure-only assertion for live records: the first problem found, as
/// a fixed text naming the platform and at most an index, never a record, a
/// request, a response or a header. `None` when every record is well formed.
fn live_structure_problem(records: &[(String, SourceItem)], platform: Platform) -> Option<String> {
    let name = platform.name();
    if records.is_empty() {
        return Some(format!("{name}: no records were delivered"));
    }
    let floor = chrono::DateTime::parse_from_rfc3339(PUBLISHED_FLOOR).expect("floor is RFC 3339");
    let ceiling = chrono::Utc::now() + chrono::Duration::hours(PUBLISHED_CEILING_HOURS);
    for (index, (key, item)) in records.iter().enumerate() {
        if item.platform != name {
            return Some(format!("{name}: record {index} has the wrong platform"));
        }
        if item.source_type != SourceType::Social {
            return Some(format!("{name}: record {index} is not a social item"));
        }
        if item.id.is_empty() || *key != format!("{name}:{}", item.id) {
            return Some(format!("{name}: record {index} has a wrong key or id"));
        }
        if platform == Platform::Reddit && !item.id.starts_with("t3_") {
            return Some(format!("{name}: record {index} id lacks the t3_ prefix"));
        }
        if !item.url.starts_with("https://") {
            return Some(format!("{name}: record {index} url is not https"));
        }
        if let Some(published) = &item.published {
            let Ok(parsed) = chrono::DateTime::parse_from_rfc3339(published) else {
                return Some(format!("{name}: record {index} published is malformed"));
            };
            if parsed < floor {
                return Some(format!("{name}: record {index} published is too early"));
            }
            if parsed > ceiling {
                return Some(format!("{name}: record {index} published is in the future"));
            }
        }
    }
    None
}

// ── Tests ─────────────────────────────────────────────────────────────────────

/// The tracer: one Reddit poll, replayed, lands exactly the fixture's three
/// items in a scratch topic, oldest first.
#[tokio::test]
async fn e2e_replay_reddit_poll_lands_exact_items_in_a_scratch_topic() -> miette::Result<()> {
    if !fluvio_or_skip().await {
        return Ok(());
    }
    let platforms = [Platform::Reddit];
    let records = drive_fluvio(&Backend::Replay, &platforms).await?;
    assert_replay_items(&records, &platforms);
    Ok(())
}

/// Both platforms in one host run: six records in one scratch topic, exact
/// items per platform, no assertion on the order across platforms.
#[tokio::test]
async fn e2e_replay_reddit_and_youtube_land_exact_items_in_a_scratch_topic() -> miette::Result<()> {
    if !fluvio_or_skip().await {
        return Ok(());
    }
    let platforms = [Platform::Reddit, Platform::YouTube];
    let records = drive_fluvio(&Backend::Replay, &platforms).await?;
    assert_replay_items(&records, &platforms);
    Ok(())
}

/// The same two-platform host run into an in-memory sink: no cluster and no
/// skip. It needs the toolchain and runs wherever the components can be built.
#[tokio::test]
async fn e2e_replay_host_path_needs_no_cluster() -> miette::Result<()> {
    let platforms = [Platform::Reddit, Platform::YouTube];
    let records = drive_memory(&Backend::Replay, &platforms).await?;
    assert_replay_items(&records, &platforms);
    Ok(())
}

// ── Live Tests ────────────────────────────────────────────────────────────────

/// The credential keys a platform's live poll needs, in the order the gate
/// reports them.
fn live_keys(platform: Platform) -> &'static [&'static str] {
    match platform {
        Platform::Reddit => &["REDDIT_CLIENT_ID", "REDDIT_CLIENT_SECRET"],
        Platform::YouTube => &["YOUTUBE_API_KEY"],
    }
}

/// One first poll of `platform` against the real API into a scratch topic.
/// Skips (returns before anything is built or any cluster is touched) when a
/// key is missing. The failure text is fixed: it never carries a request, a
/// response, a header or an item.
async fn run_live_poll(platform: Platform) -> miette::Result<()> {
    let env = load_live_env();
    if let Some(key) = missing_key(live_keys(platform), &|key| env.get(key).cloned()) {
        eprintln!("{}", skip_line(key));
        return Ok(());
    }
    if !fluvio_or_skip().await {
        return Ok(());
    }
    let name = platform.name();
    let records = drive_fluvio(&Backend::Live(env), &[platform])
        .await
        .map_err(|_| miette::miette!("the live {name} poll failed"))?;
    if let Some(problem) = live_structure_problem(&records, platform) {
        return Err(miette::miette!("{problem}"));
    }
    Ok(())
}

/// One live first poll of the app-only Reddit instance.
#[tokio::test]
async fn e2e_credentialed_reddit_live_poll_has_the_expected_structure() -> miette::Result<()> {
    run_live_poll(Platform::Reddit).await
}

/// One live first poll of the API-key `YouTube` instance.
#[tokio::test]
async fn e2e_credentialed_youtube_live_poll_has_the_expected_structure() -> miette::Result<()> {
    run_live_poll(Platform::YouTube).await
}

/// No file of the live registry holds a secret; the public client id may be
/// there. Needs no network and no cluster.
#[test]
fn e2e_credentialed_registry_files_hold_no_secret_value() {
    const FAKE_ID: &str = "fake-public-client-id";
    const FAKE_SECRET: &str = "fake-client-secret-value";
    const FAKE_KEY: &str = "fake-api-key-value";
    let live = env_of(&[
        ("REDDIT_CLIENT_ID", FAKE_ID),
        ("REDDIT_CLIENT_SECRET", FAKE_SECRET),
        ("REDDIT_USER_AGENT_CONTACT", "u/tester"),
        ("YOUTUBE_API_KEY", FAKE_KEY),
    ]);
    let tmp = tempfile::TempDir::new().expect("tempdir");
    let (_, env) = build_registry(
        &Backend::Live(live),
        &[Platform::Reddit, Platform::YouTube],
        tmp.path(),
    );
    // The secrets reach the loader only through this map.
    assert_eq!(
        env.get("REDDIT_CLIENT_SECRET").map(String::as_str),
        Some(FAKE_SECRET)
    );
    assert_eq!(
        env.get("YOUTUBE_API_KEY").map(String::as_str),
        Some(FAKE_KEY)
    );
    assert!(!env.contains_key("REDDIT_CLIENT_ID"));

    let mut reddit_toml_has_id = false;
    for entry in std::fs::read_dir(tmp.path()).expect("read dir") {
        let path = entry.expect("dir entry").path();
        if !path.is_file() {
            continue;
        }
        let text = std::fs::read_to_string(&path).expect("read file");
        assert!(
            !text.contains(FAKE_SECRET) && !text.contains(FAKE_KEY),
            "a registry file holds a secret value"
        );
        if path.file_name().is_some_and(|name| name == "reddit.toml") {
            reddit_toml_has_id = text.contains(FAKE_ID);
        }
    }
    assert!(reddit_toml_has_id, "the public client id is in reddit.toml");
}

/// A well-formed record of `platform` with `id` and `published`.
fn live_record(platform: Platform, id: &str, published: Option<&str>) -> (String, SourceItem) {
    let name = platform.name();
    (
        format!("{name}:{id}"),
        SourceItem {
            source_type: SourceType::Social,
            platform: name.to_string(),
            id: id.to_string(),
            url: format!("https://example.test/{id}"),
            title: None,
            text: String::new(),
            author: None,
            published: published.map(str::to_string),
            raw: None,
            html: None,
        },
    )
}

#[test]
fn e2e_credentialed_structure_check_accepts_wellformed_records_and_names_each_problem() {
    let recent = chrono::Utc::now().to_rfc3339();
    let reddit_ok = [
        live_record(Platform::Reddit, "t3_abc", Some(&recent)),
        live_record(Platform::Reddit, "t3_def", None),
    ];
    assert_eq!(live_structure_problem(&reddit_ok, Platform::Reddit), None);
    let youtube_ok = [live_record(Platform::YouTube, "vid1", Some(&recent))];
    assert_eq!(live_structure_problem(&youtube_ok, Platform::YouTube), None);

    // Each problem is named, and the text carries no item content.
    assert!(live_structure_problem(&[], Platform::Reddit).is_some());

    let wrong_platform = [live_record(Platform::YouTube, "t3_abc", None)];
    assert!(live_structure_problem(&wrong_platform, Platform::Reddit).is_some());

    let mut wrong_key = live_record(Platform::Reddit, "t3_abc", None);
    wrong_key.0 = "reddit:other".to_string();
    assert!(live_structure_problem(&[wrong_key], Platform::Reddit).is_some());

    let bad_prefix = [live_record(Platform::Reddit, "abc", None)];
    assert!(live_structure_problem(&bad_prefix, Platform::Reddit).is_some());

    let mut not_https = live_record(Platform::YouTube, "vid1", None);
    not_https.1.url = "http://example.test/secret-content".to_string();
    let problem = live_structure_problem(&[not_https], Platform::YouTube).expect("a problem");
    assert!(!problem.contains("secret-content"));

    for published in [
        "not a time",
        "2005-06-22T23:59:59Z",
        &(chrono::Utc::now() + chrono::Duration::hours(7)).to_rfc3339(),
    ] {
        let bad = [live_record(Platform::YouTube, "vid1", Some(published))];
        let problem = live_structure_problem(&bad, Platform::YouTube).expect("a published problem");
        assert!(!problem.contains(published), "no item content in the text");
    }
}

// ── Staleness Rule ────────────────────────────────────────────────────────────

/// A crate laid out like `plugins/<name>` under a temp root, with the WIT at
/// `<root>/wit`, every input at `base` and the wasm 100 s newer.
struct StaleFixture {
    _tmp: tempfile::TempDir,
    crate_dir: PathBuf,
    wasm: PathBuf,
    wit: PathBuf,
    extra: PathBuf,
    base: std::time::SystemTime,
}

impl StaleFixture {
    fn new() -> Self {
        let tmp = tempfile::TempDir::new().expect("tempdir");
        let crate_dir = tmp.path().join("plugins/demo");
        std::fs::create_dir_all(crate_dir.join("src")).expect("src dir");
        std::fs::create_dir_all(tmp.path().join("wit")).expect("wit dir");
        std::fs::create_dir_all(crate_dir.join("target/release")).expect("target dir");
        let wasm = crate_dir.join("target/release/demo.wasm");
        let wit = tmp.path().join("wit/host.wit");
        let extra = tmp.path().join("fixture.json");
        let base = std::time::SystemTime::now() - Duration::from_secs(10_000);
        let inputs = [
            crate_dir.join("src/lib.rs"),
            crate_dir.join("src/bindings.rs"),
            crate_dir.join("Cargo.toml"),
            crate_dir.join("Cargo.lock"),
            wit.clone(),
            extra.clone(),
        ];
        for file in inputs.iter().chain([&wasm]) {
            std::fs::write(file, b"x").expect("write file");
        }
        let fixture = Self {
            _tmp: tmp,
            crate_dir,
            wasm,
            wit,
            extra,
            base,
        };
        for file in &inputs {
            fixture.touch(file, 0);
        }
        fixture.touch(&fixture.wasm, 100);
        fixture
    }

    /// Sets the modification time of `file` to `base` plus `secs`.
    fn touch(&self, file: &Path, secs: u64) {
        std::fs::OpenOptions::new()
            .write(true)
            .open(file)
            .expect("open for write")
            .set_modified(self.base + Duration::from_secs(secs))
            .expect("set modified time");
    }

    fn stale(&self) -> bool {
        is_stale(
            &self.crate_dir,
            &self.wasm,
            std::slice::from_ref(&self.extra),
        )
    }
}

#[test]
fn e2e_replay_staleness_ignores_generated_bindings() {
    let fixture = StaleFixture::new();
    assert!(!fixture.stale(), "a fresh wasm is not stale");

    // cargo-component rewrites `src/bindings.rs` on every build: newer than
    // the wasm must still read as fresh.
    fixture.touch(&fixture.crate_dir.join("src/bindings.rs"), 200);
    assert!(!fixture.stale(), "only src/bindings.rs is newer");
}

#[test]
fn e2e_replay_staleness_sees_a_newer_source_or_fixture() {
    let fixture = StaleFixture::new();
    std::fs::remove_file(&fixture.wasm).expect("remove wasm");
    assert!(fixture.stale(), "a missing wasm is stale");

    let newer_inputs = |fixture: &StaleFixture| {
        vec![
            fixture.crate_dir.join("src/lib.rs"),
            fixture.crate_dir.join("Cargo.toml"),
            fixture.crate_dir.join("Cargo.lock"),
            fixture.wit.clone(),
            fixture.extra.clone(),
        ]
    };
    for index in 0..newer_inputs(&fixture).len() {
        let fixture = StaleFixture::new();
        assert!(!fixture.stale());
        let newer = newer_inputs(&fixture).remove(index);
        fixture.touch(&newer, 300);
        assert!(
            fixture.stale(),
            "{} is newer than the wasm",
            newer.display()
        );
    }
}

// ── Credential Gating Tests ───────────────────────────────────────────────────

fn env_of(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
        .collect()
}

#[test]
fn e2e_credentialed_gate_names_the_first_missing_key() {
    let keys = ["REDDIT_CLIENT_ID", "REDDIT_CLIENT_SECRET"];
    let none = env_of(&[]);
    assert_eq!(
        missing_key(&keys, &|key| none.get(key).cloned()),
        Some("REDDIT_CLIENT_ID")
    );
    let only_id = env_of(&[("REDDIT_CLIENT_ID", "id")]);
    assert_eq!(
        missing_key(&keys, &|key| only_id.get(key).cloned()),
        Some("REDDIT_CLIENT_SECRET")
    );
}

#[test]
fn e2e_credentialed_gate_treats_an_empty_value_as_missing() {
    let keys = ["YOUTUBE_API_KEY"];
    let empty = env_of(&[("YOUTUBE_API_KEY", "")]);
    assert_eq!(
        missing_key(&keys, &|key| empty.get(key).cloned()),
        Some("YOUTUBE_API_KEY")
    );
}

#[test]
fn e2e_credentialed_gate_passes_when_every_key_is_present() {
    let keys = ["REDDIT_CLIENT_ID", "REDDIT_CLIENT_SECRET"];
    let full = env_of(&[
        ("REDDIT_CLIENT_ID", "id"),
        ("REDDIT_CLIENT_SECRET", "secret"),
    ]);
    assert_eq!(missing_key(&keys, &|key| full.get(key).cloned()), None);
}

#[test]
fn e2e_credentialed_skip_line_has_the_documented_text() {
    assert_eq!(
        skip_line("REDDIT_CLIENT_ID"),
        "skipped: missing REDDIT_CLIENT_ID"
    );
}

#[test]
fn e2e_credentialed_process_env_wins_over_dotenv_and_an_empty_value_falls_back() {
    let dotenv = env_of(&[("A", "from-dotenv"), ("B", "from-dotenv")]);
    let process_values = env_of(&[("A", "from-process"), ("B", "")]);
    let process = |key: &str| process_values.get(key).cloned();
    assert_eq!(
        lookup_env("A", &process, &dotenv).as_deref(),
        Some("from-process")
    );
    assert_eq!(
        lookup_env("B", &process, &dotenv).as_deref(),
        Some("from-dotenv")
    );
    assert_eq!(lookup_env("C", &process, &dotenv), None);
}

#[test]
fn e2e_credentialed_dotenv_values_are_taken_literally() {
    let dir = tempfile::TempDir::new().expect("tempdir");
    let good = dir.path().join("good.env");
    std::fs::write(&good, "FILI_TEST_LITERAL_KEY='a$b'\n").expect("write dotenv");
    let dotenv = crate::plugins::secrets::read_dotenv(&good).expect("literal dotenv parses");
    assert_eq!(
        dotenv.get("FILI_TEST_LITERAL_KEY").map(String::as_str),
        Some("a$b")
    );
    // The layering keeps the literal value when the process has none.
    let layered = lookup_env("FILI_TEST_LITERAL_KEY", &|_| None, &dotenv);
    assert_eq!(layered.as_deref(), Some("a$b"));

    // A file the parser rejects loads as an empty map (and prints a fixed note).
    let bad = dir.path().join("bad.env");
    std::fs::write(&bad, "YOUTUBE_API_KEY=a$b\n").expect("write dotenv");
    let env = load_live_env_from(&bad);
    assert!(
        !env.values().any(|value| value.contains("a$b")),
        "a rejected dotenv file contributes nothing"
    );
}
