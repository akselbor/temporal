//! Workflow-side runtime helpers.

use std::{
    future::Future,
    marker::PhantomData,
    pin::Pin,
    task::{Context as TaskContext, Poll},
    time::Duration,
};

use anyhow::anyhow;
use futures::{Stream, future::try_join_all, task::noop_waker};
use temporalio_common::protos::coresdk::{FromJsonPayloadExt, PayloadDeserializeErr};
use temporalio_sdk::{
    ActivityError, CancellableFuture, IntoUpdateHandlerFunc, IntoUpdateValidatorFunc, SignalData,
    TimerOptions as TemporalTimerOptions, TimerResult as TemporalTimerResult, WfContext,
};

use crate::{
    activity::ActivityOptions,
    traits::{Activity, WorkflowSignal, WorkflowUpdate},
};

/// Context passed to workflow implementations.
///
/// Exposes type-safe helpers for invoking crate-defined activities.
#[derive(Clone)]
pub struct WorkflowContext {
    pub(crate) inner: WfContext,
}

impl WorkflowContext {
    /// Creates a durable workflow timer.
    ///
    /// The returned future resolves when the timer fires or is cancelled. Timers
    /// are persisted by Temporal and do not occupy a worker thread while waiting.
    pub fn timer(&self, options: impl Into<TimerOptions>) -> WorkflowTimer<'_> {
        WorkflowTimer {
            inner: Box::pin(self.inner.timer(options.into().to_temporal())),
        }
    }

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
                ActivityError::NonRetryable(anyhow!("activity completed without payload"))
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

    /// Registers a typed workflow update handler.
    pub fn register_update<U>(
        &self,
        validator: impl IntoUpdateValidatorFunc<U::Input>,
        handler: impl IntoUpdateHandlerFunc<U::Input, U::Output>,
    ) where
        U: WorkflowUpdate,
    {
        self.inner.update_handler(U::NAME, validator, handler);
    }

    /// Creates a typed stream for a workflow signal.
    pub fn signal_channel<S>(&self) -> WorkflowSignalStream<S>
    where
        S: WorkflowSignal,
    {
        WorkflowSignalStream::new(self.inner.make_signal_channel(S::NAME))
    }
}

/// Options used when creating a durable workflow timer.
#[derive(Default, Debug, Clone, PartialEq, Eq)]
pub struct TimerOptions {
    /// Duration to wait before the timer fires.
    pub duration: Duration,
    /// Optional single-line summary shown in Temporal UI/CLI metadata.
    pub summary: Option<String>,
}

impl TimerOptions {
    /// Converts this crate's timer options into Temporal SDK options.
    pub fn to_temporal(self) -> TemporalTimerOptions {
        TemporalTimerOptions {
            duration: self.duration,
            summary: self.summary,
        }
    }
}

impl From<Duration> for TimerOptions {
    fn from(duration: Duration) -> Self {
        Self {
            duration,
            ..Default::default()
        }
    }
}

/// Result of awaiting a workflow timer.
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum TimerResult {
    /// The timer elapsed and fired.
    Fired,
    /// The timer was cancelled before firing.
    Cancelled,
}

impl From<TemporalTimerResult> for TimerResult {
    fn from(result: TemporalTimerResult) -> Self {
        match result {
            TemporalTimerResult::Fired => Self::Fired,
            TemporalTimerResult::Cancelled => Self::Cancelled,
        }
    }
}

/// Future returned by [`WorkflowContext::timer`].
pub struct WorkflowTimer<'a> {
    inner: Pin<Box<dyn CancellableFuture<TemporalTimerResult> + Send + 'a>>,
}

impl<'a> WorkflowTimer<'a> {
    /// Requests cancellation for this timer.
    ///
    /// Awaiting the timer after cancellation resolves to [`TimerResult::Cancelled`].
    pub fn cancel(&self, ctx: &WorkflowContext) {
        self.inner.as_ref().get_ref().cancel(&ctx.inner);
    }
}

impl<'a> Future for WorkflowTimer<'a> {
    type Output = TimerResult;

