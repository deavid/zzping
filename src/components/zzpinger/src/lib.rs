//! # zzpinger Component
//!
//! Executes ICMP pings at precise system-clock-aligned intervals and reports events to MemDB.
//! Uses a dedicated scheduler actor and a pure-Tokio async backend for high-precision timing.

pub mod backend;
pub mod builder;
pub mod client;
pub mod messages;
pub mod mock;
pub mod scheduler;
pub mod traits;

#[cfg(test)]
mod tests;
