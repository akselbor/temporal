//! Type-safe client abstractions for starting and observing workflows.

use std::marker::PhantomData;

use anyhow::{Context, Result, bail};
use temporalio_client::{
    Client as TemporalNamespacedClient, ClientInitError, GetWorkflowResultOptions, RetryClient,
    WfClientExt, WorkflowClientTrait, WorkflowExecutionResult,
    WorkflowHandle as TemporalWorkflowHandle, WorkflowOptions,
};
use temporalio_common::protos::{
    coresdk::{AsJsonPayloadExt, FromJsonPayloadExt},
    temporal::api::common::v1::Payload,
};
use temporalio_common::telemetry::metrics::TemporalMeter;
use uuid::Uuid;

#[cfg(feature = "worker")]
use crate::activity::ActivityOptions;
use crate::traits::{Activity, Workflow};

/// Options for starting a workflow execution.
#[derive(Debug, Clone)]
pub struct StartWorkflowOptions {
    /// Target task queue to schedule the workflow execution on.
    pub task_queue: String,
    /// Workflow id to use for the execution.
    pub workflow_id: String,
    /// Optional request id for idempotent start semantics.
    pub request_id: Option<String>,
    /// Additional Temporal workflow start options.
    pub workflow_options: WorkflowOptions,
}

impl StartWorkflowOptions {
    /// Creates workflow start options with default [`WorkflowOptions`].
    ///
    /// A random workflow id is generated as UUID v4 hex.
    pub fn new(task_queue: impl Into<String>) -> Self {
        Self {
            task_queue: task_queue.into(),
            workflow_id: Uuid::new_v4().simple().to_string(),
            request_id: None,
            workflow_options: WorkflowOptions::default(),
        }
    }

    /// Sets the workflow id.
    pub fn with_workflow_id(mut self, workflow_id: impl Into<String>) -> Self {
        self.workflow_id = workflow_id.into();
        self
    }

    /// Sets the request id used for workflow start idempotency.
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    /// Replaces Temporal workflow options used at start time.
    pub fn with_workflow_options(mut self, workflow_options: WorkflowOptions) -> Self {
        self.workflow_options = workflow_options;
        self
    }
}

/// High-level type-safe wrapper for Temporal clients.
#[derive(Clone)]
pub struct Client<C> {
    inner: C,
}

/// Concrete Temporal client type returned by `ClientOptions::connect`.
pub type ConnectedClient = RetryClient<TemporalNamespacedClient>;

impl Client<ConnectedClient> {
    /// Connects a namespaced Temporal client and wraps it in this type-safe client.
    ///
    /// This is a convenience wrapper around `temporalio_client::ClientOptions::connect`.
    pub async fn connect(
        options: &temporalio_client::ClientOptions,
        namespace: impl Into<String>,
        metrics_meter: Option<TemporalMeter>,
    ) -> std::result::Result<Self, ClientInitError> {
        let inner = options.connect(namespace, metrics_meter).await?;
        Ok(Self::new(inner))
    }
}

impl<C> Client<C> {
    /// Wraps an existing client implementation.
    pub fn new(inner: C) -> Self {
        Self { inner }
    }

    /// Returns a shared reference to the underlying client.
    pub fn inner(&self) -> &C {
        &self.inner
    }

    /// Consumes this wrapper and returns the underlying client.
    pub fn into_inner(self) -> C {
        self.inner
    }

    /// Returns the Temporal workflow type identifier for `W`.
    pub fn workflow_type<W: Workflow>() -> &'static str {
        W::TYPE
    }

    /// Returns the Temporal activity type identifier for `A`.
    pub fn activity_type<A: Activity>() -> &'static str {
        A::TYPE
    }

    /// Returns the activity default options for `A`.
    #[cfg(feature = "worker")]
    pub fn default_activity_options<A: Activity>() -> ActivityOptions {
        A::default_options()
    }
}

