//! Provides the public builder for creating and starting the IntentConfigActor.

use crate::actor::IntentConfigActor;
use crate::network_messages::IntentConfigMessage;
use crate::permission_wrapper::PermissionWrapper;
use crate::role::IntentConfigRole;
use actix::prelude::*;
use std::rc::Rc;
use zznet_session::session_manager::SessionManager;

use crate::permissions::IntentConfigPermission;
use std::time::Duration;
use zznet_auth::role::ApplicationRole;

/// A builder for the IntentConfig component.
///
/// This is the primary public entry point for creating the actor.
/// It follows the 'Builder -> Start' pattern, ensuring that the actor
/// is constructed and started in a controlled manner.
///
/// # Example
///
pub struct IntentConfigBuilder<T: ApplicationRole + std::fmt::Debug = IntentConfigPermission> {
    role: IntentConfigRole,
    session_manager: Option<Rc<SessionManager<IntentConfigMessage, PermissionWrapper<T>>>>,
    /// Per-peer broadcast timeout used when sending messages via SessionManager
    broadcast_timeout: Duration,
}

impl IntentConfigBuilder<IntentConfigPermission> {
    /// Create a new builder with default configuration for the common
    /// `IntentConfigPermission` role type.
    ///
    /// Default role is Database (passive receiver).
    pub fn new() -> Self {
        Self {
            role: IntentConfigRole::default(),
            session_manager: None,
            broadcast_timeout: Duration::from_millis(500),
        }
    }
}

impl Default for IntentConfigBuilder<IntentConfigPermission> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T: ApplicationRole + std::fmt::Debug> IntentConfigBuilder<T> {
    /// Set the role for this IntentConfig actor
    pub fn role(mut self, role: IntentConfigRole) -> Self {
        self.role = role;
        self
    }

    /// Set the SessionManager for network communication
    pub fn session_manager(
        mut self,
        session_manager: SessionManager<IntentConfigMessage, PermissionWrapper<T>>,
    ) -> Self {
        self.session_manager = Some(Rc::new(session_manager));
        self
    }

    /// Set the per-peer broadcast timeout used when sending network messages.
    /// Default is 500ms.
    pub fn broadcast_timeout(mut self, timeout: Duration) -> Self {
        self.broadcast_timeout = timeout;
        self
    }

    /// Get the current role configuration
    pub fn get_role(&self) -> &IntentConfigRole {
        &self.role
    }

    /// Starts the IntentConfigActor and returns its address (`Addr`).
    ///
    /// This method consumes the builder (`self`) to ensure it can only be called once.
    /// It internally creates the `IntentConfigActor` and starts it on the
    /// currently running Actix System.
    ///
    /// # Errors
    ///
    /// Returns an error if role validation fails (e.g., Collector with empty file path).
    ///
    /// # Returns
    ///
    /// The returned `Addr` is the handle to the running actor, used for sending messages.
    pub fn start(mut self) -> anyhow::Result<Addr<IntentConfigActor<T>>> {
        // Validate role configuration
        self.role
            .validate()
            .map_err(|e| anyhow::anyhow!("Invalid role configuration: {}", e))?;

        // Create and start actor with role
        let mut actor = IntentConfigActor::new_with_role(self.role);

        // Configure broadcast timeout on actor
        actor.set_broadcast_timeout(self.broadcast_timeout);

        // Set session manager if provided
        if let Some(session_manager) = self.session_manager.take() {
            actor.set_session_manager(session_manager);
        }

        Ok(actor.start())
    }
}
