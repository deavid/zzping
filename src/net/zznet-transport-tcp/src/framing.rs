//! Frame delimiting for TCP byte streams.
//!
//! This module provides length-prefixed framing to delimit messages in a
//! continuous TCP byte stream. Each frame consists of:
//! 1. 4-byte length prefix (u32, big-endian)
//! 2. Frame payload (variable length)

use bytes::Bytes;
use std::io;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tracing::trace;

/// Maximum frame size (16 MB)
pub(crate) const MAX_FRAME_SIZE: usize = 16 * 1024 * 1024;

/// Reads a length-prefixed frame from an asynchronous byte stream.
///
/// This function first reads a `u32` (4 bytes) to determine the length of the
/// incoming frame, then reads that many bytes. This mechanism is crucial for
/// delimiting messages in a continuous byte stream.
pub(crate) async fn read_frame<R>(stream: &mut R) -> io::Result<Bytes>
where
    R: AsyncRead + Unpin,
{
    // Read frame length (4 bytes, big-endian u32)
    let frame_len = stream.read_u32().await? as usize;

    trace!("Reading frame of {} bytes", frame_len);

    // Validate frame size
    if frame_len > MAX_FRAME_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Frame size {} exceeds maximum {}",
                frame_len, MAX_FRAME_SIZE
            ),
        ));
    }

    if frame_len == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "Frame size cannot be zero",
        ));
    }

    // Read frame data incrementally to avoid memory exhaustion.
    // We use a limited initial capacity to prevent allocating 16MB immediately.
    let mut buffer = Vec::with_capacity(std::cmp::min(frame_len, 8192));

    let mut take = stream.take(frame_len as u64);
    take.read_to_end(&mut buffer).await?;

    if buffer.len() != frame_len {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "Connection closed before full frame",
        ));
    }

    trace!("Read frame: {} bytes", frame_len);
    Ok(Bytes::from(buffer))
}

/// Writes a length-prefixed frame to an asynchronous byte stream.
///
/// This function first writes the length of the provided `data` as a `u32` (4 bytes),
/// followed by the actual data bytes. This ensures that the receiving end can
/// correctly interpret the boundaries of each message in the stream.
pub(crate) async fn write_frame<W>(stream: &mut W, data: &[u8]) -> io::Result<()>
where
    W: AsyncWrite + Unpin,
{
    let frame_len = data.len();

    trace!("Writing frame of {} bytes", frame_len);

    // Validate frame size
    if frame_len > MAX_FRAME_SIZE {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "Frame size {} exceeds maximum {}",
                frame_len, MAX_FRAME_SIZE
            ),
        ));
    }

    // Write frame length (4 bytes, big-endian u32)
    let len_u32: u32 = frame_len.try_into().map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("Frame size {} too large for u32", frame_len),
        )
    })?;

    stream.write_u32(len_u32).await?;

    // Write frame data
    stream.write_all(data).await?;
    stream.flush().await?;

    trace!("Wrote frame: {} bytes", frame_len);
    Ok(())
}
