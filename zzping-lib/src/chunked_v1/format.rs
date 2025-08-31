//! Defines the raw data structures and constants for the `chunked_v1` file format.
//!
//! This module contains the low-level building blocks of the file format, such as
//! file headers, chunk headers, and index entries. The structures are designed for
//! direct serialization to and from disk. The primary design goal is compactness
//! and efficiency for append-only writing.

use anyhow::{Result, anyhow};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Read, Write};

// --- Validation Functions ---

/// Calculates the CRC32 checksum for the file header.
///
/// The checksum is calculated over all fields of the `FileHeader` except for the
/// `header_crc32` field itself. This allows for verification of header integrity.
pub fn calculate_file_header_crc32(header: &FileHeader) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&header.magic.to_be_bytes());
    hasher.update(&header.format_version.to_be_bytes());
    hasher.update(&header.start_time_unix_ns.to_be_bytes());
    hasher.update(&header.aggregate_entry_count.to_be_bytes());
    hasher.update(&header.index_entry_count.to_be_bytes());
    hasher.finalize()
}

/// Calculates the CRC32 checksum for a chunk header.
///
/// The checksum includes all fields of the `ChunkHeader` except for `chunk_crc32`.
/// This is used to ensure that chunk metadata has not been corrupted.
pub(super) fn calculate_chunk_header_crc32(header: &ChunkHeader) -> u32 {
    let mut hasher = crc32fast::Hasher::new();
    hasher.update(&header.minute_boundary_unix_ns.to_be_bytes());
    hasher.update(&header.first_ping_offset_ns.to_be_bytes());
    hasher.update(&header.rtt_symbol_count.to_be_bytes());
    hasher.update(&header.send_time_symbol_count.to_be_bytes());
    hasher.update(&header.rtt_stream_len_bytes.to_be_bytes());
    hasher.update(&header.send_time_stream_len_bytes.to_be_bytes());
    hasher.update(&[header.flags.bits()]);
    hasher.update(&header.base_interval_ns.to_be_bytes());

    let mut stats_buf = Vec::new();
    header.rtt_stats.write(&mut stats_buf).unwrap();
    hasher.update(&stats_buf);

    if let Some(ref stats) = header.send_time_stats {
        let mut stats_buf = Vec::new();
        stats.write(&mut stats_buf).unwrap();
        hasher.update(&stats_buf);
    }
    hasher.finalize()
}

// --- File Format Constants and Structs ---

/// The magic number used to identify a `chunked_v1` file ("zzPCV1  ").
pub const FILE_MAGIC: u64 = 0x5A5A504356312020;
/// The version number of the `chunked_v1` format.
pub const FORMAT_VERSION: u16 = 1;
/// The fixed size of the header section in bytes (64 KiB).
/// This space is reserved for the `FileHeader` and its associated tables.
pub const HEADER_SIZE: usize = 65536;

// Compile-time verification that our header size calculations are sane.
const _: () = {
    assert!(FileHeader::serialized_size() >= 26);
    assert!(FileHeader::serialized_size() <= 100);
    assert!(AggregateEntry::serialized_size() >= 20);
    assert!(AggregateEntry::serialized_size() <= 50);
    assert!(IndexEntry::serialized_size() == 8);
};

/// The recommended maximum number of chunks in a single file.
///
/// This format is designed for 24-hour data collection periods.
/// The 64KiB header can accommodate approximately 1440 chunks (one per minute for 24 hours).
/// Attempting to compress data spanning multiple days may fail due to header overflow.
pub const MAX_RECOMMENDED_CHUNKS: usize = 1440;

/// The main header for a `.zzp1` file.
///
/// This struct is the first thing read from the file and contains metadata about
/// the entire file's contents, including versioning and pointers to data tables.
pub struct FileHeader {
    /// Must be `FILE_MAGIC`.
    pub magic: u64,
    /// Must be `FORMAT_VERSION`.
    pub format_version: u16,
    /// The absolute start time of the first record in the file, as a UNIX timestamp in nanoseconds.
    pub start_time_unix_ns: u64,
    /// The number of `AggregateEntry` records in the header.
    pub aggregate_entry_count: u32,
    /// The number of `IndexEntry` records in the header.
    pub index_entry_count: u32,
    /// A CRC32 checksum of the preceding fields in this struct.
    pub header_crc32: u32,
}

impl FileHeader {
    /// Calculates the exact size of a `FileHeader` when serialized.
    pub const fn serialized_size() -> usize {
        8 + 2 + 8 + 4 + 4 + 4
    }

    /// Calculates the offset where aggregate entries start in the header.
    pub const fn aggregate_entries_offset() -> usize {
        Self::serialized_size()
    }

    /// Calculates the offset where index entries start in the header.
    pub fn index_entries_offset(&self) -> usize {
        Self::aggregate_entries_offset()
            + (self.aggregate_entry_count as usize * AggregateEntry::serialized_size())
    }

    /// Calculates the total used header space for this file.
    pub fn total_header_used(&self) -> usize {
        self.index_entries_offset()
            + (self.index_entry_count as usize * IndexEntry::serialized_size())
    }

