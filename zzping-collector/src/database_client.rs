use anyhow::Result;
use tonic::transport::{Channel, Endpoint};
use tonic::Request;
use zzping_proto::zzping::{
    ingestion_client::IngestionClient, HeartbeatRequest, HeartbeatResponse,
};

/// A lightweight, cloneable wrapper around the `tonic` gRPC client that
/// centralizes request creation and authentication logic.
#[derive(Clone)]
pub struct DatabaseClient {
    /// The underlying gRPC client.
    client: IngestionClient<Channel>,
    /// The authentication token for this collector.
    auth_token: String,
}

impl DatabaseClient {
    /// Connects to the database and creates a new `DatabaseClient`.
    ///
    /// # Arguments
    ///
    /// * `addr` - The address of the database gRPC server.
    /// * `auth_token` - The authentication token for this collector.
    pub async fn connect(addr: String, auth_token: String) -> Result<Self> {
        let endpoint = Endpoint::from_shared(addr)?;
        let channel = endpoint.connect().await?;
        let client = IngestionClient::new(channel);

        Ok(Self { client, auth_token })
    }

    /// Performs a Heartbeat RPC.
    ///
    /// # Arguments
    ///
    /// * `request` - The `HeartbeatRequest` to send.
    pub async fn heartbeat(
        &mut self,
        request: HeartbeatRequest,
    ) -> Result<tonic::Response<HeartbeatResponse>> {
        let mut tonic_request = Request::new(request);

        // Add the authentication token to the request metadata.
        let token = format!("Bearer {}", self.auth_token);
        tonic_request
            .metadata_mut()
            .insert("authorization", token.parse()?);

        let response = self.client.heartbeat(tonic_request).await?;
        Ok(response)
    }

    // `send_batch` and other RPC wrappers will be added in later steps.
}

