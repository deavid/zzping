use anyhow::Result;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tonic::Request;
use tonic::transport::{Channel, Endpoint};
use zzping_proto::zzping::{
    AnnouncePingsRequest, AnnouncePingsResponse, Command, CommandRequest, GetRecentDataRequest,
    GetRecentDataResponse, HeartbeatRequest, HeartbeatResponse, SendBatchRequest,
    SendBatchResponse, ingestion_client::IngestionClient,
}; // Added for Arc<dyn DatabaseClientTrait>

#[cfg(test)]
use mockall::automock;

// Alias for boxed async RPC results to reduce repetitive complex types
type RpcFut<'a, T> = Pin<Box<dyn Future<Output = Result<tonic::Response<T>>> + Send + 'a>>;

#[cfg_attr(test, automock)]
/// Abstraction over the database RPC client used by the collector.
///
/// Implementations execute gRPC requests and return boxed futures so callers
/// can use the trait object without exposing concrete async types.
pub trait DatabaseClientTrait: Send + Sync + 'static {
    /// Send a heartbeat RPC to the database service.
    fn heartbeat<'a>(&'a self, request: HeartbeatRequest) -> RpcFut<'a, HeartbeatResponse>;

    /// Announce a set of pings to the database service.
    fn announce_pings<'a>(
        &'a self,
        request: AnnouncePingsRequest,
    ) -> RpcFut<'a, AnnouncePingsResponse>;

    /// Send a batch of finalized pings to the database service.
    fn send_batch<'a>(&'a self, request: SendBatchRequest) -> RpcFut<'a, SendBatchResponse>;

    /// Request recent data matching the provided criteria.
    fn get_recent_data<'a>(
        &'a self,
        request: GetRecentDataRequest,
    ) -> RpcFut<'a, GetRecentDataResponse>;

    /// Subscribe to control commands from the database service.
    fn subscribe_to_commands<'a>(
        &'a self,
        request: CommandRequest,
    ) -> RpcFut<'a, tonic::Streaming<Command>>;
}

/// A lightweight, cloneable wrapper around the `tonic` gRPC client that
/// centralizes request creation and authentication logic.
#[derive(Clone, Debug)]
pub struct DatabaseClient {
    /// The underlying gRPC client.
    client: IngestionClient<Channel>,
    /// The authentication token for this collector.
    auth_token: String,
}

impl DatabaseClient {
    /// Connects to the database and returns a shared, trait-object wrapped
    /// `DatabaseClient` (as `Arc<dyn DatabaseClientTrait>`). Returning the
    /// trait object encourages callers to depend on the abstraction which
    /// makes mocking and testing simpler.
    pub async fn connect(addr: String, auth_token: String) -> Result<Arc<dyn DatabaseClientTrait>> {
        let endpoint = Endpoint::from_shared(addr)?;
        let channel = endpoint.connect().await?;
        let client = IngestionClient::new(channel);

        let db = Self { client, auth_token };
        Ok(Arc::new(db) as Arc<dyn DatabaseClientTrait>)
    }

    // -- Inherent async wrappers so callers can call .heartbeat(...) directly
    /// Send a heartbeat RPC using the configured authentication token.
    pub async fn heartbeat(
        &self,
        request: HeartbeatRequest,
    ) -> Result<tonic::Response<HeartbeatResponse>> {
        let mut tonic_request = Request::new(request);
        let token = format!("Bearer {}", self.auth_token);
        tonic_request
            .metadata_mut()
            .insert("authorization", token.parse()?);
        let resp = self.client.clone().heartbeat(tonic_request).await?;
        Ok(resp)
    }

    /// Announce pings using the configured authentication token.
    pub async fn announce_pings(
        &self,
        request: AnnouncePingsRequest,
    ) -> Result<tonic::Response<AnnouncePingsResponse>> {
        let mut tonic_request = Request::new(request);
        let token = format!("Bearer {}", self.auth_token);
        tonic_request
            .metadata_mut()
            .insert("authorization", token.parse()?);
        let resp = self.client.clone().announce_pings(tonic_request).await?;
        Ok(resp)
    }

    /// Send a batch of records to the database service.
    pub async fn send_batch(
        &self,
        request: SendBatchRequest,
    ) -> Result<tonic::Response<SendBatchResponse>> {
        let mut tonic_request = Request::new(request);
        let token = format!("Bearer {}", self.auth_token);
        tonic_request
            .metadata_mut()
            .insert("authorization", token.parse()?);
        let resp = self.client.clone().send_batch(tonic_request).await?;
        Ok(resp)
    }

