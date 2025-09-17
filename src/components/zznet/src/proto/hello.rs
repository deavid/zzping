use serde::{Deserialize, Serialize};
use zznet_api::Role;

/// The initial handshake message sent when establishing a connection.
/// Contains the role of the connecting participant to establish the connection context.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hello {
    /// The role of the connecting participant.
    pub role: Role,
}
