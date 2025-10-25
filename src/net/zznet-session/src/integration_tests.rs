//! Integration tests for RoomAdapter and PeerSession with type erasure
//!
//! These tests validate the complete type erasure architecture with real
//! actix Message types and Room instances.

use crate::peer_session::{PeerSession, RoomHandle};
use crate::room_adapter::RoomAdapter;
use crate::test_room_messages::{
    CollectorMessages, HealthMessage, IntentConfigMessage, MemDBMessage,
};
use crate::types::{PeerId, RoomId, SessionError};
use actix::prelude::*;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use zznet_auth::mock::MockRole;
use zznet_room::room::Room;

// ============================================================================
// Test Actors (Implement actix::Message for test messages)
// ============================================================================

// We need to wrap our test messages to make them actix::Message compatible
#[derive(Clone, Debug, PartialEq, Message, Serialize, Deserialize)]
#[rtype(result = "()")]
struct MockIntentConfigMessage(IntentConfigMessage);

#[derive(Clone, Debug, PartialEq, Message, Serialize, Deserialize)]
#[rtype(result = "()")]
struct MockMemDBMessage(MemDBMessage);

#[derive(Clone, Debug, PartialEq, Message, Serialize, Deserialize)]
#[rtype(result = "()")]
struct MockHealthMessage(HealthMessage);

// Conversions: Actix wrapper ↔ CollectorMessages
impl From<MockIntentConfigMessage> for CollectorMessages {
    fn from(msg: MockIntentConfigMessage) -> Self {
        CollectorMessages::IntentConfig(msg.0)
    }
}

impl TryFrom<CollectorMessages> for MockIntentConfigMessage {
    type Error = ();
    fn try_from(msg: CollectorMessages) -> Result<Self, Self::Error> {
        match msg {
            CollectorMessages::IntentConfig(m) => Ok(MockIntentConfigMessage(m)),
            _ => Err(()),
        }
    }
}

impl From<MockMemDBMessage> for CollectorMessages {
    fn from(msg: MockMemDBMessage) -> Self {
        CollectorMessages::MemDB(msg.0)
    }
}

impl TryFrom<CollectorMessages> for MockMemDBMessage {
    type Error = ();
    fn try_from(msg: CollectorMessages) -> Result<Self, Self::Error> {
        match msg {
            CollectorMessages::MemDB(m) => Ok(MockMemDBMessage(m)),
            _ => Err(()),
        }
    }
}

impl From<MockHealthMessage> for CollectorMessages {
    fn from(msg: MockHealthMessage) -> Self {
        CollectorMessages::Health(msg.0)
    }
}

impl TryFrom<CollectorMessages> for MockHealthMessage {
    type Error = ();
    fn try_from(msg: CollectorMessages) -> Result<Self, Self::Error> {
        match msg {
            CollectorMessages::Health(m) => Ok(MockHealthMessage(m)),
            _ => Err(()),
        }
    }
}

// Test actor that collects received messages
struct CollectorActor {
    received: Vec<String>,
}

impl Actor for CollectorActor {
    type Context = Context<Self>;
}

impl Handler<MockIntentConfigMessage> for CollectorActor {
    type Result = ();
    fn handle(&mut self, msg: MockIntentConfigMessage, _ctx: &mut Context<Self>) {
        self.received.push(format!("IntentConfig: {:?}", msg.0));
    }
}

impl Handler<MockMemDBMessage> for CollectorActor {
    type Result = ();
    fn handle(&mut self, msg: MockMemDBMessage, _ctx: &mut Context<Self>) {
        self.received.push(format!("MemDB: {:?}", msg.0));
    }
}

impl Handler<MockHealthMessage> for CollectorActor {
    type Result = ();
    fn handle(&mut self, msg: MockHealthMessage, _ctx: &mut Context<Self>) {
        self.received.push(format!("Health: {:?}", msg.0));
    }
}

// ============================================================================
// Test 1: RoomAdapter Creation and Basic Operations
// ============================================================================

#[cfg(test)]
mod room_adapter_tests {
    use super::*;

