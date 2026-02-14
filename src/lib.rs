//! Opinionated, type-safe building blocks for Temporal workflows and workers.
//!
//! This crate provides:
//! - Trait-based workflow/activity definitions (`[crate::traits::Workflow]`,
//!   `[crate::traits::Activity]`).
//! - A typed workflow runtime context for invoking activities.
//! - A high-level worker abstraction for registration and execution.
//! - A typed client wrapper for starting workflows and decoding workflow results.

/// Activity context and activity execution options.
#[cfg(feature = "worker")]
pub mod activity;
/// Type-safe Temporal client wrapper and typed workflow handles.
#[cfg(feature = "client")]
pub mod client;
/// Convenient re-exports for common imports.
pub mod prelude;
/// Core `Workflow` / `Activity` traits and worker registration adapters.
pub mod traits;
/// High-level worker connection and runtime lifecycle utilities.
#[cfg(feature = "worker")]
pub mod worker;
/// Workflow execution context and typed activity invocation helpers.
#[cfg(feature = "worker")]
pub mod workflow;

/// Re-export of the crate's high-level worker API.
#[cfg(feature = "worker")]
pub use worker::{Worker, WorkerOptions};
