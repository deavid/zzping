// This file contains shared data structures and logic for zzping-press.
use anyhow::Result;
use std::io::Read;
use std::marker::PhantomData;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct RawDataRecord {
    pub sent_nanos: u64,
    pub rtt_nanos: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct CaptureHeader {
    pub start_timestamp_ns: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompressedDataRecord {
    pub sent_delta_deciseconds: u16,
    pub rtt_deciseconds: u16,
}

pub trait FromBytes: Sized {
    const SIZE: usize;
    fn from_le_bytes(bytes: &[u8]) -> Result<Self>;
}

impl FromBytes for RawDataRecord {
    const SIZE: usize = 16;
    fn from_le_bytes(bytes: &[u8]) -> Result<Self> {
        let sent_nanos = u64::from_le_bytes(bytes[0..8].try_into()?);
        let rtt_nanos = u64::from_le_bytes(bytes[8..16].try_into()?);
        Ok(Self {
            sent_nanos,
            rtt_nanos,
        })
    }
}

impl FromBytes for CompressedDataRecord {
    const SIZE: usize = 4;
    fn from_le_bytes(bytes: &[u8]) -> Result<Self> {
        let sent_delta_deciseconds = u16::from_le_bytes(bytes[0..2].try_into()?);
        let rtt_deciseconds = u16::from_le_bytes(bytes[2..4].try_into()?);
        Ok(Self {
            sent_delta_deciseconds,
            rtt_deciseconds,
        })
    }
}

pub struct RecordIterator<R: Read, T: FromBytes> {
    reader: R,
    _phantom: PhantomData<T>,
}

impl<R: Read, T: FromBytes> RecordIterator<R, T> {
    pub fn new(reader: R) -> Self {
        Self {
            reader,
            _phantom: PhantomData,
        }
    }
}

impl<R: Read, T: FromBytes> Iterator for RecordIterator<R, T> {
    type Item = Result<T>;
    fn next(&mut self) -> Option<Self::Item> {
        let mut buffer = vec![0; T::SIZE];
        match self.reader.read_exact(&mut buffer) {
            Ok(()) => Some(T::from_le_bytes(&buffer)),
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => None,
            Err(e) => Some(Err(e.into())),
        }
    }
}

pub mod chunked_v1;
