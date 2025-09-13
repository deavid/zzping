use serde::{Deserialize, Serialize};

/// Represents the role of a participant in the ZZPing network protocol.
/// This enum defines the possible roles that can connect to the network.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Role {
    /// Collector role for gathering data.
    Collector,
    /// Database role for storing data.
    Database,
    /// Read-only client role.
    ClientRo,
    /// Administrative client role with full access.
    ClientAdmin,
}

/// The initial handshake message sent when establishing a connection.
/// Contains the role of the connecting participant to establish the connection context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hello {
    /// The role of the connecting participant.
    pub role: Role,
}
