//! Core traits for defining workflows and activities.
//!
//! `Workflow` and `Activity` are always available.
//! Worker-only execution and registration APIs are enabled by the `worker`
//! feature.

use serde::{Serialize, de::DeserializeOwned};

#[cfg(feature = "worker")]
use std::sync::Arc;

#[cfg(feature = "worker")]
use async_trait::async_trait;
#[cfg(feature = "worker")]
use temporalio_common::protos::coresdk::FromJsonPayloadExt;
#[cfg(feature = "worker")]
use temporalio_sdk::{ActContext, ActivityError, WfContext, Worker, WorkflowResult};

#[cfg(feature = "worker")]
use crate::{
    activity::{ActivityContext, ActivityOptions},
    workflow::WorkflowContext,
};

/// Defines a type-safe Temporal workflow.
///
/// Implement this trait for your workflow type.
/// With the `worker` feature enabled, you can also implement execution logic and
/// register the workflow on a worker.
#[cfg_attr(feature = "worker", async_trait)]
pub trait Workflow {
    /// The input type for this workflow.
    type Input: Serialize + DeserializeOwned + Send + 'static;
    /// The output type for this workflow.
    type Output: Serialize + DeserializeOwned + Send + 'static;
    /// The temporal workflow type identifier.
    const TYPE: &str;

    /// Internal adapter that deserializes workflow input and calls [`Self::execute`].
    #[cfg(feature = "worker")]
    async fn execute_internal(&self, ctx: WfContext) -> WorkflowResult<Self::Output> {
        let first_arg = ctx
            .get_args()
            .first()
            .ok_or_else(|| std::io::Error::other("no arguments"))?;
        let input = Self::Input::from_json_payload(first_arg)
            .map_err(|e| std::io::Error::other(format!("invalid workflow input payload: {e}")))?;

        let ctx = WorkflowContext { inner: ctx };

        self.execute(ctx, input).await
    }

    /// Executes workflow logic for one workflow task execution.
    ///
    /// `input` is deserialized from the first workflow argument.
    #[cfg(feature = "worker")]
    async fn execute(
        &self,
        ctx: WorkflowContext,
        input: Self::Input,
    ) -> WorkflowResult<Self::Output>;
}

/// Defines a type-safe Temporal activity.
///
/// Implement this trait for your activity type.
/// With the `worker` feature enabled, you can also implement execution logic and
/// register the activity on a worker.
#[cfg_attr(feature = "worker", async_trait)]
pub trait Activity {
    /// The input type for this activity.
    type Input: Serialize + DeserializeOwned + Send + 'static;
    /// The output type for this activity.
    type Output: Serialize + DeserializeOwned + Send + 'static;
    /// The temporal activity type identifier.
    const TYPE: &str;

    /// The default options to use for this activity.
    #[cfg(feature = "worker")]
    fn default_options() -> ActivityOptions {
        ActivityOptions::default()
    }

    /// Executes the activity for a single activity task.
    #[cfg(feature = "worker")]
    async fn execute(
        &self,
        ctx: ActivityContext,
        input: Self::Input,
    ) -> Result<Self::Output, ActivityError>;
}

/// Registers a workflow implementation on a Temporal worker.
#[cfg(feature = "worker")]
pub trait WorkflowRegistration {
    /// Registers this workflow on the provided worker.
    fn register_to(self, worker: &mut Worker);
}

#[cfg(feature = "worker")]
impl<T> WorkflowRegistration for T
where
    T: Workflow + Sized + Send + Sync + 'static,
{
    fn register_to(self, worker: &mut Worker) {
        let workflow = Arc::new(self);

        // We use a move closure. To help the compiler with the 'Send' bound,
        // we ensure the logic inside the closure returns a Send future.
        worker.register_wf(Self::TYPE, move |ctx: WfContext| {
            let wf = workflow.clone();
            async move { wf.execute_internal(ctx).await }
        });
    }
}

/// Registers an activity implementation on a Temporal worker.
#[cfg(feature = "worker")]
pub trait ActivityRegistration {
    /// Registers this activity on the provided worker.
    fn register_to(self, worker: &mut Worker);
}

#[cfg(feature = "worker")]
impl<T> ActivityRegistration for T
where
    T: Activity + Sized + Send + Sync + 'static,
{
    fn register_to(self, worker: &mut Worker) {
        let activity = Arc::new(self);

        // We use a move closure. To help the compiler with the 'Send' bound,
        // we ensure the logic inside the closure returns a Send future.
        worker.register_activity(T::TYPE, move |ctx: ActContext, input: T::Input| {
            let act = activity.clone();
            let ctx = ActivityContext { inner: ctx };
            async move { act.execute(ctx, input).await }
        });
    }
}
