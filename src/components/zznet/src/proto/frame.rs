use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub async fn read_frame(
    mut stream: impl AsyncReadExt + std::marker::Unpin,
) -> anyhow::Result<Vec<u8>> {
    let n = stream.read_u32().await?;
    // FIXME: This is allocating memory for every packet received.
    let mut buf = vec![0u8; n as usize];
    stream.read_exact(&mut buf).await?;
    Ok(buf)
}

pub async fn write_frame(
    mut stream: impl AsyncWriteExt + std::marker::Unpin,
    data: &[u8],
) -> anyhow::Result<()> {
    let len: u32 = data.len().try_into()?;
    stream.write_u32(len).await?;
    stream.write_all(data).await?;
    Ok(())
}
