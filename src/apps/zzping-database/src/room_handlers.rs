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
use zznet_session::session_manager::SessionManager;
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
    fn create_handler(&self, room_id: RoomId) -> Box<dyn RoomHandle<DatabaseMessage>> {
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

impl RoomHandle<DatabaseMessage> for DatabaseIntentConfigRoomHandler {
    fn room_id(&self) -> &RoomId {
        &self.room_id
    }

    fn send_message(&mut self, msg: DatabaseMessage) -> Result<(), SessionError> {
        match msg {
            DatabaseMessage::Intent(intent_msg) => {
                self.intent_addr
                    .do_send(zzintent_config::messages::NetworkMessageReceived(
                        intent_msg,
                    ));
                Ok(())
            }
            DatabaseMessage::MemDB(_) => Ok(()),
            DatabaseMessage::CState(_) => Ok(()),
        }
    }

    fn spawn_forwarder(
        &mut self,
        _tx: tokio::sync::mpsc::Sender<(RoomId, DatabaseMessage)>,
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
    fn create_handler(&self, room_id: RoomId) -> Box<dyn RoomHandle<DatabaseMessage>> {
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

impl RoomHandle<DatabaseMessage> for DatabaseMemDBRoomHandler {
    fn room_id(&self) -> &RoomId {
        &self.room_id
    }

    fn send_message(&mut self, msg: DatabaseMessage) -> Result<(), SessionError> {
        match msg {
            DatabaseMessage::MemDB(_memdb_msg) => {
                // TODO: Forward MemDB messages to the actor when network integration is complete
                tracing::debug!("MemDB room handler received message");
                Ok(())
            }
            DatabaseMessage::Intent(_) => Ok(()),
            DatabaseMessage::CState(_) => Ok(()),
        }
    }

    fn spawn_forwarder(
        &mut self,
        _tx: tokio::sync::mpsc::Sender<(RoomId, DatabaseMessage)>,
    ) -> Result<(), SessionError> {
        // This handler is receive-only
        Ok(())
    }
}

/// Factory for CState (Collector State) room handlers in the database.
#[allow(clippy::type_complexity)]
pub struct CStateRoomHandlerFactory {
    cstate_addr: Addr<
        CStateActor<
            DatabaseMessage,
            AuthRole,
            tokio::sync::Mutex<SessionManager<DatabaseMessage, AuthRole>>,
        >,
    >,
}

#[allow(clippy::type_complexity)]
impl CStateRoomHandlerFactory {
    /// Create a new factory with the CState actor address.
    pub fn new(
        cstate_addr: Addr<
            CStateActor<
                DatabaseMessage,
                AuthRole,
                tokio::sync::Mutex<SessionManager<DatabaseMessage, AuthRole>>,
            >,
        >,
    ) -> Self {
        Self { cstate_addr }
    }
}

impl RoomHandlerFactory<DatabaseMessage, AuthRole> for CStateRoomHandlerFactory {
    fn create_handler(&self, room_id: RoomId) -> Box<dyn RoomHandle<DatabaseMessage>> {
        Box::new(DatabaseCStateRoomHandler {
            cstate_addr: self.cstate_addr.clone(),
            room_id,
        })
    }
}

/// Wrapper that bridges DatabaseMessage → CStateMessage for the CState actor.
#[allow(clippy::type_complexity)]
struct DatabaseCStateRoomHandler {
    #[allow(dead_code)]
    cstate_addr: Addr<
        CStateActor<
            DatabaseMessage,
            AuthRole,
            tokio::sync::Mutex<SessionManager<DatabaseMessage, AuthRole>>,
        >,
    >,
    room_id: RoomId,
}

impl RoomHandle<DatabaseMessage> for DatabaseCStateRoomHandler {
    fn room_id(&self) -> &RoomId {
        &self.room_id
    }

    fn send_message(&mut self, msg: DatabaseMessage) -> Result<(), SessionError> {
        match msg {
            DatabaseMessage::CState(_cstate_msg) => {
                // TODO: Forward CState messages to the actor when network integration is complete
                tracing::debug!("CState room handler received message");
                Ok(())
            }
            DatabaseMessage::Intent(_) => Ok(()),
            DatabaseMessage::MemDB(_) => Ok(()),
        }
    }

    fn spawn_forwarder(
        &mut self,
        _tx: tokio::sync::mpsc::Sender<(RoomId, DatabaseMessage)>,
    ) -> Result<(), SessionError> {
        // This handler is receive-only
        Ok(())
    }
}
