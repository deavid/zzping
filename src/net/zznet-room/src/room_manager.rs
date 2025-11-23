//! RoomManager trait for component-provided room factories.
//!
//! Components implement this trait to provide Room<T> instances per peer.
//! Router orchestrates creation and enforces strict 1:1 room↔component mapping.
