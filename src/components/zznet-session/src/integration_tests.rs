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
use tokio::sync::mpsc;
use zznet_room::room::Room;

// ============================================================================
// Test Actors (Implement actix::Message for test messages)
// ============================================================================

// We need to wrap our test messages to make them actix::Message compatible
#[derive(Clone, Debug, PartialEq, Message)]
#[rtype(result = "()")]
struct ActixIntentConfigMessage(IntentConfigMessage);

#[derive(Clone, Debug, PartialEq, Message)]
#[rtype(result = "()")]
struct ActixMemDBMessage(MemDBMessage);

#[derive(Clone, Debug, PartialEq, Message)]
#[rtype(result = "()")]
struct ActixHealthMessage(HealthMessage);

// Conversions: Actix wrapper ↔ CollectorMessages
impl From<ActixIntentConfigMessage> for CollectorMessages {
    fn from(msg: ActixIntentConfigMessage) -> Self {
        CollectorMessages::IntentConfig(msg.0)
    }
}

impl TryFrom<CollectorMessages> for ActixIntentConfigMessage {
    type Error = ();
    fn try_from(msg: CollectorMessages) -> Result<Self, Self::Error> {
        match msg {
            CollectorMessages::IntentConfig(m) => Ok(ActixIntentConfigMessage(m)),
            _ => Err(()),
        }
    }
}

impl From<ActixMemDBMessage> for CollectorMessages {
    fn from(msg: ActixMemDBMessage) -> Self {
        CollectorMessages::MemDB(msg.0)
    }
}

impl TryFrom<CollectorMessages> for ActixMemDBMessage {
    type Error = ();
    fn try_from(msg: CollectorMessages) -> Result<Self, Self::Error> {
        match msg {
            CollectorMessages::MemDB(m) => Ok(ActixMemDBMessage(m)),
            _ => Err(()),
        }
    }
}

impl From<ActixHealthMessage> for CollectorMessages {
    fn from(msg: ActixHealthMessage) -> Self {
        CollectorMessages::Health(msg.0)
    }
}

impl TryFrom<CollectorMessages> for ActixHealthMessage {
    type Error = ();
    fn try_from(msg: CollectorMessages) -> Result<Self, Self::Error> {
        match msg {
            CollectorMessages::Health(m) => Ok(ActixHealthMessage(m)),
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

impl Handler<ActixIntentConfigMessage> for CollectorActor {
    type Result = ();
    fn handle(&mut self, msg: ActixIntentConfigMessage, _ctx: &mut Context<Self>) {
        self.received.push(format!("IntentConfig: {:?}", msg.0));
    }
}

impl Handler<ActixMemDBMessage> for CollectorActor {
    type Result = ();
    fn handle(&mut self, msg: ActixMemDBMessage, _ctx: &mut Context<Self>) {
        self.received.push(format!("MemDB: {:?}", msg.0));
    }
}

impl Handler<ActixHealthMessage> for CollectorActor {
    type Result = ();
    fn handle(&mut self, msg: ActixHealthMessage, _ctx: &mut Context<Self>) {
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
        let (_room, channels) = Room::<ActixIntentConfigMessage>::new(actor.recipient());

        // Create adapter
        let (peer_tx, _peer_rx) = mpsc::channel(10);
        let adapter = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
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
        let (mut room, channels) = Room::<ActixIntentConfigMessage>::new(actor.recipient());

        // Spawn room receiver
        room.spawn_receiver().unwrap();

        // Create adapter
        let (peer_tx, _peer_rx) = mpsc::channel(10);
        let mut adapter = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
            RoomId::from("intentconfig"),
            channels.inbound_tx,
            channels.outbound_rx,
            peer_tx,
        );

        // Send message through adapter
        let msg = CollectorMessages::IntentConfig(IntentConfigMessage::Query);
        let result = adapter.send_message(msg);

        assert!(result.is_ok());
    }

    #[actix::test]
    async fn test_room_adapter_send_wrong_message_type() {
        // Create IntentConfig room
        let actor = CollectorActor { received: vec![] }.start();
        let (_room, channels) = Room::<ActixIntentConfigMessage>::new(actor.recipient());

        // Create adapter
        let (peer_tx, _peer_rx) = mpsc::channel(10);
        let mut adapter = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
            RoomId::from("intentconfig"),
            channels.inbound_tx,
            channels.outbound_rx,
            peer_tx,
        );

        // Try to send MemDB message (wrong type!)
        let msg = CollectorMessages::MemDB(MemDBMessage::Retrieve {
            key: "test".to_string(),
        });
        let result = adapter.send_message(msg);

        // Should fail with WrongMessageType
        assert!(result.is_err());
        match result {
            Err(SessionError::WrongMessageType) => {
                // Expected
            }
            _ => panic!("Expected WrongMessageType error"),
        }
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
        let mut adapter = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
            RoomId::from("intentconfig"),
            inbound_tx,
            outbound_rx,
            peer_tx,
        );

        // Fill the channel
        let msg1 = CollectorMessages::IntentConfig(IntentConfigMessage::Query);
        adapter.send_message(msg1.clone()).unwrap();

        // Try to send another (buffer full)
        let result = adapter.send_message(msg1);

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
        let (room, channels) = Room::<ActixIntentConfigMessage>::new(actor.recipient());

        // Create adapter with peer channel
        let (peer_tx, mut peer_rx) = mpsc::channel(10);
        let _adapter = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
            RoomId::from("intentconfig"),
            channels.inbound_tx,
            channels.outbound_rx,
            peer_tx,
        );

