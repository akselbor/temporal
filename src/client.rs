//! Type-safe client abstractions for starting and observing workflows.

use std::marker::PhantomData;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use temporalio_client::{
    Client as TemporalNamespacedClient, ClientInitError, GetWorkflowResultOptions, RetryClient,
    SignalWithStartOptions as TemporalSignalWithStartOptions, WfClientExt, WorkflowClientTrait,
    WorkflowExecutionResult, WorkflowHandle as TemporalWorkflowHandle, WorkflowOptions,
};
pub use temporalio_common::protos::temporal::api::enums::v1::{
    WorkflowIdConflictPolicy, WorkflowIdReusePolicy,
};
use temporalio_common::protos::{
    coresdk::{AsJsonPayloadExt, FromJsonPayloadExt, IntoPayloadsExt},
    temporal::api::{
        common::v1::{Payload, Payloads},
        enums::v1::UpdateWorkflowExecutionLifecycleStage,
        failure::v1::Failure,
        update::v1::{WaitPolicy, outcome::Value as UpdateOutcomeValue},
    },
};
use temporalio_common::telemetry::metrics::TemporalMeter;
use uuid::Uuid;

#[cfg(feature = "worker")]
use crate::activity::ActivityOptions;
use crate::traits::{Activity, Workflow, WorkflowSignal, WorkflowUpdate};

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

    /// Sets the policy for reusing a workflow id from a previously closed workflow.
    pub fn with_workflow_id_reuse_policy(mut self, policy: WorkflowIdReusePolicy) -> Self {
        self.workflow_options.id_reuse_policy = policy;
        self
    }

    /// Sets the policy for resolving conflicts with a running workflow of the same id.
    pub fn with_workflow_id_conflict_policy(mut self, policy: WorkflowIdConflictPolicy) -> Self {
        self.workflow_options.id_conflict_policy = policy;
        self
    }

    /// Sets both workflow id reuse and running-workflow conflict policies.
    pub fn with_workflow_id_policies(
        mut self,
        reuse_policy: WorkflowIdReusePolicy,
        conflict_policy: WorkflowIdConflictPolicy,
    ) -> Self {
        self.workflow_options.id_reuse_policy = reuse_policy;
        self.workflow_options.id_conflict_policy = conflict_policy;
        self
    }

    /// Replaces Temporal workflow options used at start time.
    pub fn with_workflow_options(mut self, workflow_options: WorkflowOptions) -> Self {
        self.workflow_options = workflow_options;
        self
    }
}

/// Options for sending a workflow signal.
#[derive(Debug, Clone, Default)]
pub struct SignalWorkflowOptions {
    /// Optional request id for idempotent signal semantics.
    pub request_id: Option<String>,
}

impl SignalWorkflowOptions {
    /// Sets the request id used for signal idempotency.
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }
}

/// Options for atomically signaling or starting a workflow execution.
#[derive(Debug, Clone)]
pub struct SignalWithStartWorkflowOptions {
    /// Target task queue to schedule the workflow execution on when started.
    pub task_queue: String,
    /// Workflow id to signal or start.
    pub workflow_id: String,
    /// Optional request id for idempotent signal-with-start semantics.
    pub request_id: Option<String>,
    /// Additional Temporal workflow start options used when the workflow is started.
    pub workflow_options: WorkflowOptions,
}

impl SignalWithStartWorkflowOptions {
    /// Creates signal-with-start options with default [`WorkflowOptions`].
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

    /// Sets the request id used for signal-with-start idempotency.
    pub fn with_request_id(mut self, request_id: impl Into<String>) -> Self {
        self.request_id = Some(request_id.into());
        self
    }

    /// Sets the policy for reusing a workflow id from a previously closed workflow.
    pub fn with_workflow_id_reuse_policy(mut self, policy: WorkflowIdReusePolicy) -> Self {
        self.workflow_options.id_reuse_policy = policy;
        self
    }

    /// Sets the policy for resolving conflicts with a running workflow of the same id.
    pub fn with_workflow_id_conflict_policy(mut self, policy: WorkflowIdConflictPolicy) -> Self {
        self.workflow_options.id_conflict_policy = policy;
        self
    }

    /// Sets both workflow id reuse and running-workflow conflict policies.
    pub fn with_workflow_id_policies(
        mut self,
        reuse_policy: WorkflowIdReusePolicy,
        conflict_policy: WorkflowIdConflictPolicy,
    ) -> Self {
        self.workflow_options.id_reuse_policy = reuse_policy;
        self.workflow_options.id_conflict_policy = conflict_policy;
        self
    }

