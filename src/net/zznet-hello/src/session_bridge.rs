//! SessionBridge Actor - Encapsulates message forwarding between HelloActor and SessionManager
//!
//! This actor is responsible for the complex bidirectional message routing that was previously
//! embedded in ConnectionManager's HandshakeComplete handler. By extracting it into its own actor,
//! we achieve:
//!
//! - **Single Responsibility:** Focused only on bridging transport ↔ session messages
//! - **Testability:** Can be tested in isolation
//! - **Clarity:** Data flow is explicit and easy to reason about
//! - **Maintainability:** Complex async wiring is self-contained
//!
//! ## Architecture
//!
//! ```text
//! SessionManager
//!      ↕ (sends/receives messages)
//!      ↑ outbound_rx: RoomMessages from SessionManager
//!      ↓ conn_to_session_tx: RoomMessages to SessionManager
//!
//! SessionBridge (this actor)
//!      ↕ (spawns forwarding tasks)
//!
//! HelloActor
//!      ↕ (sends/receives protocol frames)
//!      ↑ hello_to_conn_rx: Frames from HelloActor
//!      ↓ inbound channel: Frames to HelloActor
//! ```

use actix::prelude::*;
use tokio::sync::mpsc;
use tracing::{debug, error};

use zznet_session::room_message_trait::RoomMessageTrait;
use zznet_session::types::RoomId;

use crate::actor::HelloActor;

/// SessionBridge actor - Manages bidirectional message forwarding
///
/// Generic over TMsg: the application's message enum type
pub struct SessionBridge<TMsg>
where
    TMsg: RoomMessageTrait,
{
    /// Peer identifier for logging
    peer_id: String,

    /// Reference to HelloActor for sending outbound messages
    hello_actor: Addr<HelloActor>,

    /// Receiver for outbound messages from SessionManager
    /// These messages need to be serialized and sent to HelloActor
    outbound_rx: Option<mpsc::Receiver<(RoomId, TMsg)>>,

    /// Sender for inbound messages to SessionManager
    /// These messages come from HelloActor and are deserialized
    conn_to_session_tx: mpsc::Sender<(RoomId, TMsg)>,

    /// Receiver for inbound messages from HelloActor
    /// These are raw bytes that need deserialization
    hello_to_conn_rx: Option<mpsc::Receiver<(String, Vec<u8>)>>,
}

