use tokio::io::{AsyncRead, AsyncWrite};

/// Trait combining AsyncRead and AsyncWrite for trait objects
pub trait AsyncReadWrite: AsyncRead + AsyncWrite {}

impl<T> AsyncReadWrite for T where T: AsyncRead + AsyncWrite {}
