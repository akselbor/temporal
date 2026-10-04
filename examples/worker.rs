extern crate temporal as temporalio_common;
// Native SDK macros address these crate names; both aliases use the same wrapper.
extern crate temporal as temporalio_sdk;
extern crate temporal as temporalio_workflow;

use std::time::Duration;
use temporal::prelude::*;

#[derive(Default)]
struct GreetingActivities;
#[activities]
impl GreetingActivities {
    #[activity(name = "greet")]
    async fn greet(_ctx: ActivityContext, name: String) -> Result<String, ActivityError> {
        Ok(format!("Hello, {name}"))
    }
}

#[workflow]
#[derive(Default)]
struct GreetingWorkflow {
    name: String,
    ready: bool,
}
#[workflow_methods]
impl GreetingWorkflow {
    #[init]
    fn new(_ctx: &WorkflowContextView, name: String) -> Self {
        Self { name, ready: false }
    }
    #[run(name = "greeting")]
    async fn run(ctx: &mut WorkflowContext<Self>) -> WorkflowResult<String> {
        ctx.wait_condition(|s| s.ready).await?;
        let name = ctx.state(|s| s.name.clone());
        let result = ctx
            .execute_activity(
                GreetingActivities::greet,
                name,
                ActivityOptions::start_to_close_timeout(Duration::from_secs(10)),
            )
            .await?;
        let handlers = ctx.clone();
        ctx.wait_condition(move |_| handlers.all_handlers_finished())
            .await?;
        Ok(result)
    }
    #[signal(name = "ready")]
    fn ready(&mut self, _ctx: &mut SyncWorkflowContext<Self>, _: ()) {
        self.ready = true;
    }
    #[update(name = "await_ready")]
    async fn await_ready(
        ctx: &mut WorkflowContext<Self>,
        _: (),
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        ctx.wait_condition(|s| s.ready).await?;
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client =
        temporal::client::connect("http://localhost:7233".parse()?, "default", "example", "1")
            .await?;
    let runtime = Runtime::from_current_tokio(Default::default())?;
    let options = WorkerOptions::new("example")
        .register_workflow::<GreetingWorkflow>()?
        .register_activities(GreetingActivities)
        .build();
    Worker::new(&runtime, client, options)?.run().await?;
    Ok(())
}
