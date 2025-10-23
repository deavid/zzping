//! Provides a builder for constructing `CStateActor` instances.

use crate::{actor::CStateActor, role::CStateRole};
use actix::prelude::*;
use std::marker::PhantomData;
use zznet_auth::ApplicationRole;

/// A builder for constructing `CStateActor` instances.
pub struct CStateBuilder<TRole>
where
    TRole: ApplicationRole,
{
    role: CStateRole,
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
            _phantom: PhantomData,
        }
    }

    /// Builds and starts the `CStateActor`.
    pub fn build(self) -> Addr<CStateActor<TRole>> {
        CStateActor::create(|_ctx| CStateActor::new(self.role))
    }
}
