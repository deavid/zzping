use crate::network_messages::IntentConfigNetworkMsg;
use tokio::sync::mpsc;
use zznet_session::types::{RoomId, SessionError};

/// A minimal RoomHandle implementation used only in tests to provide a room id
/// for PeerSession so joined_rooms can be negotiated. This does not process
/// inbound messages — it's only to satisfy PeerSession invariants.
pub struct DummyRoomHandle {
    id: RoomId,
}

impl DummyRoomHandle {
    pub fn new(id: RoomId) -> Self {
        Self { id }
    }
}

impl zznet_session::peer_session::RoomHandle<IntentConfigNetworkMsg> for DummyRoomHandle {
    fn room_id(&self) -> &RoomId {
        &self.id
    }

    fn send_message(&mut self, _msg: IntentConfigNetworkMsg) -> Result<(), SessionError> {
        // No-op for tests
        Ok(())
    }

    fn spawn_forwarder(
        &mut self,
        _tx: mpsc::Sender<(RoomId, IntentConfigNetworkMsg)>,
    ) -> Result<(), SessionError> {
        // No-op for tests
        Ok(())
    }
}