    fn poll(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Self::Output> {
        self.inner.as_mut().poll(cx).map(TimerResult::from)
    }
}

/// Typed stream of workflow signal inputs.
pub struct WorkflowSignalStream<S>
where
    S: WorkflowSignal,
{
    inner: Pin<Box<dyn Stream<Item = SignalData> + Send>>,
    _signal: PhantomData<fn() -> S>,
}

impl<S> WorkflowSignalStream<S>
where
    S: WorkflowSignal,
{
    fn new(inner: impl Stream<Item = SignalData> + Send + 'static) -> Self {
        Self {
            inner: Box::pin(inner),
            _signal: PhantomData,
        }
    }

    /// Drains all currently ready signal inputs without waiting for new ones.
    pub fn drain_ready(&mut self) -> Vec<Result<S::Input, PayloadDeserializeErr>> {
        let waker = noop_waker();
        let mut cx = TaskContext::from_waker(&waker);
        let mut signals = Vec::new();

        loop {
            match self.inner.as_mut().poll_next(&mut cx) {
                Poll::Ready(Some(data)) => signals.push(decode_signal_input::<S>(data)),
                Poll::Ready(None) | Poll::Pending => break,
            }
        }

        signals
    }

    /// Drains all buffered signal inputs from the stream.
    pub fn drain_all(mut self) -> Vec<Result<S::Input, PayloadDeserializeErr>> {
        self.drain_ready()
    }
}

impl<S> Unpin for WorkflowSignalStream<S> where S: WorkflowSignal {}

impl<S> Stream for WorkflowSignalStream<S>
where
    S: WorkflowSignal,
{
    type Item = Result<S::Input, PayloadDeserializeErr>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        this.inner
            .as_mut()
            .poll_next(cx)
            .map(|item| item.map(decode_signal_input::<S>))
    }
}

fn decode_signal_input<S>(data: SignalData) -> Result<S::Input, PayloadDeserializeErr>
where
    S: WorkflowSignal,
{
    match data.input.as_slice() {
        [payload] => S::Input::from_json_payload(payload),
        [] => Err(PayloadDeserializeErr::DeserializeErr(anyhow!(
            "signal `{}` did not include an input payload",
            S::NAME
        ))),
        _ => Err(PayloadDeserializeErr::DeserializeErr(anyhow!(
            "signal `{}` included {} input payloads; expected exactly one",
            S::NAME,
            data.input.len()
        ))),
    }
}

#[cfg(test)]
mod tests {
    use serde::{Deserialize, Serialize};
    use temporalio_common::protos::{
        coresdk::AsJsonPayloadExt, temporal::api::common::v1::Payload,
    };

    use super::*;
    use crate::traits::Workflow;

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct TestSignalInput {
        value: String,
    }

    struct TestWorkflow;

    #[async_trait::async_trait]
    impl Workflow for TestWorkflow {
        type Input = ();
        type Output = ();
        const TYPE: &str = "test-workflow";

        async fn execute(
            &self,
            _ctx: WorkflowContext,
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
    fn timer_options_from_duration_sets_duration() {
        let duration = Duration::from_secs(30);

        let options = TimerOptions::from(duration);

        assert_eq!(options.duration, duration);
        assert_eq!(options.summary, None);
    }

    #[test]
    fn timer_options_to_temporal_preserves_fields() {
        let duration = Duration::from_secs(30);
        let options = TimerOptions {
            duration,
            summary: Some("release hold".to_string()),
        };

        let temporal = options.to_temporal();

        assert_eq!(temporal.duration, duration);
        assert_eq!(temporal.summary.as_deref(), Some("release hold"));
    }

    #[test]
    fn timer_result_maps_from_temporal_result() {
        assert_eq!(
            TimerResult::from(temporalio_sdk::TimerResult::Fired),
            TimerResult::Fired
        );
        assert_eq!(
            TimerResult::from(temporalio_sdk::TimerResult::Cancelled),
            TimerResult::Cancelled
        );
    }

    #[test]
    fn decodes_single_json_payload() {
        let expected = TestSignalInput {
            value: "hello".to_string(),
        };
        let signal = SignalData::new([expected.as_json_payload().unwrap()]);

        let decoded = decode_signal_input::<TestSignal>(signal).unwrap();

        assert_eq!(decoded, expected);
    }

    #[test]
    fn errors_on_empty_signal_payload() {
        let signal = SignalData::new(std::iter::empty::<Payload>());

        assert!(decode_signal_input::<TestSignal>(signal).is_err());
    }

    #[test]
    fn errors_on_multiple_signal_payloads() {
        let input = TestSignalInput {
            value: "hello".to_string(),
        };
        let signal = SignalData::new([
            input.as_json_payload().unwrap(),
            input.as_json_payload().unwrap(),
        ]);

        assert!(decode_signal_input::<TestSignal>(signal).is_err());
    }

    #[test]
    fn errors_on_non_json_signal_payload() {
        let signal = SignalData::new([Payload {
            metadata: Default::default(),
            data: b"not-json".to_vec(),
            external_payloads: Default::default(),
        }]);

        assert!(decode_signal_input::<TestSignal>(signal).is_err());
    }
}
