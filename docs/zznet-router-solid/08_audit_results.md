# Architectural Audit Report: Ground Truth 3.0 vs. Implementation

**Date:** October 29, 2025
**Auditor:** GitHub Copilot
**Scope:** ZZPing Network Layer Architecture Verification
**Methodology:** Systematic code investigation against stated architectural principles

---

## Executive Summary

This audit examined the ZZPing codebase to verify compliance with the "Ground Truth 3.0" architectural vision. The investigation reveals a **mixed implementation** where some architectural principles are correctly followed while others show significant deviations from the stated design.

**Key Finding:** The codebase demonstrates partial alignment with the architectural vision. The most significant deviation is the presence of **runtime data routing responsibilities in `RouterActor`**, which contradicts the principle that `Router` should be a "Session Factory and Lifecycle Manager" only.

---

## Audit Point 1: The Role of the `Router`

### Principle to Verify
> `Router` is a *Session Factory and Lifecycle Manager*, not a runtime message router. Its job is limited to connect/disconnect events.

### Investigation Performed
1. Examined `RouterActor` message handlers in `/home/deavid/git/rust/zzping/src/net/zznet-router/src/actor.rs`
2. Identified all public message types handled by the actor
3. Categorized messages by purpose (lifecycle vs. data routing)

### Findings

#### Messages Handled by `RouterActor`:

**Lifecycle Management (✅ Aligned with Vision):**
- `RegisterManager` - Register room managers
- `OnPeerConnected` - Handle peer connection
- `OnPeerDisconnected` - Handle peer disconnection
- `HandlePublishRooms` - Room negotiation
- `PeerSender` - Get peer's outbound channel
- `SubscribePeerInbound` - Get peer's inbound broadcast
- `PeerJoinedRooms` - Query joined rooms
- `IsRoomJoined` - Query room membership

**Runtime Data Routing (❌ DEVIATION from Vision):**
- `SendToPeer` (line 195-221) - Sends data to a specific peer's room at runtime
- `BroadcastToPeers` (line 223-249) - Broadcasts data to multiple peers at runtime

### Code Evidence

```rust
/// Send message to peer room
#[derive(Message)]
#[rtype(result = "Result<(), String>")]
pub struct SendToPeer {
    pub peer_id: PeerId,
    pub room_id: RoomId,
    pub bytes: Vec<u8>,
}

impl Handler<SendToPeer> for RouterActor {
    type Result = ResponseFuture<Result<(), String>>;

    fn handle(&mut self, msg: SendToPeer, _ctx: &mut Context<Self>) -> Self::Result {
        let router_arc = self.router.clone();
        // ... routing logic ...
    }
}
```

### Verdict: ⚠️ **PARTIAL DEVIATION**

**Analysis:** The `RouterActor` correctly handles lifecycle events (connection, disconnection, room negotiation) but **also** contains runtime data routing handlers (`SendToPeer`, `BroadcastToPeers`). According to the vision, these runtime routing operations should be performed directly via `PeerChannels` without going through the `RouterActor`.

**Impact:** Medium. The architecture is conceptually correct (lifecycle management is separated), but the API surface exposes message-based routing that bypasses the intended direct channel access pattern.

**Recommendation:** The `SendToPeer` and `BroadcastToPeers` handlers should be deprecated or removed. Components should obtain channel handles via `PeerSender`/`SubscribePeerInbound` and perform sends directly.

---

## Audit Point 2: The Role of `PeerChannels`

### Principle to Verify
> `PeerChannels` is the *1:1 Session Data Handler*, owning the data path for a single peer.

### Investigation Performed
1. Inspected `PeerChannels` struct in `/home/deavid/git/rust/zzping/src/net/zznet-router/src/peer_channels.rs`
2. Verified presence of required components for managing bidirectional data flow
3. Examined the inbound processing task implementation

### Findings

#### `PeerChannels` Structure (lines 93-99):

