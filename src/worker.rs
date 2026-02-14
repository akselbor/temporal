//! High-level worker connection and lifecycle API.

use std::{str::FromStr, sync::Arc};

use anyhow::{Context, Result};
use temporalio_common::{
    telemetry::TelemetryOptions,
    worker::{WorkerConfig, WorkerTaskTypes, WorkerVersioningStrategy},
};
use temporalio_sdk::{Worker as TemporalWorker, sdk_client_options};
use temporalio_sdk_core::{CoreRuntime, RuntimeOptions, Url, init_worker};

use crate::traits::{ActivityRegistration, WorkflowRegistration};

const DEFAULT_NAMESPACE: &str = "default";
const DEFAULT_TASK_QUEUE: &str = "default-task-queue";
const DEFAULT_MAX_CONCURRENT_ACTIVITIES: usize = 16;

/// Configuration for connecting and starting a Temporal worker.
#[derive(Debug, Clone)]
pub struct WorkerOptions {
    /// Temporal server URL, for example `http://localhost:7233`.
    pub temporal_address: String,
    /// Namespace to poll for workflow and activity tasks.
    pub namespace: String,
    /// Task queue this worker should poll.
    pub task_queue: String,
    /// Maximum number of concurrently outstanding activity tasks.
    pub max_concurrent_activities: usize,
    /// Build identifier used for worker versioning metadata.
    pub build_id: String,
}

impl WorkerOptions {
    /// Creates worker options with sensible defaults.
    ///
    /// Defaults:
    /// - namespace: `default`
    /// - task queue: `default-task-queue`
    /// - max concurrent activities: `16`
    /// - build id: `<crate-name>-dev`
    pub fn new(temporal_address: impl Into<String>) -> Self {
        Self {
            temporal_address: temporal_address.into(),
            namespace: DEFAULT_NAMESPACE.to_owned(),
            task_queue: DEFAULT_TASK_QUEUE.to_owned(),
            max_concurrent_activities: DEFAULT_MAX_CONCURRENT_ACTIVITIES,
            build_id: default_build_id(),
        }
    }

    /// Reads options from environment variables.
    ///
    /// Required:
    /// - `TEMPORAL_ADDRESS`
    ///
    /// Optional:
    /// - `TEMPORAL_NAMESPACE` (default: `default`)
    /// - `TEMPORAL_TASK_QUEUE` (default: `default-task-queue`)
    /// - `TEMPORAL_MAX_CONCURRENT_ACTIVITIES` (default: `16`)
    /// - `TEMPORAL_BUILD_ID` (default: `<crate-name>-dev`)
    pub fn from_env() -> Result<Self> {
        let temporal_address =
            std::env::var("TEMPORAL_ADDRESS").context("missing TEMPORAL_ADDRESS")?;
        let namespace =
            std::env::var("TEMPORAL_NAMESPACE").unwrap_or_else(|_| DEFAULT_NAMESPACE.to_owned());
        let task_queue =
            std::env::var("TEMPORAL_TASK_QUEUE").unwrap_or_else(|_| DEFAULT_TASK_QUEUE.to_owned());
        let max_concurrent_activities = std::env::var("TEMPORAL_MAX_CONCURRENT_ACTIVITIES")
            .ok()
            .map(|value| {
                value.parse::<usize>().with_context(|| {
                    format!("failed to parse TEMPORAL_MAX_CONCURRENT_ACTIVITIES as usize: {value}")
                })
            })
            .transpose()?
            .unwrap_or(DEFAULT_MAX_CONCURRENT_ACTIVITIES);
        let build_id = std::env::var("TEMPORAL_BUILD_ID").unwrap_or_else(|_| default_build_id());

        Ok(Self {
            temporal_address,
            namespace,
            task_queue,
            max_concurrent_activities,
            build_id,
        })
    }

    /// Sets the namespace.
    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Self {
        self.namespace = namespace.into();
        self
    }