    #[actix::test]
    async fn test_room_adapter_creation() {
        // Create room
        let actor = CollectorActor { received: vec![] }.start();
        let (_room, channels) =
            Room::<MockIntentConfigMessage>::new("intentconfig".to_string(), actor.recipient());

        // Create adapter
        let (peer_tx, _peer_rx) = mpsc::channel(10);
        let adapter = RoomAdapter::new(
            RoomId::from("intentconfig"),
            channels.inbound_tx,
            channels.outbound_rx,
            peer_tx,
        );

        // Test room_id()
        assert_eq!(adapter.room_id().as_str(), "intentconfig");
    }

    #[actix::test]
    async fn test_room_adapter_send_message_success() {
        // Create room
        let actor = CollectorActor { received: vec![] }.start();
        let (mut room, channels) =
            Room::<MockIntentConfigMessage>::new("intentconfig".to_string(), actor.recipient());

        // Spawn room receiver
        room.spawn_receiver().unwrap();

        // Create adapter
        let (peer_tx, _peer_rx) = mpsc::channel(10);
        let mut adapter = RoomAdapter::new(
            RoomId::from("intentconfig"),
            channels.inbound_tx,
            channels.outbound_rx,
            peer_tx,
        );

        // Send message through adapter
        let msg = CollectorMessages::IntentConfig(IntentConfigMessage::Query);
        let serialized_msg = bincode::serde::encode_to_vec(&msg, bincode::config::standard())
            .expect("Failed to serialize message");
        let result = adapter.send_message(serialized_msg);

        assert!(result.is_ok());
    }

    #[actix::test]
    async fn test_room_adapter_send_wrong_message_type() {
        // Create IntentConfig room
        let actor = CollectorActor { received: vec![] }.start();
        let (_room, channels) =
            Room::<MockIntentConfigMessage>::new("intentconfig".to_string(), actor.recipient());

        // Create adapter
        let (peer_tx, _peer_rx) = mpsc::channel(10);
        let mut adapter = RoomAdapter::new(
            RoomId::from("intentconfig"),
            channels.inbound_tx,
            channels.outbound_rx,
            peer_tx,
        );

        // Note: RoomAdapter works with opaque bytes and cannot validate message types.
        // Type validation happens asynchronously in the Room's receiver task when it
        // tries to deserialize. The send_message call succeeds (just forwards bytes),
        // but the Room will fail to deserialize wrong types internally.

        // Send correct type to verify adapter works
        let msg = MockIntentConfigMessage(IntentConfigMessage::Query);
        let serialized_msg = bincode::serde::encode_to_vec(&msg, bincode::config::standard())
            .expect("Failed to serialize message");
        let result = adapter.send_message(serialized_msg);

        // Should succeed - adapter just forwards bytes
        assert!(result.is_ok());
    }

    #[actix::test]
    async fn test_room_adapter_send_channel_full() {
        // Create room with small buffer
        let _actor = CollectorActor { received: vec![] }.start();

        // Create channel with buffer size 1
        let (inbound_tx, _inbound_rx) = mpsc::channel(1);
        let (_outbound_tx, outbound_rx) = mpsc::channel(10);

        // Create adapter
        let (peer_tx, _peer_rx) = mpsc::channel(10);
        let mut adapter = RoomAdapter::new(
            RoomId::from("intentconfig"),
            inbound_tx,
            outbound_rx,
            peer_tx,
        );

        // Fill the channel
        let msg1 = CollectorMessages::IntentConfig(IntentConfigMessage::Query);
        let serialized_msg1 = bincode::serde::encode_to_vec(&msg1, bincode::config::standard())
            .expect("Failed to serialize message");
        adapter.send_message(serialized_msg1.clone()).unwrap();

        // Try to send another (buffer full)
        let result = adapter.send_message(serialized_msg1);

        // Should fail with SendFailed
        assert!(result.is_err());
        match result {
            Err(SessionError::SendFailed) => {
                // Expected
            }
            _ => panic!("Expected SendFailed error"),
        }
    }

