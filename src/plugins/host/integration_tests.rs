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
}

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
    let Backend::Replay = backend;
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

/// Pre-writes the stored tokens the replayed sources are handed.
fn provision_tokens(backend: &Backend, platforms: &[Platform], state_dir: &Path) {
    let Backend::Replay = backend;
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

fn load_registry(path: &Path, env: &HashMap<String, String>) -> LoadedRegistry {
    load_registry_with_env(path, &|key| env.get(key).cloned()).unwrap_or_else(|e| {
        panic!(
            "replay registry failed to load: {:?}",
            miette::Report::new(e)
        );
    })
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
    let registry = load_registry(&path, &env);
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
    let topic = scratch_topic("replay").await?;
    let body = AssertUnwindSafe(async {
        let prepared = prepare(backend, platforms)?;
        let want = expected_count(platforms);
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
