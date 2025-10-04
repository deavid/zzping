use crate::room_message_trait::RoomMessageTrait;
use crate::types::{ConnectionState, PeerId, RoomId, SessionError};
use std::collections::HashMap;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

/// Trait for type-erased room operations
///
/// This trait allows PeerSession to store rooms with different component message types
/// (Room<IntentConfigMessage>, Room<MemDBMessage>, etc.) in a single collection.
/// All rooms share the same application message type TMsg.
pub trait RoomHandle<TMsg>: Send
where
    TMsg: RoomMessageTrait,
{
    /// Get the room ID
    fn room_id(&self) -> &RoomId;

    /// Send a message to this room's component
    /// The TMsg will be converted to the room's specific type T
    fn send_message(&mut self, msg: TMsg) -> Result<(), SessionError>;

    /// Spawn a task to forward outbound messages from this room
    /// Room messages (type T) are converted to TMsg before sending
    fn spawn_forwarder(&mut self, tx: mpsc::Sender<(RoomId, TMsg)>) -> Result<(), SessionError>;
}

/// A session with one peer
/// Manages rooms and connection state for this specific peer
///
/// Generic over TMsg: the application's message enum type that wraps all room messages.
/// Each application defines its own enum (e.g., CollectorMessages, DatabaseMessages).
///
/// Each peer session contains Room<T> instances with different T types (type-erased via
/// RoomHandle trait). Messages flow through the application's enum type TMsg.
pub struct PeerSession<TMsg>
where
    TMsg: RoomMessageTrait,
{
    peer_id: PeerId,
    state: ConnectionState,

    // Type-erased room storage (each room can have different T)
    rooms: HashMap<RoomId, Box<dyn RoomHandle<TMsg>>>,

    // Outbound: send typed messages to peer
    outbound_tx: Option<mpsc::Sender<(RoomId, TMsg)>>,

    // Inbound task: receives typed messages from peer and routes to rooms
    inbound_task: Option<JoinHandle<()>>,

    // Phase 6: Room negotiation state
    // Rooms offered by the remote peer (received via PublishRooms)
    peer_offered_rooms: Option<Vec<RoomId>>,

    // Rooms we've joined (intersection of local and peer offered rooms)
    joined_rooms: Vec<RoomId>,
}

