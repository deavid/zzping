//! Contains the `MockTransportConnectionActor`, which simulates a single,
//! bidirectional network connection using in-memory channels.

use crate::actor::{FrameForTransport, FrameFromTransport, ZzNetConnActor};
use actix::prelude::*;
use tokio::sync::mpsc;

// --- Mock Connection Actor Implementation ---

/// An ephemeral actor simulating a single, raw, framed connection.
///
/// It uses two MPSC channels to create a full-duplex link: one for incoming data
/// and one for outgoing data. It forwards data between a `ZzNetConnActor` and a
/// test harness.
pub struct MockTransportConnectionActor {
    /// The sending half for data going TO the other side of the connection (i.e., the test harness).
    tx: mpsc::Sender<Vec<u8>>,
    /// The receiving half for data coming FROM the other side of the connection.
    rx: Option<mpsc::Receiver<Vec<u8>>>,
    /// The address of the `ZzNetConnActor` this transport is serving.
    peer: Addr<ZzNetConnActor>,
    /// Optional frame capture for testing. When present, all sent frames are recorded.
    pub sent_frames: Option<Vec<Vec<u8>>>,
}

impl MockTransportConnectionActor {
    /// Creates a new mock connection actor.
    ///
    /// # Arguments
    /// * `tx` - The channel sender to write outgoing data to.
    /// * `rx` - The channel receiver to read incoming data from.
    /// * `peer` - The address of the ZzNetConnActor this transport serves.
    pub fn new(
        tx: mpsc::Sender<Vec<u8>>,
        rx: mpsc::Receiver<Vec<u8>>,
        peer: Addr<ZzNetConnActor>,
    ) -> Self {
        Self {
            tx,
            rx: Some(rx),
            peer,
            sent_frames: None,
        }
    }

    /// Creates a new mock connection actor with frame capture enabled.
    /// This is useful for tests that need to inspect what frames were sent.
    pub fn new_with_capture(
        tx: mpsc::Sender<Vec<u8>>,
        rx: mpsc::Receiver<Vec<u8>>,
        peer: Addr<ZzNetConnActor>,
    ) -> Self {
        Self {
            tx,
            rx: Some(rx),
            peer,
            sent_frames: Some(Vec::new()),
        }
    }

    /// Get a copy of all captured frames (if capture is enabled).
    pub fn get_sent_frames(&self) -> Vec<Vec<u8>> {
        self.sent_frames.clone().unwrap_or_default()
    }
}

impl Actor for MockTransportConnectionActor {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        // Take ownership of the receiver and spawn a task to poll it.
        // This task will forward messages from the channel "up" to the peer actor.
        let mut rx = self.rx.take().unwrap();
        let peer = self.peer.clone();

        ctx.spawn(
            async move {
                while let Some(data) = rx.recv().await {
                    // Send the frame directly to the peer actor.
                    peer.do_send(FrameFromTransport(data));
                }
                // The channel was closed. This simulates the remote peer disconnecting.
                // Stop the actor.
                log::debug!("Mock transport channel closed. Stopping connection actor.");
            }
            .into_actor(self),
        );
    }
}

/// A message to gracefully stop the actor.
#[derive(Message)]
#[rtype(result = "()")]
pub struct PoisonPill;

impl Handler<PoisonPill> for MockTransportConnectionActor {
    type Result = ();

    fn handle(&mut self, _msg: PoisonPill, ctx: &mut Context<Self>) {
        ctx.stop();
    }
}

/// Handles frames coming "down" from the `ZzNetConnActor` to be sent "out".
impl Handler<FrameForTransport> for MockTransportConnectionActor {
    type Result = ();

    fn handle(&mut self, msg: FrameForTransport, ctx: &mut Context<Self>) {
        // If frame capture is enabled, record the frame
        if let Some(ref mut frames) = self.sent_frames {
            frames.push(msg.0.clone());
        }

        // Spawn a task to send the frame asynchronously.
        let tx = self.tx.clone();
        ctx.spawn(
            async move {
                if let Err(e) = tx.send(msg.0).await {
                    log::warn!("Failed to send frame to mock transport channel: {}", e);
                }
            }
            .into_actor(self),
        );
    }
}

/// A simple mock transport actor that just captures frames without channels.
/// Used for creating dummy connections in tests.
#[derive(Default)]
pub struct SimpleMockTransportActor {
    pub sent_frames: Vec<Vec<u8>>,
}

impl Actor for SimpleMockTransportActor {
    type Context = Context<Self>;
}

impl Handler<FrameForTransport> for SimpleMockTransportActor {
    type Result = ();

    fn handle(&mut self, msg: FrameForTransport, _ctx: &mut Context<Self>) {
        self.sent_frames.push(msg.0);
    }
}
