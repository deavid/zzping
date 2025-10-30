//! Test utilities for the zzping project.
//!
//! This crate provides small helpers used by unit and integration tests in the
//! workspace. The helpers create in-memory session managers, message capture
//! channels, and minimal room handles so tests can exercise session logic
//! without real network or IO dependencies.
//!
//! These utilities are intentionally lightweight and synchronous-friendly so
//! they can be composed easily in tests.

pub mod message_capture;