impl<TMsg> PeerSession<TMsg>
where
    TMsg: RoomMessageTrait,
{
    /// Create a new disconnected peer session
    ///
    /// Rooms must be added via `add_room()` before connecting.
    pub fn new(peer_id: PeerId) -> Self {
        Self {
            peer_id,
            state: ConnectionState::Disconnected,
            rooms: HashMap::new(),
            outbound_tx: None,
            inbound_task: None,
            peer_offered_rooms: None,
            joined_rooms: Vec::new(),
        }
    }

    /// Add a room to this peer session
    ///
    /// Must be called before `connect()`. The room will be wired to the peer
    /// connection when `connect()` is called.
    ///
    /// The room must already be boxed as dyn RoomHandle<TMsg> (type-erased).
    ///
    /// Returns an error if the room already exists for this peer.
    pub fn add_room(
        &mut self,
        room_id: RoomId,
        room: Box<dyn RoomHandle<TMsg>>,
    ) -> Result<(), SessionError> {
        if self.rooms.contains_key(&room_id) {
            return Err(SessionError::RoomAlreadyExists {
                peer_id: self.peer_id.clone(),
                room_id,
            });
        }

        // Store the type-erased room
        self.rooms.insert(room_id.clone(), room);

        Ok(())
    }

    /// Get current connection state
    pub fn state(&self) -> ConnectionState {
        self.state
    }

    /// Check if peer is connected
    pub fn is_connected(&self) -> bool {
        self.state == ConnectionState::Connected
    }

    /// Get the list of room IDs configured for this peer
    pub fn room_ids(&self) -> Vec<RoomId> {
        self.rooms.keys().cloned().collect()
    }

    /// Routes a single inbound message to the appropriate room
    ///
    /// This is the core message routing logic extracted from the inbound task loop.
    /// Isolated for testability and clarity per coding standards.
    ///
    /// # Arguments
    /// * `rooms` - Map of room IDs to their handlers
    /// * `peer_id` - ID of the peer (for logging)
    /// * `room_id` - Target room ID
    /// * `msg` - Message to route (application's message enum type)
    ///
    /// Phase 6 Note: This method doesn't check joined_rooms because it runs in a separate task
    /// that doesn't have access to PeerSession state. The joined rooms check happens in
    /// send_to_room() before sending outbound messages. For inbound messages, we accept
    /// anything the peer sends - if they send to an unjoined room, we log a warning.
    async fn route_inbound_message(
        rooms: &mut HashMap<RoomId, Box<dyn RoomHandle<TMsg>>>,
        peer_id: &PeerId,
        room_id: RoomId,
        msg: TMsg,
    ) {
        if let Some(room) = rooms.get_mut(&room_id) {
            // Send to room's handler via RoomHandle trait
            if let Err(e) = room.send_message(msg) {
                tracing::warn!(
                    "Failed to route message to room {} on peer {}: {:?}",
                    room_id,
                    peer_id,
                    e
                );
            }
        } else {
            // Phase 6: This could mean peer sent to unjoined room, or room doesn't exist locally
            tracing::warn!(
                "Received message for unknown/unjoined room {} on peer {}",
                room_id,
                peer_id
            );
        }
    }

    /// Connect this peer session with channels
    ///
    /// This wires all rooms to the peer connection:
    /// 1. Spawns forwarder tasks for all rooms (Component → Peer, via TMsg)
    /// 2. Spawns routing task (Peer → Room, via TMsg)
    ///
    /// After connection, messages sent via `send_to_room` will be delivered
    /// to the peer, and messages from the peer will be routed to room handlers.
    pub fn connect(
        &mut self,
        outbound_tx: mpsc::Sender<(RoomId, TMsg)>,
        inbound_rx: mpsc::Receiver<(RoomId, TMsg)>,
    ) -> Result<(), SessionError> {
        if self.state == ConnectionState::Connected {
            return Err(SessionError::PeerAlreadyConnected(self.peer_id.clone()));
        }

        let outbound_clone = outbound_tx.clone();
        self.outbound_tx = Some(outbound_tx);

        // Spawn forwarder for each room (Component → Peer)
        for (room_id, room) in &mut self.rooms {
            room.spawn_forwarder(outbound_clone.clone()).map_err(|_| {
                SessionError::RoomReceiverAlreadySpawned {
                    peer_id: self.peer_id.clone(),
                    room_id: room_id.clone(),
                }
            })?;
        }

        // Spawn task to route inbound peer messages to appropriate rooms
        // This task owns the rooms HashMap and routes messages
        let rooms = std::mem::take(&mut self.rooms);
        let peer_id = self.peer_id.clone();

        let task = tokio::spawn(Self::inbound_task_loop(rooms, peer_id, inbound_rx));

        self.inbound_task = Some(task);
        self.state = ConnectionState::Connected;

        Ok(())
    }

    /// Inbound task loop - routes messages from peer to rooms
    ///
    /// Extracted from connect() for testability per coding standards.
    /// Runs continuously until the inbound channel is closed.
    ///
    /// This is the core message pump for all inbound messages from a peer.
    async fn inbound_task_loop(
        mut rooms: HashMap<RoomId, Box<dyn RoomHandle<TMsg>>>,
        peer_id: PeerId,
        mut inbound_rx: mpsc::Receiver<(RoomId, TMsg)>,
    ) {
        while let Some((room_id, msg)) = inbound_rx.recv().await {
            Self::route_inbound_message(&mut rooms, &peer_id, room_id, msg).await;
        }
        tracing::debug!("Peer {} inbound task stopped", peer_id);
    }

    /// Disconnect this peer session
    /// Drops channels and aborts inbound task
    pub fn disconnect(&mut self) {
        if self.state == ConnectionState::Disconnected {
            return;
        }

        // Drop outbound channel
        self.outbound_tx = None;

        // Abort inbound task
        if let Some(task) = self.inbound_task.take() {
            task.abort();
        }

        self.state = ConnectionState::Disconnected;

        tracing::debug!("Peer {} disconnected", self.peer_id);
    }

    // --- Phase 6: Room Negotiation ---

    /// Get the list of rooms we offer locally (from our added rooms)
    pub fn local_offered_rooms(&self) -> Vec<RoomId> {
        self.rooms.keys().cloned().collect()
    }

    /// Handle PublishRooms message from the remote peer
    ///
    /// Computes the intersection of local and peer rooms, and updates joined_rooms.
    /// Returns an error if the intersection is empty.
    pub fn handle_peer_offered_rooms(
        &mut self,
        peer_rooms: Vec<RoomId>,
    ) -> Result<(), SessionError> {
        self.peer_offered_rooms = Some(peer_rooms);
        self.joined_rooms = self.compute_intersection();

        if self.joined_rooms.is_empty() {
            tracing::warn!(
                "Peer {} offered rooms have no intersection with local rooms",
                self.peer_id
            );
            return Err(SessionError::EmptyIntersection);
        }

        tracing::debug!(
            "Peer {} negotiated {} joined rooms: {:?}",
            self.peer_id,
            self.joined_rooms.len(),
            self.joined_rooms
        );

        Ok(())
    }

    /// Compute the intersection of local rooms and peer-offered rooms
    fn compute_intersection(&self) -> Vec<RoomId> {
        use std::collections::HashSet;

        let local_rooms: HashSet<_> = self.rooms.keys().cloned().collect();

        if let Some(peer_rooms) = &self.peer_offered_rooms {
            let peer_set: HashSet<_> = peer_rooms.iter().cloned().collect();

            local_rooms.intersection(&peer_set).cloned().collect()
        } else {
            // Peer hasn't sent PublishRooms yet
            Vec::new()
        }
    }

    /// Check if a room is joined (in the intersection)
    pub fn is_room_joined(&self, room_id: &RoomId) -> bool {
        self.joined_rooms.contains(room_id)
    }

    /// Get the list of joined rooms (intersection)
    pub fn joined_rooms(&self) -> &[RoomId] {
        &self.joined_rooms
    }

    // --- End Phase 6 ---

    /// Send a typed message to a specific room on this peer
    ///
    /// The message is the application's enum type (TMsg) and will be sent over
    /// the network to the peer.
    ///
    /// Phase 6: Now checks if the room is joined before sending.
    pub async fn send_to_room(&self, room_id: &RoomId, msg: TMsg) -> Result<(), SessionError> {
        if !self.is_connected() {
            return Err(SessionError::PeerNotConnected(self.peer_id.clone()));
        }

        // Phase 6: Check if room is joined
        if !self.is_room_joined(room_id) {
            return Err(SessionError::RoomNotJoined(room_id.clone()));
        }

        // INVARIANT: If is_connected() is true, outbound_tx MUST be Some
        let tx = self
            .outbound_tx
            .as_ref()
            .expect("BUG: outbound_tx is None but state is Connected - this violates invariants");

        tx.send((room_id.clone(), msg))
            .await
            .map_err(|_| SessionError::SendFailed)?;

        Ok(())
    }
}

impl<TMsg> Drop for PeerSession<TMsg>
where
    TMsg: RoomMessageTrait,
{
    fn drop(&mut self) {
        self.disconnect();
    }
}

// Phase 5: Tests updated to use RoomAdapter and test_room_messages enums
#[cfg(test)]
mod tests {
    use super::*;
    use crate::room_adapter::RoomAdapter;
    use crate::test_room_messages::{
        CollectorMessages, HealthMessage, IntentConfigMessage, MemDBMessage,
    };
    use actix::prelude::*;
    use tokio::sync::mpsc;
    use zznet_room::room::Room;