impl<TMsg> SessionBridge<TMsg>
where
    TMsg: RoomMessageTrait,
{
    /// Create a new SessionBridge
    ///
    /// # Arguments
    ///
    /// * `peer_id` - Identifier for this peer (for logging)
    /// * `hello_actor` - Address of the HelloActor to send messages to
    /// * `outbound_rx` - Channel from SessionManager with outbound messages
    /// * `conn_to_session_tx` - Channel to SessionManager for inbound messages
    /// * `hello_to_conn_rx` - Channel from HelloActor with raw messages
    pub fn new(
        peer_id: String,
        hello_actor: Addr<HelloActor>,
        outbound_rx: mpsc::Receiver<(RoomId, TMsg)>,
        conn_to_session_tx: mpsc::Sender<(RoomId, TMsg)>,
        hello_to_conn_rx: mpsc::Receiver<(String, Vec<u8>)>,
    ) -> Self {
        Self {
            peer_id,
            hello_actor,
            outbound_rx: Some(outbound_rx),
            conn_to_session_tx,
            hello_to_conn_rx: Some(hello_to_conn_rx),
        }
    }

    /// Spawn task: SessionManager outbound → HelloActor (with serialization)
    fn start_outbound_forwarding(&mut self) {
        let hello_actor = self.hello_actor.clone();
        let peer_id = self.peer_id.clone();

        if let Some(mut outbound_rx) = self.outbound_rx.take() {
            tokio::spawn(async move {
                while let Some((room_id, message)) = outbound_rx.recv().await {
                    // Serialize message using RoomMessageTrait
                    let payload = match message.serialize_inner() {
                        Ok(data) => data,
                        Err(e) => {
                            error!("Failed to serialize message for peer {}: {:?}", peer_id, e);
                            continue;
                        }
                    };

                    // Create SendMessage for HelloActor
                    let send_msg = crate::actor::SendMessage {
                        from_room: room_id.as_str().to_string(),
                        to_room: room_id.as_str().to_string(),
                        payload,
                    };

                    // Send to HelloActor
                    if let Err(e) = hello_actor.send(send_msg).await {
                        error!(
                            "Failed to send outbound message to HelloActor for peer {}: {:?}",
                            peer_id, e
                        );
                        break;
                    }
                }
                debug!("Outbound forwarding task for peer {} completed", peer_id);
            });
        }
    }

    /// Spawn task: HelloActor inbound → SessionManager (with deserialization)
    fn start_inbound_forwarding(&mut self) {
        let peer_id = self.peer_id.clone();
        let conn_to_session_tx = self.conn_to_session_tx.clone();

        if let Some(mut hello_to_conn_rx) = self.hello_to_conn_rx.take() {
            tokio::spawn(async move {
                while let Some((room_name, payload)) = hello_to_conn_rx.recv().await {
                    // Deserialize message using RoomMessageTrait
                    let room_id = RoomId::from(room_name.as_str());
                    match TMsg::deserialize_for_room(&room_id, &payload) {
                        Ok(message) => {
                            // Send to SessionManager
                            if let Err(e) = conn_to_session_tx.try_send((room_id.clone(), message))
                            {
                                error!(
                                    "Failed to send inbound message to SessionManager for peer {}: {:?}",
                                    peer_id, e
                                );
                                break;
                            }
                        }
                        Err(e) => {
                            error!(
                                "Failed to deserialize inbound message for peer {}: {:?}",
                                peer_id, e
                            );
                        }
                    }
                }
                debug!("Inbound forwarding task for peer {} completed", peer_id);
            });
        }
    }
}

impl<TMsg> Actor for SessionBridge<TMsg>
where
    TMsg: RoomMessageTrait + 'static,
{
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        debug!("SessionBridge started for peer {}", self.peer_id);

        // Spawn both forwarding tasks when actor starts
        self.start_outbound_forwarding();
        self.start_inbound_forwarding();
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        debug!("SessionBridge stopped for peer {}", self.peer_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Simple test message enum for SessionBridge tests
    #[derive(Debug, Clone)]
    #[allow(dead_code)]
    enum TestMessages {
        IntentConfig,
        MemDB,
        Health,
    }

    impl RoomMessageTrait for TestMessages {
        fn room_id(&self) -> RoomId {
            match self {
                TestMessages::IntentConfig => RoomId::from("intentconfig"),
                TestMessages::MemDB => RoomId::from("memdb"),
                TestMessages::Health => RoomId::from("health"),
            }
        }

        fn serialize_inner(
            &self,
        ) -> Result<Vec<u8>, zznet_session::room_message_trait::SerializationError> {
            Ok(vec![1, 2, 3]) // Stub for testing
        }

        fn deserialize_for_room(
            _room_id: &RoomId,
            _bytes: &[u8],
        ) -> Result<Self, zznet_session::room_message_trait::DeserializationError> {
            Ok(TestMessages::IntentConfig) // Stub for testing
        }

        fn supported_rooms() -> Vec<RoomId> {
            vec![
                RoomId::from("intentconfig"),
                RoomId::from("memdb"),
                RoomId::from("health"),
            ]
        }
    }

    #[test]
    fn test_session_bridge_creation() {
        // Just verify it compiles and constructs
        let (_outbound_tx, outbound_rx) = mpsc::channel::<(RoomId, TestMessages)>(100);
        let (conn_to_session_tx, _conn_to_session_rx) =
            mpsc::channel::<(RoomId, TestMessages)>(100);
        let (_hello_to_conn_tx, hello_to_conn_rx) = mpsc::channel::<(String, Vec<u8>)>(100);

        // We can't easily test actix actors in unit tests, so just verify compilation
        let _ = (outbound_rx, conn_to_session_tx, hello_to_conn_rx);
    }
}
