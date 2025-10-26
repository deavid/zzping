//! Provides a builder for constructing `CStateActor` instances.

use crate::{actor::CStateActor, role::CStateRole};
use actix::prelude::*;
use zznet_session::SessionManager;

/// A builder for constructing `CStateActor` instances.
pub struct CStateBuilder {
    role: CStateRole,
    session_manager: Option<actix::Addr<SessionManager>>,
}

impl CStateBuilder {
    /// Creates a new `CStateBuilder`.
    pub fn new(role: CStateRole) -> Self {
        Self {
            role,
            session_manager: None,
        }
    }

    /// Configure the builder with a SessionManager for auto-registration.
    ///
    /// When a SessionManager is provided, the component's Room<T> will
    /// automatically register with the SessionManager during actor creation,
    /// eliminating the need for manual channel wiring.
    pub fn with_session_manager(mut self, session_manager: actix::Addr<SessionManager>) -> Self {
        self.session_manager = Some(session_manager);
        self
    }

    /// Builds and starts the `CStateActor`.
    pub fn build(self) -> Addr<CStateActor> {
        CStateActor::create(|_ctx| CStateActor::new(self.role, self.session_manager))
    }
}
