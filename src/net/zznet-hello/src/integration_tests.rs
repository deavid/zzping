//! Integration test for HelloActor ↔ handshake recipient communication
//!
//! Tests bidirectional message flow between HelloActor and handshake recipient

#[cfg(test)]
mod hello_session_integration {
    use crate::actor::HelloConfig;
    // Integration tests operate at application level; pass role identifier strings.
    use crate::session_messages::HandshakeComplete;
    use actix::prelude::*;
    use std::time::Duration;
    use tokio::sync::mpsc;
    use zznet_api::mock::create_mock_pair;

    /// Mock SessionManager Actor that receives HandshakeComplete messages
    struct MockSessionManager {
        handshake_received: mpsc::UnboundedSender<HandshakeComplete>,
    }

    impl Actor for MockSessionManager {
        type Context = Context<Self>;
    }

    impl Handler<HandshakeComplete> for MockSessionManager {
        type Result = ();

        fn handle(&mut self, msg: HandshakeComplete, _ctx: &mut Context<Self>) {
            println!(
                "MockSessionManager received HandshakeComplete: role={:?}, rooms={:?}",
                msg.peer_role_str, msg.active_rooms
            );
            let _ = self.handshake_received.send(msg);
        }
    }

    #[actix::test]
    async fn test_hello_actor_notifies_session_manager_on_handshake() {
        // Create mock transports
        let (client_transport, server_transport) = create_mock_pair("test-handshake");

        // Create channel to verify HandshakeComplete was received
        let (handshake_tx, mut handshake_rx) = mpsc::unbounded_channel();

        // Create MockSessionManager
        let session_manager = MockSessionManager {
            handshake_received: handshake_tx,
        }
        .start();

        // Configure client and server
        let client_config = HelloConfig {
            our_role: "collector".to_string(),
            offered_rooms: vec!["intentconfig".to_string(), "health".to_string()],
            handshake_timeout: Duration::from_millis(10),
            hostname: "client-host".to_string(),
        };

        let server_config = HelloConfig {
            our_role: "database".to_string(),
            offered_rooms: vec!["intentconfig".to_string(), "memdb".to_string()],
            handshake_timeout: Duration::from_millis(10),
            hostname: "server-host".to_string(),
        };

        // Start client HelloActor with handshake recipient integration
        let client_actor = crate::actor::start_hello_actor_with_handshake_recipient(
            Box::new(client_transport),
            client_config,
            Some(session_manager.recipient()),
        );

        // Start server HelloActor
        let _server_actor = crate::actor::start_hello_actor_with_handshake_recipient(
            Box::new(server_transport),
            server_config,
            None,
        );

        // Wait for handshake to complete and verify handshake recipient was notified
        tokio::time::sleep(Duration::from_millis(1)).await;

        // Try receiving the HandshakeComplete from the mock session manager
        if let Some(handshake) = handshake_rx.recv().await {
            // Basic assertions
            assert!(!handshake.peer_id.is_empty());
            assert!(
                handshake.active_rooms.contains(&"intentconfig".to_string())
                    || handshake.active_rooms.contains(&"memdb".to_string())
            );
        } else {
            panic!("HandshakeComplete was not received by MockSessionManager");
        }

        // Cleanup
        client_actor.do_send(crate::actor::Disconnect);
    }

