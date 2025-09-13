use zznet::connection::{ClientConfig, ServerConfig};

pub enum ZzNetConfig {
    Client(ClientConfig),
    Server(ServerConfig),
}
