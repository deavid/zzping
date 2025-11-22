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
//! use zznet_transport_tcp::client::TcpTransportClient;
//! use zznet_api::transport::TransportClient;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create client (plain TCP)
//! let client = TcpTransportClient::new("127.0.0.1:8080".to_string(), None)?;
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
pub mod tls_utils;
