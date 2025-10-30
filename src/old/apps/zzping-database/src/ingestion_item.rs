//! Types representing an ingestion item to store.

use std::net::IpAddr;
use zzping_lib::protocol::RawDataRecord;

/// A struct that bundles a ping record with the metadata of its source.
#[derive(Debug, Clone)]
pub struct IngestionItem {
    /// Hostname that produced this record.
    pub source_hostname: String,
    /// Target IP that was probed.
    pub target: IpAddr,
    /// The raw ping record payload.
    pub record: RawDataRecord,
}
