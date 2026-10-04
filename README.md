# Temporal

A small, feature-gated facade over the released Temporal Rust SDK 1.0.0. It reexports the native SDK rather than maintaining another client, workflow scheduler, or registration layer.

```toml
[dependencies]
temporal = { git = "https://github.com/akselbor/temporal", rev = "<commit>", default-features = false, features = ["client"] }
```

- `client`: native typed clients, handles, options, and contract traits.
- `worker`: client plus native worker runtime, workflow/activity declarations, and deterministic concurrency.
- `testing`: worker plus upstream test environments.
- No default features: no SDK dependency is enabled.

The `client::connect(address, namespace, name, version)` convenience takes explicit configuration. Applications own environment variables.

## Workflows and activities

Use `#[workflow]`, `#[workflow_methods]`, and `#[activities]` from `temporal::prelude`. The upstream macros need these aliases and a direct `futures` dependency in a worker crate:

```rust
extern crate temporal as temporalio_common;
extern crate temporal as temporalio_sdk;
extern crate temporal as temporalio_workflow;
```

See [the worker example](examples/worker.rs) for activities, initial signals, asynchronous Updates, state access, and handler draining. See [the client example](examples/client.rs) for a lightweight public contract and signal-with-start. Public contract crates can implement `common::{WorkflowDefinition, HasWorkflowDefinition, SignalDefinition, UpdateDefinition}` without enabling worker dependencies.

Use the native `state`/`state_mut`, `wait_condition`, `all_handlers_finished`, `select!`, and `join_all` APIs. Do not use Tokio channels or mutexes for workflow coordination. Use native activity options at scheduling sites; application failures explicitly control retryability.

## Migrating from 0.1

This release replaces the wrapper traits and forwarding methods. Declare worker implementations with native macros, use native typed definition values in client calls, and register definitions through `WorkerOptions` before constructing a worker. Workflow results are ordinary values; execution failures are native client errors. Non-retryable activity errors use `ApplicationFailure::non_retryable(error).into()`.

Keep existing wire names, JSON contracts, activity ordering, timers, and retry policies. Replay recorded histories before deploying changed workflow implementations. This crate does not provide an old-API compatibility layer.

## Validation

```sh
cargo check --no-default-features
cargo check --all-targets --features client
cargo test --all-targets --features worker
cargo clippy --all-targets --features worker -- -D warnings
TEMPORAL_TEST_ADDRESS=http://localhost:7233 cargo test --features worker --test worker -- --ignored
```

The integration test submits an initial signal and concurrent Updates before polling starts, checks serialization across waits and completion draining, and replays the resulting history with nondeterminism detection enabled.
