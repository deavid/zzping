# Phase 1 — Introduce a generic RoomActor<T> and prepare APIs

Date: 2025-10-30

Objective
- Create a reusable, generic Actix actor RoomActor<T> inside zznet-room that centralizes (de-)serialization.
- Adjust the Router ↔ RoomManager creation contract so RoomManager can wire outbound sending without ad-hoc lookups.
- Keep the system compiling after this phase with zero behavior change for components (they may still use their current NetworkActor). This phase lays the plumbing.

Success criteria
- New zznet-room::actor::RoomActor<T> exists with unit tests.
- RoomManager::create_for_peer can receive the peer outbound sender (clone) to wire RoomActor.
- RouterActor::OnPeerConnected passes the outbound sender to RoomManager::create_for_peer.
- All RoomManager implementors compile after signature change (they can ignore the new param for now).

Why these API tweaks are required
- Today, RouterActor constructs PeerChannels using an outbound_tx known only inside OnPeerConnected. RoomManager::create_for_peer currently has no way to access that sender; creating RoomActor<T> inside RoomManager would be impossible without changing the API.
- Passing a clone of outbound_tx into RoomManager::create_for_peer preserves current layering and avoids global lookups.

Planned changes (concrete)

1) Add RoomActor<T> in zznet-room
- File: src/net/zznet-room/src/actor.rs
- Type: pub struct RoomActor<T>
  - Fields:
    - room_id: zznet_api::types::RoomId
    - outbound_tx: tokio::sync::mpsc::Sender<(RoomId, Vec<u8>)>
    - component_recipient: actix::Recipient<T>
  - Bounds: T: actix::Message<Result = ()> + serde::Serialize + serde::de::DeserializeOwned + Send + 'static
- Impl:
  - impl actix::Actor for RoomActor<T>
  - impl actix::Handler<zznet_room::room_manager::InboundRoomPayload> for RoomActor<T>
    - Deserialize Vec<u8> → T with bincode::serde::decode_from_slice
    - Forward to component_recipient.do_send(msg)
  - impl actix::Handler<T> for RoomActor<T>
    - Serialize T → Vec<u8> with bincode::serde::encode_to_vec
    - outbound_tx.send((room_id.clone(), bytes)).await
- Constructor:
  - pub fn new(room_id: RoomId, outbound_tx: mpsc::Sender<(RoomId, Vec<u8>)>, component_recipient: Recipient<T>) -> Self
- Tests (src/net/zznet-room/src/tests_room_actor.rs or appended to existing tests.rs):
  - Round-trip: send InboundRoomPayload with encoded bytes → recipient receives T
  - Outbound: do_send(T) on actor → mpsc receiver gets (room_id, bytes) that decode back to T

2) Export RoomActor from zznet-room
- File: src/net/zznet-room/src/lib.rs
  - pub mod actor;
  - pub use actor::RoomActor;

3) Extend RoomManager::create_for_peer signature to receive outbound sender
- File: src/net/zznet-room/src/room_manager.rs
- Current:
  async fn create_for_peer(&self, peer_id: PeerId, permission: Permission, room_id: &RoomId) -> Result<Option<Recipient<InboundRoomPayload>>, CreateError>;
- Change to:
  async fn create_for_peer(
      &self,
      peer_id: PeerId,
      permission: Permission,
      room_id: &RoomId,
      outbound_to_peer: tokio::sync::mpsc::Sender<(RoomId, Vec<u8>)>,
  ) -> Result<Option<Recipient<InboundRoomPayload>>, CreateError>;
- Rationale: Lets RoomManager construct RoomActor<T> (now or in Phase 2) with a working outbound.

4) Pass outbound sender from RouterActor::OnPeerConnected into RoomManager::create_for_peer
- File: src/net/zznet-router/src/actor.rs
- In OnPeerConnected handler, before building PeerChannels, clone outbound_tx for each call:
  - let mut builder = PeerChannelsBuilder::new(peer_id.clone());
  - for manager in router.managers.values() {
      for room_id in manager.managed_rooms() {
        let sender_clone = outbound_tx.clone();
        if let Ok(Some(room)) = manager.create_for_peer(peer_id.clone(), permission.clone(), &room_id, sender_clone).await { builder.add_room(room_id.clone(), room)?; }
      }
    }

5) Make all RoomManager implementors compile with the new signature
- Files:
  - src/components/zzmem-db/src/network_manager.rs
  - src/components/zzpinger/src/network_manager.rs
  - src/components/zzcollector-state/src/network_manager.rs
  - src/components/zzintent-config/src/network_manager.rs
- Temporary action for Phase 1: accept the new parameter `_outbound_to_peer: mpsc::Sender<(RoomId, Vec<u8>)>` and ignore it; keep returning the existing NetworkActor Recipient<InboundRoomPayload>.

6) Build & test plan for Phase 1
- Build gates:
  - cargo check at workspace root
- Unit tests:
  - Add RoomActor tests (see step 1 Tests) and run: cargo test -p zznet-room
- Integration smoke:
  - Ensure zzping-database and zzping-collector apps compile and their basic runtime is unaffected (no functional changes intended yet)

Risk assessment and mitigations
- API change ripple in RoomManager implementors: We address by adding parameter but not changing behavior yet, keeping compile green.
- RouterActor change: purely plumbing to pass sender; PeerChannels still built the same; no behavior change.

Nice-to-haves (optional in Phase 1)
- Add a small doc section in zznet-room/README.md clarifying that RoomActor<T> now provides the centralized (de-)serialization for the router path.

Deliverables
- New: src/net/zznet-room/src/actor.rs with tests
- Updated: src/net/zznet-room/src/lib.rs exports
- Updated: src/net/zznet-room/src/room_manager.rs trait signature
- Updated: src/net/zznet-router/src/actor.rs uses new signature
- Updated: all component RoomManager impls accept new param (ignored for now)

Acceptance
- All changes compile and tests pass; no functional behavior change to runtime.