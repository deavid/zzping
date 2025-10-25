//! Provides a builder for constructing `CStateActor` instances.

use crate::{actor::CStateActor, role::CStateRole};
use actix::prelude::*;
use std::marker::PhantomData;
use zznet_auth::ApplicationRole;
use zznet_session::SessionManager;

/// A builder for constructing `CStateActor` instances.
pub struct CStateBuilder<TRole>
where
    TRole: ApplicationRole,
{
    role: CStateRole,
    session_manager: Option<actix::Addr<SessionManager<TRole>>>,
    _phantom: PhantomData<TRole>,
}

impl<TRole> CStateBuilder<TRole>
where
    TRole: ApplicationRole,
{
    /// Creates a new `CStateBuilder`.
    pub fn new(role: CStateRole) -> Self {
        Self {
            role,
            session_manager: None,
            _phantom: PhantomData,
        }
    }

    /// Configure the builder with a SessionManager for auto-registration.
    ///
    /// When a SessionManager is provided, the component's Room<T> will
    /// automatically register with the SessionManager during actor creation,
    /// eliminating the need for manual channel wiring.
    pub fn with_session_manager(
        mut self,
        session_manager: actix::Addr<SessionManager<TRole>>,
    ) -> Self {
        self.session_manager = Some(session_manager);
        self
    }

    /// Builds and starts the `CStateActor`.
    pub fn build(self) -> Addr<CStateActor<TRole>> {
        CStateActor::create(|_ctx| CStateActor::new(self.role, self.session_manager))
    }
}
