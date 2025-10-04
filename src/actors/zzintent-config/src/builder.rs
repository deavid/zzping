//! Provides the public builder for creating and starting the IntentConfigActor.

use crate::actor::IntentConfigActor;
use crate::role::IntentConfigRole;
use actix::prelude::*;

/// A builder for the IntentConfig component.
///
/// This is the primary public entry point for creating the actor.
/// It follows the 'Builder -> Start' pattern, ensuring that the actor
/// is constructed and started in a controlled manner.
///
/// # Example
///
/// ```ignore
/// use zzintent_config::builder::IntentConfigBuilder;
/// use zzintent_config::role::IntentConfigRole;
/// use std::path::PathBuf;
///
/// // Collector role
/// let collector = IntentConfigBuilder::new()
///     .role(IntentConfigRole::Collector {
///         config_file_path: PathBuf::from("/etc/intent.ron"),
///     })
///     .start();
///
/// // Database role
/// let database = IntentConfigBuilder::new()
///     .role(IntentConfigRole::Database)
///     .start();
/// ```
#[derive(Debug)]
pub struct IntentConfigBuilder {
    role: IntentConfigRole,
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
        }
    }

    /// Set the role for this IntentConfig actor
    ///
    /// # Example
    /// ```ignore
    /// use zzintent_config::builder::IntentConfigBuilder;
    /// use zzintent_config::role::IntentConfigRole;
    /// use std::path::PathBuf;
    ///
    /// let builder = IntentConfigBuilder::new()
    ///     .role(IntentConfigRole::Collector {
    ///         config_file_path: PathBuf::from("/etc/intent.ron"),
    ///     });
    /// ```
    pub fn role(mut self, role: IntentConfigRole) -> Self {
        self.role = role;
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
    pub fn start(self) -> Addr<IntentConfigActor> {
        // Validate role configuration
        self.role.validate().expect("Invalid role configuration");

        // Create and start actor with role
        IntentConfigActor::new_with_role(self.role).start()
    }
}
