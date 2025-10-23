#![allow(missing_docs)]

use std::time::Duration;
use zznet_api::transport::{TransportClient as _, TransportServer as _};
use zznet_auth::mock::MockRole;
use zznet_hello::actor::HelloConfig;
use zznet_hello::connection_manager::{ConnectionManager, HandleTransport};
use zznet_transport_tcp::server::TcpTransportServer;

#[tokio::test]
async fn transport_accept_and_send_to_connection_manager() {
    // Run test body inside a LocalSet so spawn_local can be used by underlying libs
    let local = tokio::task::LocalSet::new();
    local
        .run_until(async move {
            // Start a plain TCP transport server on ephemeral port
            let mut server = TcpTransportServer::plain("127.0.0.1:0")
                .await
                .expect("server start");
            let addr = server.local_addr().expect("local addr");
            // Aggressive timeout for test operations (10ms)
            let timeout_ms = 10u64;

            // Run accept() and client.connect() concurrently within same task to avoid spawn_local
            let client = zznet_transport_tcp::client::TcpTransportClient::plain(addr.to_string());

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
            use actix::prelude::*;
            let offered_rooms = vec![];
            // Simple authorizer that accepts all peers as Admin
            let authorizer =
                Box::new(|_auth_ctx: &zznet_api::types::AuthContext| Some(MockRole::Admin))
                    as Box<
                        dyn Fn(&zznet_api::types::AuthContext) -> Option<MockRole> + Send + Sync,
                    >;
            let mgr = ConnectionManager::<MockRole>::new(offered_rooms, authorizer).start();

            // Send transport using HandleTransport, ensure try_send succeeds
            let config = HelloConfig::default();
            let res = mgr.try_send(HandleTransport { transport, config });
            assert!(res.is_ok(), "HandleTransport try_send should succeed");
        })
        .await;
}