    // Test actors for different message types
    #[derive(Clone, Debug, PartialEq, Message)]
    #[rtype(result = "()")]
    struct ActixIntentConfigMessage(IntentConfigMessage);

    #[derive(Clone, Debug, PartialEq, Message)]
    #[rtype(result = "()")]
    struct ActixHealthMessage(HealthMessage);

    // Conversions for CollectorMessages
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

    // Simple test actor
    struct TestActor;
    impl Actor for TestActor {
        type Context = Context<Self>;
    }
    impl Handler<ActixIntentConfigMessage> for TestActor {
        type Result = ();
        fn handle(&mut self, _msg: ActixIntentConfigMessage, _ctx: &mut Context<Self>) {}
    }
    impl Handler<ActixHealthMessage> for TestActor {
        type Result = ();
        fn handle(&mut self, _msg: ActixHealthMessage, _ctx: &mut Context<Self>) {}
    }

    #[actix::test]
    async fn test_peer_session_new() {
        let peer_id = PeerId::from("test_peer");
        let session = PeerSession::<CollectorMessages>::new(peer_id.clone());

        assert_eq!(session.peer_id, peer_id);
        assert_eq!(session.state(), ConnectionState::Disconnected);
        assert!(!session.is_connected());
        assert_eq!(session.room_ids().len(), 0);
    }

    #[actix::test]
    async fn test_peer_session_add_room() {
        let mut session = PeerSession::<CollectorMessages>::new(PeerId::from("test_peer"));

        // Create room with RoomAdapter
        let actor = TestActor.start();
        let (mut room, channels) = Room::<ActixIntentConfigMessage>::new(actor.recipient());
        room.spawn_receiver().unwrap();

        let (peer_tx, _peer_rx) = mpsc::channel(10);
        let adapter = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
            RoomId::from("intentconfig"),
            channels.inbound_tx,
            channels.outbound_rx,
            peer_tx,
        );

        // Add room via RoomHandle
        let result = session.add_room(RoomId::from("intentconfig"), Box::new(adapter));
        assert!(result.is_ok());

