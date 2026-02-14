# temporal

Type-safe Temporal helpers for defining workflows and activities, running workers, and starting workflows from a typed client.

## What this crate gives you

- `Workflow` and `Activity` traits with typed input/output.
- `WorkflowContext` helpers for typed activity calls from workflows.
- A high-level `Worker` abstraction for connecting and registering handlers.
- A typed `client::Client` wrapper for starting workflows and decoding results.

## Feature flags

- `worker`: enables worker runtime APIs (`worker`, `workflow`, `activity`) and worker-side trait methods/registration.
- `client`: enables typed client APIs (`client`).
- Traits are always available (`temporal::traits::{Workflow, Activity}`), even without features.

By default, no features are enabled.
Enable what you need explicitly.

Build examples:

- traits only: `cargo check --no-default-features`
- client only: `cargo check --no-default-features --features client`
- worker only: `cargo check --no-default-features --features worker`

## Minimal example

The snippets below show an end-to-end setup:

1. Worker process: defines and registers one activity and one workflow.
2. Client process: starts that workflow and reads the typed result.

### Worker (`src/bin/worker.rs`)

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use temporal::activity::ActivityContext;
use temporal::traits::{Activity, Workflow};
use temporal::workflow::WorkflowContext;
use temporal::{Worker, WorkerOptions};
use temporalio_sdk::{ActivityError, WorkflowResult};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GreetWorkflowInput {
    name: String,
}

struct GreetActivity;

#[async_trait]
impl Activity for GreetActivity {
    type Input = String;
    type Output = String;
    const TYPE: &str = "greet-activity";

    async fn execute(
        &self,
        _ctx: ActivityContext,
        input: Self::Input,
    ) -> Result<Self::Output, ActivityError> {
        Ok(format!("Hello, {input}!"))
    }
}

struct GreetWorkflow;

#[async_trait]
impl Workflow for GreetWorkflow {
    type Input = GreetWorkflowInput;
    type Output = String;
    const TYPE: &str = "greet-workflow";

    async fn execute(
        &self,
        ctx: WorkflowContext,
        input: Self::Input,
    ) -> WorkflowResult<Self::Output> {
        let result = ctx.execute_activity::<GreetActivity>(input.name).await?;
        Ok(result)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let options = WorkerOptions::new("http://localhost:7233")
        .with_namespace("default")
        .with_task_queue("default-task-queue");

    let mut worker = Worker::connect(options).await?;
    worker.register_activity(GreetActivity);
    worker.register_workflow(GreetWorkflow);

    worker.run().await
}
```

### Client (`src/bin/client.rs`)

```rust
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use temporal::client::Client;
use temporal::traits::Workflow;
use temporalio_client::WorkflowExecutionResult;
use temporalio_sdk::sdk_client_options;
use temporalio_sdk_core::Url;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GreetWorkflowInput {
    name: String,
}

struct GreetWorkflow;

#[async_trait::async_trait]
impl Workflow for GreetWorkflow {
    type Input = GreetWorkflowInput;
    type Output = String;
    const TYPE: &str = "greet-workflow";

    async fn execute(
        &self,
        _ctx: temporal::workflow::WorkflowContext,
        _input: Self::Input,
    ) -> temporalio_sdk::WorkflowResult<Self::Output> {
        unreachable!("Executed by worker process, not client process")
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let url = Url::from_str("http://localhost:7233")?;
    let options = sdk_client_options(url).build();

    let client = Client::connect(&options, "default", None).await?;

    let handle = client
        .start_workflow::<GreetWorkflow>(
            "default-task-queue",
            "greet-workflow-run-001",
            GreetWorkflowInput {
                name: "Temporal".to_string(),
            },
        )
        .await?;

    match handle.get_result().await? {
        WorkflowExecutionResult::Succeeded(output) => {
            println!("Workflow succeeded: {output}");
        }
        WorkflowExecutionResult::Failed(_) => {
            println!("Workflow failed");
        }
        WorkflowExecutionResult::Cancelled(_) => {
            println!("Workflow cancelled");
        }
        WorkflowExecutionResult::Terminated(_) => {
            println!("Workflow terminated");
        }
        WorkflowExecutionResult::TimedOut => {
            println!("Workflow timed out");
        }
        WorkflowExecutionResult::ContinuedAsNew => {
            println!("Workflow continued as new");
        }
    }

    Ok(())
}
```

## Running locally

1. Start a local Temporal server (for example with Temporal CLI):
   - `temporal server start-dev`
2. Start the worker:
   - `cargo run --features worker --bin worker`
3. In another terminal, run the client:
   - `cargo run --features client --bin client`

If you run the same workflow id repeatedly (`greet-workflow-run-001` in the example), Temporal may reject duplicates depending on workflow-id reuse/conflict policies.

## Worker environment variables

`WorkerOptions::from_env()` supports:

- Required: `TEMPORAL_ADDRESS`
- Optional: `TEMPORAL_NAMESPACE` (default: `default`)
- Optional: `TEMPORAL_TASK_QUEUE` (default: `default-task-queue`)
- Optional: `TEMPORAL_MAX_CONCURRENT_ACTIVITIES` (default: `16`)
- Optional: `TEMPORAL_BUILD_ID` (default: `<crate-name>-dev`)
