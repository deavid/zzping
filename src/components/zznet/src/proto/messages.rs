use serde::{Deserialize, Serialize};

pub type ChannelId = u16;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Frame {
    Control(ControlMsg),
    Data(DataMsg),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ControlMsg {
    Hello(super::hello::Hello),
    RequestChannel { name: String },
    ChannelOpened { name: String, id: ChannelId },
    CloseChannel { id: ChannelId },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DataMsg {
    pub channel_id: ChannelId,
    pub payload: Vec<u8>,
}
