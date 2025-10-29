use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex as TokioMutex, broadcast, mpsc};
use tokio::task::JoinHandle;
use zznet_api::types::{PeerChannels as PeerChannelsTrait, PeerId, RoomId, SessionError};
use zznet_room::room_handle::RoomHandle;

/// Shared room storage for a peer.
type SessionRooms = Arc<TokioMutex<HashMap<RoomId, Box<dyn RoomHandle>>>>;

/// Builder for PeerChannels with immutable construction.
///
/// Collects rooms before connecting transport channels.
pub struct PeerChannelsBuilder {
    peer_id: PeerId,
    rooms: HashMap<RoomId, Box<dyn RoomHandle>>,
}

impl PeerChannelsBuilder {
    /// Create a new builder for a peer.
    pub fn new(peer_id: PeerId) -> Self {
        Self {
            peer_id,
            rooms: HashMap::new(),
        }
    }

    /// Add a room to this peer.
    pub fn add_room(
        &mut self,
        room_id: RoomId,
        room: Box<dyn RoomHandle>,
    ) -> Result<(), SessionError> {
        if self.rooms.contains_key(&room_id) {
            return Err(SessionError::RoomAlreadyExists {
                peer_id: self.peer_id.clone(),
                room_id,
            });
        }
        self.rooms.insert(room_id, room);
        Ok(())
    }

    /// Build the PeerChannels by connecting transport channels.
    ///
    /// Spawns the inbound routing task and sets up channels.
    pub async fn build(
        self,
        outbound_tx: mpsc::Sender<(RoomId, Vec<u8>)>,
        inbound_rx: mpsc::Receiver<(RoomId, Vec<u8>)>,
    ) -> Result<PeerChannels, SessionError> {
        let (broadcast_tx, _) = broadcast::channel(100);

        let mut rooms_map = self.rooms;

        // Spawn forwarder for each room (Component → Peer)
        for (room_id, room) in &mut rooms_map {
            room.spawn_forwarder(outbound_tx.clone()).map_err(|_| {
                SessionError::RoomReceiverAlreadySpawned {
                    peer_id: self.peer_id.clone(),
                    room_id: room_id.clone(),
                }
            })?;
        }

        // Wrap in Arc<Mutex<>> for sharing
        let rooms = Arc::new(TokioMutex::new(rooms_map));
        let peer_id = self.peer_id.clone();
        let broadcast_tx_clone = broadcast_tx.clone();
        let task = tokio::spawn(PeerChannels::inbound_task_loop(
            Arc::clone(&rooms),
            peer_id,
            inbound_rx,
            broadcast_tx_clone,
        ));

        Ok(PeerChannels {
            peer_id: self.peer_id,
            outbound_tx,
            inbound_broadcast: broadcast_tx,
            joined_rooms: TokioMutex::new(Vec::new()),
            inbound_task: task,
        })
    }
}

/// Data-plane channel set for a peer.
///
/// Immutable after construction; owns transport channels and routing task.
pub struct PeerChannels {
    peer_id: PeerId,
    outbound_tx: mpsc::Sender<(RoomId, Vec<u8>)>,
    inbound_broadcast: broadcast::Sender<(RoomId, Vec<u8>)>,
    joined_rooms: TokioMutex<Vec<RoomId>>,
    inbound_task: JoinHandle<()>,
}

impl PeerChannels {
    /// Handle PublishRooms negotiation and compute joined rooms.
    ///
    /// Takes both local offered rooms and peer offered rooms, computes intersection.
    /// Returns error if intersection is empty.
    pub async fn handle_publish_rooms(
        &self,
        local_rooms: &[RoomId],
        peer_rooms: Vec<RoomId>,
    ) -> Result<(), SessionError> {
        use std::collections::HashSet;

        let local_set: HashSet<_> = local_rooms.iter().cloned().collect();
        let peer_set: HashSet<_> = peer_rooms.into_iter().collect();
        let intersection: Vec<RoomId> = local_set.intersection(&peer_set).cloned().collect();

        let mut joined = self.joined_rooms.lock().await;
        *joined = intersection;

        if joined.is_empty() {
            tracing::warn!(
                "Peer {} offered rooms have no intersection with local rooms",
                self.peer_id
            );
            return Err(SessionError::EmptyIntersection);
        }

        Ok(())
    }

