use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use futures::future::try_join_all;
use serde::{Serialize, de::DeserializeOwned};
use temporalio_client::Priority;
use temporalio_common::protos::{
    coresdk::{AsJsonPayloadExt, FromJsonPayloadExt, workflow_commands::ActivityCancellationType},
    temporal::api::common::v1::RetryPolicy,
};
use temporalio_sdk;
use temporalio_sdk::{ActContext, ActivityError, WfContext, Worker, WorkflowResult};
