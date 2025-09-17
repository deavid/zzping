use zznet::connection::{ClientConfig, ServerConfig};

/// Unified configuration for zznet client or server operation.
///
/// Provides a single entry point while separating client and server concerns,
/// simplifying configuration management across the zzping ecosystem.
pub enum ZzNetConfig {
    Client(ClientConfig),
    Server(ServerConfig),
}
