use std::net::IpAddr;
use zzping_lib::protocol::RawDataRecord;

/// A struct that bundles a ping record with the metadata of its source.
#[derive(Debug, Clone)]
pub struct IngestionItem {
    pub source_hostname: String,
    pub target: IpAddr,
    pub record: RawDataRecord,
}
