//! Room handler factory for collector components.
//!
//! This module provides a reusable factory pattern for wiring component rooms
//! to the collector's message envelope.

use actix::Addr;
use zzintent_config::actor::IntentConfigActor;
use zzintent_config::permissions::IntentConfigPermission;
use zznet_builder::RoomHandlerFactory;
use zznet_session::peer_session::RoomHandle;
use zznet_session::types::{RoomId, SessionError};
use zzping_auth::AuthRole;

/// Factory for IntentConfig room handlers in the collector.
pub struct IntentConfigRoomHandlerFactory {
    intent_addr: Addr<IntentConfigActor<IntentConfigPermission>>,
}

impl IntentConfigRoomHandlerFactory {
    /// Create a new factory with the IntentConfig actor address.
    pub fn new(intent_addr: Addr<IntentConfigActor<IntentConfigPermission>>) -> Self {
        Self { intent_addr }
    }
}

impl RoomHandlerFactory<AuthRole> for IntentConfigRoomHandlerFactory {
    fn create_handler(&self, room_id: RoomId) -> Box<dyn RoomHandle> {
        Box::new(CollectorIntentConfigRoomHandler {
            intent_addr: self.intent_addr.clone(),
            room_id,
        })
    }
}

/// Wrapper that bridges CollectorMessage → IntentConfigNetworkMsg for the IntentConfig actor.
struct CollectorIntentConfigRoomHandler {
    intent_addr: Addr<IntentConfigActor<IntentConfigPermission>>,
    room_id: RoomId,
}

impl RoomHandle for CollectorIntentConfigRoomHandler {
    fn room_id(&self) -> &RoomId {
        &self.room_id
    }

    fn send_message(&mut self, bytes: Vec<u8>) -> Result<(), SessionError> {
        // Deserialize bytes to IntentConfigNetworkMsg
        let config = bincode::config::standard();
        match bincode::serde::decode_from_slice::<
            zzintent_config::network_messages::IntentConfigNetworkMsg,
            _,
        >(&bytes, config)
        {
            Ok((msg, _)) => {
                // Forward directly to the actor (no wrapper needed)
                self.intent_addr.do_send(msg);
                Ok(())
            }
            Err(e) => {
                log::error!("Failed to deserialize IntentConfigNetworkMsg: {:?}", e);
                Err(SessionError::RoomNotFound {
                    peer_id: zznet_session::types::PeerId::from("unknown"),
                    room_id: self.room_id.clone(),
                })
            }
        }
    }

    fn spawn_forwarder(
        &mut self,
        _tx: tokio::sync::mpsc::Sender<(RoomId, Vec<u8>)>,
    ) -> Result<(), SessionError> {
        // This handler is receive-only
        Ok(())
    }
}