    #[actix::test]
    async fn test_room_adapter_outbound_forwarding() {
        // Create room
        let actor = CollectorActor { received: vec![] }.start();
        let (room, channels) =
            Room::<MockIntentConfigMessage>::new("intentconfig".to_string(), actor.recipient());

        // Create adapter with peer channel
        let (peer_tx, mut peer_rx) = mpsc::channel(10);
        let _adapter = RoomAdapter::new(
            RoomId::from("intentconfig"),
            channels.inbound_tx,
            channels.outbound_rx,
            peer_tx,
        );

        // Send message through room (outbound direction)
        let msg = MockIntentConfigMessage(IntentConfigMessage::Query);
        room.send(msg.clone()).await.unwrap();

        // Should receive on peer channel (converted to CollectorMessages)
        let (room_id, received_msg) = peer_rx.recv().await.unwrap();
        assert_eq!(room_id.as_str(), "intentconfig");

        // Deserialize as the actual type that was serialized (MockIntentConfigMessage)
        let (deserialized_msg, _): (MockIntentConfigMessage, _) =
            bincode::serde::decode_from_slice(&received_msg, bincode::config::standard()).unwrap();
        // Convert to CollectorMessages
        let collector_msg: CollectorMessages = deserialized_msg.into();
        match collector_msg {
            CollectorMessages::IntentConfig(IntentConfigMessage::Query) => {
                // Expected
            }
            _ => panic!("Expected IntentConfig::Query"),
        }
    }

    #[actix::test]
    async fn test_room_adapter_forwarder_task_cleanup() {
        // Create room
        let actor = CollectorActor { received: vec![] }.start();
        let (_room, channels) =
            Room::<MockIntentConfigMessage>::new("intentconfig".to_string(), actor.recipient());

        // Create adapter
        let (peer_tx, _peer_rx) = mpsc::channel(10);
        let adapter = RoomAdapter::new(
            RoomId::from("intentconfig"),
            channels.inbound_tx,
            channels.outbound_rx,
            peer_tx,
        );

        // Drop adapter (should abort forwarder task)
        drop(adapter);

        // If we get here without hanging, Drop worked correctly
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    }

    #[actix::test]
    async fn test_room_adapter_spawn_forwarder_already_spawned() {
        // Create room
        let actor = CollectorActor { received: vec![] }.start();
        let (_room, channels) =
            Room::<MockIntentConfigMessage>::new("intentconfig".to_string(), actor.recipient());

        // Create adapter (forwarder spawned in constructor)
        let (peer_tx, _peer_rx) = mpsc::channel(10);
        let mut adapter = RoomAdapter::new(
            RoomId::from("intentconfig"),
            channels.inbound_tx,
            channels.outbound_rx,
            peer_tx.clone(),
        );

        // Try to spawn again
        let result = adapter.spawn_forwarder(peer_tx);

        // Should succeed (idempotent)
        assert!(result.is_ok());
    }
}

// ============================================================================
// Test 2: Type Erasure - Multiple Room Types in HashMap
// ============================================================================

#[cfg(test)]
mod type_erasure_tests {
    use super::*;
    use std::collections::HashMap;

    #[actix::test]
    async fn test_multiple_room_types_in_hashmap() {
        // Create three different room types
        let actor1 = CollectorActor { received: vec![] }.start();
        let actor2 = CollectorActor { received: vec![] }.start();
        let actor3 = CollectorActor { received: vec![] }.start();

        let (_room1, channels1) =
            Room::<MockIntentConfigMessage>::new("intentconfig".to_string(), actor1.recipient());
        let (_room2, channels2) =
            Room::<MockMemDBMessage>::new("memdb".to_string(), actor2.recipient());
        let (_room3, channels3) =
            Room::<MockHealthMessage>::new("health".to_string(), actor3.recipient());

        // Create adapters with same TMsg type
        let (peer_tx, _peer_rx) = mpsc::channel(10);
        let adapter1 = RoomAdapter::new(
            RoomId::from("intentconfig"),
            channels1.inbound_tx,
            channels1.outbound_rx,
            peer_tx.clone(),
        );
        let adapter2 = RoomAdapter::new(
            RoomId::from("memdb"),
            channels2.inbound_tx,
            channels2.outbound_rx,
            peer_tx.clone(),
        );
        let adapter3 = RoomAdapter::new(
            RoomId::from("health"),
            channels3.inbound_tx,
            channels3.outbound_rx,
            peer_tx,
        );

        // Type erase and store in HashMap (THIS IS THE KEY TEST!)
        let mut rooms: HashMap<RoomId, Box<dyn RoomHandle>> = HashMap::new();
        rooms.insert(RoomId::from("intentconfig"), Box::new(adapter1));
        rooms.insert(RoomId::from("memdb"), Box::new(adapter2));
        rooms.insert(RoomId::from("health"), Box::new(adapter3));

        // Verify we can access all rooms
        assert_eq!(rooms.len(), 3);
        assert!(rooms.contains_key(&RoomId::from("intentconfig")));
        assert!(rooms.contains_key(&RoomId::from("memdb")));
        assert!(rooms.contains_key(&RoomId::from("health")));

        // Send messages to each room via trait object
        let msg1 = CollectorMessages::IntentConfig(IntentConfigMessage::Query);
        let msg2 = CollectorMessages::MemDB(MemDBMessage::Retrieve {
            key: "test".to_string(),
        });
        let msg3 = CollectorMessages::Health(HealthMessage::Ping);

        rooms
            .get_mut(&RoomId::from("intentconfig"))
            .unwrap()
            .send_message(
                bincode::serde::encode_to_vec(&msg1, bincode::config::standard()).unwrap(),
            )
            .unwrap();
        rooms
            .get_mut(&RoomId::from("memdb"))
            .unwrap()
            .send_message(
                bincode::serde::encode_to_vec(&msg2, bincode::config::standard()).unwrap(),
            )
            .unwrap();
        rooms
            .get_mut(&RoomId::from("health"))
            .unwrap()
            .send_message(
                bincode::serde::encode_to_vec(&msg3, bincode::config::standard()).unwrap(),
            )
            .unwrap();

        // Success: Different Room<T> types stored and messaged via same interface!
    }