impl<C> Client<C>
where
    C: WorkflowClientTrait + WfClientExt + Clone,
{
    /// Starts a typed workflow with default [`WorkflowOptions`].
    pub async fn start_workflow<W>(
        &self,
        task_queue: impl Into<String>,
        workflow_id: impl Into<String>,
        input: W::Input,
    ) -> Result<TypedWorkflowHandle<C, W>>
    where
        W: Workflow,
    {
        let options = StartWorkflowOptions::new(task_queue).with_workflow_id(workflow_id);
        self.start_workflow_with::<W>(options, input).await
    }

    /// Starts a typed workflow with custom start options.
    ///
    /// The workflow type name is inferred from `W::TYPE`, and `input` is serialized
    /// as JSON payload.
    pub async fn start_workflow_with<W>(
        &self,
        options: StartWorkflowOptions,
        input: W::Input,
    ) -> Result<TypedWorkflowHandle<C, W>>
    where
        W: Workflow,
    {
        let StartWorkflowOptions {
            task_queue,
            workflow_id,
            request_id,
            workflow_options,
        } = options;

        let input_payload = input
            .as_json_payload()
            .context("failed to serialize workflow input to payload")?;

        let response = self
            .inner
            .start_workflow(
                vec![input_payload],
                task_queue,
                workflow_id.clone(),
                W::TYPE.to_owned(),
                request_id,
                workflow_options,
            )
            .await
            .with_context(|| format!("failed to start workflow `{}`", W::TYPE))?;

        Ok(self.workflow_handle::<W>(workflow_id, response.run_id))
    }

    /// Returns a typed workflow handle for an existing workflow execution.
    ///
    /// `run_id` can be empty to target the latest run.
    pub fn workflow_handle<W>(
        &self,
        workflow_id: impl Into<String>,
        run_id: impl Into<String>,
    ) -> TypedWorkflowHandle<C, W>
    where
        W: Workflow,
    {
        let handle = self.inner.get_untyped_workflow_handle(workflow_id, run_id);
        TypedWorkflowHandle::new(handle)
    }
}

/// Typed workflow handle tied to a `Workflow` implementation.
pub struct TypedWorkflowHandle<C, W>
where
    W: Workflow,
{
    inner: TemporalWorkflowHandle<C, Vec<Payload>>,
    _workflow: PhantomData<W>,
}

impl<C, W> TypedWorkflowHandle<C, W>
where
    W: Workflow,
{
    /// Creates a typed handle from an untyped Temporal workflow handle.
    fn new(inner: TemporalWorkflowHandle<C, Vec<Payload>>) -> Self {
        Self {
            inner,
            _workflow: PhantomData,
        }
    }

    /// Returns a shared reference to the underlying untyped Temporal workflow handle.
    pub fn inner(&self) -> &TemporalWorkflowHandle<C, Vec<Payload>> {
        &self.inner
    }

    /// Consumes this typed handle and returns the untyped Temporal workflow handle.
    pub fn into_inner(self) -> TemporalWorkflowHandle<C, Vec<Payload>> {
        self.inner
    }
}

impl<C, W> TypedWorkflowHandle<C, W>
where
    C: temporalio_client::WorkflowService + Clone,
    W: Workflow,
{
    /// Waits for workflow completion and deserializes a typed `W::Output` on success.
    pub async fn get_result_with(
        &self,
        options: GetWorkflowResultOptions,
    ) -> Result<WorkflowExecutionResult<W::Output>> {
        let raw = self
            .inner
            .get_workflow_result(options)
            .await
            .context("failed to fetch workflow result")?;

        decode_workflow_result::<W>(raw)
    }

    /// Waits for workflow completion using default result retrieval options.
    pub async fn get_result(&self) -> Result<WorkflowExecutionResult<W::Output>> {
        self.get_result_with(GetWorkflowResultOptions::default())
            .await
    }

    /// Returns static execution metadata (namespace/workflow id/run id).
    pub fn info(&self) -> &temporalio_client::WorkflowExecutionInfo {
        self.inner.info()
    }
}

/// Converts an untyped workflow execution result into a typed one.
fn decode_workflow_result<W>(
    raw: WorkflowExecutionResult<Vec<Payload>>,
) -> Result<WorkflowExecutionResult<W::Output>>
where
    W: Workflow,
{
    match raw {
        WorkflowExecutionResult::Succeeded(payloads) => {
            let output = decode_workflow_output::<W>(payloads)?;
            Ok(WorkflowExecutionResult::Succeeded(output))
        }
        WorkflowExecutionResult::Failed(failure) => Ok(WorkflowExecutionResult::Failed(failure)),
        WorkflowExecutionResult::Cancelled(details) => {
            Ok(WorkflowExecutionResult::Cancelled(details))
        }
        WorkflowExecutionResult::Terminated(details) => {
            Ok(WorkflowExecutionResult::Terminated(details))
        }
        WorkflowExecutionResult::TimedOut => Ok(WorkflowExecutionResult::TimedOut),
        WorkflowExecutionResult::ContinuedAsNew => Ok(WorkflowExecutionResult::ContinuedAsNew),
    }
}

/// Deserializes a workflow success payload list into `W::Output`.
///
/// This expects exactly one payload encoded as JSON.
fn decode_workflow_output<W>(payloads: Vec<Payload>) -> Result<W::Output>
where
    W: Workflow,
{
    match payloads.as_slice() {
        [payload] => W::Output::from_json_payload(payload)
            .with_context(|| format!("failed to deserialize output for workflow `{}`", W::TYPE)),
        [] => bail!("workflow `{}` completed without an output payload", W::TYPE),
        _ => bail!(
            "workflow `{}` completed with {} output payloads; expected exactly one",
            W::TYPE,
            payloads.len()
        ),
    }
}
