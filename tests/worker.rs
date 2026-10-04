#![cfg(feature = "worker")]
extern crate temporal as temporalio_sdk;
extern crate temporal as temporalio_workflow;

use std::time::Duration;
use temporal::prelude::*;

#[workflow]
#[derive(Default)]
struct Counter {
    ready: bool,
    busy: bool,
    count: u32,
    done: bool,
}

#[workflow_methods]
impl Counter {
    #[run(name = "wrapper-counter")]
    async fn run(ctx: &mut WorkflowContext<Self>) -> WorkflowResult<u32> {
        ctx.wait_condition(|s| s.done).await?;
        let handlers = ctx.clone();
        ctx.wait_condition(move |_| handlers.all_handlers_finished())
            .await?;
        Ok(ctx.state(|s| s.count))
    }

    #[signal]
    fn ready(&mut self, _: &mut SyncWorkflowContext<Self>, _: ()) {
        self.ready = true;
    }

    #[update]
    async fn increment(
        ctx: &mut WorkflowContext<Self>,
        _: (),
    ) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
        ctx.wait_condition(|s| s.ready && !s.busy).await?;
        ctx.state_mut(|s| s.busy = true);
        ctx.timer(Duration::from_millis(20)).await;
        Ok(ctx.state_mut(|s| {
            s.count += 1;
            s.busy = false;
            s.count
        }))
    }

    #[update]
    fn finish(&mut self, _: &mut SyncWorkflowContext<Self>, _: ()) {
        self.done = true;
    }
}

#[test]
fn native_definitions_preserve_typed_contracts() {
    assert_eq!(Counter::run.name(), "wrapper-counter");
    assert_eq!(Counter::ready.name(), "ready");
    assert_eq!(Counter::increment.name(), "increment");
}

#[tokio::test]
#[ignore = "requires a Temporal server; set TEMPORAL_TEST_ADDRESS"]
async fn initial_signal_overlapping_updates_and_replay() -> Result<(), Box<dyn std::error::Error>> {
    let address =
        std::env::var("TEMPORAL_TEST_ADDRESS").unwrap_or_else(|_| "http://localhost:7233".into());
    let client =
        temporal::client::connect(address.parse()?, "default", "wrapper-test", "1").await?;
    let id = format!(
        "wrapper-test-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    );
    let handle = client
        .signal_with_start_workflow(
            Counter::run,
            (),
            Counter::ready,
            (),
            WorkflowStartOptions::new(id.clone(), id.clone()).build(),
        )
        .await?;
    // Submit before polling begins: handlers must exist in the first activation.
    let test = async {
        let (one, two) = tokio::join!(
            handle.execute_update(
                Counter::increment,
                (),
                WorkflowExecuteUpdateOptions::default()
            ),
            handle.execute_update(
                Counter::increment,
                (),
                WorkflowExecuteUpdateOptions::default()
            ),
        );
        let mut counts = [one?, two?];
        counts.sort();
        assert_eq!(counts, [1, 2]);
        handle
            .execute_update(Counter::finish, (), WorkflowExecuteUpdateOptions::default())
            .await?;
        assert_eq!(
            handle
                .get_result(WorkflowGetResultOptions::default())
                .await?,
            2
        );
        let history = handle.fetch_history(Default::default());
        temporal::workflow_replayer::WorkflowReplayer::new(
            temporal::workflow_replayer::WorkflowReplayerOptions::new()
                .register_workflow::<Counter>()?
                .build(),
        )?
        .replay_workflow(history)
        .await?;
        Ok::<_, Box<dyn std::error::Error>>(())
    };
    let runtime = Runtime::from_current_tokio(Default::default())?;
    let mut worker = Worker::new(
        &runtime,
        client,
        WorkerOptions::new(id)
            .register_workflow::<Counter>()?
            .build(),
    )?;
    let shutdown = worker.shutdown_handle();
    let test = async {
        let result = tokio::time::timeout(Duration::from_secs(30), test).await;
        shutdown();
        result?
    };
    let (work, test) = tokio::join!(worker.run(), test);
    work?;
    test?;
    Ok(())
}
