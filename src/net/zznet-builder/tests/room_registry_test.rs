//! Integration test demonstrating the new RoomRegistry pattern.
//!
//! This test shows how to use the refactored `zznet_builder::RoomRegistry`
//! to simplify room handler registration compared to the old manual pattern.

#[cfg(test)]
mod room_registry_integration_tests {
    use serde::{Deserialize, Serialize};
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use zznet_auth::ApplicationRole;
    use zznet_builder::{RoomHandlerFactory, RoomRegistry};
    use zznet_session::peer_session::RoomHandle;
    use zznet_session::room_message_trait::RoomMessageTrait;
    use zznet_session::session_manager::SessionManager;
    use zznet_session::types::{RoomId, SessionError};

    // Simple test role
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
    enum TestRole {
        Server,
        Client,
    }

    impl ApplicationRole for TestRole {
        fn from_cn(cn: &str) -> Result<Self, zznet_auth::error::AuthError> {
            match cn {
                "server" => Ok(TestRole::Server),
                "client" => Ok(TestRole::Client),
                _ => Err(zznet_auth::error::AuthError::UnknownRole(cn.to_string())),
            }
        }

        fn as_str(&self) -> &'static str {
            match self {
                TestRole::Server => "server",
                TestRole::Client => "client",
            }
        }

        fn can_connect_to(&self, _: &Self) -> bool {
            true
        }

        fn can_access_room(&self, _: &str) -> bool {
            true
        }
    }

    // Simple test message
    #[derive(Debug, Clone, Serialize, Deserialize)]
    enum TestMessage {
        Data(String),
    }

    impl RoomMessageTrait for TestMessage {
        fn room_id(&self) -> RoomId {
            RoomId::from("test")
        }

        fn serialize_inner(
            &self,
        ) -> Result<Vec<u8>, zznet_session::room_message_trait::SerializationError> {
            bincode::serde::encode_to_vec(self, bincode::config::standard()).map_err(|e| {
                zznet_session::room_message_trait::SerializationError::BincodeError(e.to_string())
            })
        }

        fn deserialize_for_room(
            _room_id: &RoomId,
            bytes: &[u8],
        ) -> Result<Self, zznet_session::room_message_trait::DeserializationError> {
            bincode::serde::decode_from_slice(bytes, bincode::config::standard())
                .map(|(v, _)| v)
                .map_err(|e| {
                    zznet_session::room_message_trait::DeserializationError::BincodeError(
                        e.to_string(),
                    )
                })
        }

        fn supported_rooms() -> Vec<RoomId> {
            vec![RoomId::from("test")]
        }
    }

    // Simple test handler
    struct TestRoomHandler {
        room_id: RoomId,
        #[allow(dead_code)]
        message_count: Arc<Mutex<usize>>,
    }

    impl RoomHandle for TestRoomHandler {
        fn room_id(&self) -> &RoomId {
            &self.room_id
        }

        fn send_message(&mut self, _msg: Vec<u8>) -> Result<(), SessionError> {
            Ok(())
        }

        fn spawn_forwarder(
            &mut self,
            _tx: tokio::sync::mpsc::Sender<(RoomId, Vec<u8>)>,
        ) -> Result<(), SessionError> {
            Ok(())
        }
    }

    // Test factory
    struct TestRoomHandlerFactory {
        message_count: Arc<Mutex<usize>>,
    }

    impl RoomHandlerFactory<TestMessage, TestRole> for TestRoomHandlerFactory {
        fn create_handler(&self, room_id: RoomId) -> Box<dyn RoomHandle> {
            Box::new(TestRoomHandler {
                room_id,
                message_count: Arc::clone(&self.message_count),
            })
        }
    }

    #[tokio::test]
    async fn test_room_registry_creation() {
        let session_manager = Arc::new(Mutex::new(SessionManager::<TestRole>::new(vec![
            RoomId::from("test"),
        ])));

        let _registry: RoomRegistry<TestMessage, TestRole> =
            RoomRegistry::new(Arc::clone(&session_manager));
        // Registry created successfully
    }

    #[tokio::test]
    async fn test_room_registry_register_and_wire() {
        let session_manager = Arc::new(Mutex::new(SessionManager::<TestRole>::new(vec![
            RoomId::from("test"),
        ])));

        let mut registry = RoomRegistry::new(Arc::clone(&session_manager));

        // Register a handler factory
        let factory = TestRoomHandlerFactory {
            message_count: Arc::new(Mutex::new(0)),
        };
        registry.register_room_handler(RoomId::from("test"), Arc::new(factory));

        // Wire all peers (should succeed even with no peers)
        let result = registry.wire_all_peers().await;
        assert!(result.is_ok(), "Failed to wire peers: {:?}", result);
    }

    #[test]
    fn test_room_registry_documentation() {
        // This test documents the intended usage pattern for future maintainers
        //
        // Pattern 1: Create registry
        //   let registry = RoomRegistry::new(session_manager);
        //
        // Pattern 2: Register handlers (can do multiple)
        //   registry.register_room_handler(room_id, Arc::new(factory_instance));
        //
        // Pattern 3a: Wire at startup
        //   registry.wire_all_peers().await?;
        //
        // Pattern 3b: Wire for dynamic peer
        //   registry.wire_peer(&peer_id).await?;
        //
        // This replaces the old manual pattern of:
        //   1. Lock SessionManager
        //   2. Get peer IDs
        //   3. For each peer, create handler and call add_room_to_peer()
        //   4. Repeat for each room
        //
        // The new pattern is cleaner, more testable, and reusable across apps.
    }
}
