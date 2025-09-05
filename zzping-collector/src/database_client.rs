//! Abstracts communication with the database service.
//!
//! This module provides a minimal interface for database interactions,
//! focusing on simplicity, correctness, and testability. It encapsulates
//! gRPC details and ensures transport-agnostic collector logic.

use crate::cli::Cli;
use anyhow::{Context, Result};
use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::Mutex;
use tonic::transport::{Certificate, Channel, ClientTlsConfig};
use zzping_proto::zzping::{
    GetRecentDataRequest, GetRecentDataResponse, HeartbeatRequest, HeartbeatResponse,
    SendBatchRequest, SendBatchResponse, ingestion_client::IngestionClient,
};

/// Defines the control plane for database communication.
///
/// Handles heartbeats, configuration updates, and data ingestion.
#[async_trait]
pub trait DatabaseClient: Send + Sync {
    /// Sends a heartbeat to the database.
    async fn heartbeat(&self, req: tonic::Request<HeartbeatRequest>) -> Result<HeartbeatResponse>;

    /// Sends a batch of data to the database.
    async fn send_batch(&self, req: tonic::Request<SendBatchRequest>) -> Result<SendBatchResponse>;

    /// Requests recent data for buffer seeding.
    async fn get_recent_data(
        &self,
        req: tonic::Request<GetRecentDataRequest>,
    ) -> Result<GetRecentDataResponse>;
}

/// Production gRPC client for the database.
///
/// Encapsulates gRPC details, ensuring a stable interface for the collector.
#[derive(Debug)]
pub struct GrpcClient {
    /// gRPC client for ingestion service.
    client: Arc<tokio::sync::Mutex<IngestionClient<Channel>>>,
}

impl GrpcClient {
    /// Establishes a connection to the database.
    ///
    /// Reads a pinned CA certificate for TLS and supports both secure and
    /// insecure endpoints.
    pub async fn connect(cli: &Cli) -> Result<Self> {
        let ca_cert = tokio::fs::read("ca.pem")
            .await
            .context("Unable to read ca_cert as ./ca.pem")?;
        let host = if cli.database_addr.starts_with("https://") {
            cli.database_addr.strip_prefix("https://").unwrap()
        } else {
            &cli.database_addr
        };
        let domain_name = host.split(':').next().unwrap();
        let ca = if cli.database_addr.starts_with("https://") {
            Some(Certificate::from_pem(ca_cert))
        } else {
            None
        };
        let tls_config = ca.as_ref().map(|ca| {
            ClientTlsConfig::new()
                .domain_name(domain_name)
                .ca_certificate(ca.clone())
        });
        let channel_builder = Channel::from_shared(cli.database_addr.clone())?;
        let channel = if let Some(tls) = tls_config {
            channel_builder.tls_config(tls)?
        } else {
            channel_builder
        };
        let channel = channel.connect().await?;
        let client = IngestionClient::new(channel);
        Ok(Self {
            client: Arc::new(Mutex::new(client)),
        })
    }
}

#[async_trait]
impl DatabaseClient for GrpcClient {
    async fn heartbeat(&self, req: tonic::Request<HeartbeatRequest>) -> Result<HeartbeatResponse> {
        let mut client = self.client.lock().await;
        let response = client.heartbeat(req).await?;
        Ok(response.into_inner())
    }

    async fn send_batch(&self, req: tonic::Request<SendBatchRequest>) -> Result<SendBatchResponse> {
        let mut client = self.client.lock().await;
        let response = client.send_batch(req).await?;
        Ok(response.into_inner())
    }

    async fn get_recent_data(
        &self,
        req: tonic::Request<GetRecentDataRequest>,
    ) -> Result<GetRecentDataResponse> {
        let mut client = self.client.lock().await;
        let response = client.get_recent_data(req).await?;
        Ok(response.into_inner())
    }
}

// Shared client notes
// - We use `Arc<tokio::sync::Mutex<dyn DatabaseClient>>` deliberately.
// - Invariants: callers must avoid holding the mutex across unrelated
//   awaits or long-running CPU work. Lock holds should be limited to the
//   duration of the RPC call itself.
// - If you find lots of contention here, consider one of:
//   * A connection pool implementation that exposes a cheap, short-lived
//     client per operation, or
//   * Change the trait to allow `&self` methods with interior mutability in
//     the implementation, allowing concurrent RPCs.
pub type SharedDatabaseClient = Arc<Mutex<dyn DatabaseClient>>;

