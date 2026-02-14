//! Convenient imports for common `temporal` usage.
//!
//! Import with:
//! `use temporal::prelude::*;`
//!
//! Re-exports are feature-gated to match this crate's `worker` and `client`
//! feature flags.

/// Core type-safe workflow/activity traits (always available).
pub use crate::traits::{Activity, Workflow};

/// Worker-side async trait macro used by workflow/activity implementations.
#[cfg(feature = "worker")]
pub use async_trait::async_trait;

/// Worker-side activity context and activity scheduling options.
#[cfg(feature = "worker")]
pub use crate::activity::{ActivityContext, ActivityOptions};

/// Worker-side workflow runtime context.
#[cfg(feature = "worker")]
pub use crate::workflow::WorkflowContext;

/// Worker-side registration helpers.
#[cfg(feature = "worker")]
pub use crate::traits::{ActivityRegistration, WorkflowRegistration};

/// Worker wrapper APIs.
#[cfg(feature = "worker")]
pub use crate::worker::{Worker, WorkerOptions};

/// Worker-side Temporal SDK result/error aliases used in trait signatures.
#[cfg(feature = "worker")]
pub use temporalio_sdk::{ActivityError, WorkflowResult};

/// Common supporting types for constructing [`crate::activity::ActivityOptions`].
#[cfg(feature = "worker")]
pub use temporalio_client::Priority;
/// Common supporting types for constructing [`crate::activity::ActivityOptions`].
#[cfg(feature = "worker")]
pub use temporalio_common::protos::coresdk::workflow_commands::ActivityCancellationType;
/// Common supporting types for constructing [`crate::activity::ActivityOptions`].
#[cfg(feature = "worker")]
pub use temporalio_common::protos::temporal::api::common::v1::RetryPolicy;

/// Client-side typed wrapper APIs.
#[cfg(feature = "client")]
pub use crate::client::{Client, ConnectedClient, StartWorkflowOptions, TypedWorkflowHandle};

/// Client-side Temporal workflow start/result options and result types.
#[cfg(feature = "client")]
pub use temporalio_client::{
    ClientInitError, ClientOptions, GetWorkflowResultOptions, WorkflowExecutionResult,
    WorkflowOptions,
};

/// Client-side metrics type used by [`crate::client::Client::connect`].
#[cfg(feature = "client")]
pub use temporalio_common::telemetry::metrics::TemporalMeter;