    /// Replaces Temporal workflow options used when starting the workflow.
    pub fn with_workflow_options(mut self, workflow_options: WorkflowOptions) -> Self {
        self.workflow_options = workflow_options;
        self
    }
}

/// Options for executing a workflow update.
#[derive(Debug, Clone)]
pub struct UpdateWorkflowOptions {
    /// Temporal wait policy controlling when the update response is returned.
    pub wait_policy: WaitPolicy,
}

impl Default for UpdateWorkflowOptions {
    fn default() -> Self {
        Self {
            wait_policy: WaitPolicy {
                lifecycle_stage: UpdateWorkflowExecutionLifecycleStage::Completed as i32,
            },
        }
    }
}

/// Typed result of a workflow update execution.
#[derive(Debug, Clone)]
pub enum WorkflowUpdateResult<T> {
    Succeeded(T),
    Failed(Failure),
}

impl<T> WorkflowUpdateResult<T> {
    /// Returns `true` when the update completed successfully.
    pub fn is_success(&self) -> bool {
        matches!(self, Self::Succeeded(_))
    }

    /// Converts the update result into a standard result.
    pub fn into_result(self) -> std::result::Result<T, Failure> {
        match self {
            Self::Succeeded(value) => Ok(value),
            Self::Failed(failure) => Err(failure),
        }
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

    /// Atomically sends a typed signal to a workflow or starts it if it is not running.
    ///
    /// This maps to Temporal's signal-with-start RPC. It does not perform a
    /// separate start call followed by a signal call.
    pub async fn signal_with_start_workflow<S>(
        &self,
        options: SignalWithStartWorkflowOptions,
        workflow_input: <S::Workflow as Workflow>::Input,
        signal_input: S::Input,
    ) -> Result<TypedWorkflowHandle<C, S::Workflow>>
    where
        S: WorkflowSignal,
    {
        let SignalWithStartWorkflowOptions {
            task_queue,
            workflow_id,
            request_id,
            workflow_options,
        } = options;

        let workflow_input = one_payloads(encode_json_payload(&workflow_input, || {
            format!(
                "failed to serialize input for signal-with-start workflow `{}`",
                S::Workflow::TYPE
            )
        })?);
        let signal_input = one_payloads(encode_json_payload(&signal_input, || {
            format!(
                "failed to serialize input for workflow signal `{}`",
                S::NAME
            )
        })?);

        let response = self
            .inner
            .signal_with_start_workflow_execution(
                TemporalSignalWithStartOptions {
                    input: workflow_input,
                    task_queue,
                    workflow_id: workflow_id.clone(),
                    workflow_type: S::Workflow::TYPE.to_owned(),
                    request_id,
                    signal_name: S::NAME.to_owned(),
                    signal_input,
                    signal_header: None,
                },
                workflow_options,
            )
            .await
            .with_context(|| {
                format!(
                    "failed to signal-with-start workflow `{}` with signal `{}`",
                    S::Workflow::TYPE,
                    S::NAME
                )
            })?;

        Ok(self.workflow_handle::<S::Workflow>(workflow_id, response.run_id))
    }

    /// Atomically sends a typed signal to a workflow id or starts it on a task queue.
    pub async fn signal_with_start_workflow_on_queue<S>(
        &self,
        task_queue: impl Into<String>,
        workflow_id: impl Into<String>,
        workflow_input: <S::Workflow as Workflow>::Input,
        signal_input: S::Input,
    ) -> Result<TypedWorkflowHandle<C, S::Workflow>>
    where
        S: WorkflowSignal,
    {
        let options = SignalWithStartWorkflowOptions::new(task_queue).with_workflow_id(workflow_id);
        self.signal_with_start_workflow::<S>(options, workflow_input, signal_input)
            .await
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

impl<C, W> TypedWorkflowHandle<C, W>
where
    C: WorkflowClientTrait + temporalio_client::WorkflowService + Clone,
    W: Workflow,
{
    /// Executes a typed workflow update and waits for it to complete.
    pub async fn execute_update<U>(
        &self,
        input: U::Input,
    ) -> Result<WorkflowUpdateResult<U::Output>>
    where
        U: WorkflowUpdate<Workflow = W>,
    {
        self.execute_update_with::<U>(UpdateWorkflowOptions::default(), input)
            .await
    }

    /// Executes a typed workflow update with explicit Temporal update options.
    pub async fn execute_update_with<U>(
        &self,
        options: UpdateWorkflowOptions,
        input: U::Input,
    ) -> Result<WorkflowUpdateResult<U::Output>>
    where
        U: WorkflowUpdate<Workflow = W>,
    {
        let input_payload = input
            .as_json_payload()
            .context("failed to serialize workflow update input to payload")?;

        let info = self.info();
        let response = self
            .inner
            .client()
            .update_workflow_execution(
                info.workflow_id.clone(),
                info.run_id.clone().unwrap_or_default(),
                U::NAME.to_owned(),
                options.wait_policy,
                vec![input_payload].into_payloads(),
            )
            .await
            .with_context(|| {
                format!(
                    "failed to execute workflow update `{}` for workflow `{}`",
                    U::NAME,
                    W::TYPE
                )
            })?;

        decode_workflow_update_result::<U>(response)
    }

    /// Sends a typed signal to this workflow execution using default signal options.
    pub async fn signal<S>(&self, input: S::Input) -> Result<()>
    where
        S: WorkflowSignal<Workflow = W>,
    {
        self.signal_with::<S>(SignalWorkflowOptions::default(), input)
            .await
    }

    /// Sends a typed signal to this workflow execution.
    pub async fn signal_with<S>(
        &self,
        options: SignalWorkflowOptions,
        input: S::Input,
    ) -> Result<()>
    where
        S: WorkflowSignal<Workflow = W>,
    {
        let input_payload = encode_json_payload(&input, || {
            format!(
                "failed to serialize input for workflow signal `{}`",
                S::NAME
            )
        })?;

        let info = self.info();
        self.inner
            .client()
            .signal_workflow_execution(
                info.workflow_id.clone(),
                info.run_id.clone().unwrap_or_default(),
                S::NAME.to_owned(),
                one_payloads(input_payload),
                options.request_id,
            )
            .await
            .with_context(|| {
                format!(
                    "failed to signal workflow `{}` with signal `{}`",
                    W::TYPE,
                    S::NAME
                )
            })?;

        Ok(())
    }
}

/// Serializes a value into a JSON Temporal payload.
fn encode_json_payload<T, C>(value: &T, context: C) -> Result<Payload>
where
    T: Serialize,
    C: FnOnce() -> String,
{
    value.as_json_payload().with_context(context)
}

/// Converts a single payload into Temporal's optional payload collection shape.
fn one_payloads(payload: Payload) -> Option<Payloads> {
    vec![payload].into_payloads()
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

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct TestWorkflowInput {
        value: String,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct TestSignalInput {
        value: String,
    }

    struct TestWorkflow;

    #[cfg_attr(feature = "worker", async_trait::async_trait)]
    impl Workflow for TestWorkflow {
        type Input = TestWorkflowInput;
        type Output = ();
        const TYPE: &str = "test-workflow";

        #[cfg(feature = "worker")]
        async fn execute(
            &self,
            _ctx: crate::workflow::WorkflowContext,
            _input: Self::Input,
        ) -> temporalio_sdk::WorkflowResult<Self::Output> {
            Ok(temporalio_sdk::WfExitValue::Normal(()))
        }
    }

    struct TestSignal;

    impl WorkflowSignal for TestSignal {
        type Workflow = TestWorkflow;
        type Input = TestSignalInput;
        const NAME: &str = "test-signal";
    }

    #[test]
    fn signal_workflow_options_sets_request_id() {
        let options = SignalWorkflowOptions::default().with_request_id("request-001");

        assert_eq!(options.request_id.as_deref(), Some("request-001"));
    }

    #[test]
    fn signal_with_start_workflow_options_set_fields() {
        let options = SignalWithStartWorkflowOptions::new("queue")
            .with_workflow_id("workflow-001")
            .with_request_id("request-001");

        assert_eq!(options.task_queue, "queue");
        assert_eq!(options.workflow_id, "workflow-001");
        assert_eq!(options.request_id.as_deref(), Some("request-001"));
    }

    #[test]
    fn start_workflow_options_sets_workflow_id_reuse_policy() {
        let options = StartWorkflowOptions::new("queue")
            .with_workflow_id_reuse_policy(WorkflowIdReusePolicy::AllowDuplicate);

        assert_eq!(
            options.workflow_options.id_reuse_policy,
            WorkflowIdReusePolicy::AllowDuplicate
        );
    }

    #[test]
    fn start_workflow_options_sets_workflow_id_conflict_policy() {
        let options = StartWorkflowOptions::new("queue")
            .with_workflow_id_conflict_policy(WorkflowIdConflictPolicy::UseExisting);

        assert_eq!(
            options.workflow_options.id_conflict_policy,
            WorkflowIdConflictPolicy::UseExisting
        );
    }

    #[test]
    fn start_workflow_options_sets_workflow_id_policies() {
        let options = StartWorkflowOptions::new("queue").with_workflow_id_policies(
            WorkflowIdReusePolicy::AllowDuplicate,
            WorkflowIdConflictPolicy::UseExisting,
        );

        assert_eq!(
            options.workflow_options.id_reuse_policy,
            WorkflowIdReusePolicy::AllowDuplicate
        );
        assert_eq!(
            options.workflow_options.id_conflict_policy,
            WorkflowIdConflictPolicy::UseExisting
        );
    }

    #[test]
    fn signal_with_start_workflow_options_sets_workflow_id_reuse_policy() {
        let options = SignalWithStartWorkflowOptions::new("queue")
            .with_workflow_id_reuse_policy(WorkflowIdReusePolicy::AllowDuplicate);

        assert_eq!(
            options.workflow_options.id_reuse_policy,
            WorkflowIdReusePolicy::AllowDuplicate
        );
    }

    #[test]
    fn signal_with_start_workflow_options_sets_workflow_id_conflict_policy() {
        let options = SignalWithStartWorkflowOptions::new("queue")
            .with_workflow_id_conflict_policy(WorkflowIdConflictPolicy::UseExisting);

        assert_eq!(
            options.workflow_options.id_conflict_policy,
            WorkflowIdConflictPolicy::UseExisting
        );
    }

    #[test]
    fn signal_with_start_workflow_options_sets_workflow_id_policies() {
        let options = SignalWithStartWorkflowOptions::new("queue").with_workflow_id_policies(
            WorkflowIdReusePolicy::AllowDuplicate,
            WorkflowIdConflictPolicy::UseExisting,
        );

        assert_eq!(
            options.workflow_options.id_reuse_policy,
            WorkflowIdReusePolicy::AllowDuplicate
        );
        assert_eq!(
            options.workflow_options.id_conflict_policy,
            WorkflowIdConflictPolicy::UseExisting
        );
    }

    #[test]
    fn one_payloads_wraps_single_json_payload() {
        let payload = encode_json_payload(
            &TestSignalInput {
                value: "hello".to_string(),
            },
            || "failed".to_string(),
        )
        .unwrap();

        let payloads = one_payloads(payload).unwrap();

        assert_eq!(payloads.payloads.len(), 1);
    }

    #[test]
    fn typed_signal_trait_binds_to_workflow() {
        assert_eq!(<TestSignal as WorkflowSignal>::NAME, "test-signal");
        assert_eq!(
            <TestSignal as WorkflowSignal>::Workflow::TYPE,
            "test-workflow"
        );
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

/// Converts an untyped workflow update execution response into a typed one.
fn decode_workflow_update_result<U>(
    response: temporalio_common::protos::temporal::api::workflowservice::v1::UpdateWorkflowExecutionResponse,
) -> Result<WorkflowUpdateResult<U::Output>>
where
    U: WorkflowUpdate,
{
    let outcome = response
        .outcome
        .context("workflow update response did not include an outcome")?;

    match outcome
        .value
        .context("workflow update outcome did not include a value")?
    {
        UpdateOutcomeValue::Success(payloads) => Ok(WorkflowUpdateResult::Succeeded(
            decode_workflow_update_output::<U>(payloads.payloads)?,
        )),
        UpdateOutcomeValue::Failure(failure) => Ok(WorkflowUpdateResult::Failed(failure)),
    }
}

/// Deserializes a workflow update success payload list into `U::Output`.
fn decode_workflow_update_output<U>(payloads: Vec<Payload>) -> Result<U::Output>
where
    U: WorkflowUpdate,
{
    match payloads.as_slice() {
        [payload] => U::Output::from_json_payload(payload).with_context(|| {
            format!(
                "failed to deserialize output for workflow update `{}`",
                U::NAME
            )
        }),
        [] => bail!(
            "workflow update `{}` completed without an output payload",
            U::NAME
        ),
        _ => bail!(
            "workflow update `{}` completed with {} output payloads; expected exactly one",
            U::NAME,
            payloads.len()
        ),
    }
}
