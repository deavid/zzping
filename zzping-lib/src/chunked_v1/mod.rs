//! The `chunked_v1` storage format implementation.
//!
//! This module contains the logic for compressing and decompressing ping data
//! into the `.zzp1` file format. The format is designed to be append-only and
//! optimized for high-frequency ping data over a 24-hour period.

mod compression;
mod decompression;
mod format;
mod quantization;

#[cfg(test)]
mod tests;

pub use compression::{
    compress_chunked_v1, create_chunk_body, create_chunked_v1_header,
};
pub use decompression::decompress_chunked_v1;
pub use format::{
    calculate_file_header_crc32, AggregateEntry, ChunkFlags, ChunkHeader, FileHeader, IndexEntry,
    FILE_MAGIC, FORMAT_VERSION, HEADER_SIZE, MAX_RECOMMENDED_CHUNKS,
};
pub use quantization::Quantizer;
