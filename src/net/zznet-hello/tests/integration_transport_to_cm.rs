#![allow(missing_docs)]

use actix::prelude::*;
use std::time::Duration;
use zznet_api::messages::OnPeerConnected;
use zznet_api::transport::{TransportClient as _, TransportServer as _};
use zznet_api::types::Role;
use zznet_hello::actor::HelloConfig;
use zznet_hello::connection_manager::{ConnectionManager, HandleTransport};
use zznet_transport_tcp::server::TcpTransportServer;

/// Mock actor for testing - receives OnPeerConnected but does nothing
struct MockPeerHandler;

impl Actor for MockPeerHandler {
    type Context = Context<Self>;
}

impl Handler<OnPeerConnected> for MockPeerHandler {
    type Result = Result<(), String>;

    fn handle(&mut self, _msg: OnPeerConnected, _ctx: &mut Context<Self>) -> Self::Result {
        Ok(())
    }
}

#[tokio::test]
async fn transport_accept_and_send_to_connection_manager() {
    // Run test body inside a LocalSet so spawn_local can be used by underlying libs
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            // Start a plain TCP transport server on ephemeral port
            let mut server = TcpTransportServer::new("127.0.0.1:0", None)
                .await
                .expect("server start");
            let addr = server.local_addr().expect("local addr");
            // Aggressive timeout for test operations (10ms)
            let timeout_ms = 10u64;

            // Run accept() and client.connect() concurrently within same task to avoid spawn_local
            let client =
                zznet_transport_tcp::client::TcpTransportClient::new(addr.to_string(), None)
                    .unwrap();

            let combined = async { tokio::join!(server.accept(), client.connect()) };

            let res = tokio::time::timeout(Duration::from_millis(timeout_ms), combined)
                .await
                .unwrap_or_else(|_| panic!("accept/connect timed out after {}ms", timeout_ms));

            let (accept_res, connect_res) = res;

            let transport = match accept_res {
                Ok(t) => t,
                Err(e) => panic!("accept returned error: {}", e),
            };

            match connect_res {
                Ok(_conn) => { /* connected */ }
                Err(e) => panic!("client connect failed: {}", e),
            }

            // Start ConnectionManager actor
            // Create a mock peer handler
            let mock_handler = MockPeerHandler.start();
            let peer_recipient = mock_handler.recipient();
            // Build allowed roles set (accept admin)
            let mut allowed = std::collections::HashSet::new();
            allowed.insert(Role::new("admin"));
            let mgr = ConnectionManager::new(peer_recipient, "test".to_string(), allowed).start();

            // Send transport using HandleTransport, ensure try_send succeeds
            let config = HelloConfig::default();
            let res = mgr.try_send(HandleTransport { transport, config });
            assert!(res.is_ok(), "HandleTransport try_send should succeed");
        })
        .await;
}
