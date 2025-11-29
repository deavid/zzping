//! Integration test helpers used by the whole-choreography suite.
//!
//! This crate exposes the `SystemHarness`, a reusable fixture that wires up the
//! collector and database stacks with deterministic mock transports so the
//! hermetic tests can script sever/restore scenarios under `tokio::time::pause`.

pub mod harness;