    /// Fetch recent data from the database service.
    pub async fn get_recent_data(
        &self,
        request: GetRecentDataRequest,
    ) -> Result<tonic::Response<GetRecentDataResponse>> {
        let mut tonic_request = Request::new(request);
        let token = format!("Bearer {}", self.auth_token);
        tonic_request
            .metadata_mut()
            .insert("authorization", token.parse()?);
        let resp = self.client.clone().get_recent_data(tonic_request).await?;
        Ok(resp)
    }

    /// Subscribe to commands streamed from the database service.
    pub async fn subscribe_to_commands(
        &self,
        request: CommandRequest,
    ) -> Result<tonic::Response<tonic::Streaming<Command>>> {
        let mut tonic_request = Request::new(request);
        let token = format!("Bearer {}", self.auth_token);
        tonic_request
            .metadata_mut()
            .insert("authorization", token.parse()?);
        let resp = self
            .client
            .clone()
            .subscribe_to_commands(tonic_request)
            .await?;
        Ok(resp)
    }

    // Note: test helpers were removed to avoid dead-code with workspace lints.
}

impl DatabaseClientTrait for DatabaseClient {
    fn heartbeat<'a>(
        &'a self, // Changed from &'a mut self
        request: HeartbeatRequest,
    ) -> Pin<Box<dyn Future<Output = Result<tonic::Response<HeartbeatResponse>>> + Send + 'a>> {
        Box::pin(async move {
            let mut tonic_request = Request::new(request);
            let token = format!("Bearer {}", self.auth_token);
            tonic_request
                .metadata_mut()
                .insert("authorization", token.parse()?);
            let response = self.client.clone().heartbeat(tonic_request).await?; // Added .clone()
            Ok(response)
        })
    }

    fn announce_pings<'a>(
        &'a self, // Changed from &'a mut self
        request: AnnouncePingsRequest,
    ) -> Pin<Box<dyn Future<Output = Result<tonic::Response<AnnouncePingsResponse>>> + Send + 'a>>
    {
        Box::pin(async move {
            let mut tonic_request = Request::new(request);
            let token = format!("Bearer {}", self.auth_token);
            tonic_request
                .metadata_mut()
                .insert("authorization", token.parse()?);
            let response = self.client.clone().announce_pings(tonic_request).await?; // Added .clone()
            Ok(response)
        })
    }

    fn send_batch<'a>(
        &'a self, // Changed from &'a mut self
        request: SendBatchRequest,
    ) -> Pin<Box<dyn Future<Output = Result<tonic::Response<SendBatchResponse>>> + Send + 'a>> {
        Box::pin(async move {
            let mut tonic_request = Request::new(request);
            let token = format!("Bearer {}", self.auth_token);
            tonic_request
                .metadata_mut()
                .insert("authorization", token.parse()?);
            let response = self.client.clone().send_batch(tonic_request).await?; // Added .clone()
            Ok(response)
        })
    }

    fn get_recent_data<'a>(
        &'a self, // Changed from &'a mut self
        request: GetRecentDataRequest,
    ) -> Pin<Box<dyn Future<Output = Result<tonic::Response<GetRecentDataResponse>>> + Send + 'a>>
    {
        Box::pin(async move {
            let mut tonic_request = Request::new(request);
            let token = format!("Bearer {}", self.auth_token);
            tonic_request
                .metadata_mut()
                .insert("authorization", token.parse()?);
            let response = self.client.clone().get_recent_data(tonic_request).await?; // Added .clone()
            Ok(response)
        })
    }

    fn subscribe_to_commands<'a>(
        &'a self, // Changed from &'a mut self
        request: CommandRequest,
    ) -> Pin<Box<dyn Future<Output = Result<tonic::Response<tonic::Streaming<Command>>>> + Send + 'a>>
    {
        Box::pin(async move {
            let mut tonic_request = Request::new(request);
            let token = format!("Bearer {}", self.auth_token);
            tonic_request
                .metadata_mut()
                .insert("authorization", token.parse()?);
            let response = self
                .client
                .clone()
                .subscribe_to_commands(tonic_request)
                .await?;
            Ok(response)
        })
    }
}
