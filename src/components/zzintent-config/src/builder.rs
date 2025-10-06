//! Provides the public builder for creating and starting the IntentConfigActor.

use crate::actor::IntentConfigActor;
use crate::network_messages::IntentConfigMessage;
use crate::role::IntentConfigRole;
use actix::prelude::*;
use std::rc::Rc;
use zznet_session::session_manager::SessionManager;
use zzping_auth::AuthRole;

/// A builder for the IntentConfig component.
///
/// This is the primary public entry point for creating the actor.
/// It follows the 'Builder -> Start' pattern, ensuring that the actor
/// is constructed and started in a controlled manner.
///
/// # Example
///
pub struct IntentConfigBuilder {
    role: IntentConfigRole,
    session_manager: Option<Rc<SessionManager<IntentConfigMessage, AuthRole>>>,
}

impl Default for IntentConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl IntentConfigBuilder {
    /// Create a new builder with default configuration
    ///
    /// Default role is Database (passive receiver).
    pub fn new() -> Self {
        Self {
            role: IntentConfigRole::default(),
            session_manager: None,
        }
    }

    /// Set the role for this IntentConfig actor
    pub fn role(mut self, role: IntentConfigRole) -> Self {
        self.role = role;
        self
    }

    /// Set the SessionManager for network communication
    pub fn session_manager(
        mut self,
        session_manager: SessionManager<IntentConfigMessage, AuthRole>,
    ) -> Self {
        self.session_manager = Some(Rc::new(session_manager));
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
    /// # Panics
    ///
    /// Panics if role validation fails (e.g., Collector with empty file path).
    ///
    /// # Returns
    ///
    /// The returned `Addr` is the handle to the running actor, used for sending messages.
    pub fn start(mut self) -> Addr<IntentConfigActor> {
        // Validate role configuration
        self.role.validate().expect("Invalid role configuration");

        // Create and start actor with role
        let mut actor = IntentConfigActor::new_with_role(self.role);

        // Set session manager if provided
        if let Some(session_manager) = self.session_manager.take() {
            actor.set_session_manager(session_manager);
        }

        actor.start()
    }
}
