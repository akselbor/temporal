//! Convenient imports for native SDK contracts, clients, and workers.

#[cfg(feature = "worker")]
pub use crate::activities::{ActivityContext, ActivityError};
#[cfg(feature = "client")]
pub use crate::client::{
    Client, ClientOptions, ConnectionOptions, WorkflowExecuteUpdateOptions,
    WorkflowGetResultOptions, WorkflowHandle, WorkflowIdConflictPolicy, WorkflowIdReusePolicy,
    WorkflowSignalOptions, WorkflowStartOptions,
};
#[cfg(feature = "client")]
pub use crate::common::{
    HasWorkflowDefinition, RetryPolicy, SignalDefinition, UpdateDefinition, WorkflowDefinition,
};
#[cfg(feature = "worker")]
pub use crate::{
    ActivityCancellationType, ActivityCloseTimeouts, ActivityOptions, ApplicationFailure, Runtime,
    SyncWorkflowContext, TimerOptions, Worker, WorkerOptions, WorkflowCancellationToken,
    WorkflowContext, WorkflowContextView, WorkflowResult, WorkflowTermination, activities, join,
    join_all, select, workflow, workflow_methods,
};
