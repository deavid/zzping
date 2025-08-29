use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RawDataRecord {
    pub sent_nanos: u64,
    pub rtt_nanos: u64,
}
