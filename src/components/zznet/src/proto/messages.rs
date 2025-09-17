use serde::{Deserialize, Serialize};

/// A unique identifier for a logical communication channel within a connection.
///
/// This type is used to distinguish between different application-level
/// channels multiplexed over a single underlying network connection.
pub type ChannelId = u16;

/// Represents a top-level message frame exchanged over the `zznet` protocol.
///
/// A frame can either be a `Control` message, used for managing the connection
/// and channels, or a `Data` message, carrying application-level payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Frame {
    Control(ControlMsg),
    Data(DataMsg),
}

/// Represents various control messages used for managing the `zznet` connection
/// and its multiplexed channels.
///
/// These messages facilitate operations such as initial handshakes,
/// channel requests, channel establishment confirmations, and channel closures.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ControlMsg {
    Hello(super::hello::Hello),
    RequestChannel { name: String },
    ChannelOpened { name: String, id: ChannelId },
    CloseChannel { id: ChannelId },
}

/// Represents an application-level data message sent over a specific channel.
///
/// This struct encapsulates the payload of a message along with the identifier
/// of the channel it belongs to, allowing for multiplexing of data streams
/// over a single connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMsg {
    /// The identifier of the channel to which this data message belongs.
    pub channel_id: ChannelId,
    /// The raw application-level data being transmitted.
    pub payload: Vec<u8>,
}
