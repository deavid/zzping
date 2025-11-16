//! # zzpinger Component
//!
//! Executes ICMP pings at precise system-clock-aligned intervals and reports events to MemDB.
//! Uses a dedicated scheduler actor and a pool of backend actors for high-precision timing.

pub mod api;
pub mod backend;
pub mod builder;
pub mod messages;
pub mod scheduler;
