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

use crate::service::CollectorMessage;

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

impl RoomHandlerFactory<CollectorMessage, AuthRole> for IntentConfigRoomHandlerFactory {
    fn create_handler(&self, room_id: RoomId) -> Box<dyn RoomHandle<CollectorMessage>> {
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

impl RoomHandle<CollectorMessage> for CollectorIntentConfigRoomHandler {
    fn room_id(&self) -> &RoomId {
        &self.room_id
    }

    fn send_message(&mut self, msg: CollectorMessage) -> Result<(), SessionError> {
        match msg {
            CollectorMessage::Intent(intent_msg) => {
                self.intent_addr
                    .do_send(zzintent_config::messages::NetworkMessageReceived(
                        intent_msg,
                    ));
                Ok(())
            }
        }
    }

    fn spawn_forwarder(
        &mut self,
        _tx: tokio::sync::mpsc::Sender<(RoomId, CollectorMessage)>,
    ) -> Result<(), SessionError> {
        // This handler is receive-only
        Ok(())
    }
}