        // Verify it's in the list
        let room_ids = session.room_ids();
        assert_eq!(room_ids.len(), 1);
        assert!(room_ids.contains(&RoomId::from("intentconfig")));
    }

    #[actix::test]
    async fn test_peer_session_add_room_duplicate_error() {
        let mut session = PeerSession::<CollectorMessages>::new(PeerId::from("test_peer"));

        // Add first room
        let actor1 = TestActor.start();
        let (_room1, channels1) = Room::<ActixIntentConfigMessage>::new(actor1.recipient());
        let (peer_tx1, _peer_rx1) = mpsc::channel(10);
        let adapter1 = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
            RoomId::from("intentconfig"),
            channels1.inbound_tx,
            channels1.outbound_rx,
            peer_tx1,
        );
        session
            .add_room(RoomId::from("intentconfig"), Box::new(adapter1))
            .unwrap();

        // Try to add same room again
        let actor2 = TestActor.start();
        let (_room2, channels2) = Room::<ActixIntentConfigMessage>::new(actor2.recipient());
        let (peer_tx2, _peer_rx2) = mpsc::channel(10);
        let adapter2 = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
            RoomId::from("intentconfig"),
            channels2.inbound_tx,
            channels2.outbound_rx,
            peer_tx2,
        );

        let result = session.add_room(RoomId::from("intentconfig"), Box::new(adapter2));
        assert!(matches!(
            result,
            Err(SessionError::RoomAlreadyExists { .. })
        ));
    }

    #[actix::test]
    async fn test_peer_session_room_ids() {
        let mut session = PeerSession::<CollectorMessages>::new(PeerId::from("test_peer"));

        // Add two rooms
        let actor1 = TestActor.start();
        let (_room1, channels1) = Room::<ActixIntentConfigMessage>::new(actor1.recipient());
        let (peer_tx, _peer_rx) = mpsc::channel(10);
        let adapter1 = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
            RoomId::from("intentconfig"),
            channels1.inbound_tx,
            channels1.outbound_rx,
            peer_tx.clone(),
        );

        let actor2 = TestActor.start();
        let (_room2, channels2) = Room::<ActixHealthMessage>::new(actor2.recipient());
        let adapter2 = RoomAdapter::<ActixHealthMessage, CollectorMessages>::new(
            RoomId::from("health"),
            channels2.inbound_tx,
            channels2.outbound_rx,
            peer_tx,
        );

        session
            .add_room(RoomId::from("intentconfig"), Box::new(adapter1))
            .unwrap();
        session
            .add_room(RoomId::from("health"), Box::new(adapter2))
            .unwrap();

        let mut room_ids = session.room_ids();
        room_ids.sort_by(|a, b| a.as_str().cmp(b.as_str()));

        assert_eq!(room_ids.len(), 2);
        assert_eq!(room_ids[0], RoomId::from("health"));
        assert_eq!(room_ids[1], RoomId::from("intentconfig"));
    }

    #[actix::test]
    async fn test_peer_session_connect_disconnect() {
        let mut session = PeerSession::<CollectorMessages>::new(PeerId::from("test_peer"));

        // Add a room
        let actor = TestActor.start();
        let (_room, channels) = Room::<ActixIntentConfigMessage>::new(actor.recipient());
        let (peer_tx, _peer_rx) = mpsc::channel(10);
        let adapter = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
            RoomId::from("intentconfig"),
            channels.inbound_tx,
            channels.outbound_rx,
            peer_tx,
        );
        session
            .add_room(RoomId::from("intentconfig"), Box::new(adapter))
            .unwrap();

        let (tx_out, _rx_out) = mpsc::channel(10);
        let (_tx_in, rx_in) = mpsc::channel(10);

        // Connect
        session.connect(tx_out, rx_in).unwrap();
        assert_eq!(session.state(), ConnectionState::Connected);
        assert!(session.is_connected());

        // Disconnect
        session.disconnect();
        assert_eq!(session.state(), ConnectionState::Disconnected);
        assert!(!session.is_connected());
    }

    #[actix::test]
    async fn test_peer_session_connect_already_connected_returns_error() {
        let mut session = PeerSession::<CollectorMessages>::new(PeerId::from("test_peer"));

        // Add a room
        let actor = TestActor.start();
        let (_room, channels) = Room::<ActixIntentConfigMessage>::new(actor.recipient());
        let (peer_tx, _peer_rx) = mpsc::channel(10);
        let adapter = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
            RoomId::from("intentconfig"),
            channels.inbound_tx,
            channels.outbound_rx,
            peer_tx,
        );
        session
            .add_room(RoomId::from("intentconfig"), Box::new(adapter))
            .unwrap();

        let (tx1, _rx1) = mpsc::channel(10);
        let (_tx_in1, rx_in1) = mpsc::channel(10);

        // First connect should succeed
        session.connect(tx1, rx_in1).unwrap();
        assert!(session.is_connected());

        // Second connect should fail
        let (tx2, _rx2) = mpsc::channel(10);
        let (_tx_in2, rx_in2) = mpsc::channel(10);

        let result = session.connect(tx2, rx_in2);
        assert!(matches!(result, Err(SessionError::PeerAlreadyConnected(_))));
    }

    #[actix::test]
    async fn test_peer_session_send_to_room() {
        let mut session = PeerSession::<CollectorMessages>::new(PeerId::from("test_peer"));

        // Add room
        let actor = TestActor.start();
        let (_room, channels) = Room::<ActixIntentConfigMessage>::new(actor.recipient());
        let (peer_tx, _peer_rx) = mpsc::channel(10);
        let adapter = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
            RoomId::from("intentconfig"),
            channels.inbound_tx,
            channels.outbound_rx,
            peer_tx,
        );
        session
            .add_room(RoomId::from("intentconfig"), Box::new(adapter))
            .unwrap();

        // Phase 6: Negotiate room (peer offers same room)
        session
            .handle_peer_offered_rooms(vec![RoomId::from("intentconfig")])
            .unwrap();

        let (tx_out, _rx_out) = mpsc::channel(10);
        let (_tx_in, rx_in) = mpsc::channel(10);

        session.connect(tx_out, rx_in).unwrap();

        let msg = CollectorMessages::IntentConfig(IntentConfigMessage::Query);
        let room_id = RoomId::from("intentconfig");

        // Send should succeed
        let result = session.send_to_room(&room_id, msg).await;
        assert!(result.is_ok());
    }

    #[actix::test]
    async fn test_peer_session_send_when_disconnected() {
        let mut session = PeerSession::<CollectorMessages>::new(PeerId::from("test_peer"));

        // Add room
        let actor = TestActor.start();
        let (_room, channels) = Room::<ActixIntentConfigMessage>::new(actor.recipient());
        let (peer_tx, _peer_rx) = mpsc::channel(10);
        let adapter = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
            RoomId::from("intentconfig"),
            channels.inbound_tx,
            channels.outbound_rx,
            peer_tx,
        );
        session
            .add_room(RoomId::from("intentconfig"), Box::new(adapter))
            .unwrap();

        let msg = CollectorMessages::IntentConfig(IntentConfigMessage::Query);
        let room_id = RoomId::from("intentconfig");

        // Send should fail when disconnected
        let result = session.send_to_room(&room_id, msg).await;
        assert!(matches!(result, Err(SessionError::PeerNotConnected(_))));
    }

    #[actix::test]
    async fn test_peer_session_drop_cleanup() {
        let mut session = PeerSession::<CollectorMessages>::new(PeerId::from("test_peer"));

        // Add room
        let actor = TestActor.start();
        let (_room, channels) = Room::<ActixIntentConfigMessage>::new(actor.recipient());
        let (peer_tx, _peer_rx) = mpsc::channel(10);
        let adapter = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
            RoomId::from("intentconfig"),
            channels.inbound_tx,
            channels.outbound_rx,
            peer_tx,
        );
        session
            .add_room(RoomId::from("intentconfig"), Box::new(adapter))
            .unwrap();

        let (tx_out, mut rx_out) = mpsc::channel(10);
        let (_tx_in, rx_in) = mpsc::channel(10);

        session.connect(tx_out, rx_in).unwrap();
        assert!(session.is_connected());

        // Drop the session - should trigger disconnect via Drop impl
        drop(session);

        // Try to receive from outbound - should get None because sender was dropped
        let received = rx_out.recv().await;
        assert!(
            received.is_none(),
            "Expected channel to be closed after drop"
        );
    }

    // TODO: Reconnection currently not supported because Room::spawn_receiver()
    // can only be called once. To support reconnection, we need to either:
    // 1. Track room receiver tasks and abort them on disconnect
    // 2. Add Room::stop_receiver() method
    // 3. Recreate Room instances on reconnect
    // For now, this is deferred as it adds complexity and is not required for PoC integration.
    #[actix::test]
    #[ignore = "Reconnection not yet supported - Room receivers can't be respawned"]
    async fn test_peer_session_reconnect_after_disconnect() {
        let mut session = PeerSession::<CollectorMessages>::new(PeerId::from("test_peer"));

        // Add room
        let actor = TestActor.start();
        let (_room, channels) = Room::<ActixIntentConfigMessage>::new(actor.recipient());
        let (peer_tx, _peer_rx) = mpsc::channel(10);
        let adapter = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
            RoomId::from("intentconfig"),
            channels.inbound_tx,
            channels.outbound_rx,
            peer_tx,
        );
        session
            .add_room(RoomId::from("intentconfig"), Box::new(adapter))
            .unwrap();

        // First connection
        let (tx1, _rx1) = mpsc::channel(10);
        let (_tx_in1, rx_in1) = mpsc::channel(10);
        session.connect(tx1, rx_in1).unwrap();
        assert!(session.is_connected());

        // Disconnect
        session.disconnect();
        assert!(!session.is_connected());

        // Second connection (reconnect) would require recreating Room/RoomAdapter
        // Not supported yet
        let (tx2, _rx2) = mpsc::channel(10);
        let (_tx_in2, rx_in2) = mpsc::channel(10);
        let result = session.connect(tx2, rx_in2);
        assert!(result.is_ok());
        assert!(session.is_connected());
    }

    // Note: route_inbound_message tests removed - this is internal implementation
    // that's already covered by integration_tests.rs which tests the full flow
    // with real RoomAdapter instances.

    // --- Phase 6 Tests: Room Negotiation ---

    #[actix::test]
    async fn test_local_offered_rooms() {
        let mut session = PeerSession::<CollectorMessages>::new(PeerId::from("test_peer"));

        // Add rooms
        let actor1 = TestActor.start();
        let (_room1, channels1) = Room::<ActixIntentConfigMessage>::new(actor1.recipient());
        let (peer_tx1, _) = mpsc::channel(10);
        let adapter1 = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
            RoomId::from("intentconfig"),
            channels1.inbound_tx,
            channels1.outbound_rx,
            peer_tx1,
        );
        session
            .add_room(RoomId::from("intentconfig"), Box::new(adapter1))
            .unwrap();

        let actor2 = TestActor.start();
        let (_room2, channels2) = Room::<ActixHealthMessage>::new(actor2.recipient());
        let (peer_tx2, _) = mpsc::channel(10);
        let adapter2 = RoomAdapter::<ActixHealthMessage, CollectorMessages>::new(
            RoomId::from("health"),
            channels2.inbound_tx,
            channels2.outbound_rx,
            peer_tx2,
        );
        session
            .add_room(RoomId::from("health"), Box::new(adapter2))
            .unwrap();

        // Get local offered rooms
        let offered = session.local_offered_rooms();
        assert_eq!(offered.len(), 2);
        assert!(offered.contains(&RoomId::from("intentconfig")));
        assert!(offered.contains(&RoomId::from("health")));
    }

    #[actix::test]
    async fn test_room_intersection_full_match() {
        let mut session = PeerSession::<CollectorMessages>::new(PeerId::from("test_peer"));

        // Add local rooms
        let actor1 = TestActor.start();
        let (_room1, channels1) = Room::<ActixIntentConfigMessage>::new(actor1.recipient());
        let (peer_tx1, _) = mpsc::channel(10);
        let adapter1 = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
            RoomId::from("intentconfig"),
            channels1.inbound_tx,
            channels1.outbound_rx,
            peer_tx1,
        );
        session
            .add_room(RoomId::from("intentconfig"), Box::new(adapter1))
            .unwrap();

        let actor2 = TestActor.start();
        let (_room2, channels2) = Room::<ActixHealthMessage>::new(actor2.recipient());
        let (peer_tx2, _) = mpsc::channel(10);
        let adapter2 = RoomAdapter::<ActixHealthMessage, CollectorMessages>::new(
            RoomId::from("health"),
            channels2.inbound_tx,
            channels2.outbound_rx,
            peer_tx2,
        );
        session
            .add_room(RoomId::from("health"), Box::new(adapter2))
            .unwrap();

        // Peer offers same rooms
        let result = session
            .handle_peer_offered_rooms(vec![RoomId::from("intentconfig"), RoomId::from("health")]);
        assert!(result.is_ok());

        // Check joined rooms (should be all)
        let joined = session.joined_rooms();
        assert_eq!(joined.len(), 2);
        assert!(joined.contains(&RoomId::from("intentconfig")));
        assert!(joined.contains(&RoomId::from("health")));
    }

    #[actix::test]
    async fn test_room_intersection_partial_match() {
        let mut session = PeerSession::<CollectorMessages>::new(PeerId::from("test_peer"));

        // Add local rooms: intentconfig, health
        let actor1 = TestActor.start();
        let (_room1, channels1) = Room::<ActixIntentConfigMessage>::new(actor1.recipient());
        let (peer_tx1, _) = mpsc::channel(10);
        let adapter1 = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
            RoomId::from("intentconfig"),
            channels1.inbound_tx,
            channels1.outbound_rx,
            peer_tx1,
        );
        session
            .add_room(RoomId::from("intentconfig"), Box::new(adapter1))
            .unwrap();

        let actor2 = TestActor.start();
        let (_room2, channels2) = Room::<ActixHealthMessage>::new(actor2.recipient());
        let (peer_tx2, _) = mpsc::channel(10);
        let adapter2 = RoomAdapter::<ActixHealthMessage, CollectorMessages>::new(
            RoomId::from("health"),
            channels2.inbound_tx,
            channels2.outbound_rx,
            peer_tx2,
        );
        session
            .add_room(RoomId::from("health"), Box::new(adapter2))
            .unwrap();

        // Peer offers: intentconfig, admin (different set)
        let result = session
            .handle_peer_offered_rooms(vec![RoomId::from("intentconfig"), RoomId::from("admin")]);
        assert!(result.is_ok());

        // Check joined rooms (should only be intentconfig)
        let joined = session.joined_rooms();
        assert_eq!(joined.len(), 1);
        assert!(joined.contains(&RoomId::from("intentconfig")));
        assert!(!joined.contains(&RoomId::from("health")));
        assert!(!joined.contains(&RoomId::from("admin")));
    }

    #[actix::test]
    async fn test_empty_intersection_error() {
        let mut session = PeerSession::<CollectorMessages>::new(PeerId::from("test_peer"));

        // Add local rooms
        let actor1 = TestActor.start();
        let (_room1, channels1) = Room::<ActixIntentConfigMessage>::new(actor1.recipient());
        let (peer_tx1, _) = mpsc::channel(10);
        let adapter1 = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
            RoomId::from("intentconfig"),
            channels1.inbound_tx,
            channels1.outbound_rx,
            peer_tx1,
        );
        session
            .add_room(RoomId::from("intentconfig"), Box::new(adapter1))
            .unwrap();

        let actor2 = TestActor.start();
        let (_room2, channels2) = Room::<ActixHealthMessage>::new(actor2.recipient());
        let (peer_tx2, _) = mpsc::channel(10);
        let adapter2 = RoomAdapter::<ActixHealthMessage, CollectorMessages>::new(
            RoomId::from("health"),
            channels2.inbound_tx,
            channels2.outbound_rx,
            peer_tx2,
        );
        session
            .add_room(RoomId::from("health"), Box::new(adapter2))
            .unwrap();

        // Peer offers completely different rooms
        let result =
            session.handle_peer_offered_rooms(vec![RoomId::from("admin"), RoomId::from("metrics")]);

        // Should fail with EmptyIntersection
        assert!(matches!(result, Err(SessionError::EmptyIntersection)));
        assert_eq!(session.joined_rooms().len(), 0);
    }

    #[actix::test]
    async fn test_send_to_unjoined_room_rejected() {
        let mut session = PeerSession::<CollectorMessages>::new(PeerId::from("test_peer"));

        // Add two local rooms
        let actor1 = TestActor.start();
        let (_room1, channels1) = Room::<ActixIntentConfigMessage>::new(actor1.recipient());
        let (peer_tx1, _) = mpsc::channel(10);
        let adapter1 = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
            RoomId::from("intentconfig"),
            channels1.inbound_tx,
            channels1.outbound_rx,
            peer_tx1,
        );
        session
            .add_room(RoomId::from("intentconfig"), Box::new(adapter1))
            .unwrap();

        let actor2 = TestActor.start();
        let (_room2, channels2) = Room::<ActixHealthMessage>::new(actor2.recipient());
        let (peer_tx2, _) = mpsc::channel(10);
        let adapter2 = RoomAdapter::<ActixHealthMessage, CollectorMessages>::new(
            RoomId::from("health"),
            channels2.inbound_tx,
            channels2.outbound_rx,
            peer_tx2,
        );
        session
            .add_room(RoomId::from("health"), Box::new(adapter2))
            .unwrap();

        // Peer only offers intentconfig (so only intentconfig is joined)
        session
            .handle_peer_offered_rooms(vec![RoomId::from("intentconfig")])
            .unwrap();

        // Connect
        let (tx, _rx) = mpsc::channel(10);
        let (_tx_in, rx_in) = mpsc::channel(10);
        session.connect(tx, rx_in).unwrap();

        // Try to send to health (not joined)
        let result = session
            .send_to_room(
                &RoomId::from("health"),
                CollectorMessages::Health(HealthMessage::Ping),
            )
            .await;

        // Should fail with RoomNotJoined
        assert!(matches!(result, Err(SessionError::RoomNotJoined(_))));
    }

    #[actix::test]
    async fn test_send_to_joined_room_succeeds() {
        let mut session = PeerSession::<CollectorMessages>::new(PeerId::from("test_peer"));

        // Add room
        let actor = TestActor.start();
        let (_room, channels) = Room::<ActixIntentConfigMessage>::new(actor.recipient());
        let (peer_tx, _) = mpsc::channel(10);
        let adapter = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
            RoomId::from("intentconfig"),
            channels.inbound_tx,
            channels.outbound_rx,
            peer_tx,
        );
        session
            .add_room(RoomId::from("intentconfig"), Box::new(adapter))
            .unwrap();

        // Peer offers same room
        session
            .handle_peer_offered_rooms(vec![RoomId::from("intentconfig")])
            .unwrap();

        // Connect
        let (tx, mut rx) = mpsc::channel(10);
        let (_tx_in, rx_in) = mpsc::channel(10);
        session.connect(tx, rx_in).unwrap();

        // Send to joined room should succeed
        let result = session
            .send_to_room(
                &RoomId::from("intentconfig"),
                CollectorMessages::IntentConfig(IntentConfigMessage::Query),
            )
            .await;

        assert!(result.is_ok());

        // Message should be received
        let received = rx.recv().await;
        assert!(received.is_some());
    }

    #[actix::test]
    async fn test_is_room_joined() {
        let mut session = PeerSession::<CollectorMessages>::new(PeerId::from("test_peer"));

        // Add rooms
        let actor1 = TestActor.start();
        let (_room1, channels1) = Room::<ActixIntentConfigMessage>::new(actor1.recipient());
        let (peer_tx1, _) = mpsc::channel(10);
        let adapter1 = RoomAdapter::<ActixIntentConfigMessage, CollectorMessages>::new(
            RoomId::from("intentconfig"),
            channels1.inbound_tx,
            channels1.outbound_rx,
            peer_tx1,
        );
        session
            .add_room(RoomId::from("intentconfig"), Box::new(adapter1))
            .unwrap();

        let actor2 = TestActor.start();
        let (_room2, channels2) = Room::<ActixHealthMessage>::new(actor2.recipient());
        let (peer_tx2, _) = mpsc::channel(10);
        let adapter2 = RoomAdapter::<ActixHealthMessage, CollectorMessages>::new(
            RoomId::from("health"),
            channels2.inbound_tx,
            channels2.outbound_rx,
            peer_tx2,
        );
        session
            .add_room(RoomId::from("health"), Box::new(adapter2))
            .unwrap();

        // Before negotiation, nothing is joined
        assert!(!session.is_room_joined(&RoomId::from("intentconfig")));
        assert!(!session.is_room_joined(&RoomId::from("health")));

        // After negotiation, only intersection is joined
        session
            .handle_peer_offered_rooms(vec![RoomId::from("intentconfig")])
            .unwrap();

        assert!(session.is_room_joined(&RoomId::from("intentconfig")));
        assert!(!session.is_room_joined(&RoomId::from("health")));
    }

    // --- Critical Path Tests: Inbound Task Loop and Error Handling ---

    /// MockRoomHandle for testing routing logic without real actors
    struct MockRoomHandle {
        room_id: RoomId,
        sent_messages: std::sync::Arc<std::sync::Mutex<Vec<CollectorMessages>>>,
        should_fail: bool,
        forwarder_spawned: bool,
    }

    impl MockRoomHandle {
        fn new(room_id: RoomId) -> Self {
            Self {
                room_id,
                sent_messages: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
                should_fail: false,
                forwarder_spawned: false,
            }
        }

        fn new_failing(room_id: RoomId) -> Self {
            Self {
                room_id,
                sent_messages: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
                should_fail: true,
                forwarder_spawned: false,
            }
        }

        #[allow(dead_code)]
        fn get_sent_messages(&self) -> Vec<CollectorMessages> {
            self.sent_messages.lock().unwrap().clone()
        }
    }

    impl RoomHandle<CollectorMessages> for MockRoomHandle {
        fn room_id(&self) -> &RoomId {
            &self.room_id
        }

        fn send_message(&mut self, msg: CollectorMessages) -> Result<(), SessionError> {
            if self.should_fail {
                Err(SessionError::SendFailed)
            } else {
                self.sent_messages.lock().unwrap().push(msg);
                Ok(())
            }
        }

        fn spawn_forwarder(
            &mut self,
            _tx: mpsc::Sender<(RoomId, CollectorMessages)>,
        ) -> Result<(), SessionError> {
            if self.forwarder_spawned {
                Err(SessionError::SendFailed)
            } else {
                self.forwarder_spawned = true;
                Ok(())
            }
        }
    }

    #[actix::test]
    async fn test_inbound_task_loop_single_message() {
        // Create mock room
        let mock_room = MockRoomHandle::new(RoomId::from("test"));
        let messages_ref = mock_room.sent_messages.clone();

        let mut rooms: HashMap<RoomId, Box<dyn RoomHandle<CollectorMessages>>> = HashMap::new();
        rooms.insert(RoomId::from("test"), Box::new(mock_room));

        // Create channel and send one message
        let (inbound_tx, inbound_rx) = mpsc::channel(10);
        let msg = CollectorMessages::IntentConfig(IntentConfigMessage::Query);
        inbound_tx
            .send((RoomId::from("test"), msg.clone()))
            .await
            .unwrap();
        drop(inbound_tx); // Close channel to stop loop

        // Run the loop
        let peer_id = PeerId::from("test_peer");
        PeerSession::<CollectorMessages>::inbound_task_loop(rooms, peer_id, inbound_rx).await;

        // Verify message was delivered
        let sent = messages_ref.lock().unwrap();
        assert_eq!(sent.len(), 1);
        match &sent[0] {
            CollectorMessages::IntentConfig(IntentConfigMessage::Query) => {
                // Expected
            }
            _ => panic!("Wrong message type"),
        }
    }

    #[actix::test]
    async fn test_inbound_task_loop_multiple_messages() {
        // Create mock room
        let mock_room = MockRoomHandle::new(RoomId::from("test"));
        let messages_ref = mock_room.sent_messages.clone();

        let mut rooms: HashMap<RoomId, Box<dyn RoomHandle<CollectorMessages>>> = HashMap::new();
        rooms.insert(RoomId::from("test"), Box::new(mock_room));

        // Create channel and send multiple messages
        let (inbound_tx, inbound_rx) = mpsc::channel(10);
        let msg1 = CollectorMessages::IntentConfig(IntentConfigMessage::Query);
        let msg2 = CollectorMessages::Health(HealthMessage::Ping);
        let msg3 = CollectorMessages::MemDB(MemDBMessage::Retrieve {
            key: "test".to_string(),
        });

        inbound_tx.send((RoomId::from("test"), msg1)).await.unwrap();
        inbound_tx.send((RoomId::from("test"), msg2)).await.unwrap();
        inbound_tx.send((RoomId::from("test"), msg3)).await.unwrap();
        drop(inbound_tx); // Close channel to stop loop

        // Run the loop
        let peer_id = PeerId::from("test_peer");
        PeerSession::<CollectorMessages>::inbound_task_loop(rooms, peer_id, inbound_rx).await;

        // Verify all messages were delivered in order
        let sent = messages_ref.lock().unwrap();
        assert_eq!(sent.len(), 3);

        match &sent[0] {
            CollectorMessages::IntentConfig(IntentConfigMessage::Query) => {}
            _ => panic!("Wrong message 1"),
        }
        match &sent[1] {
            CollectorMessages::Health(HealthMessage::Ping) => {}
            _ => panic!("Wrong message 2"),
        }
        match &sent[2] {
            CollectorMessages::MemDB(MemDBMessage::Retrieve { key }) => {
                assert_eq!(key, "test");
            }
            _ => panic!("Wrong message 3"),
        }
    }

    #[actix::test]
    async fn test_inbound_task_loop_multiple_rooms() {
        // Create two mock rooms
        let mock_room1 = MockRoomHandle::new(RoomId::from("room1"));
        let mock_room2 = MockRoomHandle::new(RoomId::from("room2"));
        let messages_ref1 = mock_room1.sent_messages.clone();
        let messages_ref2 = mock_room2.sent_messages.clone();

        let mut rooms: HashMap<RoomId, Box<dyn RoomHandle<CollectorMessages>>> = HashMap::new();
        rooms.insert(RoomId::from("room1"), Box::new(mock_room1));
        rooms.insert(RoomId::from("room2"), Box::new(mock_room2));

        // Send messages to different rooms
        let (inbound_tx, inbound_rx) = mpsc::channel(10);
        let msg1 = CollectorMessages::IntentConfig(IntentConfigMessage::Query);
        let msg2 = CollectorMessages::Health(HealthMessage::Ping);

        inbound_tx
            .send((RoomId::from("room1"), msg1))
            .await
            .unwrap();
        inbound_tx
            .send((RoomId::from("room2"), msg2))
            .await
            .unwrap();
        drop(inbound_tx);

        // Run the loop
        let peer_id = PeerId::from("test_peer");
        PeerSession::<CollectorMessages>::inbound_task_loop(rooms, peer_id, inbound_rx).await;

        // Verify messages routed to correct rooms
        let sent1 = messages_ref1.lock().unwrap();
        let sent2 = messages_ref2.lock().unwrap();

        assert_eq!(sent1.len(), 1);
        assert_eq!(sent2.len(), 1);

        match &sent1[0] {
            CollectorMessages::IntentConfig(_) => {}
            _ => panic!("Wrong message in room1"),
        }
        match &sent2[0] {
            CollectorMessages::Health(_) => {}
            _ => panic!("Wrong message in room2"),
        }
    }

    #[actix::test]
    async fn test_route_inbound_message_unknown_room() {
        // Create rooms HashMap without the target room
        let mut rooms: HashMap<RoomId, Box<dyn RoomHandle<CollectorMessages>>> = HashMap::new();
        let mock_room = MockRoomHandle::new(RoomId::from("known"));
        rooms.insert(RoomId::from("known"), Box::new(mock_room));

        let peer_id = PeerId::from("test_peer");
        let msg = CollectorMessages::IntentConfig(IntentConfigMessage::Query);

        // Route to unknown room - should not panic, just log warning
        PeerSession::<CollectorMessages>::route_inbound_message(
            &mut rooms,
            &peer_id,
            RoomId::from("unknown"),
            msg,
        )
        .await;

        // Should complete without error
        // (Warning is logged but not testable without tracing subscriber)
    }

    #[actix::test]
    async fn test_route_inbound_message_send_failure() {
        // Create room that fails to send
        let mock_room = MockRoomHandle::new_failing(RoomId::from("test"));
        let mut rooms: HashMap<RoomId, Box<dyn RoomHandle<CollectorMessages>>> = HashMap::new();
        rooms.insert(RoomId::from("test"), Box::new(mock_room));

        let peer_id = PeerId::from("test_peer");
        let msg = CollectorMessages::IntentConfig(IntentConfigMessage::Query);

        // Route message - should handle error gracefully
        PeerSession::<CollectorMessages>::route_inbound_message(
            &mut rooms,
            &peer_id,
            RoomId::from("test"),
            msg,
        )
        .await;

        // Should complete without panicking
        // (Error is logged but not testable without tracing subscriber)
    }

    #[actix::test]
    async fn test_inbound_task_loop_graceful_shutdown() {
        // Create mock room
        let mock_room = MockRoomHandle::new(RoomId::from("test"));
        let messages_ref = mock_room.sent_messages.clone();

        let mut rooms: HashMap<RoomId, Box<dyn RoomHandle<CollectorMessages>>> = HashMap::new();
        rooms.insert(RoomId::from("test"), Box::new(mock_room));

        // Create channel, send message, then close
        let (inbound_tx, inbound_rx) = mpsc::channel(10);
        let msg = CollectorMessages::Health(HealthMessage::Ping);
        inbound_tx.send((RoomId::from("test"), msg)).await.unwrap();

        // Close channel immediately after sending
        drop(inbound_tx);

        // Run the loop - should process message and then exit gracefully
        let peer_id = PeerId::from("test_peer");
        PeerSession::<CollectorMessages>::inbound_task_loop(rooms, peer_id, inbound_rx).await;

        // Verify message was processed before shutdown
        let sent = messages_ref.lock().unwrap();
        assert_eq!(sent.len(), 1);
    }

    #[actix::test]
    async fn test_disconnect_stops_inbound_task() {
        let mut session = PeerSession::<CollectorMessages>::new(PeerId::from("test_peer"));

        // Add mock room
        let mock_room = MockRoomHandle::new(RoomId::from("test"));
        session
            .add_room(RoomId::from("test"), Box::new(mock_room))
            .unwrap();

        // Connect with channels
        let (tx_out, _rx_out) = mpsc::channel(10);
        let (_tx_in, rx_in) = mpsc::channel(10);

        session.connect(tx_out, rx_in).unwrap();
        assert!(session.is_connected());

        // Get task handle before disconnect
        let task_handle = session.inbound_task.as_ref().unwrap();
        assert!(!task_handle.is_finished());

        // Disconnect
        session.disconnect();
        assert!(!session.is_connected());

        // Give task a moment to abort
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        // Task should be finished (aborted)
        // Note: We can't check the handle after disconnect because it's been taken
        // This test validates that disconnect doesn't panic and changes state correctly
    }

    #[actix::test]
    async fn test_disconnect_idempotent() {
        let mut session = PeerSession::<CollectorMessages>::new(PeerId::from("test_peer"));

        // Add mock room and connect
        let mock_room = MockRoomHandle::new(RoomId::from("test"));
        session
            .add_room(RoomId::from("test"), Box::new(mock_room))
            .unwrap();

        let (tx_out, _rx_out) = mpsc::channel(10);
        let (_tx_in, rx_in) = mpsc::channel(10);
        session.connect(tx_out, rx_in).unwrap();

        // Disconnect multiple times - should not panic
        session.disconnect();
        assert!(!session.is_connected());

        session.disconnect();
        assert!(!session.is_connected());

        session.disconnect();
        assert!(!session.is_connected());

        // All disconnect calls should complete successfully
    }
}
