//! Small, feature-gated Temporal SDK 1.0 integration.
//!
//! Contracts and clients use the upstream definition traits and typed handles.
//! Workers use the upstream runtime, registration, and workflow primitives.

/// Native typed client APIs and a connection helper.
#[cfg(feature = "client")]
pub mod client;
/// Native contract definitions and JSON conversion.
#[cfg(feature = "client")]
pub use temporalio_common as common;
#[cfg(feature = "client")]
pub use temporalio_common::{ActivityDefinition, data_converters};
/// Native declarations for workflows and activities.
#[cfg(feature = "worker")]
pub use temporalio_macros::{activities, workflow, workflow_methods};
/// Native worker APIs, including workflow contexts and registration.
#[cfg(feature = "worker")]
pub use temporalio_sdk::*;
#[cfg(feature = "worker")]
#[doc(hidden)]
pub use temporalio_workflow::__private;
/// Native workflow macros and deterministic concurrency helpers.
#[cfg(feature = "worker")]
pub use temporalio_workflow::{join, join_all, select};
/// Convenient imports matching the enabled client and worker features.
pub mod prelude;
