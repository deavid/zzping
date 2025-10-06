//! Integration test for HelloActor ↔ SessionManager communication
//!
//! Tests Phase 4 Task 1: Bidirectional message flow between HelloActor and SessionManager

#[cfg(test)]
mod hello_session_integration {
    use crate::actor::{HelloConfig, start_hello_actor};
    use crate::auth::AuthRole;
    use crate::session_messages::{HandshakeComplete, InboundRoomMessage};
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
                msg.peer_role, msg.active_rooms
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
            our_role: AuthRole::Collector,
            offered_rooms: vec!["intentconfig".to_string(), "health".to_string()],
            handshake_timeout: Duration::from_secs(5),
            hostname: "client-host".to_string(),
        };

        let server_config = HelloConfig {
            our_role: AuthRole::Database,
            offered_rooms: vec!["intentconfig".to_string(), "memdb".to_string()],
            handshake_timeout: Duration::from_secs(5),
            hostname: "server-host".to_string(),
        };

        // Start client HelloActor with SessionManager integration
        let client_actor = crate::actor::start_hello_actor_with_session_manager(
            Box::new(client_transport),
            client_config,
            Some(session_manager.recipient()),
        );

        // Start server HelloActor
        let _server_actor = start_hello_actor(Box::new(server_transport), server_config);

        // Wait for handshake to complete and verify SessionManager was notified
        tokio::time::sleep(Duration::from_millis(100)).await;

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
    async fn test_session_manager_can_send_to_hello_actor() {
        // Create mock transports
        let (client_transport, server_transport) = create_mock_pair("test-send");

        // Configure and start actors
        let client_config = HelloConfig {
            our_role: AuthRole::Collector,
            offered_rooms: vec!["intentconfig".to_string()],
            handshake_timeout: Duration::from_secs(5),
            hostname: "client-host".to_string(),
        };

        let server_config = HelloConfig {
            our_role: AuthRole::Database,
            offered_rooms: vec!["intentconfig".to_string()],
            handshake_timeout: Duration::from_secs(5),
            hostname: "server-host".to_string(),
        };

        let client_actor = start_hello_actor(Box::new(client_transport), client_config);
        let _server_actor = start_hello_actor(Box::new(server_transport), server_config);

        // Wait for handshake
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Try to send InboundRoomMessage to HelloActor
        let msg = InboundRoomMessage {
            from_room: "intentconfig".to_string(),
            to_room: "intentconfig".to_string(),
            payload: vec![1, 2, 3, 4],
        };

        let result = client_actor.send(msg).await;

        // Should succeed (HelloActor has Handler<InboundRoomMessage>)
        assert!(result.is_ok());

        // Cleanup
        client_actor.do_send(crate::actor::Disconnect);
    }
}