        // Send message through room (outbound direction)
        let msg = ActixIntentConfigMessage(IntentConfigMessage::Query);
        room.send(msg.clone()).await.unwrap();

        // Should receive on peer channel (converted to CollectorMessages)
        let (room_id, received_msg) = peer_rx.recv().await.unwrap();
        assert_eq!(room_id.as_str(), "intentconfig");

        match received_msg {
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
        let (_room, channels) = Room::<ActixIntentConfigMessage>::new(actor.recipient());

        // Create adapter
        let (peer_tx, _peer_rx) = mpsc::channel(10);
        let adapter = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
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
        let (_room, channels) = Room::<ActixIntentConfigMessage>::new(actor.recipient());

        // Create adapter (forwarder spawned in constructor)
        let (peer_tx, _peer_rx) = mpsc::channel(10);
        let mut adapter = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
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

        let (_room1, channels1) = Room::<ActixIntentConfigMessage>::new(actor1.recipient());
        let (_room2, channels2) = Room::<ActixMemDBMessage>::new(actor2.recipient());
        let (_room3, channels3) = Room::<ActixHealthMessage>::new(actor3.recipient());

        // Create adapters with same TMsg type
        let (peer_tx, _peer_rx) = mpsc::channel(10);
        let adapter1 = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
            RoomId::from("intentconfig"),
            channels1.inbound_tx,
            channels1.outbound_rx,
            peer_tx.clone(),
        );
        let adapter2 = RoomAdapter::<ActixMemDBMessage, CollectorMessages>::new(
            RoomId::from("memdb"),
            channels2.inbound_tx,
            channels2.outbound_rx,
            peer_tx.clone(),
        );
        let adapter3 = RoomAdapter::<ActixHealthMessage, CollectorMessages>::new(
            RoomId::from("health"),
            channels3.inbound_tx,
            channels3.outbound_rx,
            peer_tx,
        );

        // Type erase and store in HashMap (THIS IS THE KEY TEST!)
        let mut rooms: HashMap<RoomId, Box<dyn RoomHandle<CollectorMessages>>> = HashMap::new();
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
            .send_message(msg1)
            .unwrap();
        rooms
            .get_mut(&RoomId::from("memdb"))
            .unwrap()
            .send_message(msg2)
            .unwrap();
        rooms
            .get_mut(&RoomId::from("health"))
            .unwrap()
            .send_message(msg3)
            .unwrap();

