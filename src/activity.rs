//! Activity-side types and configuration helpers.

use std::time::Duration;

use temporalio_client::Priority;
use temporalio_common::protos::{
    coresdk::{AsJsonPayloadExt, workflow_commands::ActivityCancellationType},
    temporal::api::common::v1::RetryPolicy,
};
use temporalio_sdk;
use temporalio_sdk::ActContext;

use crate::traits::Activity;

/// Context passed to activity implementations.
///
/// This wraps Temporal's internal activity context type and can be expanded with
/// additional convenience helpers over time.
pub struct ActivityContext {
    #[allow(unused)]
    pub(crate) inner: ActContext,
}

/// Options used when scheduling an activity from a workflow.
///
/// These map closely to Temporal activity scheduling options and are converted to
/// [`temporalio_sdk::ActivityOptions`] by [`Self::to_temporal`].
#[derive(Default, Debug, Clone)]
pub struct ActivityOptions {
    /// Identifier to use for tracking the activity in Workflow history.
    /// The `activityId` can be accessed by the activity function.
    /// Does not need to be unique.
    ///
    /// If `None` use the context's sequence number
    pub activity_id: Option<String>,
    /// Task queue to schedule the activity in
    ///
    /// If `None`, use the same task queue as the parent workflow.
    pub task_queue: Option<String>,
    /// Time that the Activity Task can stay in the Task Queue before it is picked up by a Worker.
    /// Do not specify this timeout unless using host specific Task Queues for Activity Tasks are
    /// being used for routing.
    /// `schedule_to_start_timeout` is always non-retryable.
    /// Retrying after this timeout doesn't make sense as it would just put the Activity Task back
    /// into the same Task Queue.
    pub schedule_to_start_timeout: Option<Duration>,
    /// Maximum time of a single Activity execution attempt.
    /// Note that the Temporal Server doesn't detect Worker process failures directly.
    /// It relies on this timeout to detect that an Activity that didn't complete on time.
    /// So this timeout should be as short as the longest possible execution of the Activity body.
    /// Potentially long running Activities must specify `heartbeat_timeout` and heartbeat from the
    /// activity periodically for timely failure detection.
    /// Either this option or `schedule_to_close_timeout` is required.
    pub start_to_close_timeout: Option<Duration>,
    /// Total time that a workflow is willing to wait for Activity to complete.
    /// `schedule_to_close_timeout` limits the total time of an Activity's execution including
    /// retries (use `start_to_close_timeout` to limit the time of a single attempt).
    /// Either this option or `start_to_close_timeout` is required.
    pub schedule_to_close_timeout: Option<Duration>,
    /// Heartbeat interval. Activity must heartbeat before this interval passes after a last
    /// heartbeat or activity start.
    pub heartbeat_timeout: Option<Duration>,
    /// Determines what the SDK does when the Activity is cancelled.
    pub cancellation_type: ActivityCancellationType,
    /// Activity retry policy
    pub retry_policy: Option<RetryPolicy>,
    /// Summary of the activity
    pub summary: Option<String>,
    /// Priority for the activity
    pub priority: Option<Priority>,
    /// If true, disable eager execution for this activity
    pub do_not_eagerly_execute: bool,
}

impl ActivityOptions {
    /// Converts this crate's activity options into Temporal SDK options.
    ///
    /// The activity type is taken from `T::TYPE`, and `input` is serialized as a
    /// JSON payload.
    pub fn to_temporal<T: Activity>(self, input: T::Input) -> temporalio_sdk::ActivityOptions {
        temporalio_sdk::ActivityOptions {
            activity_id: self.activity_id,
            activity_type: T::TYPE.to_string(),
            input: input.as_json_payload().expect("failed to serialize object"),
            task_queue: self.task_queue,
            schedule_to_start_timeout: self.schedule_to_start_timeout,
            start_to_close_timeout: self.start_to_close_timeout,
            schedule_to_close_timeout: self.schedule_to_close_timeout,
            heartbeat_timeout: self.heartbeat_timeout,
            cancellation_type: self.cancellation_type,
            retry_policy: self.retry_policy,
            summary: self.summary,
            priority: self.priority,
            do_not_eagerly_execute: self.do_not_eagerly_execute,
        }
    }
}
