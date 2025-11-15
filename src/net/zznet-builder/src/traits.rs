//! Core traits for ZZNet builder: service and config abstractions.

use async_trait::async_trait;
use serde::de::DeserializeOwned;

/// Marker for a deserializable configuration used by services.
pub trait ZZNetConfig: DeserializeOwned + Send + Sync + 'static {}

/// Service lifecycle: construction and run.
#[async_trait]
pub trait ZZNetService: Sized + Send + 'static {
    /// Config type for this service.
    type Config: ZZNetConfig;

    /// Service error type.
    type Error: Into<anyhow::Error> + Send + Sync + 'static;

    /// Construct a service from `Config`.
    fn new(config: Self::Config) -> Result<Self, Self::Error>;

    /// Start the service; return `Ok(())` once running.
    async fn run(self) -> Result<(), Self::Error>;

    /// Human-readable name for logs; default: type name.
    fn service_name() -> &'static str {
        std::any::type_name::<Self>()
    }
}
