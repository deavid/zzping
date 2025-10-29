use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex as TokioMutex, broadcast, mpsc};
use tokio::task::JoinHandle;
use zznet_api::types::{PeerChannels as PeerChannelsTrait, PeerId, RoomId, SessionError};
use zznet_room::room_handle::RoomHandle;

/// Shared room storage for a peer.
type SessionRooms = Arc<TokioMutex<HashMap<RoomId, Box<dyn RoomHandle>>>>;

/// Data-plane channel set for a peer.
///
/// Owns the transport-facing channels and room routing infrastructure without
/// any knowledge of authentication or control-plane state.
pub struct PeerChannels {
    peer_id: PeerId,
    rooms: SessionRooms,
    outbound_tx: Option<mpsc::Sender<(RoomId, Vec<u8>)>>,
    inbound_task: Option<JoinHandle<()>>,
    inbound_broadcast: Option<broadcast::Sender<(RoomId, Vec<u8>)>>,
    peer_offered_rooms: Option<Vec<RoomId>>,
    joined_rooms: Vec<RoomId>,
}

impl PeerChannels {
    /// Create a new peer channel set in disconnected state with no rooms.
    pub fn new(peer_id: PeerId) -> Self {
        Self {
            peer_id,
            rooms: Arc::new(TokioMutex::new(HashMap::new())),
            outbound_tx: None,
            inbound_task: None,
            inbound_broadcast: None,
            peer_offered_rooms: None,
            joined_rooms: Vec::new(),
        }
    }

    /// Add a room to this peer.
    pub async fn add_room(
        &mut self,
        room_id: RoomId,
        room: Box<dyn RoomHandle>,
    ) -> Result<(), SessionError> {
        let mut rooms = self.rooms.lock().await;
        if rooms.contains_key(&room_id) {
            return Err(SessionError::RoomAlreadyExists {
                peer_id: self.peer_id.clone(),
                room_id,
            });
        }
        rooms.insert(room_id, room);
        Ok(())
    }

    /// Connect the peer channels to transport wiring.
    pub async fn connect(
        &mut self,
        outbound_tx: mpsc::Sender<(RoomId, Vec<u8>)>,
        inbound_rx: mpsc::Receiver<(RoomId, Vec<u8>)>,
    ) -> Result<(), SessionError> {
        if self.outbound_tx.is_some() {
            return Err(SessionError::PeerAlreadyConnected(self.peer_id.clone()));
        }

        let outbound_clone = outbound_tx.clone();
        self.outbound_tx = Some(outbound_tx);

        if self.inbound_broadcast.is_none() {
            let (tx, _rx) = broadcast::channel(100);
            self.inbound_broadcast = Some(tx);
        }

        // Spawn forwarder for each room (Component → Peer)
        {
            let mut rooms = self.rooms.lock().await;
            for (room_id, room) in rooms.iter_mut() {
                room.spawn_forwarder(outbound_clone.clone()).map_err(|_| {
                    SessionError::RoomReceiverAlreadySpawned {
                        peer_id: self.peer_id.clone(),
                        room_id: room_id.clone(),
                    }
                })?;
            }
        }

        // Spawn inbound routing task
        let rooms = Arc::clone(&self.rooms);
        let peer_id = self.peer_id.clone();
        let broadcast_tx = self.inbound_broadcast.clone();
        let task = tokio::spawn(Self::inbound_task_loop(
            rooms,
            peer_id,
            inbound_rx,
            broadcast_tx,
        ));
        self.inbound_task = Some(task);
        Ok(())
    }

    /// Disconnect transport wiring and stop routing tasks.
    pub fn disconnect(&mut self) {
        self.outbound_tx = None;
        if let Some(task) = self.inbound_task.take() {
            task.abort();
        }
    }

    /// Handle PublishRooms negotiation and compute joined rooms.
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

        Ok(())
    }

    fn compute_intersection(&self) -> Vec<RoomId> {
        use std::collections::HashSet;

        let local_rooms: HashSet<_> = self
            .rooms
            .try_lock()
            .map(|rooms| rooms.keys().cloned().collect())
            .unwrap_or_default();

        if let Some(peer_rooms) = &self.peer_offered_rooms {
            let peer_set: HashSet<_> = peer_rooms.iter().cloned().collect();
            local_rooms.intersection(&peer_set).cloned().collect()
        } else {
            Vec::new()
        }
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
        broadcast_tx: Option<broadcast::Sender<(RoomId, Vec<u8>)>>,
    ) {
        while let Some((room_id, bytes)) = inbound_rx.recv().await {
            if let Some(ref tx) = broadcast_tx {
                let _ = tx.send((room_id.clone(), bytes.clone()));
            }
            Self::route_inbound_message(&rooms, &peer_id, room_id, bytes).await;
        }
        tracing::debug!("Peer {} inbound task stopped", peer_id);
    }

    /// Returns the rooms offered locally for negotiation (debug/testing helper).
    pub fn local_offered_rooms(&self) -> Vec<RoomId> {
        self.rooms
            .try_lock()
            .map(|rooms| rooms.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Clone of the outbound sender when connected.
    pub fn outbound_sender(&self) -> Option<mpsc::Sender<(RoomId, Vec<u8>)>> {
        self.outbound_tx.clone()
    }

    /// Subscribe to inbound broadcast channel when connected.
    pub fn subscribe_inbound(&self) -> Option<broadcast::Receiver<(RoomId, Vec<u8>)>> {
        self.inbound_broadcast.as_ref().map(|tx| tx.subscribe())
    }

    /// Send raw bytes to the specified room.
    pub async fn send_raw_to_room(
        &self,
        room_id: &RoomId,
        bytes: Vec<u8>,
    ) -> Result<(), SessionError> {
        let sender = self
            .outbound_tx
            .clone()
            .ok_or_else(|| SessionError::PeerNotConnected(self.peer_id.clone()))?;

        sender
            .send((room_id.clone(), bytes))
            .await
            .map_err(|_| SessionError::SendFailed)
    }

    /// Inspect joined rooms.
    pub fn joined_rooms(&self) -> &[RoomId] {
        &self.joined_rooms
    }

    /// Check whether a room was negotiated with the peer.
    pub fn is_room_joined(&self, room_id: &RoomId) -> bool {
        self.joined_rooms.contains(room_id)
    }
}

#[async_trait::async_trait]
impl PeerChannelsTrait for PeerChannels {
    fn peer_id(&self) -> &PeerId {
        &self.peer_id
    }

    fn joined_rooms(&self) -> &[RoomId] {
        &self.joined_rooms
    }

    fn is_room_joined(&self, room_id: &RoomId) -> bool {
        self.joined_rooms.contains(room_id)
    }

    fn outbound_sender(&self) -> Option<mpsc::Sender<(RoomId, Vec<u8>)>> {
        self.outbound_tx.clone()
    }

    fn subscribe_inbound(&self) -> Option<broadcast::Receiver<(RoomId, Vec<u8>)>> {
        self.inbound_broadcast.as_ref().map(|tx| tx.subscribe())
    }

    async fn send_to_room(&self, room_id: &RoomId, bytes: Vec<u8>) -> Result<(), SessionError> {
        self.send_raw_to_room(room_id, bytes).await
    }
}