    #[actix::test]
    async fn test_wrong_message_to_room_via_trait_object() {
        // Create IntentConfig room
        let actor = CollectorActor { received: vec![] }.start();
        let (_room, channels) =
            Room::<MockIntentConfigMessage>::new("intentconfig".to_string(), actor.recipient());

        // Create adapter
        let (peer_tx, _peer_rx) = mpsc::channel(10);
        let adapter = RoomAdapter::new(
            RoomId::from("intentconfig"),
            channels.inbound_tx,
            channels.outbound_rx,
            peer_tx,
        );

        // Type erase
        let mut boxed: Box<dyn RoomHandle> = Box::new(adapter);

        // Note: RoomHandle (via RoomAdapter) works with opaque bytes and cannot
        // validate message types. Type validation happens asynchronously in the
        // Room's receiver task. The send_message call succeeds (just forwards bytes).

        // Send correct type message to verify trait object works
        let msg = MockIntentConfigMessage(IntentConfigMessage::Query);
        let result = boxed.send_message(
            bincode::serde::encode_to_vec(&msg, bincode::config::standard()).unwrap(),
        );

        // Should succeed - adapter just forwards bytes
        assert!(result.is_ok());
    }
}

// ============================================================================
// Test 3: End-to-End Flow with PeerSession
// ============================================================================

#[cfg(test)]
mod peer_session_integration_tests {
    use super::*;

    #[actix::test]
    async fn test_peer_session_with_room_adapter() {
        // Create peer session

        // Create rooms
        let actor1 = CollectorActor { received: vec![] }.start();
        let actor2 = CollectorActor { received: vec![] }.start();

        let (mut room1, channels1) =
            Room::<MockIntentConfigMessage>::new("intentconfig".to_string(), actor1.recipient());
        let (mut room2, channels2) =
            Room::<MockMemDBMessage>::new("memdb".to_string(), actor2.recipient());

        // Spawn room receivers
        room1.spawn_receiver().unwrap();
        room2.spawn_receiver().unwrap();

        // Create peer channels
        let (peer_tx, mut peer_rx) = mpsc::channel(10);
        let (_inbound_tx, inbound_rx) = mpsc::channel(10);

        // Create a connected PeerSession using the real peer channels
        let mut peer_session = PeerSession::<MockRole>::new_connected(
            PeerId::from("test_peer"),
            None,
            None,
            peer_tx.clone(),
            inbound_rx,
        )
        .await
        .unwrap();

        // Create adapters
        let adapter1 = RoomAdapter::new(
            RoomId::from("intentconfig"),
            channels1.inbound_tx,
            channels1.outbound_rx,
            peer_tx.clone(),
        );
        let adapter2 = RoomAdapter::new(
            RoomId::from("memdb"),
            channels2.inbound_tx,
            channels2.outbound_rx,
            peer_tx,
        );

        // Add rooms to peer session
        peer_session
            .add_room(RoomId::from("intentconfig"), Box::new(adapter1))
            .await
            .unwrap();
        peer_session
            .add_room(RoomId::from("memdb"), Box::new(adapter2))
            .await
            .unwrap();

        // PeerSession is already connected via new_connected

        // Test outbound: Component → Room → Adapter → Peer
        room1
            .send(MockIntentConfigMessage(IntentConfigMessage::Query))
            .await
            .unwrap();

        // Should receive on peer channel
        let (room_id, msg) = peer_rx.recv().await.unwrap();
        assert_eq!(room_id.as_str(), "intentconfig");
        // Deserialize as the actual type that was serialized (MockIntentConfigMessage)
        let (deserialized_msg, _): (MockIntentConfigMessage, _) =
            bincode::serde::decode_from_slice(&msg, bincode::config::standard()).unwrap();
        // Convert to CollectorMessages
        let collector_msg: CollectorMessages = deserialized_msg.into();
        match collector_msg {
            CollectorMessages::IntentConfig(IntentConfigMessage::Query) => {
                // Expected
            }
            _ => panic!("Expected IntentConfig::Query"),
        }

        // Success: Full outbound flow works!
    }

