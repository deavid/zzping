pub mod frame;

use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Role {
    Collector,
    Database,
    ClientRo,
    ClientAdmin,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Hello {
    pub role: Role,
}
