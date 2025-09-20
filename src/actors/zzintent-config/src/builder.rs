//! Provides the public builder for creating and starting the IntentConfigActor.

use crate::actor::IntentConfigActor;
use actix::prelude::*;

/// A builder for the IntentConfig component.
///
/// This is the primary public entry point for creating the actor.
/// It follows the 'Builder -> Start' pattern, ensuring that the actor
/// is constructed and started in a controlled manner.
#[derive(Debug, Default)]
pub struct IntentConfigBuilder;

impl IntentConfigBuilder {
    /// Starts the IntentConfigActor and returns its address (`Addr`).
    ///
    /// This method consumes the builder (`self`) to ensure it can only be called once.
    /// It internally creates the `IntentConfigActor` and starts it on the
    /// currently running Actix System.
    ///
    /// The returned `Addr` is the handle to the running actor, used for sending messages.
    pub fn start(self) -> Addr<IntentConfigActor> {
        // In the future, this is where you would pass dependencies (like a
        // zznet handle) that were "wired" to the builder into the actor's `new` function.
        // For now, it's simple.
        IntentConfigActor::start_default()
    }
}
