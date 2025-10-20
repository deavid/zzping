//! TCP/TLS transport implementation for zznet.
//!
//! This crate provides production-quality TCP and TLS transports implementing
//! the `TransportConnection` trait from zznet-api.
//!
//! ## Features
//!
//! - **TCP with TLS**: Mutual TLS authentication using rustls
//! - **Framing**: Length-prefixed frames for reliable message delimiting
//! - **Role-based certificates**: Automatic cert selection based on role
//! - **Client and Server**: Both sides of the connection
//!
//! ## Usage
//!
//! ```rust,no_run
//! use zznet_transport_tcp::{TcpTransportClient, TlsConfig};
//! use zznet_api::transport::TransportClient;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create TLS configuration
//! let tls_config = TlsConfig::from_role_name("collector", None)?;
//!
//! // Create client
//! let client = TcpTransportClient::new("127.0.0.1:8080".to_string(), Some(tls_config))?;
//!
//! // Connect and get a transport connection
//! let transport = client.connect().await?;
//! # Ok(())
//! # }
//! ```

pub mod client;
pub mod config;
pub mod connection;
pub mod framing;
pub mod server;

pub use client::TcpTransportClient;
pub use config::{TlsCertAndKey, TlsConfig};
pub use connection::TcpTransport;
pub use server::TcpTransportServer;
