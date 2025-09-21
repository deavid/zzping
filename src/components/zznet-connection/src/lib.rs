//! Manages the `zznet` session protocol and acts as a multiplexer for logical "Rooms".
//!
//! ## Crate Overview
//!
//! This crate is the central protocol engine of the `zznet` networking stack. Its primary
//! responsibility is to bridge a raw, frame-based **Transport Layer** (which knows only
//! about bytes) with a logical, application-oriented **Room Layer** (which knows about
//! services like "intent-config").
//!
//! It takes established transport connections, performs a mandatory two-phase protocol
//! to validate the connection and discover services, and then multiplexes data for
//! different services over that single connection.
//!
//! ## The Problem it Solves
//!
//! Building a reliable, multi-service communication layer on top of a single network
//! connection is complex. This crate solves several key problems:
//!
//! 1.  **Protocol State Management:** It encapsulates the complex, stateful logic of
//!     the symmetric `zznet` handshake and room discovery protocols, ensuring connections
//!     are properly validated before any application data is exchanged.
//! 2.  **Service Discovery:** It provides a subscription-based mechanism for other
//!     components to learn when a logical "Room" (e.g., a specific service) becomes
//!     available on a network connection.
//! 3.  **Multiplexing:** It acts as a central switchboard, routing incoming data frames
//!     from a single transport to the many different application components that are
//!     listening for room-specific data, and vice-versa for outgoing data.
//! 4.  **Lifecycle Management:** It cleanly manages the lifecycle of a connection, providing
//!     explicit, cascading termination signals to all dependent components when the
//!     underlying transport is lost. This prevents ambiguity and reliance on implicit
//!     error handling for lifecycle events.
//! 5.  **Race Condition Prevention:** It solves the "late subscriber" problem by
//!     dynamically re-publishing the list of available rooms when the set of local
//!     subscribers changes, guaranteeing that components will be notified of all
//!     available services regardless of startup order.
//!
//! ## Architecture
//!
//! The crate is built around two primary actor roles:
//!
//! *   **`ZzNetConnManager` (The Orchestrator):** A long-lived, singleton actor that
//!     serves as the public facade for the crate. It manages room subscriptions and
//!     spawns an ephemeral `ZzNetConnActor` for each new transport connection.
//!
//! *   **`ZzNetConnActor` (The Worker):** An ephemeral actor whose lifetime is tied
//!     1:1 with an underlying transport connection. It is responsible for executing
//!     the handshake and room discovery state machines and then performing the
//!     active multiplexing of data frames.
//!
//! By depending on this crate, higher-level components can interact with a clean,
//! abstract, and robust bus for network services without ever needing to know the
//! details of protocol handshakes or data framing.

pub mod actor;
pub mod bus;
pub mod manager;
pub mod mocks;
pub mod protocol;

#[cfg(test)]
pub mod tests;
