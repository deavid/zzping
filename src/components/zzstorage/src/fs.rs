// src/components/zzstorage/src/fs.rs

//! Defines the file format for the append-only storage log.

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use std::io::{Read, Write, Result, Seek};

/// Magic bytes to identify a `zzstorage` v2 file.
pub const FILE_MAGIC: &[u8; 4] = b"ZZS2";
/// The version of the file format.
pub const FORMAT_VERSION: u16 = 2;
/// The total size of the file header in bytes.
pub const HEADER_SIZE: usize = 14;

/// The header for a `zzstorage` data file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileHeader {
    /// Must be `ZZS2`.
    pub magic: [u8; 4],
    /// Must be `2`.
    pub format_version: u16,
    /// The number of compressed blobs stored in the file.
    pub blob_count: u64,
}

impl FileHeader {
    /// Writes the header to a writer.
    pub fn write<W: Write>(&self, writer: &mut W) -> Result<()> {
        writer.write_all(&self.magic)?;
        writer.write_u16::<BigEndian>(self.format_version)?;
        writer.write_u64::<BigEndian>(self.blob_count)?;
        Ok(())
    }

    /// Reads the header from a reader.
    pub fn read<R: Read>(reader: &mut R) -> Result<Self> {
        let mut magic = [0u8; 4];
        reader.read_exact(&mut magic)?;
        let format_version = reader.read_u16::<BigEndian>()?;
        let blob_count = reader.read_u64::<BigEndian>()?;
        Ok(Self {
            magic,
            format_version,
            blob_count,
        })
    }
}

/// Scans a storage file from the beginning to find all valid blobs and truncates any
/// partially written data at the end.
///
/// Returns the number of valid blobs found.
pub fn scan_and_recover(file: &mut std::fs::File) -> Result<u64> {
    file.seek(std::io::SeekFrom::Start(0))?;
    let header = FileHeader::read(file)?;

    if header.magic != *FILE_MAGIC || header.format_version != FORMAT_VERSION {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "Invalid file magic or version",
        ));
    }

    let mut blob_count = 0;
    let mut current_pos = HEADER_SIZE as u64;

    loop {
        file.seek(std::io::SeekFrom::Start(current_pos))?;

        let blob_len = match file.read_u32::<BigEndian>() {
            Ok(len) => len,
            Err(ref e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                // This is the expected end of the file.
                break;
            }
            Err(e) => return Err(e),
        };

        let next_pos = current_pos + 4 + blob_len as u64;
        if next_pos > file.metadata()?.len() {
            // A partial write occurred. Truncate the file to the last known good position.
            file.set_len(current_pos)?;
            break;
        }

        blob_count += 1;
        current_pos = next_pos;
    }

    Ok(blob_count)
}
