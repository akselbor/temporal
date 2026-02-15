//! Workflow-side runtime helpers.

use futures::future::try_join_all;
use temporalio_common::protos::coresdk::FromJsonPayloadExt;
use temporalio_sdk::{ActivityError, WfContext};

use crate::{activity::ActivityOptions, traits::Activity};

/// Context passed to workflow implementations.
///
/// Exposes type-safe helpers for invoking crate-defined activities.
pub struct WorkflowContext {
    pub(crate) inner: WfContext,
}

impl WorkflowContext {
    /// Executes one activity using `T::default_options()`.
    ///
    /// Returns the typed activity output or a Temporal activity error.
    pub async fn execute_activity<T: Activity>(
        &self,
        input: T::Input,
    ) -> Result<T::Output, ActivityError> {
        self.execute_activity_with::<T>(T::default_options(), input)
            .await
    }

    /// Executes many activities concurrently using `T::default_options()`.
    ///
    /// The returned vector preserves input order.
    pub async fn execute_activities<T: Activity>(
        &self,
        inputs: Vec<T::Input>,
    ) -> Result<Vec<T::Output>, ActivityError> {
        self.execute_activities_with::<T>(T::default_options(), inputs)
            .await
    }

    /// Executes one activity with explicit scheduling options.
    ///
    /// This serializes `input` as JSON, schedules the activity, and deserializes
    /// the completion payload into `T::Output`.
    pub async fn execute_activity_with<T: Activity>(
        &self,
        options: ActivityOptions,
        input: T::Input,
    ) -> Result<T::Output, ActivityError> {
        let payload = self
            .inner
            .activity(options.to_temporal::<T>(input))
            .await
            .success_payload_or_error()
            .map_err(ActivityError::from)?
            .ok_or_else(|| {
                ActivityError::NonRetryable("activity completed without payload".into())
            })?;
        Ok(T::Output::from_json_payload(&payload)?)
    }

    /// Executes many activities concurrently with explicit scheduling options.
    ///
    /// The same `options` value is cloned and used for each input.
    pub async fn execute_activities_with<T: Activity>(
        &self,
        options: ActivityOptions,
        inputs: Vec<T::Input>,
    ) -> Result<Vec<T::Output>, ActivityError> {
        let futures = inputs
            .into_iter()
            .map(|i| self.execute_activity_with::<T>(options.clone(), i));
        let results = try_join_all(futures).await?;
        Ok(results)
    }
}