    /// Validates that the header will fit within the allocated `HEADER_SIZE`.
    pub fn validate_header_fits(&self) -> Result<()> {
        let used_space = self.total_header_used();
        if used_space > HEADER_SIZE {
            return Err(anyhow!(
                "Header space exceeded: {} bytes used, {} bytes available.",
                used_space,
                HEADER_SIZE
            ));
        }
        Ok(())
    }

    /// Writes the header to a writer in big-endian format.
    pub fn write(&self, mut w: impl Write) -> std::io::Result<()> {
        w.write_u64::<BigEndian>(self.magic)?;
        w.write_u16::<BigEndian>(self.format_version)?;
        w.write_u64::<BigEndian>(self.start_time_unix_ns)?;
        w.write_u32::<BigEndian>(self.aggregate_entry_count)?;
        w.write_u32::<BigEndian>(self.index_entry_count)?;
        w.write_u32::<BigEndian>(self.header_crc32)?;
        Ok(())
    }

    /// Reads a header from a reader in big-endian format.
    pub fn read(mut r: impl Read) -> std::io::Result<Self> {
        Ok(Self {
            magic: r.read_u64::<BigEndian>()?,
            format_version: r.read_u16::<BigEndian>()?,
            start_time_unix_ns: r.read_u64::<BigEndian>()?,
            aggregate_entry_count: r.read_u32::<BigEndian>()?,
            index_entry_count: r.read_u32::<BigEndian>()?,
            header_crc32: r.read_u32::<BigEndian>()?,
        })
    }
}

/// A pre-calculated summary of one minute of data.
///
/// These entries are stored in the file header to provide a fast overview of the
/// data without needing to decompress the chunks.
#[derive(Debug, Clone, Copy, Default)]
pub struct AggregateEntry {
    /// p00 (minimum) RTT as a quantized symbol.
    pub p00_symbol: u16,
    /// p10 RTT as a quantized symbol.
    pub p10_symbol: u16,
    /// p20 RTT as a quantized symbol.
    pub p20_symbol: u16,
    /// p30 RTT as a quantized symbol.
    pub p30_symbol: u16,
    /// p40 RTT as a quantized symbol.
    pub p40_symbol: u16,
    /// p50 (median) RTT as a quantized symbol.
    pub p50_symbol: u16,
    /// p60 RTT as a quantized symbol.
    pub p60_symbol: u16,
    /// p70 RTT as a quantized symbol.
    pub p70_symbol: u16,
    /// p80 RTT as a quantized symbol.
    pub p80_symbol: u16,
    /// p90 RTT as a quantized symbol.
    pub p90_symbol: u16,
    /// p100 (maximum) RTT as a quantized symbol.
    pub p100_symbol: u16,
    /// The exact count of lost packets in this minute.
    pub lost_packet_count: u32,
}

impl AggregateEntry {
    /// Calculates the exact size of an `AggregateEntry` when serialized.
    pub const fn serialized_size() -> usize {
        11 * 2 + 4
    }

    /// Writes the entry to a writer in big-endian format.
    pub fn write(&self, mut w: impl Write) -> std::io::Result<()> {
        w.write_u16::<BigEndian>(self.p00_symbol)?;
        w.write_u16::<BigEndian>(self.p10_symbol)?;
        w.write_u16::<BigEndian>(self.p20_symbol)?;
        w.write_u16::<BigEndian>(self.p30_symbol)?;
        w.write_u16::<BigEndian>(self.p40_symbol)?;
        w.write_u16::<BigEndian>(self.p50_symbol)?;
        w.write_u16::<BigEndian>(self.p60_symbol)?;
        w.write_u16::<BigEndian>(self.p70_symbol)?;
        w.write_u16::<BigEndian>(self.p80_symbol)?;
        w.write_u16::<BigEndian>(self.p90_symbol)?;
        w.write_u16::<BigEndian>(self.p100_symbol)?;
        w.write_u32::<BigEndian>(self.lost_packet_count)?;
        Ok(())
    }

    /// Reads an entry from a reader in big-endian format.
    pub fn read(mut r: impl Read) -> std::io::Result<Self> {
        Ok(Self {
            p00_symbol: r.read_u16::<BigEndian>()?,
            p10_symbol: r.read_u16::<BigEndian>()?,
            p20_symbol: r.read_u16::<BigEndian>()?,
            p30_symbol: r.read_u16::<BigEndian>()?,
            p40_symbol: r.read_u16::<BigEndian>()?,
            p50_symbol: r.read_u16::<BigEndian>()?,
            p60_symbol: r.read_u16::<BigEndian>()?,
            p70_symbol: r.read_u16::<BigEndian>()?,
            p80_symbol: r.read_u16::<BigEndian>()?,
            p90_symbol: r.read_u16::<BigEndian>()?,
            p100_symbol: r.read_u16::<BigEndian>()?,
            lost_packet_count: r.read_u32::<BigEndian>()?,
        })
    }
}

