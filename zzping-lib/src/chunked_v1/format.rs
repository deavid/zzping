use anyhow::{Result, anyhow};
use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Read, Write};

// --- Validation Functions (Phase 2) ---

/// Calculate CRC32 for file header excluding the CRC32 field itself
pub(super) fn calculate_file_header_crc32(header: &FileHeader) -> u32 {
    let mut hasher = crc32fast::Hasher::new();

    // Hash all fields except header_crc32
    hasher.update(&header.magic.to_be_bytes());
    hasher.update(&header.format_version.to_be_bytes());
    hasher.update(&header.start_time_unix_ns.to_be_bytes());
    hasher.update(&header.aggregate_entry_count.to_be_bytes());
    hasher.update(&header.index_entry_count.to_be_bytes());

    hasher.finalize()
}

/// Calculate CRC32 for chunk header excluding the CRC32 field itself
pub(super) fn calculate_chunk_header_crc32(header: &ChunkHeader) -> u32 {
    let mut hasher = crc32fast::Hasher::new();

    // Hash all fields except chunk_crc32
    hasher.update(&header.minute_boundary_unix_ns.to_be_bytes());
    hasher.update(&header.first_ping_offset_ns.to_be_bytes());
    hasher.update(&header.rtt_symbol_count.to_be_bytes());
    hasher.update(&header.send_time_symbol_count.to_be_bytes());
    hasher.update(&header.rtt_stream_len_bytes.to_be_bytes());
    hasher.update(&header.send_time_stream_len_bytes.to_be_bytes());
    hasher.update(&[header.flags.bits()]);
    hasher.update(&header.base_interval_ns.to_be_bytes());

    // Hash rtt_stats
    let mut stats_buf = Vec::new();
    header.rtt_stats.write(&mut stats_buf).unwrap();
    hasher.update(&stats_buf);

    // Hash send_time_stats if present
    if let Some(ref stats) = header.send_time_stats {
        let mut stats_buf = Vec::new();
        stats.write(&mut stats_buf).unwrap();
        hasher.update(&stats_buf);
    }

    hasher.finalize()
}


/// # IMPORTANT: Avoiding Hardcoded Size Bugs
///
/// This module was previously affected by hardcoded size values in tests that became
/// incorrect when CRC32 fields were added. The FileHeader size changed from 26 to 30 bytes,
/// but tests were hardcoded to use 26, causing them to read from wrong offsets.
///
/// **SOLUTION**: All struct sizes are now calculated using `serialized_size()` methods
/// and verified at compile-time with const assertions. Tests use calculated offsets
/// via `file_header.index_entries_offset()` instead of hardcoded values.
///
/// **PREVENTION**:
/// - Use `FileHeader::serialized_size()` instead of hardcoding sizes
/// - Use `file_header.index_entries_offset()` for calculated offsets
/// - Compile-time assertions prevent size miscalculations
/// - Runtime tests verify calculations match actual serialization
pub const FILE_MAGIC: u64 = 0x5A5A504356312020;
pub const FORMAT_VERSION: u16 = 1;
pub const HEADER_SIZE: usize = 65536;

// Compile-time verification that our header size calculations are sane
const _: () = {
    // Ensure FileHeader fits within reasonable bounds (should be around 30 bytes)
    assert!(FileHeader::serialized_size() >= 26); // Minimum expected size
    assert!(FileHeader::serialized_size() <= 100); // Maximum reasonable size

    // Ensure AggregateEntry size is reasonable (should be around 26 bytes)
    assert!(AggregateEntry::serialized_size() >= 20);
    assert!(AggregateEntry::serialized_size() <= 50);

    // Ensure IndexEntry is exactly 8 bytes as expected
    assert!(IndexEntry::serialized_size() == 8);
};

// FORMAT DESIGN LIMITATION: This format is designed for 24-hour data periods.
// The 64KiB header can accommodate approximately 1440 chunks (one per minute for 24 hours).
// Attempting to compress data spanning multiple days will fail due to header overflow.
// Each index entry requires 8 bytes, so max chunks ≈ (65536 - overhead) / 8 ≈ 8000 theoretical,
// but in practice ~1440 chunks (24 hours) is the intended design limit.
pub const MAX_RECOMMENDED_CHUNKS: usize = 1440; // 24 hours * 60 minutes

pub struct FileHeader {
    pub magic: u64,
    pub format_version: u16,
    pub start_time_unix_ns: u64,
    pub aggregate_entry_count: u32,
    pub index_entry_count: u32,
    pub header_crc32: u32, // Phase 2: CRC32 of entire file header (excluding this field)
}

impl FileHeader {
    /// Calculate the exact size of a FileHeader when serialized
    pub const fn serialized_size() -> usize {
        8 + 2 + 8 + 4 + 4 + 4 // magic + format_version + start_time + aggregate_count + index_count + crc32
    }

