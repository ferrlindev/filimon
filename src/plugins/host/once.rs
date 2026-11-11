// ─────────────────────────────────────────────────────────────────────────────
// plugins/host/once.rs - One poll cycle without a loop, for `fili plugins run-once`
//
// `poll_once` is the poll half of one scheduler cycle: instantiate, `init`,
// `poll`, map. It takes no sink and no cursor store, so it cannot send or
// persist anything. The size rule for the next cursor and the single place a
// secret is exposed are the scheduler's own helpers, not copies. The token
// provider is consulted through the scheduler's `auth_for_poll`, so a stored
// login reaches the guest here exactly as it does in the loop.
//
// `commit_once` is the `--produce` half: it sends the batch with the
// scheduler's own `send_records` and only then writes the cursor (D-07), so a
// failing sink never moves the cursor.
//
// Nothing here is logged or returned that carries settings, secret values or
// init arguments: failures are `CycleOutcome`s whose `describe` text is cleaned.
// ─────────────────────────────────────────────────────────────────────────────

use super::cursor::CursorStore;
use super::engine::clean_guest_text;
use super::instance::{CallOutcome, InstanceFactory, PluginInstance};
use super::map::map_items;
use super::scheduler::{
    CycleOutcome, Phase, PluginSpec, auth_for_poll, expose_secrets, oversize_cursor_outcome,
    send_records, warn_login_hint,
};
use super::sink::{ItemSink, SinkRecord};
use crate::plugins::auth::store::AccessToken;

/// Longest error text kept from a failed cursor write, in characters.
const COMMIT_TEXT_CLIP: usize = 512;

/// What one successful poll returned, ready to print and, when asked, to send.
#[derive(Debug)]
pub struct PolledOnce {
    /// The mapped records, in the order the guest returned them.
    pub records: Vec<SinkRecord>,
    /// Items rejected during mapping.
    pub dropped: usize,
    /// The cursor the guest returned for the next poll.
    pub next_cursor: Option<Vec<u8>>,
}

/// Runs the poll half of one cycle against a fresh instance: instantiate,
/// `init`, then `poll` from `cursor`, then map the items.
///
/// # Errors
/// The cycle's `CycleOutcome` when the instance cannot be built, `init` or
/// `poll` fails, the token provider reports a problem, or the next cursor is
/// over the byte cap.
pub async fn poll_once<F: InstanceFactory>(
    spec: &PluginSpec,
    factory: &F,
    cursor: Option<&[u8]>,
) -> Result<PolledOnce, CycleOutcome> {
    let mut instance = factory
        .instantiate(&spec.name)
        .await
        .map_err(|e| CycleOutcome::BuildFailed(e.0))?;

    match instance
        .init(&spec.settings_toml, &expose_secrets(spec))
        .await
    {
        CallOutcome::Ok(()) => {}
        CallOutcome::Guest(error) => {
            return Err(CycleOutcome::Guest {
                phase: Phase::Init,
                error,
            });
        }
        CallOutcome::InstanceFatal(text) => return Err(CycleOutcome::InstanceFatal(text)),
    }

    let auth = auth_for_poll(spec).await?;
    if let Some(hint) = auth.hint {
        warn_login_hint(&spec.name, hint);
    }

    let response = match instance
        .poll(cursor, auth.token.as_ref().map(AccessToken::expose))
        .await
    {
        CallOutcome::Ok(response) => response,
        CallOutcome::Guest(error) => {
            return Err(CycleOutcome::Guest {
                phase: Phase::Poll,
                error,
            });
        }
        CallOutcome::InstanceFatal(text) => return Err(CycleOutcome::InstanceFatal(text)),
    };
    if let Some(outcome) = oversize_cursor_outcome(&response) {
        return Err(outcome);
    }

    let mapped = map_items(&spec.name, response.items);
    Ok(PolledOnce {
        records: mapped.records,
        dropped: mapped.dropped,
        next_cursor: response.next_cursor,
    })
}

/// Why committing a polled batch failed.
#[derive(Debug)]
pub enum CommitFailure {
    /// The sink failed; the cursor was not touched. The text is cleaned.
    Sink(String),
    /// The batch was sent, but the cursor could not be written, so the next
    /// poll replays it. `sent` is how many records were delivered.
    Cursor { sent: u64, text: String },
}

