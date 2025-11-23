//! Executes ICMP pings at precise intervals and reports events to MemDB.

pub mod backend;
pub mod builder;
pub mod client;
pub mod messages;
pub mod mock;
pub mod scheduler;
pub mod traits;

#[cfg(test)]
mod tests;