        // Success: Different Room<T> types stored and messaged via same interface!
    }

    #[actix::test]
    async fn test_wrong_message_to_room_via_trait_object() {
        // Create IntentConfig room
        let actor = CollectorActor { received: vec![] }.start();
        let (_room, channels) = Room::<ActixIntentConfigMessage>::new(actor.recipient());

        // Create adapter
        let (peer_tx, _peer_rx) = mpsc::channel(10);
        let adapter = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
            RoomId::from("intentconfig"),
            channels.inbound_tx,
            channels.outbound_rx,
            peer_tx,
        );

        // Type erase
        let mut boxed: Box<dyn RoomHandle<CollectorMessages>> = Box::new(adapter);

        // Try to send MemDB message to IntentConfig room
        let msg = CollectorMessages::MemDB(MemDBMessage::Retrieve {
            key: "test".to_string(),
        });
        let result = boxed.send_message(msg);

        // Should fail
        assert!(result.is_err());
        match result {
            Err(SessionError::WrongMessageType) => {
                // Expected
            }
            _ => panic!("Expected WrongMessageType error"),
        }
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
        let mut peer_session = PeerSession::<CollectorMessages>::new(PeerId::from("test_peer"));

        // Create rooms
        let actor1 = CollectorActor { received: vec![] }.start();
        let actor2 = CollectorActor { received: vec![] }.start();

        let (mut room1, channels1) = Room::<ActixIntentConfigMessage>::new(actor1.recipient());
        let (mut room2, channels2) = Room::<ActixMemDBMessage>::new(actor2.recipient());

        // Spawn room receivers
        room1.spawn_receiver().unwrap();
        room2.spawn_receiver().unwrap();

        // Create peer channels
        let (peer_tx, mut peer_rx) = mpsc::channel(10);
        let (inbound_tx, inbound_rx) = mpsc::channel(10);

        // Create adapters
        let adapter1 = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
            RoomId::from("intentconfig"),
            channels1.inbound_tx,
            channels1.outbound_rx,
            peer_tx.clone(),
        );
        let adapter2 = RoomAdapter::<ActixMemDBMessage, CollectorMessages>::new(
            RoomId::from("memdb"),
            channels2.inbound_tx,
            channels2.outbound_rx,
            peer_tx,
        );

        // Add rooms to peer session
        peer_session
            .add_room(RoomId::from("intentconfig"), Box::new(adapter1))
            .unwrap();
        peer_session
            .add_room(RoomId::from("memdb"), Box::new(adapter2))
            .unwrap();

        // Connect peer session
        peer_session.connect(inbound_tx, inbound_rx).unwrap();

        // Test outbound: Component → Room → Adapter → Peer
        room1
            .send(ActixIntentConfigMessage(IntentConfigMessage::Query))
            .await
            .unwrap();

        // Should receive on peer channel
        let (room_id, msg) = peer_rx.recv().await.unwrap();
        assert_eq!(room_id.as_str(), "intentconfig");
        match msg {
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
        let mut peer_session = PeerSession::<CollectorMessages>::new(PeerId::from("test_peer"));

        // Create 3 rooms
        let actor1 = CollectorActor { received: vec![] }.start();
        let actor2 = CollectorActor { received: vec![] }.start();
        let actor3 = CollectorActor { received: vec![] }.start();

        let (mut room1, channels1) = Room::<ActixIntentConfigMessage>::new(actor1.recipient());
        let (mut room2, channels2) = Room::<ActixMemDBMessage>::new(actor2.recipient());
        let (mut room3, channels3) = Room::<ActixHealthMessage>::new(actor3.recipient());

        room1.spawn_receiver().unwrap();
        room2.spawn_receiver().unwrap();
        room3.spawn_receiver().unwrap();

        // Create peer channels
        let (peer_tx, mut peer_rx) = mpsc::channel(10);
        let (inbound_tx, inbound_rx) = mpsc::channel(10);

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
            .unwrap();
        peer_session
            .add_room(RoomId::from("memdb"), Box::new(adapter2))
            .unwrap();
        peer_session
            .add_room(RoomId::from("health"), Box::new(adapter3))
            .unwrap();

        // Connect
        peer_session.connect(inbound_tx, inbound_rx).unwrap();

        // Send messages from all rooms concurrently
        room1
            .send(ActixIntentConfigMessage(IntentConfigMessage::Query))
            .await
            .unwrap();
        room2
            .send(ActixMemDBMessage(MemDBMessage::Retrieve {
                key: "test".to_string(),
            }))
            .await
            .unwrap();
        room3
            .send(ActixHealthMessage(HealthMessage::Ping))
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