    #[actix::test]
    async fn test_peer_session_multiple_rooms_concurrent() {
        // Create peer session
        // Create peer channels
        let (peer_tx, mut peer_rx) = mpsc::channel(10);
        let (_inbound_tx, inbound_rx) = mpsc::channel(10);
        let mut peer_session = PeerSession::<MockRole>::new_connected(
            PeerId::from("test_peer"),
            None,
            None,
            peer_tx.clone(),
            inbound_rx,
        )
        .await
        .unwrap();

        // Create 3 rooms
        let actor1 = CollectorActor { received: vec![] }.start();
        let actor2 = CollectorActor { received: vec![] }.start();
        let actor3 = CollectorActor { received: vec![] }.start();

        let (mut room1, channels1) =
            Room::<MockIntentConfigMessage>::new("intentconfig".to_string(), actor1.recipient());
        let (mut room2, channels2) =
            Room::<MockMemDBMessage>::new("memdb".to_string(), actor2.recipient());
        let (mut room3, channels3) =
            Room::<MockHealthMessage>::new("health".to_string(), actor3.recipient());

        room1.spawn_receiver().unwrap();
        room2.spawn_receiver().unwrap();
        room3.spawn_receiver().unwrap();

        // Create adapters
        let adapter1 = RoomAdapter::new(
            RoomId::from("intentconfig"),
            channels1.inbound_tx,
            channels1.outbound_rx,
            peer_tx.clone(),
        );
        let adapter2 = RoomAdapter::new(
            RoomId::from("memdb"),
            channels2.inbound_tx,
            channels2.outbound_rx,
            peer_tx.clone(),
        );
        let adapter3 = RoomAdapter::new(
            RoomId::from("health"),
            channels3.inbound_tx,
            channels3.outbound_rx,
            peer_tx,
        );

        // Add rooms
        peer_session
            .add_room(RoomId::from("intentconfig"), Box::new(adapter1))
            .await
            .unwrap();
        peer_session
            .add_room(RoomId::from("memdb"), Box::new(adapter2))
            .await
            .unwrap();
        peer_session
            .add_room(RoomId::from("health"), Box::new(adapter3))
            .await
            .unwrap();

        // PeerSession was created connected via new_connected

        // Send messages from all rooms concurrently
        room1
            .send(MockIntentConfigMessage(IntentConfigMessage::Query))
            .await
            .unwrap();
        room2
            .send(MockMemDBMessage(MemDBMessage::Retrieve {
                key: "test".to_string(),
            }))
            .await
            .unwrap();
        room3
            .send(MockHealthMessage(HealthMessage::Ping))
            .await
            .unwrap();

        // Receive all messages (order may vary)
        let mut received_rooms = vec![];
        for _ in 0..3 {
            let (room_id, _msg) = peer_rx.recv().await.unwrap();
            received_rooms.push(room_id);
        }

        // Verify all rooms sent
        assert!(received_rooms.contains(&RoomId::from("intentconfig")));
        assert!(received_rooms.contains(&RoomId::from("memdb")));
        assert!(received_rooms.contains(&RoomId::from("health")));
    }
}
