//! Native Temporal client APIs.

pub use temporalio_client::errors::ClientConnectError;
pub use temporalio_client::*;

/// Connect with an explicit namespace and application identity.
///
/// The caller owns configuration; this helper does not read environment variables.
pub async fn connect(
    address: Url,
    namespace: impl Into<String>,
    name: &str,
    version: &str,
) -> Result<Client, ClientConnectError> {
    Client::connect(
        ConnectionOptions::new(address)
            .client_name(name.to_owned())
            .client_version(version.to_owned())
            .build(),
        ClientOptions::new(namespace).build(),
    )
    .await
}