    #[actix::test]
    async fn test_tls_validation_rejects_mismatched_cn() {
        use zznet_api::types::PeerTLSIdentity;

        // Create mock transports with TLS identity
        let (mut client_transport, server_transport) = create_mock_pair("test-tls-reject");

        // Configure client transport with TLS identity that DOES NOT match the HELLO role
        // The HELLO will say "collector" but the TLS CN will say "attacker"
        client_transport = client_transport.with_peer_identity(Some(PeerTLSIdentity {
            role: "attacker".to_string(), // Mismatch!
            username: "attacker_user".to_string(),
        }));

        // Create channel to verify HandshakeComplete was NOT received
        let (handshake_tx, mut handshake_rx) = mpsc::unbounded_channel();

        // Create MockSessionManager
        let session_manager = MockSessionManager {
            handshake_received: handshake_tx,
        }
        .start();

        // Configure client - note role is "collector"
        let client_config = HelloConfig {
            our_role: "collector".to_string(), // This does NOT match TLS CN "attacker"
            offered_rooms: vec!["intentconfig".to_string()],
            handshake_timeout: Duration::from_millis(10),
            hostname: "client-host".to_string(),
        };

        let server_config = HelloConfig {
            our_role: "database".to_string(),
            offered_rooms: vec!["intentconfig".to_string()],
            handshake_timeout: Duration::from_millis(10),
            hostname: "server-host".to_string(),
        };

        // Start client HelloActor with handshake recipient integration
        let client_actor = crate::actor::start_hello_actor_with_handshake_recipient(
            Box::new(client_transport),
            client_config,
            Some(session_manager.recipient()),
        );

        // Start server HelloActor
        let _server_actor = crate::actor::start_hello_actor_with_handshake_recipient(
            Box::new(server_transport),
            server_config,
            None,
        );

        // Wait for handshake attempt
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Verify HandshakeComplete was NOT received (because TLS validation should fail)
        assert!(
            handshake_rx.try_recv().is_err(),
            "HandshakeComplete should NOT be received when TLS CN doesn't match HELLO role"
        );

        // Cleanup
        client_actor.do_send(crate::actor::Disconnect);
    }

    #[actix::test]
    async fn test_tls_validation_accepts_matching_cn() {
        use zznet_api::types::PeerTLSIdentity;

        // Create mock transports with TLS identity
        let (mut client_transport, mut server_transport) = create_mock_pair("test-tls-accept");

        // Configure peer_identity for each transport:
        // - client_transport.peer_identity = what CLIENT sees (server's cert) = "database"
        // - server_transport.peer_identity = what SERVER sees (client's cert) = "collector"
        client_transport = client_transport.with_peer_identity(Some(PeerTLSIdentity {
            role: "database".to_string(), // Server's cert as seen by client
            username: "database_user".to_string(),
        }));

        server_transport = server_transport.with_peer_identity(Some(PeerTLSIdentity {
            role: "collector".to_string(), // Client's cert as seen by server
            username: "collector_user".to_string(),
        }));

        // Create channel to verify HandshakeComplete was received
        let (handshake_tx, mut handshake_rx) = mpsc::unbounded_channel();

        // Create MockSessionManager
        let session_manager = MockSessionManager {
            handshake_received: handshake_tx,
        }
        .start();

        // Configure client - note role is "collector"
        let client_config = HelloConfig {
            our_role: "collector".to_string(), // This MATCHES TLS CN "collector"
            offered_rooms: vec!["intentconfig".to_string()],
            handshake_timeout: Duration::from_millis(10),
            hostname: "client-host".to_string(),
        };

        let server_config = HelloConfig {
            our_role: "database".to_string(),
            offered_rooms: vec!["intentconfig".to_string()],
            handshake_timeout: Duration::from_millis(10),
            hostname: "server-host".to_string(),
        };

        // Start client HelloActor with handshake recipient integration
        let client_actor = crate::actor::start_hello_actor_with_handshake_recipient(
            Box::new(client_transport),
            client_config,
            Some(session_manager.recipient()),
        );

        // Start server HelloActor
        let _server_actor = crate::actor::start_hello_actor_with_handshake_recipient(
            Box::new(server_transport),
            server_config,
            None,
        );

        // Wait for handshake to complete
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Verify HandshakeComplete WAS received (because TLS validation should succeed)
        if let Ok(Some(handshake)) =
            tokio::time::timeout(Duration::from_millis(50), handshake_rx.recv()).await
        {
            assert_eq!(handshake.peer_role_str, "database");
            assert!(!handshake.active_rooms.is_empty());
        } else {
            panic!("HandshakeComplete should be received when TLS CN matches HELLO role");
        }

        // Cleanup
        client_actor.do_send(crate::actor::Disconnect);
    }
}
