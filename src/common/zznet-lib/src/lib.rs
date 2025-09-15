// lib.rs
mod client;
mod config;
mod facade;
mod server;

pub use config::ZzNetConfig;
pub use facade::{ActorCommand, ZzNet, ZzNetBuilder, ZzNetHandle, ZzNetManager};