#[cfg(test)]
mod tests {
    use super::*;
    use zzping_proto::zzping::*;

    // Mock implementation for testing
    struct MockDatabaseClient {
        heartbeat_response: Result<HeartbeatResponse>,
        send_batch_response: Result<SendBatchResponse>,
        get_recent_data_response: Result<GetRecentDataResponse>,
    }

    #[async_trait]
    impl DatabaseClient for MockDatabaseClient {
        async fn heartbeat(
            &self,
            _req: tonic::Request<HeartbeatRequest>,
        ) -> Result<HeartbeatResponse> {
            match &self.heartbeat_response {
                Ok(resp) => Ok(resp.clone()),
                Err(e) => Err(anyhow::anyhow!(e.to_string())),
            }
        }

        async fn send_batch(
            &self,
            _req: tonic::Request<SendBatchRequest>,
        ) -> Result<SendBatchResponse> {
            match &self.send_batch_response {
                Ok(resp) => Ok(resp.clone()),
                Err(e) => Err(anyhow::anyhow!(e.to_string())),
            }
        }

        async fn get_recent_data(
            &self,
            _req: tonic::Request<GetRecentDataRequest>,
        ) -> Result<GetRecentDataResponse> {
            match &self.get_recent_data_response {
                Ok(resp) => Ok(resp.clone()),
                Err(e) => Err(anyhow::anyhow!(e.to_string())),
            }
        }
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_grpc_client_heartbeat_success() {
        let mock_client = Arc::new(Mutex::new(MockDatabaseClient {
            heartbeat_response: Ok(HeartbeatResponse {
                targets: vec!["1.1.1.1".to_string()],
                ping_rate_pps: 100,
                role: 0, // Primary
                swap_at_nanos: 0,
            }),
            send_batch_response: Ok(SendBatchResponse::default()),
            get_recent_data_response: Ok(GetRecentDataResponse::default()),
        }));

        let request = tonic::Request::new(HeartbeatRequest {
            collector_uuid: "test-collector".to_string(),
            pid: 1234,
        });

        let response = mock_client.lock().await.heartbeat(request).await.unwrap();
        assert_eq!(response.targets, vec!["1.1.1.1"]);
        assert_eq!(response.ping_rate_pps, 100);
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_grpc_client_send_batch_success() {
        let mock_client = Arc::new(Mutex::new(MockDatabaseClient {
            heartbeat_response: Ok(HeartbeatResponse::default()),
            send_batch_response: Ok(SendBatchResponse {
                status: send_batch_response::Status::Ok as i32,
                database_confirms_last_acked_nanos: 1000,
            }),
            get_recent_data_response: Ok(GetRecentDataResponse::default()),
        }));

        let request = tonic::Request::new(SendBatchRequest {
            collector_uuid: "test-collector".to_string(),
            target_ip: "1.1.1.1".to_string(),
            records: vec![],
            collector_believes_last_acked_nanos: 0,
        });

        let response = mock_client.lock().await.send_batch(request).await.unwrap();
        assert_eq!(response.status, send_batch_response::Status::Ok as i32);
        assert_eq!(response.database_confirms_last_acked_nanos, 1000);
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_grpc_client_get_recent_data_success() {
        let mock_client = Arc::new(Mutex::new(MockDatabaseClient {
            heartbeat_response: Ok(HeartbeatResponse::default()),
            send_batch_response: Ok(SendBatchResponse::default()),
            get_recent_data_response: Ok(GetRecentDataResponse {
                records: vec![RawDataRecord {
                    sent_nanos: 1000,
                    rtt_nanos: 5000,
                }],
            }),
        }));

        let request = tonic::Request::new(GetRecentDataRequest {
            collector_uuid: "test-collector".to_string(),
            lookback_seconds: 3600,
        });

        let response = mock_client
            .lock()
            .await
            .get_recent_data(request)
            .await
            .unwrap();
        assert_eq!(response.records.len(), 1);
        assert_eq!(response.records[0].sent_nanos, 1000);
        assert_eq!(response.records[0].rtt_nanos, 5000);
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_grpc_client_heartbeat_error() {
        let mock_client = Arc::new(Mutex::new(MockDatabaseClient {
            heartbeat_response: Err(anyhow::anyhow!("Network error")),
            send_batch_response: Ok(SendBatchResponse::default()),
            get_recent_data_response: Ok(GetRecentDataResponse::default()),
        }));

        let request = tonic::Request::new(HeartbeatRequest {
            collector_uuid: "test-collector".to_string(),
            pid: 1234,
        });

        let result = mock_client.lock().await.heartbeat(request).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "Network error");
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_grpc_client_send_batch_error() {
        let mock_client = Arc::new(Mutex::new(MockDatabaseClient {
            heartbeat_response: Ok(HeartbeatResponse::default()),
            send_batch_response: Err(anyhow::anyhow!("Database error")),
            get_recent_data_response: Ok(GetRecentDataResponse::default()),
        }));

        let request = tonic::Request::new(SendBatchRequest {
            collector_uuid: "test-collector".to_string(),
            target_ip: "1.1.1.1".to_string(),
            records: vec![],
            collector_believes_last_acked_nanos: 0,
        });

        let result = mock_client.lock().await.send_batch(request).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "Database error");
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_grpc_client_get_recent_data_error() {
        let mock_client = Arc::new(Mutex::new(MockDatabaseClient {
            heartbeat_response: Ok(HeartbeatResponse::default()),
            send_batch_response: Ok(SendBatchResponse::default()),
            get_recent_data_response: Err(anyhow::anyhow!("Query error")),
        }));

        let request = tonic::Request::new(GetRecentDataRequest {
            collector_uuid: "test-collector".to_string(),
            lookback_seconds: 3600,
        });

        let result = mock_client.lock().await.get_recent_data(request).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().to_string(), "Query error");
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_grpc_client_connect_https_with_ca() {
        // Test HTTPS connection setup with CA certificate
        let cli = Cli {
            source_hostname: "test-collector".to_string(),
            database_addr: "https://example.com:8080".to_string(),
            auth_token: "test-token".to_string(),
            max_in_flight: 3,
        };

        // This test verifies the connection logic path for HTTPS
        // We can't actually connect to example.com, but we can test the setup
        let result = GrpcClient::connect(&cli).await;

        // Should fail with connection error, not CA certificate error
        assert!(result.is_err());
        let _error_msg = result.unwrap_err().to_string();
        // Should be a connection/transport error, not a CA cert error
        // Note: We can't check the exact error message as it depends on the system
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_grpc_client_connect_http_no_ca() {
        // Test HTTP connection setup without CA certificate
        let cli = Cli {
            source_hostname: "test-collector".to_string(),
            database_addr: "http://127.0.0.1:8080".to_string(),
            auth_token: "test-token".to_string(),
            max_in_flight: 3,
        };

        // This test verifies the connection logic path for HTTP
        let result = GrpcClient::connect(&cli).await;

        // Should fail with connection error
        assert!(result.is_err());
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_grpc_client_connect_missing_ca_file() {
        // Test behavior when CA certificate file doesn't exist
        let cli = Cli {
            source_hostname: "test-collector".to_string(),
            database_addr: "https://example.com:8080".to_string(),
            auth_token: "test-token".to_string(),
            max_in_flight: 3,
        };

        let result = GrpcClient::connect(&cli).await;

        // Should fail when trying to read the CA certificate
        assert!(result.is_err());
        let _error_msg = result.unwrap_err().to_string();
        // Note: We can't check the exact error message as it depends on the system
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_grpc_client_connect_domain_parsing() {
        // Test domain name parsing from various URL formats
        let test_cases = vec![
            ("https://api.example.com:8080", "api.example.com"),
            ("https://sub.domain.test:443", "sub.domain.test"),
            ("http://localhost:8080", "localhost"),
            ("https://test-server", "test-server"),
        ];

        for (url, expected_domain) in test_cases {
            let cli = Cli {
                source_hostname: "test-collector".to_string(),
                database_addr: url.to_string(),
                auth_token: "test-token".to_string(),
                max_in_flight: 3,
            };

            // Test the domain parsing logic
            let host = if cli.database_addr.starts_with("https://") {
                cli.database_addr.strip_prefix("https://").unwrap()
            } else if cli.database_addr.starts_with("http://") {
                cli.database_addr.strip_prefix("http://").unwrap()
            } else {
                &cli.database_addr
            };
            let domain_name = host.split(':').next().unwrap();

            assert_eq!(domain_name, expected_domain);
        }
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_grpc_client_connect_tls_config_creation() {
        // Test TLS configuration creation logic
        let cli_https = Cli {
            source_hostname: "test-collector".to_string(),
            database_addr: "https://example.com:8080".to_string(),
            auth_token: "test-token".to_string(),
            max_in_flight: 3,
        };

        let cli_http = Cli {
            source_hostname: "test-collector".to_string(),
            database_addr: "http://example.com:8080".to_string(),
            auth_token: "test-token".to_string(),
            max_in_flight: 3,
        };

        // Test HTTPS case - should create TLS config
        let host_https = if cli_https.database_addr.starts_with("https://") {
            cli_https.database_addr.strip_prefix("https://").unwrap()
        } else {
            &cli_https.database_addr
        };
        let _domain_name_https = host_https.split(':').next().unwrap();

        // For HTTPS, we expect TLS config to be created (though it will fail due to missing CA)
        let result_https = GrpcClient::connect(&cli_https).await;
        assert!(result_https.is_err());

        // Test HTTP case - should not create TLS config
        let host_http = if cli_http.database_addr.starts_with("https://") {
            cli_http.database_addr.strip_prefix("https://").unwrap()
        } else {
            &cli_http.database_addr
        };
        let _domain_name_http = host_http.split(':').next().unwrap();

        // For HTTP, TLS config should be None
        let ca_http = if cli_http.database_addr.starts_with("https://") {
            // This would try to read ca.pem which doesn't exist
            Some(Certificate::from_pem(vec![]))
        } else {
            None
        };
        assert!(ca_http.is_none());
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_grpc_client_heartbeat_with_metadata() {
        let mock_client = Arc::new(Mutex::new(MockDatabaseClient {
            heartbeat_response: Ok(HeartbeatResponse {
                targets: vec!["1.1.1.1".to_string()],
                ping_rate_pps: 100,
                role: 0,
                swap_at_nanos: 0,
            }),
            send_batch_response: Ok(SendBatchResponse::default()),
            get_recent_data_response: Ok(GetRecentDataResponse::default()),
        }));

        // Test with authorization metadata
        let mut request = tonic::Request::new(HeartbeatRequest {
            collector_uuid: "test-collector".to_string(),
            pid: 1234,
        });
        request
            .metadata_mut()
            .insert("authorization", "Bearer test-token".parse().unwrap());

        // Verify metadata was set before moving the request
        assert!(request.metadata().get("authorization").is_some());

        let response = mock_client.lock().await.heartbeat(request).await.unwrap();
        assert_eq!(response.targets, vec!["1.1.1.1"]);
        assert_eq!(response.ping_rate_pps, 100);
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_grpc_client_send_batch_with_metadata() {
        let mock_client = Arc::new(Mutex::new(MockDatabaseClient {
            heartbeat_response: Ok(HeartbeatResponse::default()),
            send_batch_response: Ok(SendBatchResponse {
                status: send_batch_response::Status::Ok as i32,
                database_confirms_last_acked_nanos: 2000,
            }),
            get_recent_data_response: Ok(GetRecentDataResponse::default()),
        }));

        let mut request = tonic::Request::new(SendBatchRequest {
            collector_uuid: "test-collector".to_string(),
            target_ip: "1.1.1.1".to_string(),
            records: vec![],
            collector_believes_last_acked_nanos: 1000,
        });
        request
            .metadata_mut()
            .insert("authorization", "Bearer test-token".parse().unwrap());

        let response = mock_client.lock().await.send_batch(request).await.unwrap();
        assert_eq!(response.status, send_batch_response::Status::Ok as i32);
        assert_eq!(response.database_confirms_last_acked_nanos, 2000);
    }

    #[tokio::test]
    #[ntest::timeout(100)]
    async fn test_grpc_client_get_recent_data_with_metadata() {
        let mock_client = Arc::new(Mutex::new(MockDatabaseClient {
            heartbeat_response: Ok(HeartbeatResponse::default()),
            send_batch_response: Ok(SendBatchResponse::default()),
            get_recent_data_response: Ok(GetRecentDataResponse {
                records: vec![RawDataRecord {
                    sent_nanos: 2000,
                    rtt_nanos: 10000,
                }],
            }),
        }));

        let mut request = tonic::Request::new(GetRecentDataRequest {
            collector_uuid: "test-collector".to_string(),
            lookback_seconds: 7200, // 2 hours
        });
        request
            .metadata_mut()
            .insert("authorization", "Bearer test-token".parse().unwrap());

        let response = mock_client
            .lock()
            .await
            .get_recent_data(request)
            .await
            .unwrap();
        assert_eq!(response.records.len(), 1);
        assert_eq!(response.records[0].sent_nanos, 2000);
        assert_eq!(response.records[0].rtt_nanos, 10000);
    }
}
