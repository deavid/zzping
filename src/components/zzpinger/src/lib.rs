//! Pinger component for zzping.
//!
//! This component manages ICMP ping operations for multiple targets with rate limiting,
//! timeout detection, and result submission to zzmem-db.

pub mod actor;
pub mod api;
pub mod builder;
pub mod error;
pub mod messages;
pub mod pinger;
