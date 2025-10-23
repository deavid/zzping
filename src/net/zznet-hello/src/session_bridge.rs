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

use zznet_session::types::RoomId;

use crate::actor::HelloActor;

/// SessionBridge actor - Manages bidirectional message forwarding
///
/// No longer generic - works directly with serialized bytes (Vec<u8>)
/// since Room<T> handles serialization at the component level
pub struct SessionBridge {
    /// Peer identifier for logging
    peer_id: String,

    /// Reference to HelloActor for sending outbound messages
    hello_actor: Addr<HelloActor>,

    /// Receiver for outbound messages from SessionManager
    /// These messages are already serialized by Room<T>
    outbound_rx: Option<mpsc::Receiver<(RoomId, Vec<u8>)>>,

    /// Sender for inbound messages to SessionManager
    /// These messages are raw bytes that will be deserialized by Room<T>
    conn_to_session_tx: mpsc::Sender<(RoomId, Vec<u8>)>,

    /// Receiver for inbound messages from HelloActor
    /// These are raw bytes that will be forwarded to SessionManager
    hello_to_conn_rx: Option<mpsc::Receiver<(String, Vec<u8>)>>,
}

impl SessionBridge {
    /// Create a new SessionBridge
    ///
    /// # Arguments
    ///
    /// * `peer_id` - Identifier for this peer (for logging)
    /// * `hello_actor` - Address of the HelloActor to send messages to
    /// * `outbound_rx` - Channel from SessionManager with serialized outbound messages
    /// * `conn_to_session_tx` - Channel to SessionManager for serialized inbound messages
    /// * `hello_to_conn_rx` - Channel from HelloActor with raw messages
    pub fn new(
        peer_id: String,
        hello_actor: Addr<HelloActor>,
        outbound_rx: mpsc::Receiver<(RoomId, Vec<u8>)>,
        conn_to_session_tx: mpsc::Sender<(RoomId, Vec<u8>)>,
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

    /// Spawn task: SessionManager outbound → HelloActor (already serialized)
    fn start_outbound_forwarding(&mut self) {
        let hello_actor = self.hello_actor.clone();
        let peer_id = self.peer_id.clone();

        if let Some(mut outbound_rx) = self.outbound_rx.take() {
            tokio::spawn(async move {
                while let Some((room_id, payload)) = outbound_rx.recv().await {
                    debug!(
                        "SessionBridge outbound received message for room {:?}",
                        room_id.as_str()
                    );

                    // Messages are already serialized by Room<T>, just forward them
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

    /// Spawn task: HelloActor inbound → SessionManager (no deserialization needed)
    fn start_inbound_forwarding(&mut self) {
        let peer_id = self.peer_id.clone();
        let conn_to_session_tx = self.conn_to_session_tx.clone();

        if let Some(mut hello_to_conn_rx) = self.hello_to_conn_rx.take() {
            tokio::spawn(async move {
                while let Some((room_name, payload)) = hello_to_conn_rx.recv().await {
                    // Just forward the raw bytes - Room<T> will deserialize
                    let room_id = RoomId::from(room_name.as_str());
                    if let Err(e) = conn_to_session_tx.try_send((room_id.clone(), payload)) {
                        error!(
                            "Failed to send inbound message to SessionManager for peer {}: {:?}",
                            peer_id, e
                        );
                        break;
                    }
                }
                debug!("Inbound forwarding task for peer {} completed", peer_id);
            });
        }
    }
}

impl Actor for SessionBridge {
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
