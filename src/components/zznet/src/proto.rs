use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum ControlMsg {
    RequestRoom { name: String },
    RoomOpened { name: String, id: u16 },
    CloseRoom { id: u16 },
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DataMsg {
    pub channel_id: u16,
    pub payload: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug)]
pub enum Frame {
    Control(ControlMsg),
    Data(DataMsg),
}
