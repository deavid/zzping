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

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt;

    #[tokio::test]
    async fn test_read_write_frame_roundtrip() {
        let data = b"Hello, World!";

        // Create in-memory buffer
        let mut buffer = Vec::new();

        // Write frame
        write_frame(&mut buffer, data).await.unwrap();

        // Read frame back
        let mut cursor = io::Cursor::new(buffer);
        let result = read_frame(&mut cursor).await.unwrap();

        assert_eq!(result.as_ref(), data);
    }

    #[tokio::test]
    async fn test_multiple_frames() {
        let frame1 = b"First frame";
        let frame2 = b"Second frame";
        let frame3 = b"Third frame";

        let mut buffer = Vec::new();

        // Write multiple frames
        write_frame(&mut buffer, frame1).await.unwrap();
        write_frame(&mut buffer, frame2).await.unwrap();
        write_frame(&mut buffer, frame3).await.unwrap();

        // Read them back
        let mut cursor = io::Cursor::new(buffer);

        let result1 = read_frame(&mut cursor).await.unwrap();
        assert_eq!(result1.as_ref(), frame1);

        let result2 = read_frame(&mut cursor).await.unwrap();
        assert_eq!(result2.as_ref(), frame2);

        let result3 = read_frame(&mut cursor).await.unwrap();
        assert_eq!(result3.as_ref(), frame3);
    }

    #[tokio::test]
    async fn test_large_frame() {
        // Create a large frame (1 MB)
        let data = vec![0x42; 1024 * 1024];

        let mut buffer = Vec::new();
        write_frame(&mut buffer, &data).await.unwrap();

        let mut cursor = io::Cursor::new(buffer);
        let result = read_frame(&mut cursor).await.unwrap();

        assert_eq!(result.len(), data.len());
        assert_eq!(result.as_ref(), data.as_slice());
    }

    #[tokio::test]
    async fn test_frame_too_large() {
        // Try to write a frame larger than MAX_FRAME_SIZE
        let data = vec![0; MAX_FRAME_SIZE + 1];

        let mut buffer = Vec::new();
        let result = write_frame(&mut buffer, &data).await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidInput);
    }

    #[tokio::test]
    async fn test_read_frame_too_large() {
        // Manually write an invalid frame size
        let mut buffer = Vec::new();
        buffer.write_u32((MAX_FRAME_SIZE + 1) as u32).await.unwrap();

        let mut cursor = io::Cursor::new(buffer);
        let result = read_frame(&mut cursor).await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn test_read_incomplete_frame() {
        let mut buffer = Vec::new();

        // Write length prefix for 10 bytes
        buffer.write_u32(10).await.unwrap();
        // But only write 5 bytes of data
        buffer.write_all(b"Hello").await.unwrap();

        let mut cursor = io::Cursor::new(buffer);
        let result = read_frame(&mut cursor).await;

        // Should fail with UnexpectedEof
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::UnexpectedEof);
    }

    #[tokio::test]
    async fn test_empty_frame_rejected() {
        let mut buffer = Vec::new();

        // Write length prefix of 0
        buffer.write_u32(0).await.unwrap();

        let mut cursor = io::Cursor::new(buffer);
        let result = read_frame(&mut cursor).await;

        assert!(result.is_err());
        assert_eq!(result.unwrap_err().kind(), io::ErrorKind::InvalidData);
    }
}
