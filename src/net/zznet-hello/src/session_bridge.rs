//! An actor that bridges the `HelloActor` with the `RouterActor`.
//!
//! This actor's sole responsibility is to manage the bidirectional message
//! forwarding between the transport-level `HelloActor` and the session-level
//! `RouterActor`. It encapsulates the complexity of wiring together the
//! various channels after a handshake is successfully completed.

use actix::prelude::*;
use tokio::sync::mpsc;
use tracing::{debug, error};

use zznet_api::types::RoomId;

use crate::actor::HelloActor;

/// An actor that manages the bidirectional message forwarding for a single session.
pub(crate) struct SessionBridge {
    peer_id: String,
    hello_actor: Addr<HelloActor>,
    outbound_rx: Option<mpsc::Receiver<(RoomId, Vec<u8>)>>,
    conn_to_session_tx: mpsc::Sender<(RoomId, Vec<u8>)>,
    hello_to_conn_rx: Option<mpsc::Receiver<(String, Vec<u8>)>>,
}

impl SessionBridge {
    /// Creates a new `SessionBridge`.
    pub(crate) fn new(
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

    /// Spawns a task to forward messages from the `RouterActor` to the `HelloActor`.
    fn start_outbound_forwarding(&mut self, ctx: &mut Context<Self>) {
        let hello_actor = self.hello_actor.clone();
        let peer_id = self.peer_id.clone();
        let addr = ctx.address();

        if let Some(mut outbound_rx) = self.outbound_rx.take() {
            tokio::spawn(async move {
                while let Some((room_id, payload)) = outbound_rx.recv().await {
                    debug!(
                        "SessionBridge outbound received message for room {:?}",
                        room_id.as_str()
                    );

                    let send_msg = crate::actor::SendMessage {
                        from_room: room_id.as_str().to_string(),
                        to_room: room_id.as_str().to_string(),
                        payload,
                    };

                    if let Err(e) = hello_actor.send(send_msg).await {
                        error!(
                            "Failed to send outbound message to HelloActor for peer {}: {:?}",
                            peer_id, e
                        );
                        break;
                    }
                }
                debug!("Outbound forwarding task for peer {} completed", peer_id);
                addr.do_send(StopActor);
            });
        }
    }

    /// Spawns a task to forward messages from the `HelloActor` to the `RouterActor`.
    fn start_inbound_forwarding(&mut self, ctx: &mut Context<Self>) {
        let peer_id = self.peer_id.clone();
        let conn_to_session_tx = self.conn_to_session_tx.clone();
        let addr = ctx.address();

        if let Some(mut hello_to_conn_rx) = self.hello_to_conn_rx.take() {
            tokio::spawn(async move {
                while let Some((room_name, payload)) = hello_to_conn_rx.recv().await {
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
                addr.do_send(StopActor);
            });
        }
    }
}

impl Actor for SessionBridge {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        debug!("SessionBridge started for peer {}", self.peer_id);
        self.start_outbound_forwarding(ctx);
        self.start_inbound_forwarding(ctx);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        debug!("SessionBridge stopped for peer {}", self.peer_id);
    }
}

/// A message that signals the actor to stop.
#[derive(Message)]
#[rtype(result = "()")]
struct StopActor;

impl Handler<StopActor> for SessionBridge {
    type Result = ();

    fn handle(&mut self, _: StopActor, ctx: &mut Context<Self>) {
        ctx.stop();
    }
}
