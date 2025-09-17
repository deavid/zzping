use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Reads a length-prefixed frame from an asynchronous byte stream.
///
/// This function first reads a `u32` (4 bytes) to determine the length of the
/// incoming frame, then reads that many bytes into a buffer. This mechanism
/// is crucial for delimiting messages in a continuous byte stream, allowing
/// the receiver to know exactly how many bytes constitute a single logical message.
pub async fn read_frame(
    mut stream: impl AsyncReadExt + std::marker::Unpin,
) -> anyhow::Result<Vec<u8>> {
    let n = stream.read_u32().await?;
    // FIXME: This is allocating memory for every packet received.
    let mut buf = vec![0u8; n as usize];
    stream.read_exact(&mut buf).await?;
    Ok(buf)
}

/// Writes a length-prefixed frame to an asynchronous byte stream.
///
/// This function first writes the length of the provided `data` as a `u32` (4 bytes),
/// followed by the actual data bytes. This ensures that the receiving end can
/// correctly interpret the boundaries of each message in the stream.
pub async fn write_frame(
    mut stream: impl AsyncWriteExt + std::marker::Unpin,
    data: &[u8],
) -> anyhow::Result<()> {
    let len: u32 = data.len().try_into()?;
    stream.write_u32(len).await?;
    stream.write_all(data).await?;
    Ok(())
}
