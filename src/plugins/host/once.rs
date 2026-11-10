// ─────────────────────────────────────────────────────────────────────────────
// plugins/host/once.rs - One poll cycle without a loop, for `fili plugins run-once`
//
// `poll_once` is the poll half of one scheduler cycle: instantiate, `init`,
// `poll`, map. It takes no sink and no cursor store, so it cannot send or
// persist anything. The size rule for the next cursor and the single place a
// secret is exposed are the scheduler's own helpers, not copies.
//
// Nothing here is logged or returned that carries settings, secret values or
// init arguments: failures are `CycleOutcome`s whose `describe` text is cleaned.
// ─────────────────────────────────────────────────────────────────────────────

use super::instance::{CallOutcome, InstanceFactory, PluginInstance};
use super::map::map_items;
use super::scheduler::{CycleOutcome, Phase, PluginSpec, expose_secrets, oversize_cursor_outcome};
use super::sink::SinkRecord;

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
/// `poll` fails, or the next cursor is over the byte cap.
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

    let response = match instance.poll(cursor).await {
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::plugins::host::cursor::MAX_CURSOR_BYTES;
    use crate::plugins::host::engine::PollError;
    use crate::plugins::host::scheduler::describe;
    use crate::plugins::host::testing::{FakeEvent, FakeFactory, FakeStep, spec, wit_item};

    fn once_spec() -> PluginSpec {
        spec("fake", 60, 60, 3600, 2.0)
    }

    fn poll_ok(items: Vec<crate::plugins::host::engine::WitItem>, next: Option<Vec<u8>>) -> FakeStep {
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
            async fn instantiate(&self, _plugin: &str) -> Result<Self::Instance, InstanceBuildError> {
                Err(InstanceBuildError("no memory".to_string()))
            }
        }

        let outcome = poll_once(&once_spec(), &Failing, None)
            .await
            .expect_err("build fails");
        assert!(matches!(outcome, CycleOutcome::BuildFailed(_)), "{outcome:?}");
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
}