```rust
pub struct PeerChannels {
    peer_id: PeerId,
    outbound_tx: mpsc::Sender<(RoomId, Vec<u8>)>,
    inbound_broadcast: broadcast::Sender<(RoomId, Vec<u8>)>,
    joined_rooms: TokioMutex<Vec<RoomId>>,
    inbound_task: JoinHandle<()>,
}
```

#### Confirmed Components:
- ✅ Outbound channel (`outbound_tx`)
- ✅ Inbound broadcast channel (`inbound_broadcast`)
- ✅ Inbound processing task (`inbound_task`)
- ✅ Room membership tracking (`joined_rooms`)

#### Task Implementation (lines 159-168):

```rust
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
}
```

### Verdict: ✅ **FULLY ALIGNED**

**Analysis:** The `PeerChannels` struct correctly contains all the machinery necessary to manage a single peer's bidirectional data flow. The implementation includes:
1. Dedicated channels for outbound and inbound data
2. A background task that processes incoming messages
3. Room-based message routing logic
4. Clean separation of concerns (one instance per peer)

**Impact:** None. This component is implemented exactly as envisioned.

---

## Audit Point 3: `zznet-hello` as the Protocol Boundary

### Principle to Verify
> `zznet-hello` is responsible for handling both Protocol A (`HELLO`) and the outer envelope of Protocol B (`RoomMessage`).

### Investigation Performed
1. Reviewed `Frame` enum in `/home/deavid/git/rust/zzping/src/net/zznet-hello/src/protocol.rs`
2. Examined `HelloActor` message handling logic in `/home/deavid/git/rust/zzping/src/net/zznet-hello/src/actor.rs`
3. Verified state machine behavior for handshake vs. ready states

### Findings

#### Frame Enum Definition (lines 13-18):

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Frame {
    /// Handshake frame - used during connection establishment.
    Handshake(HandshakeFrame),
    /// Room frame - used for actual room-to-room communication.
    Room(RoomFrame),
}
```

#### HelloActor Frame Handling (lines 309-344):

```rust
fn handle_room_frame(&mut self, data: Vec<u8>, ctx: &mut Context<Self>) {
    match Frame::deserialize(&data) {
        Ok(Frame::Room(room_frame)) => {
            match room_frame {
                RoomFrame::Message { from_room, to_room, payload } => {
                    // Forward to ConnectionManager via inbound_tx
                    if let Some(ref tx) = self.inbound_tx {
                        if let Err(e) = tx.try_send((to_room.clone(), payload)) {
                            error!("Failed to forward inbound message: {:?}", e);
                        }
                    }
                }
                RoomFrame::Disconnect => {
                    info!("Peer sent disconnect");
                    ctx.stop();
                }
            }
        }
        Ok(Frame::Handshake(_)) => {
            warn!("Received handshake frame after handshake complete, ignoring");
        }
        Err(e) => {
            error!("Failed to deserialize room frame: {}", e);
            self.handle_error(e.into(), ctx);
        }
    }
}
```

#### State Machine (lines 67-77):

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActorState {
    /// Performing handshake.
    Handshaking,
    /// Handshake complete, ready for room communication.
    Ready,
    /// Connection failed or closed.
    Failed,
}
```

### Verdict: ✅ **FULLY ALIGNED**

**Analysis:** The `zznet-hello` component correctly implements both protocol layers:
1. **Protocol A (Handshake):** Handles `Frame::Handshake` variants during the `Handshaking` state
2. **Protocol B (Room Messages):** Handles `Frame::Room` variants in the `Ready` state
3. **State Machine:** Properly transitions between states and enforces that room frames are only processed when ready
4. **Envelope Processing:** Deserializes the outer `Frame` structure and dispatches based on variant

The actor correctly serves as the protocol boundary, handling frame-level concerns before forwarding application-level payloads to the session layer.

**Impact:** None. This component is implemented exactly as envisioned.

---

## Audit Point 4: The `PeerChannels` to `Room<T>` Connection

