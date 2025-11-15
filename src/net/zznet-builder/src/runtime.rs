//! Runtime initialization for ZZNet applications.
//!
//! This module handles Actix runtime setup, TLS crypto provider installation,
//! and other runtime concerns.

use anyhow::Result;

/// Install the rustls crypto provider.
///
/// This must be called early in the application lifecycle, before any TLS
/// operations are performed. It's safe to call multiple times (subsequent
/// calls will be ignored).
pub(crate) fn install_crypto_provider() {
    let _ =
        rustls::crypto::CryptoProvider::install_default(rustls::crypto::ring::default_provider());
}

/// Run an async main function with Actix runtime.
///
/// This creates an Actix `System` and runs the provided async function
/// within it, ensuring the Tokio reactor is installed correctly for Actix.
pub(crate) fn run_actix<F, Fut>(f: F) -> Result<()>
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<()>>,
{
    actix_rt::System::new().block_on(f())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_install_crypto_provider_doesnt_panic() {
        install_crypto_provider();
        // Calling again should be safe
        install_crypto_provider();
    }

    #[test]
    fn test_run_actix_executes_future() {
        let result = run_actix(|| async { Ok(()) });
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_actix_propagates_error() {
        let result = run_actix(|| async { Err(anyhow::anyhow!("test error")) });
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("test error"));
    }
}