/// Sends `polled`'s records and only then writes its cursor (D-07).
///
/// With no records the sink is not called and the cursor is still written.
/// Returns the number of records sent.
///
/// # Errors
/// `CommitFailure::Sink` when the sink fails (the cursor file is untouched),
/// `CommitFailure::Cursor` when the send succeeded but the cursor write failed.
pub async fn commit_once<S: ItemSink>(
    plugin: &str,
    polled: PolledOnce,
    sink: &S,
    cursors: &CursorStore,
) -> Result<u64, CommitFailure> {
    // The send first: a failure leaves the cursor file exactly as it was.
    let sent = send_records(sink, plugin, polled.records)
        .await
        .map_err(CommitFailure::Sink)?;
    // Only after the sink returned (or was skipped for an empty batch).
    let written = cursors.write(plugin, polled.next_cursor.as_deref());
    written.map_err(|e| CommitFailure::Cursor {
        sent,
        text: clean_guest_text(&e.to_string(), COMMIT_TEXT_CLIP),
    })?;
    Ok(sent)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;
    use crate::plugins::host::cursor::MAX_CURSOR_BYTES;
    use crate::plugins::host::engine::PollError;
    use crate::plugins::host::scheduler::describe;
    use crate::plugins::host::testing::{FakeEvent, FakeFactory, FakeStep, spec, wit_item};

    fn once_spec() -> PluginSpec {
        spec("fake", 60, 60, 3600, 2.0)
    }

    fn poll_ok(
        items: Vec<crate::plugins::host::engine::WitItem>,
        next: Option<Vec<u8>>,
    ) -> FakeStep {
        FakeStep::PollOk {
            items,
            next_cursor: next,
            delay: Duration::ZERO,
        }
    }

    #[tokio::test]
    async fn once_polls_one_cycle_and_maps_items() {
        let factory = FakeFactory::new(vec![
            FakeStep::InitOk,
            poll_ok(vec![wit_item("a"), wit_item("b")], Some(vec![7])),
        ]);

        let polled = poll_once(&once_spec(), &factory, None)
            .await
            .expect("one cycle");

        let keys: Vec<&str> = polled.records.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, ["fake:a", "fake:b"]);
        assert_eq!(polled.dropped, 0);
        assert_eq!(polled.next_cursor, Some(vec![7]));
        assert_eq!(factory.builds(), 1);
        assert_eq!(factory.inits(), 1);
        let polls = factory
            .events()
            .iter()
            .filter(|e| matches!(e, FakeEvent::Poll { .. }))
            .count();
        assert_eq!(polls, 1, "exactly one poll");
    }

    #[tokio::test]
    async fn once_counts_dropped_items_and_keeps_duplicate_keys_in_guest_order() {
        let mut blank = wit_item("x");
        blank.id = String::new();
        let factory = FakeFactory::new(vec![
            FakeStep::InitOk,
            poll_ok(vec![wit_item("a"), wit_item("a"), blank], None),
        ]);

        let polled = poll_once(&once_spec(), &factory, None)
            .await
            .expect("one cycle");

        let keys: Vec<&str> = polled.records.iter().map(|r| r.key.as_str()).collect();
        assert_eq!(keys, ["fake:a", "fake:a"], "equal keys are all kept");
        assert_eq!(polled.dropped, 1);
    }

    #[tokio::test]
    async fn once_passes_the_saved_cursor_and_returns_the_next_one() {
        let factory = FakeFactory::new(vec![FakeStep::InitOk, poll_ok(vec![], Some(vec![9, 9]))]);

        let polled = poll_once(&once_spec(), &factory, Some(&[3]))
            .await
            .expect("one cycle");

        assert_eq!(factory.poll_cursors(), vec![Some(vec![3])]);
        assert_eq!(polled.next_cursor, Some(vec![9, 9]));
    }

    #[tokio::test]
    async fn once_failures_are_cycle_outcomes_with_describe_text() {
        let cases: [(Vec<FakeStep>, &str); 3] = [
            (
                vec![
                    FakeStep::InitOk,
                    FakeStep::PollErr(PollError::Transient("later".to_string())),
                ],
                "poll transient:",
            ),
            (
                vec![FakeStep::InitOk, FakeStep::Trap("wasm trap".to_string())],
                "instance fault:",
            ),
            (
                vec![FakeStep::InitErr(PollError::Config("bad".to_string()))],
                "init ",
            ),
        ];
        for (script, expected) in cases {
            let factory = FakeFactory::new(script);
            let outcome = poll_once(&once_spec(), &factory, None)
                .await
                .expect_err("the cycle fails");
            let text = describe(&outcome);
            assert!(text.starts_with(expected), "{expected:?} vs {text}");
        }

        let outcome = CycleOutcome::BuildFailed("no memory".to_string());
        assert!(describe(&outcome).starts_with("instance build failed:"));
    }

    #[tokio::test]
    async fn once_build_failure_is_a_build_failed_outcome() {
        use crate::plugins::host::instance::InstanceBuildError;

        struct Failing;
        impl InstanceFactory for Failing {
            type Instance = <FakeFactory as InstanceFactory>::Instance;
            async fn instantiate(
                &self,
                _plugin: &str,
            ) -> Result<Self::Instance, InstanceBuildError> {
                Err(InstanceBuildError("no memory".to_string()))
            }
        }

        let outcome = poll_once(&once_spec(), &Failing, None)
            .await
            .expect_err("build fails");
        assert!(
            matches!(outcome, CycleOutcome::BuildFailed(_)),
            "{outcome:?}"
        );
        assert!(describe(&outcome).starts_with("instance build failed:"));
    }

    #[tokio::test]
    async fn once_rejects_an_oversize_cursor_like_the_scheduler() {
        let factory = FakeFactory::new(vec![
            FakeStep::InitOk,
            poll_ok(vec![wit_item("a")], Some(vec![1; MAX_CURSOR_BYTES + 1])),
        ]);

        let outcome = poll_once(&once_spec(), &factory, None)
            .await
            .expect_err("oversize");

        let CycleOutcome::Guest {
            phase: Phase::Poll,
            error: PollError::Fatal(text),
        } = &outcome
        else {
            panic!("expected a poll fatal error, got {outcome:?}");
        };
        assert!(text.contains("next-cursor"), "{text}");
        assert!(text.contains("4096"), "{text}");

        // The very same rule the scheduler uses.
        let response = crate::plugins::host::engine::PollResponse {
            items: vec![],
            next_cursor: Some(vec![1; MAX_CURSOR_BYTES + 1]),
        };
        let shared = oversize_cursor_outcome(&response).expect("over the cap");
        assert_eq!(describe(&shared), describe(&outcome));
    }

    // ── The stored login (D-15) ───────────────────────────────────────────────

    use crate::plugins::auth::testing::{provider_for, unix_now, write_tokens};

    #[tokio::test]
    async fn once_hands_the_stored_access_token_to_poll() {
        let dir = TempDir::new().expect("tempdir");
        write_tokens(dir.path(), "fake", "AT-once", unix_now() + 3600);
        let mut spec = once_spec();
        spec.auth = Some(provider_for(dir.path(), "fake"));
        let factory = FakeFactory::new(vec![FakeStep::InitOk, poll_ok(vec![], None)]);

        poll_once(&spec, &factory, None).await.expect("one cycle");

        assert_eq!(factory.poll_tokens(), vec![Some("AT-once".to_string())]);
    }

    #[tokio::test]
    async fn once_reports_a_token_file_problem_naming_plugin_and_path() {
        let dir = TempDir::new().expect("tempdir");
        let auth_dir = dir.path().join("auth");
        std::fs::create_dir_all(&auth_dir).expect("auth dir");
        let path = auth_dir.join("fake.json");
        std::fs::write(&path, "NOT-JSON-SENTINEL").expect("write the corrupt file");
        let mut spec = once_spec();
        spec.auth = Some(provider_for(dir.path(), "fake"));
        let factory = FakeFactory::new(vec![FakeStep::InitOk, poll_ok(vec![], None)]);

        let outcome = poll_once(&spec, &factory, None)
            .await
            .expect_err("a corrupt token file fails the cycle");

        let CycleOutcome::TokenFile(text) = &outcome else {
            panic!("expected a token file outcome, got {outcome:?}");
        };
        assert!(text.contains("`fake`"), "{text}");
        assert!(text.contains(&path.display().to_string()), "{text}");
        assert!(!text.contains("NOT-JSON-SENTINEL"), "{text}");
        assert!(factory.poll_tokens().is_empty(), "the guest was never polled");
        assert_eq!(
            std::fs::read(&path).expect("bytes"),
            b"NOT-JSON-SENTINEL",
            "the file is left as found"
        );
    }

    // ── Refresh before the poll (D-07, D-09, D-15) ────────────────────────────

    use crate::plugins::auth::provider::TokenProvider;
    use crate::plugins::auth::store::{RefreshToken, StoredTokens, TokenStore};
    use crate::plugins::auth::testing::{oauth_config_for, token_body};
    use crate::plugins::host::testing::{LogCapture, Reply, TestHttpServer};

    /// Stores an already expired login with a refresh token for `fake`.
    fn store_expired_login(state_dir: &std::path::Path) {
        TokenStore::new(state_dir, "fake")
            .write(&StoredTokens {
                access_token: AccessToken::new("AT-OLD".to_string()),
                refresh_token: Some(RefreshToken::new("RT-OLD".to_string())),
                expires_at: unix_now() - 10,
                token_type: None,
                scope: None,
                needs_login: false,
                obtained_at: unix_now() - 3600,
            })
            .expect("write the login");
    }

    /// A spec whose provider refreshes against `server`.
    fn refreshing_spec(state_dir: &std::path::Path, server: &TestHttpServer) -> PluginSpec {
        let mut spec = once_spec();
        spec.auth = Some(Arc::new(TokenProvider::new(
            "fake",
            oauth_config_for(&server.url("/token"), None),
            state_dir,
        )));
        spec
    }

    #[tokio::test]
    async fn once_refreshes_and_persists_the_token_like_the_poll_loop() {
        let dir = TempDir::new().expect("tempdir");
        store_expired_login(dir.path());
        let server =
            TestHttpServer::start(|_| Reply::json(200, &token_body("AT-NEW", None, Some(3600))))
                .await;
        let factory = FakeFactory::new(vec![FakeStep::InitOk, poll_ok(vec![], None)]);

        poll_once(&refreshing_spec(dir.path(), &server), &factory, None)
            .await
            .expect("one cycle");

        assert_eq!(factory.poll_tokens(), vec![Some("AT-NEW".to_string())]);
        let stored = TokenStore::new(dir.path(), "fake")
            .read()
            .expect("read")
            .expect("a login");
        assert_eq!(stored.access_token.expose(), "AT-NEW");
        assert_eq!(stored.refresh_token.expect("kept").expose(), "RT-OLD");
    }

    #[tokio::test]
    async fn once_reports_a_failed_refresh_as_a_token_refresh_outcome() {
        let dir = TempDir::new().expect("tempdir");
        store_expired_login(dir.path());
        let server = TestHttpServer::start(|_| Reply::status(503)).await;
        let factory = FakeFactory::new(vec![FakeStep::InitOk, poll_ok(vec![], None)]);

        let outcome = poll_once(&refreshing_spec(dir.path(), &server), &factory, None)
            .await
            .expect_err("the refresh fails");

        let CycleOutcome::TokenRefresh(text) = &outcome else {
            panic!("expected a token refresh outcome, got {outcome:?}");
        };
        assert!(text.contains("HTTP 503"), "{text}");
        assert!(!text.contains("AT-OLD") && !text.contains("RT-OLD"), "{text}");
        assert!(factory.poll_tokens().is_empty(), "the guest was never polled");
        assert!(describe(&outcome).starts_with("token refresh failed:"));
    }

    #[tokio::test]
    async fn once_polls_with_none_and_logs_the_login_hint_when_not_logged_in() {
        let (logs, _guard) = LogCapture::start();
        let dir = TempDir::new().expect("tempdir");
        let mut spec = once_spec();
        spec.auth = Some(provider_for(dir.path(), "fake"));
        let factory = FakeFactory::new(vec![FakeStep::InitOk, poll_ok(vec![], None)]);

        poll_once(&spec, &factory, None).await.expect("one cycle");

        assert_eq!(factory.poll_tokens(), vec![None]);
        let text = logs.text();
        let hints = text
            .lines()
            .filter(|line| line.contains("run: fili auth login"))
            .count();
        assert_eq!(hints, 1, "{text}");
    }

    // ── commit_once (D-07) ────────────────────────────────────────────────────

    use std::path::PathBuf;
    use std::sync::Mutex;

    use tempfile::TempDir;

    use crate::plugins::host::cursor::read_cursor_file;
    use crate::plugins::host::sink::{SinkError, VecSink};
    use crate::plugins::host::testing::FlakySink;

    /// A poll result with one record per id and the given next cursor.
    fn polled(ids: &[&str], next_cursor: Option<Vec<u8>>) -> PolledOnce {
        let mapped = map_items("fake", ids.iter().map(|id| wit_item(id)).collect());
        PolledOnce {
            records: mapped.records,
            dropped: mapped.dropped,
            next_cursor,
        }
    }

    fn open_store() -> (TempDir, CursorStore) {
        let dir = TempDir::new().expect("tempdir");
        let store = CursorStore::open(dir.path()).expect("cursor store");
        (dir, store)
    }

    /// A sink that records what the plugin's cursor file held at the moment
    /// each batch arrived, to prove the cursor is written after the send.
    struct InspectingSink {
        inner: VecSink,
        dir: PathBuf,
        seen: Mutex<Vec<Option<Vec<u8>>>>,
    }

    impl ItemSink for InspectingSink {
        async fn send_batch(
            &self,
            plugin: &str,
            records: Vec<SinkRecord>,
        ) -> Result<(), SinkError> {
            let on_disk = read_cursor_file(&self.dir, plugin).expect("cursor file readable");
            self.seen.lock().expect("seen").push(on_disk);
            self.inner.send_batch(plugin, records).await
        }
    }

    #[tokio::test]
    async fn commit_sends_then_writes_the_cursor_in_that_order() {
        let (dir, store) = open_store();
        store.write("fake", Some(&[1])).expect("old cursor");
        let sink = InspectingSink {
            inner: VecSink::default(),
            dir: dir.path().to_path_buf(),
            seen: Mutex::new(Vec::new()),
        };

        let sent = commit_once("fake", polled(&["a", "b"], Some(vec![2])), &sink, &store)
            .await
            .expect("commit");

        assert_eq!(sent, 2);
        assert_eq!(
            *sink.seen.lock().expect("seen"),
            vec![Some(vec![1])],
            "during the send the file still holds the old cursor"
        );
        assert_eq!(sink.inner.records().len(), 2);
        assert_eq!(
            read_cursor_file(dir.path(), "fake").expect("read"),
            Some(vec![2])
        );
    }

    #[tokio::test]
    async fn commit_sink_failure_leaves_the_cursor_file_byte_identical() {
        let (dir, store) = open_store();
        store.write("fake", Some(&[1])).expect("old cursor");
        let path = store.path_for("fake");
        let before = std::fs::read(&path).expect("bytes");

        let failure = commit_once(
            "fake",
            polled(&["a"], Some(vec![2])),
            &FlakySink::new(usize::MAX),
            &store,
        )
        .await
        .expect_err("the sink fails");

        let CommitFailure::Sink(text) = failure else {
            panic!("expected a sink failure, got {failure:?}");
        };
        assert!(text.contains("broker unavailable"), "{text}");
        assert_eq!(std::fs::read(&path).expect("bytes"), before);

        // With no cursor file before, none is created.
        let (_dir2, store2) = open_store();
        commit_once(
            "fake",
            polled(&["a"], Some(vec![2])),
            &FlakySink::new(usize::MAX),
            &store2,
        )
        .await
        .expect_err("the sink fails");
        assert!(!store2.path_for("fake").exists());
        drop(dir);
    }

    #[tokio::test]
    async fn commit_with_zero_items_skips_the_sink_and_still_writes_the_cursor() {
        let (dir, store) = open_store();
        let sink = FlakySink::new(0);

        let sent = commit_once("fake", polled(&[], Some(vec![9])), &sink, &store)
            .await
            .expect("commit");

        assert_eq!(sent, 0);
        assert_eq!(sink.calls(), 0, "an empty list makes no sink call");
        assert_eq!(
            read_cursor_file(dir.path(), "fake").expect("read"),
            Some(vec![9])
        );
    }

    #[tokio::test]
    async fn commit_cursor_write_failure_reports_how_many_records_were_sent() {
        let dir = TempDir::new().expect("tempdir");
        let state = dir.path().join("state");
        let store = CursorStore::open(&state).expect("cursor store");
        std::fs::remove_dir_all(&state).expect("remove the state directory");
        let sink = VecSink::default();

        let failure = commit_once("fake", polled(&["a", "b"], Some(vec![5])), &sink, &store)
            .await
            .expect_err("the cursor cannot be written");

        let CommitFailure::Cursor { sent, text } = failure else {
            panic!("expected a cursor failure, got {failure:?}");
        };
        assert_eq!(sent, 2);
        assert!(text.contains("fake"), "{text}");
        assert_eq!(sink.records().len(), 2, "the batch was delivered");
    }
}
