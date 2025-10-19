//! Provides a builder for constructing `CStateActor` instances.

use crate::{actor::CStateActor, network_messages::CStateMessage, role::CStateRole};
use actix::prelude::*;
use std::{marker::PhantomData, sync::Arc};
use zznet_auth::ApplicationRole;
use zznet_session::{
    room_message_trait::RoomMessageTrait, session_manager_like::SessionManagerLike,
};

/// A builder for constructing `CStateActor` instances.
pub struct CStateBuilder<TMsg, TRole, SM>
where
    TMsg: RoomMessageTrait + Clone + Send + 'static + From<CStateMessage> + Unpin,
    TRole: ApplicationRole,
    SM: SessionManagerLike<TMsg, TRole> + 'static,
{
    role: CStateRole,
    session_manager: Option<Arc<SM>>,
    _phantom: PhantomData<(TMsg, TRole)>,
}

impl<TMsg, TRole, SM> CStateBuilder<TMsg, TRole, SM>
where
    TMsg: RoomMessageTrait + Clone + Send + 'static + From<CStateMessage> + Unpin,
    TRole: ApplicationRole,
    SM: SessionManagerLike<TMsg, TRole> + 'static,
{
    /// Creates a new `CStateBuilder`.
    pub fn new(role: CStateRole) -> Self {
        Self {
            role,
            session_manager: None,
            _phantom: PhantomData,
        }
    }

    /// Sets the session manager for the actor.
    pub fn session_manager(mut self, sm: Arc<SM>) -> Self {
        self.session_manager = Some(sm);
        self
    }

    /// Builds and starts the `CStateActor`.
    pub fn build(self) -> Addr<CStateActor<TMsg, TRole, SM>> {
        CStateActor::create(|_ctx| CStateActor::new(self.role, self.session_manager))
    }
}
