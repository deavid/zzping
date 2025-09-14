// lib.rs
mod client;
mod config;
mod facade;
mod server;

pub use config::ZzNetConfig;
pub use facade::ZzNet;

pub type ListenerMap = std::collections::HashMap<
    String,
    tokio::sync::mpsc::Sender<(u64, Box<dyn zznet_api::ZzChannel>)>,
>;