/// An entry in the file's index table.
///
/// The index table provides fast random access to any one-minute chunk of data
/// in the file without needing to scan from the beginning.
#[derive(Debug, Clone, Copy, Default)]
pub struct IndexEntry {
    /// The byte offset from the beginning of the file where the chunk's data begins.
    pub chunk_offset_bytes: u64,
}

impl IndexEntry {
    /// Calculates the exact size of an `IndexEntry` when serialized.
    pub const fn serialized_size() -> usize {
        8
    }

    /// Writes the entry to a writer in big-endian format.
    pub fn write(&self, mut w: impl Write) -> std::io::Result<()> {
        w.write_u64::<BigEndian>(self.chunk_offset_bytes)?;
        Ok(())
    }

    /// Reads an entry from a reader in big-endian format.
    pub fn read(mut r: impl Read) -> std::io::Result<Self> {
        Ok(Self {
            chunk_offset_bytes: r.read_u64::<BigEndian>()?,
        })
    }
}

bitflags::bitflags! {
    /// Flags used in the `ChunkHeader` to indicate the properties of the chunk's data.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct ChunkFlags: u8 {
        /// If set, indicates that the ping send times were not constant and are
        /// encoded separately in the chunk payload.
        const IS_VARIABLE_RATE = 0b00000001;
        /// Reserved for future use.
        const RAW_DELTAS = 0b00000010;
    }
}

/// The header for a single one-minute chunk of compressed data.
pub struct ChunkHeader {
    /// The UNIX timestamp for the start of the minute this chunk represents, in nanoseconds.
    pub minute_boundary_unix_ns: u64,
    /// The offset of the first ping in this chunk, relative to the `minute_boundary_unix_ns`.
    pub first_ping_offset_ns: u64,
    /// The total number of RTT symbols in the RTT data stream.
    pub rtt_symbol_count: u32,
    /// The total number of symbols in the send-time data stream (0 for constant rate).
    pub send_time_symbol_count: u32,
    /// The length of the compressed RTT data stream in bytes.
    pub rtt_stream_len_bytes: u32,
    /// The length of the compressed send-time data stream in bytes.
    pub send_time_stream_len_bytes: u32,
    /// Flags describing the chunk's data. See `ChunkFlags`.
    pub flags: ChunkFlags,
    /// For constant-rate chunks, the base interval between pings in nanoseconds.
    pub base_interval_ns: u64,
    /// An aggregate summary of the RTT data for this chunk.
    pub rtt_stats: AggregateEntry,
    /// An optional aggregate summary for send-time deltas, if they are variable.
    pub send_time_stats: Option<AggregateEntry>,
    /// A CRC32 checksum of the preceding fields in this struct.
    pub chunk_crc32: u32,
}

impl ChunkHeader {
    /// Writes the header to a writer in big-endian format.
    pub fn write(&self, mut w: impl Write) -> Result<(), std::io::Error> {
        w.write_u64::<BigEndian>(self.minute_boundary_unix_ns)?;
        w.write_u64::<BigEndian>(self.first_ping_offset_ns)?;
        w.write_u32::<BigEndian>(self.rtt_symbol_count)?;
        w.write_u32::<BigEndian>(self.send_time_symbol_count)?;
        w.write_u32::<BigEndian>(self.rtt_stream_len_bytes)?;
        w.write_u32::<BigEndian>(self.send_time_stream_len_bytes)?;
        w.write_u8(self.flags.bits())?;
        w.write_u64::<BigEndian>(self.base_interval_ns)?;
        self.rtt_stats.write(&mut w)?;
        if let Some(stats) = &self.send_time_stats {
            stats.write(&mut w)?;
        }
        w.write_u32::<BigEndian>(self.chunk_crc32)?;
        Ok(())
    }

    /// Reads a header from a reader in big-endian format.
    pub fn read(mut r: impl Read) -> Result<Self, std::io::Error> {
        let minute_boundary_unix_ns = r.read_u64::<BigEndian>()?;
        let first_ping_offset_ns = r.read_u64::<BigEndian>()?;
        let rtt_symbol_count = r.read_u32::<BigEndian>()?;
        let send_time_symbol_count = r.read_u32::<BigEndian>()?;
        let rtt_stream_len_bytes = r.read_u32::<BigEndian>()?;
        let send_time_stream_len_bytes = r.read_u32::<BigEndian>()?;
        let flags = ChunkFlags::from_bits_truncate(r.read_u8()?);
        let base_interval_ns = r.read_u64::<BigEndian>()?;
        let rtt_stats = AggregateEntry::read(&mut r)?;

        let send_time_stats = if flags.contains(ChunkFlags::IS_VARIABLE_RATE) {
            Some(AggregateEntry::read(&mut r)?)
        } else {
            None
        };

        let chunk_crc32 = r.read_u32::<BigEndian>()?;

        Ok(Self {
            minute_boundary_unix_ns,
            first_ping_offset_ns,
            rtt_symbol_count,
            send_time_symbol_count,
            rtt_stream_len_bytes,
            send_time_stream_len_bytes,
            flags,
            base_interval_ns,
            rtt_stats,
            send_time_stats,
            chunk_crc32,
        })
    }
}