    /// Calculate the offset where aggregate entries start in the header
    pub const fn aggregate_entries_offset() -> usize {
        Self::serialized_size()
    }

    /// Calculate the offset where index entries start in the header
    pub fn index_entries_offset(&self) -> usize {
        Self::aggregate_entries_offset()
            + (self.aggregate_entry_count as usize * AggregateEntry::serialized_size())
    }

    /// Calculate the total used header space for this file
    pub fn total_header_used(&self) -> usize {
        self.index_entries_offset()
            + (self.index_entry_count as usize * IndexEntry::serialized_size())
    }

    /// Validate that the header will fit within the allocated header space
    pub fn validate_header_fits(&self) -> Result<()> {
        let used_space = self.total_header_used();
        if used_space > HEADER_SIZE {
            return Err(anyhow!(
                "Header space exceeded: {} bytes used, {} bytes available. Reduce aggregate/index entries.",
                used_space,
                HEADER_SIZE
            ));
        }
        Ok(())
    }

    pub fn write(&self, mut w: impl Write) -> std::io::Result<()> {
        w.write_u64::<BigEndian>(self.magic)?;
        w.write_u16::<BigEndian>(self.format_version)?;
        w.write_u64::<BigEndian>(self.start_time_unix_ns)?;
        w.write_u32::<BigEndian>(self.aggregate_entry_count)?;
        w.write_u32::<BigEndian>(self.index_entry_count)?;
        w.write_u32::<BigEndian>(self.header_crc32)?;
        Ok(())
    }

    pub fn read(mut r: impl Read) -> std::io::Result<Self> {
        let magic = r.read_u64::<BigEndian>()?;
        let format_version = r.read_u16::<BigEndian>()?;
        let start_time_unix_ns = r.read_u64::<BigEndian>()?;
        let aggregate_entry_count = r.read_u32::<BigEndian>()?;
        let index_entry_count = r.read_u32::<BigEndian>()?;
        let header_crc32 = r.read_u32::<BigEndian>()?;
        Ok(Self {
            magic,
            format_version,
            start_time_unix_ns,
            aggregate_entry_count,
            index_entry_count,
            header_crc32,
        })
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct AggregateEntry {
    // Percentile symbols for RTT distribution (u16 provides full quantizer symbol range)
    pub p00_symbol: u16,
    pub p10_symbol: u16,
    pub p20_symbol: u16,
    pub p30_symbol: u16,
    pub p40_symbol: u16,
    pub p50_symbol: u16,
    pub p60_symbol: u16,
    pub p70_symbol: u16,
    pub p80_symbol: u16,
    pub p90_symbol: u16,
    pub p100_symbol: u16,
    // Exact count of lost packets for precise frequency estimation in compression model
    pub lost_packet_count: u32, // Only relevant for RTT stats.
}

impl AggregateEntry {
    /// Calculate the exact size of an AggregateEntry when serialized
    pub const fn serialized_size() -> usize {
        11 * 2 + 4 // 11 u16 percentile symbols + 1 u32 lost_packet_count
    }

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

#[derive(Debug, Clone, Copy, Default)]
pub struct IndexEntry {
    pub chunk_offset_bytes: u64,
}

impl IndexEntry {
    /// Calculate the exact size of an IndexEntry when serialized
    pub const fn serialized_size() -> usize {
        8 // chunk_offset_bytes: u64
    }

    pub fn write(&self, mut w: impl Write) -> std::io::Result<()> {
        w.write_u64::<BigEndian>(self.chunk_offset_bytes)?;
        Ok(())
    }

    pub fn read(mut r: impl Read) -> std::io::Result<Self> {
        Ok(Self {
            chunk_offset_bytes: r.read_u64::<BigEndian>()?,
        })
    }
}

bitflags::bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
    pub struct ChunkFlags: u8 {
        const IS_VARIABLE_RATE = 0b00000001;
        const RAW_DELTAS = 0b00000010;
    }
}

pub struct ChunkHeader {
    pub minute_boundary_unix_ns: u64, // Unix timestamp rounded to minute boundary
    pub first_ping_offset_ns: u64, // Nanoseconds from minute boundary to first ping (0-59999999999)
    pub rtt_symbol_count: u32,
    pub send_time_symbol_count: u32,
    pub rtt_stream_len_bytes: u32,
    pub send_time_stream_len_bytes: u32,
    pub flags: ChunkFlags,
    pub base_interval_ns: u64, // Base interval in nanoseconds (exact integer)
    pub rtt_stats: AggregateEntry,
    pub send_time_stats: Option<AggregateEntry>,
    pub chunk_crc32: u32, // Phase 2: CRC32 of chunk header (excluding this field)
}

impl ChunkHeader {
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

        let send_time_stats = if flags.contains(ChunkFlags::IS_VARIABLE_RATE)
            && !flags.contains(ChunkFlags::RAW_DELTAS)
        {
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