### Principle to Verify
> The connection between `PeerChannels` and the various `Room<T>` actors is achieved via actor messaging (`Recipient`), not trait objects (`Box<dyn RoomHandle>`).

### Investigation Performed
1. Searched `zznet-router` crate for `Box<dyn RoomHandle>` usage
2. Inspected `PeerChannels` struct definition for storage mechanisms
3. Examined how rooms are stored and referenced

### Findings

#### Evidence of Trait Object Usage (line 9):

```rust
type SessionRooms = Arc<TokioMutex<HashMap<RoomId, Box<dyn RoomHandle>>>>;
```

#### PeerChannelsBuilder (lines 16-17):

```rust
pub struct PeerChannelsBuilder {
    peer_id: PeerId,
    rooms: HashMap<RoomId, Box<dyn RoomHandle>>,
}
```

#### Add Room Method (lines 28-40):

```rust
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
```

### Verdict: ❌ **MAJOR DEVIATION**

**Analysis:** The codebase **extensively uses** `Box<dyn RoomHandle>` throughout the `PeerChannels` implementation. This represents the old trait-object-based wiring pattern and directly contradicts the vision of actor-based messaging via `Recipient`.

**Specific Deviations:**
1. Rooms are stored as `HashMap<RoomId, Box<dyn RoomHandle>>` instead of `HashMap<RoomId, Recipient<SomeMessageType>>`
2. The builder pattern still accepts and stores trait objects
3. Message routing calls trait methods on `dyn RoomHandle` instead of sending actor messages

**Impact:** High. This is a fundamental architectural pattern that affects testability, composition, and adherence to the actor model throughout the system.

**Recommendation:** This represents incomplete migration work. The `RoomHandle` trait-object approach should be replaced with:
- `Room<T>` instances returned as actor `Recipient` handles
- Message-based communication via Actix `Recipient<RoomMessage>`
- Removal of the `RoomHandle` trait in favor of pure actor messaging

---

## Audit Point 5: The Unidirectional "Club Sandwich" Flow

### Principle to Verify
> Information flows one-way: `ConnectionManager` -> `PeerManagerActor` -> `RouterActor`.

### Investigation Performed
1. Started at `HandshakeComplete` handler in `/home/deavid/git/rust/zzping/src/net/zznet-hello/src/connection_manager.rs`
2. Traced the code path for handling authenticated connections
3. Identified direct communications and message passing patterns
4. Examined `PeerManagerActor` to verify event forwarding

### Findings

#### ConnectionManager HandshakeComplete Handler (lines 311-456):

**Authorization and Setup:**
```rust
impl Handler<HandshakeComplete> for ConnectionManager {
    type Result = ();

    fn handle(&mut self, msg: HandshakeComplete, _ctx: &mut Context<Self>) {
        // ... authorization logic ...

        // Spawn async task
        tokio::spawn(async move {
            let peer_state = PeerState::new_connected(...);

            // 1. Send ConnectPeerWithChannels to PeerManager
            let connect_result = pm_addr
                .send(ConnectPeerWithChannels {
                    peer_id: peer_id_api.clone(),
                    outbound_tx: outbound_tx.clone(),
                    inbound_rx: conn_to_session_rx,
                })
                .await;

            // 2. Send AddPeer to PeerManager
            let add_result = pm_addr.send(AddPeer { peer_state }).await;

            // ... no direct RouterActor communication ...
        });
    }
}
```

#### PeerManagerActor ConnectPeerWithChannels Handler (lines 250-305):

```rust
impl Handler<ConnectPeerWithChannels> for PeerManagerActor {
    type Result = Result<(), String>;

    fn handle(&mut self, msg: ConnectPeerWithChannels, _ctx: &mut Context<Self>) -> Self::Result {
        // ... permission derivation ...

        // Forward to RouterActor
        if let Some(router_actor) = &self.router_actor {
            let msg = OnPeerConnected {
                peer_id,
                permission,
                outbound_tx: msg.outbound_tx,
                inbound_rx: msg.inbound_rx,
            };
            router_actor.do_send(msg);  // Fire-and-forget
        } else {
            tracing::warn!("No RouterActor set, cannot forward OnPeerConnected");
        }

        Ok(())
    }
}
```