    /// Sets the task queue.
    pub fn with_task_queue(mut self, task_queue: impl Into<String>) -> Self {
        self.task_queue = task_queue.into();
        self
    }

    /// Sets the maximum number of concurrent activities.
    pub fn with_max_concurrent_activities(mut self, max: usize) -> Self {
        self.max_concurrent_activities = max;
        self
    }

    /// Sets the build id.
    pub fn with_build_id(mut self, build_id: impl Into<String>) -> Self {
        self.build_id = build_id.into();
        self
    }
}

/// Computes the default build id used when none is configured.
fn default_build_id() -> String {
    format!("{}-dev", env!("CARGO_PKG_NAME"))
}

/// High-level Temporal worker abstraction for this crate.
pub struct Worker {
    _runtime: CoreRuntime,
    inner: TemporalWorker,
    options: WorkerOptions,
}

impl Worker {
    /// Connects to Temporal and creates a worker from explicit options.
    pub async fn connect(options: WorkerOptions) -> Result<Self> {
        let telemetry_options = TelemetryOptions::builder().build();
        let runtime_options = RuntimeOptions::builder()
            .telemetry_options(telemetry_options)
            .build()
            .map_err(std::io::Error::other)
            .context("failed to build Temporal runtime options")?;

        let runtime = CoreRuntime::new_assume_tokio(runtime_options)
            .context("failed to initialize Temporal runtime")?;
        Self::connect_with_runtime(runtime, options).await
    }

    /// Connects to Temporal using options loaded from environment variables.
    pub async fn connect_from_env() -> Result<Self> {
        Self::connect(WorkerOptions::from_env()?).await
    }

    /// Connects using a caller-provided Temporal core runtime.
    ///
    /// This keeps runtime initialization separate from connection/configuration
    /// logic so both `connect` and tests can share the same setup path.
    async fn connect_with_runtime(runtime: CoreRuntime, options: WorkerOptions) -> Result<Self> {
        let temporal_url = Url::from_str(&options.temporal_address)
            .with_context(|| format!("invalid temporal address: {}", options.temporal_address))?;

        let client_options = sdk_client_options(temporal_url).build();
        let client = client_options
            .connect_no_namespace(runtime.telemetry().get_temporal_metric_meter())
            .await
            .context("failed to connect to Temporal")?;

        let worker_config = WorkerConfig::builder()
            .namespace(options.namespace.clone())
            .task_queue(options.task_queue.clone())
            .task_types(WorkerTaskTypes::all())
            .max_outstanding_activities(options.max_concurrent_activities)
            .versioning_strategy(WorkerVersioningStrategy::None {
                build_id: options.build_id.clone(),
            })
            .build()
            .map_err(std::io::Error::other)
            .context("failed to build Temporal worker config")?;

        let core_worker =
            init_worker(&runtime, worker_config, client).context("failed to initialize worker")?;
        let worker =
            TemporalWorker::new_from_core(Arc::new(core_worker), options.task_queue.clone());

        Ok(Self {
            _runtime: runtime,
            inner: worker,
            options,
        })
    }

    /// Returns the options used to construct this worker.
    pub fn options(&self) -> &WorkerOptions {
        &self.options
    }

    /// Registers a workflow implementation on this worker.
    pub fn register_workflow<T>(&mut self, workflow: T)
    where
        T: WorkflowRegistration,
    {
        workflow.register_to(&mut self.inner);
    }

    /// Registers an activity implementation on this worker.
    pub fn register_activity<T>(&mut self, activity: T)
    where
        T: ActivityRegistration,
    {
        activity.register_to(&mut self.inner);
    }

    /// Returns mutable access to the underlying Temporal worker.
    ///
    /// This is useful for advanced SDK integrations not directly exposed by this
    /// crate's wrapper.
    pub fn inner_mut(&mut self) -> &mut TemporalWorker {
        &mut self.inner
    }

    /// Runs the worker event loop until it exits or fails.
    pub async fn run(&mut self) -> Result<()> {
        self.inner
            .run()
            .await
            .context("Temporal worker exited with error")
    }
}
