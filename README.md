# temporal

Type-safe Temporal helpers for defining workflows and activities, running workers, and starting workflows from a typed client.

## What this crate gives you

- `Workflow` and `Activity` traits with typed input/output.
- `WorkflowSignal` and `WorkflowUpdate` traits for typed workflow messages.
- `WorkflowContext` helpers for typed activity calls from workflows.
- Durable workflow timers with cancellable timer futures.
- A high-level `Worker` abstraction for connecting and registering handlers.
- A typed `client::Client` wrapper for starting, signaling, updating, and decoding workflows.

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

## Typed signals

Define each workflow signal as a `WorkflowSignal` type. The signal type binds the
signal name and input payload to the workflow it targets.

```rust
use serde::{Deserialize, Serialize};
use temporal::traits::{Workflow, WorkflowSignal};

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GreetWorkflowInput {
    name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct RenameInput {
    name: String,
}

struct GreetWorkflow;

impl Workflow for GreetWorkflow {
    type Input = GreetWorkflowInput;
    type Output = String;
    const TYPE: &str = "greet-workflow";
}

struct RenameSignal;

impl WorkflowSignal for RenameSignal {
    type Workflow = GreetWorkflow;
    type Input = RenameInput;
    const NAME: &str = "rename";
}
```

With the `worker` feature enabled, workflows can subscribe to typed signal
streams from `WorkflowContext`.

```rust
use futures::StreamExt;
use temporal::prelude::*;

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
        let mut name = input.name;
        let mut renames = ctx.signal_channel::<RenameSignal>();

        if let Some(rename) = renames.next().await {
            name = rename?.name;
        }

        Ok(WfExitValue::Normal(format!("Hello, {name}!")))
    }
}
```

`WorkflowSignalStream<S>` yields `Result<S::Input, PayloadDeserializeErr>`.
Each signal is expected to contain exactly one JSON payload. Empty, multiple, or
non-JSON payloads are returned as decode errors.

With the `client` feature enabled, typed handles can send typed signals.

```rust
handle
    .signal::<RenameSignal>(RenameInput {
        name: "Temporal".to_string(),
    })
    .await?;
```

The client also supports Temporal signal-with-start through one RPC. If a
workflow with the given id is already running, it is signaled. Otherwise, it is
started and receives the signal atomically.

```rust
let handle = client
    .signal_with_start_workflow_on_queue::<RenameSignal>(
        "default-task-queue",
        "greet-workflow-run-001",
        GreetWorkflowInput {
            name: "Initial".to_string(),
        },
        RenameInput {
            name: "Temporal".to_string(),
        },
    )
    .await?;
```

## Timers

With the `worker` feature enabled, workflows can create durable Temporal timers
from `WorkflowContext`. A timer resolves when it fires or when it is cancelled.

```rust
use std::time::Duration;
use temporal::prelude::*;

struct ReminderWorkflow;

#[async_trait]
impl Workflow for ReminderWorkflow {
    type Input = ();
    type Output = ();
    const TYPE: &str = "reminder-workflow";

    async fn execute(
        &self,
        ctx: WorkflowContext,
        _input: Self::Input,
    ) -> WorkflowResult<Self::Output> {
        let result = ctx
            .timer(TimerOptions {
                duration: Duration::from_secs(60),
                summary: Some("send reminder".to_string()),
            })
            .await;

        if matches!(result, TimerResult::Fired) {
            // Continue workflow logic.
        }

        Ok(WfExitValue::Normal(()))
    }
}
```

Timer futures can be cancelled without exposing the underlying Temporal SDK
context.

```rust
let timer = ctx.timer(Duration::from_secs(300));
timer.cancel(&ctx);
assert_eq!(timer.await, TimerResult::Cancelled);
```

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
use temporalio_sdk::{ActivityError, WfExitValue, WorkflowResult};

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
        Ok(WfExitValue::Normal(result))
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