### Verdict: ✅ **FULLY ALIGNED**

**Analysis:** The connection flow correctly implements the unidirectional "club sandwich" pattern:

1. **ConnectionManager** handles `HandshakeComplete`
   - Performs authorization
   - Creates channels
   - Sends `ConnectPeerWithChannels` to `PeerManagerActor` (one-way)
   - Sends `AddPeer` to `PeerManagerActor` (one-way)
   - **Does NOT** communicate directly with `RouterActor`

2. **PeerManagerActor** receives connection event
   - Derives permission from role
   - Forwards `OnPeerConnected` to `RouterActor` via `do_send()` (fire-and-forget)
   - **No response awaited**

3. **Flow Characteristics:**
   - ✅ Unidirectional (no callbacks or awaited responses)
   - ✅ No direct ConnectionManager → RouterActor communication
   - ✅ Fire-and-forget message passing (`do_send`)
   - ✅ Each layer operates independently

**Impact:** None. The event-driven, unidirectional architecture is correctly implemented.

---

## Summary of Deviations

| Audit Point | Principle | Status | Severity | Notes |
|------------|-----------|--------|----------|-------|
| 1. Router Role | Lifecycle only, not runtime routing | ⚠️ Partial | Medium | `SendToPeer`/`BroadcastToPeers` handlers present |
| 2. PeerChannels | 1:1 Session Data Handler | ✅ Aligned | None | Fully implements required machinery |
| 3. Hello Protocol | Handles both Protocol A & B | ✅ Aligned | None | Correct state machine and frame handling |
| 4. Room Wiring | Actor messaging, not trait objects | ❌ Deviation | High | Still uses `Box<dyn RoomHandle>` |
| 5. Unidirectional Flow | ConnectionManager → PeerManager → Router | ✅ Aligned | None | Correct event-driven pattern |

---

## Recommendations

### Priority 1: High Severity

**Remove Trait Object Wiring (Audit Point 4)**
- Replace `Box<dyn RoomHandle>` with actor `Recipient` handles
- Implement message-based `Room<T>` communication
- Remove or deprecate the `RoomHandle` trait
- Update `PeerChannels` to store `HashMap<RoomId, Recipient<RoomMsg>>`

### Priority 2: Medium Severity

**Refactor Router Data Routing API (Audit Point 1)**
- Deprecate `SendToPeer` and `BroadcastToPeers` actor messages
- Document that components should use `PeerSender`/`SubscribePeerInbound` for direct channel access
- Add migration guide for components currently using these messages
- Consider making these handlers emit deprecation warnings

### Priority 3: Documentation

**Update Architectural Documentation**
- Document the current hybrid state (what's aligned, what's not)
- Create a migration roadmap for full "Ground Truth 3.0" compliance
- Add examples showing the intended pattern (direct channel usage)

---

## Conclusion

The ZZPing codebase demonstrates **strong adherence to the core architectural principles** in 3 out of 5 audit points, with one **major deviation** and one **partial deviation**.

### Strengths:
- ✅ `PeerChannels` correctly implements 1:1 session data handling
- ✅ `zznet-hello` properly handles both protocol layers with clean state machine
- ✅ Unidirectional event flow is correctly implemented end-to-end

### Weaknesses:
- ❌ Trait object wiring (`Box<dyn RoomHandle>`) contradicts actor messaging vision
- ⚠️ Runtime routing messages on `RouterActor` bypass direct channel pattern

The most critical finding is the **extensive use of trait object wiring** where actor messaging was intended. This represents incomplete migration work and should be prioritized for refactoring to achieve full architectural alignment.

---

**Audit completed:** October 29, 2025
**Next steps:** Address Priority 1 recommendation (trait object removal) to achieve full architectural compliance.
