//! Room handler factory for database components.
//!
//! This module provides a reusable factory pattern for wiring component rooms
//! to the database's message envelope.

use actix::Addr;
use zzcollector_state::actor::CStateActor;
use zzintent_config::actor::IntentConfigActor;
use zzintent_config::permissions::IntentConfigPermission;
use zzmem_db::actor::MemDBActor;
use zzmem_db::permissions::MemDBPermission;
use zznet_builder::RoomHandlerFactory;
use zznet_session::peer_session::RoomHandle;
use zznet_session::types::{RoomId, SessionError};
use zzping_auth::AuthRole;

use crate::service::DatabaseMessage;

/// Factory for IntentConfig room handlers in the database.
pub struct IntentConfigRoomHandlerFactory {
    intent_addr: Addr<IntentConfigActor<IntentConfigPermission>>,
}

impl IntentConfigRoomHandlerFactory {
    /// Create a new factory with the IntentConfig actor address.
    pub fn new(intent_addr: Addr<IntentConfigActor<IntentConfigPermission>>) -> Self {
        Self { intent_addr }
    }
}

impl RoomHandlerFactory<DatabaseMessage, AuthRole> for IntentConfigRoomHandlerFactory {
    fn create_handler(&self, room_id: RoomId) -> Box<dyn RoomHandle> {
        Box::new(DatabaseIntentConfigRoomHandler {
            intent_addr: self.intent_addr.clone(),
            room_id,
        })
    }
}

/// Wrapper that bridges DatabaseMessage → IntentConfigNetworkMsg for the IntentConfig actor.
struct DatabaseIntentConfigRoomHandler {
    intent_addr: Addr<IntentConfigActor<IntentConfigPermission>>,
    room_id: RoomId,
}

impl RoomHandle for DatabaseIntentConfigRoomHandler {
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

/// Factory for MemDB room handlers in the database.
pub struct MemDBRoomHandlerFactory {
    memdb_addr: Addr<MemDBActor<MemDBPermission>>,
}

impl MemDBRoomHandlerFactory {
    /// Create a new factory with the MemDB actor address.
    pub fn new(memdb_addr: Addr<MemDBActor<MemDBPermission>>) -> Self {
        Self { memdb_addr }
    }
}

impl RoomHandlerFactory<DatabaseMessage, AuthRole> for MemDBRoomHandlerFactory {
    fn create_handler(&self, room_id: RoomId) -> Box<dyn RoomHandle> {
        Box::new(DatabaseMemDBRoomHandler {
            memdb_addr: self.memdb_addr.clone(),
            room_id,
        })
    }
}

/// Wrapper that bridges DatabaseMessage → MemDBMessage for the MemDB actor.
struct DatabaseMemDBRoomHandler {
    #[allow(dead_code)]
    memdb_addr: Addr<MemDBActor<MemDBPermission>>,
    room_id: RoomId,
}

impl RoomHandle for DatabaseMemDBRoomHandler {
    fn room_id(&self) -> &RoomId {
        &self.room_id
    }

    fn send_message(&mut self, bytes: Vec<u8>) -> Result<(), SessionError> {
        // Deserialize bytes to MemDBMessage
        let config = bincode::config::standard();
        match bincode::serde::decode_from_slice::<zzmem_db::network_messages::MemDBMessage, _>(
            &bytes, config,
        ) {
            Ok((msg, _)) => {
                // Forward directly to the actor
                self.memdb_addr.do_send(msg);
                Ok(())
            }
            Err(e) => {
                log::error!("Failed to deserialize MemDBMessage: {:?}", e);
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

/// Factory for CState (Collector State) room handlers in the database.
pub struct CStateRoomHandlerFactory {
    cstate_addr: Addr<CStateActor<AuthRole>>,
}

impl CStateRoomHandlerFactory {
    /// Create a new factory with the CState actor address.
    pub fn new(cstate_addr: Addr<CStateActor<AuthRole>>) -> Self {
        Self { cstate_addr }
    }
}

impl RoomHandlerFactory<DatabaseMessage, AuthRole> for CStateRoomHandlerFactory {
    fn create_handler(&self, room_id: RoomId) -> Box<dyn RoomHandle> {
        Box::new(DatabaseCStateRoomHandler {
            cstate_addr: self.cstate_addr.clone(),
            room_id,
        })
    }
}

/// Wrapper that bridges DatabaseMessage → CStateMessage for the CState actor.
struct DatabaseCStateRoomHandler {
    #[allow(dead_code)]
    cstate_addr: Addr<CStateActor<AuthRole>>,
    room_id: RoomId,
}

impl RoomHandle for DatabaseCStateRoomHandler {
    fn room_id(&self) -> &RoomId {
        &self.room_id
    }

    fn send_message(&mut self, bytes: Vec<u8>) -> Result<(), SessionError> {
        // Deserialize bytes to CStateMessage
        let config = bincode::config::standard();
        match bincode::serde::decode_from_slice::<
            zzcollector_state::network_messages::CStateMessage,
            _,
        >(&bytes, config)
        {
            Ok((msg, _)) => {
                // Wrap the message with peer_id (using placeholder for now since RoomHandle doesn't provide peer context)
                let wrapped = zzcollector_state::messages::WrappedCStateMessage {
                    peer_id: zznet_session::types::PeerId::from("unknown"),
                    message: msg,
                };
                self.cstate_addr.do_send(wrapped);
                Ok(())
            }
            Err(e) => {
                log::error!("Failed to deserialize CStateMessage: {:?}", e);
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