    /// Helper for routing inbound bytes to rooms.
    async fn route_inbound_message(
        rooms: &SessionRooms,
        peer_id: &PeerId,
        room_id: RoomId,
        bytes: Vec<u8>,
    ) {
        let mut rooms_lock = rooms.lock().await;
        if let Some(room) = rooms_lock.get_mut(&room_id) {
            if let Err(e) = room.send_message(bytes) {
                tracing::warn!(
                    "Failed to route message to room {} on peer {}: {:?}",
                    room_id,
                    peer_id,
                    e
                );
            }
        } else {
            tracing::warn!(
                "Received message for unknown/unjoined room {} on peer {}",
                room_id,
                peer_id
            );
        }
    }

    async fn inbound_task_loop(
        rooms: SessionRooms,
        peer_id: PeerId,
        mut inbound_rx: mpsc::Receiver<(RoomId, Vec<u8>)>,
        broadcast_tx: broadcast::Sender<(RoomId, Vec<u8>)>,
    ) {
        while let Some((room_id, bytes)) = inbound_rx.recv().await {
            let _ = broadcast_tx.send((room_id.clone(), bytes.clone()));
            Self::route_inbound_message(&rooms, &peer_id, room_id, bytes).await;
        }
        tracing::debug!("Peer {} inbound task stopped", peer_id);
    }

    /// Clone of the outbound sender (always available since connected).
    pub fn outbound_sender(&self) -> Option<mpsc::Sender<(RoomId, Vec<u8>)>> {
        Some(self.outbound_tx.clone())
    }

    /// Subscribe to inbound broadcast channel (always available since connected).
    pub fn subscribe_inbound(&self) -> Option<broadcast::Receiver<(RoomId, Vec<u8>)>> {
        Some(self.inbound_broadcast.subscribe())
    }

    /// Send raw bytes to the specified room.
    pub async fn send_raw_to_room(
        &self,
        room_id: &RoomId,
        bytes: Vec<u8>,
    ) -> Result<(), SessionError> {
        self.outbound_tx
            .send((room_id.clone(), bytes))
            .await
            .map_err(|_| SessionError::SendFailed)
    }

    /// Disconnect transport wiring and stop routing tasks.
    pub fn disconnect(&self) {
        self.inbound_task.abort();
    }

    /// Inspect joined rooms.
    pub async fn joined_rooms(&self) -> Vec<RoomId> {
        self.joined_rooms.lock().await.clone()
    }

    /// Check whether a room was negotiated with the peer.
    pub async fn is_room_joined(&self, room_id: &RoomId) -> bool {
        self.joined_rooms.lock().await.contains(room_id)
    }
}

#[async_trait::async_trait]
impl PeerChannelsTrait for PeerChannels {
    fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    async fn joined_rooms(&self) -> Vec<RoomId> {
        self.joined_rooms.lock().await.clone()
    }

    async fn is_room_joined(&self, room_id: &RoomId) -> bool {
        self.joined_rooms.lock().await.contains(room_id)
    }

    fn outbound_sender(&self) -> Option<mpsc::Sender<(RoomId, Vec<u8>)>> {
        Some(self.outbound_tx.clone())
    }

    fn subscribe_inbound(&self) -> Option<broadcast::Receiver<(RoomId, Vec<u8>)>> {
        Some(self.inbound_broadcast.subscribe())
    }

    async fn send_to_room(&self, room_id: &RoomId, bytes: Vec<u8>) -> Result<(), SessionError> {
        self.send_raw_to_room(room_id, bytes).await
    }
}
