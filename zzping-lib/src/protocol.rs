use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// The fundamental data structure for a single ping measurement.
/// This is the canonical format for data transfer between the collector and the database.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct RawDataRecord {
    /// The timestamp when the ping was sent, as a UNIX timestamp in nanoseconds.
    pub sent_nanos: u64,
    /// The round-trip time in nanoseconds. If the packet was lost, this will be `u64::MAX`.
    pub rtt_nanos: u64,
}

/// Writes a length-prefixed, JSON-serialized `RawDataRecord` to an async writer.
///
/// The protocol is:
/// - 4 bytes: `u32` length of the JSON payload, big-endian.
/// - N bytes: JSON payload.
pub async fn write_record<W: AsyncWriteExt + Unpin>(
    stream: &mut W,
    record: &RawDataRecord,
) -> Result<()> {
    let json_data = serde_json::to_vec(record)?;
    stream.write_u32(json_data.len() as u32).await?;
    stream.write_all(&json_data).await?;
    Ok(())
}

/// Reads a length-prefixed, JSON-serialized `RawDataRecord` from an async reader.
///
/// Returns `Ok(None)` if the stream is closed cleanly (EOF).
pub async fn read_record<R: AsyncReadExt + Unpin>(stream: &mut R) -> Result<Option<RawDataRecord>> {
    let len = match stream.read_u32().await {
        Ok(len) => len,
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e.into()),
    };

    if len == 0 {
        return Ok(None);
    }

    let mut buffer = vec![0; len as usize];
    stream.read_exact(&mut buffer).await?;
    let record: RawDataRecord = serde_json::from_slice(&buffer)?;
    Ok(Some(record))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[tokio::test]
    async fn test_protocol_roundtrip() {
        let record = RawDataRecord {
            sent_nanos: 12345,
            rtt_nanos: 67890,
        };

        // 1. Write the record to a buffer
        let mut buffer = Vec::new();
        write_record(&mut buffer, &record).await.unwrap();

        // 2. Read the record back from the buffer
        let mut cursor = Cursor::new(buffer);
        let received_record = read_record(&mut cursor).await.unwrap().unwrap();

        // 3. Assert they are the same
        assert_eq!(record, received_record);
    }

    #[tokio::test]
    async fn test_read_empty_stream() {
        let mut empty: &[u8] = &[];
        let result = read_record(&mut empty).await.unwrap();
        assert!(result.is_none());
    }
}
