//! Client implementation for the PingerClient trait using surge_ping.

use async_trait::async_trait;
use std::net::IpAddr;
use std::time::Duration;
use surge_ping::{Client, PingIdentifier, PingSequence, SurgeError};

use crate::traits::{PingError, PingerClient};

const ZZPINGER_IDENTIFIER: u16 = 59179;

#[async_trait]
impl PingerClient for Client {
    async fn ping(&self, target: IpAddr, seq: u16) -> Result<Duration, PingError> {
        let ident = PingIdentifier(ZZPINGER_IDENTIFIER);
        let mut pinger = self.pinger(target, ident).await;
        pinger.timeout(Duration::from_secs(10));

        match pinger.ping(PingSequence(seq), &[]).await {
            Ok((_, duration)) => Ok(duration),
            Err(SurgeError::Timeout { seq: _ }) => Err(PingError::Timeout),
            Err(_) => Err(PingError::NetworkError),
        }
    }
}
