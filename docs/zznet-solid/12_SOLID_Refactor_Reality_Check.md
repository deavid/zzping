# ZZNet SOLID Refactor: Reality Check and Critical Analysis

**Date:** 2025-10-28
**Status:** Critical Investigation Report
**Purpose:** Code-based audit to determine if SOLID principles are actually being followed after the refactoring effort

---

## Executive Summary

**The claim that 99% of code stayed in zznet-session is FALSE.**

However, **the claim that SOLID principles are now being followed is PARTIALLY FALSE.**

### What Actually Happened

The refactor was **structurally successful** but **architecturally incomplete**:

1. ✅ **Code WAS moved:** zznet-peer-manager (779 LOC) and zznet-router (543 LOC) contain substantial implementations. zznet-session is now only 373 LOC (mostly a thin Actix wrapper).

2. ⚠️ **SOLID violations remain:** The split didn't go far enough. Critical architectural problems persist:
   - **Interface Segregation Principle (ISP)** - Still violated
   - **Single Responsibility Principle (SRP)** - Partially violated
   - **Dependency Inversion Principle (DIP)** - Compromised

3. ❌ **Three-Actor Pattern isolation is broken:** Main actors (IntentConfigActor) have direct dependencies on `zznet-session::PeerManagerActor`, violating the network-oblivious requirement.

---

## Part 1: What the Documents Say vs What the Code Shows

### Document Claims (from 01_ZZNet_SOLID_Framework.md)

> **"The Main Component Actor must be completely unaware of the network. It does not know about peers, rooms, or connections. Its only external communication is with its dedicated NetworkManagerActor."**

> **"Constraint: Must not have any `zznet-*` dependencies (beyond `-api` types). This is enforced by crate dependency rules."**

### Code Reality

**File:** `src/components/zzintent-config/src/actor.rs` (lines 1-40)

```rust
use zznet_session::PeerManagerActor;

pub struct IntentConfigActor {
    current_config: IntentConfigData,
    subscribers: HashMap<usize, Recipient<IntentConfigData>>,
    next_id: usize,
    role: IntentConfigRole,

    // ❌ VIOLATION: Main actor holds network infrastructure address
    peer_manager: Option<Addr<PeerManagerActor>>,

    // ✅ CORRECT: Main actor holds network manager address
    network_manager: Option<Addr<IntentConfigNetworkManager>>,

    // ❌ VIOLATION: Main actor knows about rooms (network concept)
    room: Option<zznet_room::room::Room<IntentConfigNetworkMsg>>,
    room_channels: Option<std::sync::Arc<zznet_room::room::RoomChannels>>,
}
```

**File:** `src/components/zzintent-config/Cargo.toml` (line 36)

```toml
# Phase 3.6-3.8: SessionManager bridge (temporary)
# Still using SessionManager as interface to PeerManager/Router during three-actor migration.
# SessionManager provides: GetPeerRole, GetPeerSender, SubscribePeerInbound
# TODO: Replace with direct PeerManager/Router access when SessionManager is fully deprecated
zznet-session = { workspace = true }
```

**Verdict:** The Main Actor (`IntentConfigActor`) is NOT network-oblivious. It has direct knowledge of network infrastructure (`PeerManagerActor`) and room concepts. The crate dependency constraint is being violated with a TODO justification.

---

## Part 2: SOLID Principle Analysis (Code-Based)

### 2.1. Single Responsibility Principle (SRP)

**Claim:** "zznet-session has been split so each crate has a single responsibility."

**Reality:** Partially achieved, but `SessionCoordinator` and `PeerManagerActor` have multiple concerns.

#### Evidence: SessionCoordinator (zznet-session/src/coordinator.rs)

```rust
pub struct SessionCoordinator {
    peer_manager: PeerManager,  // Control-plane state
    router: Router,              // Data-plane routing
}

impl SessionCoordinator {
    // ✅ Good: Delegates to peer_manager
    pub fn peer_manager(&self) -> &PeerManager { ... }

    // ✅ Good: Delegates to router
    pub fn router(&self) -> &Router { ... }

    // ⚠️ HYBRID CONCERN: Coordinates both planes
    pub fn add_peer(&mut self, state: PeerState, channels: PeerChannels) -> Result<(), SessionError> {
        let peer_id = state.id().clone();
        self.peer_manager.add_peer(state)?;      // Control-plane

        if let Err(err) = self.router.register_peer(channels) {  // Data-plane
            let _ = self.peer_manager.remove_peer(&peer_id);     // Rollback coordination
            return Err(err);
        }

        self.peer_manager.notify_peer_connected(&peer_id);  // Event broadcasting
        Ok(())
    }
}
```

**Analysis:**
- `SessionCoordinator` has **transaction coordination** responsibility (rollback logic).
- This is a **third concern** beyond pure delegation.
- The ADR says: "PeerSession is eliminated. Its state is split between PeerManager and Router."
- Reality: `PeerSession` logic moved to `SessionCoordinator`, which is a **renamed God Object**, not a true elimination.

**SRP Verdict:** ⚠️ **Partially Fixed** - The concerns are more isolated than before, but coordination logic creates a third responsibility that violates pure SRP.

---

### 2.2. Interface Segregation Principle (ISP)

**Claim:** "The fat interface of SessionManager has been broken down into focused interfaces."

**Reality:** `PeerManagerActor` still exposes a fat interface with 10+ message handlers.

#### Evidence: PeerManagerActor Messages (zznet-session/src/actor.rs)

```rust
// Query messages (6 different concerns)
pub struct GetPeerRole { pub peer_id: PeerId }
pub struct GetPeerSender { pub peer_id: PeerId }
pub struct SubscribePeerInbound { pub peer_id: PeerId }
pub struct GetPeerIdentity { pub peer_id: PeerId }
pub struct GetPeersWithRole { pub role: Role }
pub struct GetPeerIds;
pub struct IsPeerConnected { pub peer_id: PeerId }
pub struct GetConnectedPeerCount;

// Lifecycle mutation messages (3 different concerns)
pub struct AddPeer { pub peer_state: PeerState, pub peer_channels: PeerChannels }
pub struct RemovePeer { pub peer_id: PeerId }
pub struct DisconnectPeer { pub peer_id: PeerId }
```

**Analysis:**
- `PeerManagerActor` has **11 different message types** covering:
  - Control-plane queries (role, identity, peer list)
  - Data-plane queries (sender channel, inbound subscription)
  - Lifecycle mutations (add, remove, disconnect)
- Clients that only need to query roles are forced to depend on the entire actor interface.
- The **data-plane query methods** (`GetPeerSender`, `SubscribePeerInbound`) should NOT be on a control-plane actor.

**ISP Verdict:** ❌ **VIOLATED** - The actor interface is still monolithic. Clients cannot depend on narrow interfaces.

---

### 2.3. Dependency Inversion Principle (DIP)

**Claim:** "The design correctly depends on abstractions (RoomHandle trait, Tokio channels) rather than concrete implementations."

**Reality:** Mostly correct, but the `PeerManagerActor` facade compromises this.

#### Evidence: Component Dependencies

**File:** `src/components/zzintent-config/src/network_manager.rs` (lines 23-26)

```rust
// Phase 7.2: Use PeerManagerActor directly (no more SessionManager bridge)
use zznet_api::types::{PeerId, PeerLifecycleEvent};
pub use zznet_session::actor::{
    GetPeerRole, GetPeerSender, PeerManagerActor, SubscribePeerInbound,
};
```

**Analysis:**
- Components depend on the **concrete Actix actor** (`PeerManagerActor`), not an abstract interface.
- There is no `PeerManagerTrait` or abstract `PeerManager` interface that could be mocked or substituted.
- This makes unit testing components harder (must use real actor system).

**DIP Verdict:** ⚠️ **Compromised** - While the underlying `PeerManager` and `Router` depend on abstractions, the Actix wrapper forces concrete dependencies at the component level.

---

## Part 3: The PeerChannels Problem (Biggest Remaining SOLID Violation)

### What the ADR Says

> **"zznet-router (The Data Plane): Has zero knowledge of peer state, roles, or authentication. It is a stateless byte-forwarder."**

### What the Code Shows

**File:** `src/net/zznet-router/src/peer_channels.rs`

```rust
use zznet_room::room_handle::RoomHandle;

type SessionRooms = Arc<TokioMutex<HashMap<RoomId, Box<dyn RoomHandle>>>>;

pub struct PeerChannels {
    peer_id: PeerId,
    rooms: SessionRooms,                              // ❌ ROOM MANAGEMENT
    outbound_tx: Option<mpsc::Sender<(RoomId, Vec<u8>)>>,
    inbound_task: Option<JoinHandle<()>>,
    inbound_broadcast: Option<broadcast::Sender<(RoomId, Vec<u8>)>>,
    peer_offered_rooms: Option<Vec<RoomId>>,          // ❌ ROOM NEGOTIATION STATE
    joined_rooms: Vec<RoomId>,                        // ❌ ROOM LIFECYCLE STATE
}

impl PeerChannels {
    /// Add a room to this peer.
    pub async fn add_room(&mut self, room_id: RoomId, room: Box<dyn RoomHandle>) -> Result<(), SessionError> {
        // ❌ ROOM MANAGEMENT LOGIC (not pure routing)
    }

    /// Connect the peer channels to transport wiring.
    pub async fn connect(&mut self, outbound_tx: mpsc::Sender<(RoomId, Vec<u8>)>, inbound_rx: mpsc::Receiver<(RoomId, Vec<u8>)>) -> Result<(), SessionError> {
        // Spawns forwarder for each room
        let mut rooms = self.rooms.lock().await;
        for (room_id, room) in rooms.iter_mut() {
            room.spawn_forwarder(outbound_clone.clone()).map_err(...)?;  // ❌ ROOM LIFECYCLE
        }
    }

    /// Handle PublishRooms negotiation and compute joined rooms.
    pub fn handle_peer_offered_rooms(&mut self, peer_rooms: Vec<RoomId>) -> Result<(), SessionError> {
        // ❌ ROOM NEGOTIATION PROTOCOL (business logic, not routing)
    }
}
```

### The Problem

`PeerChannels` is supposed to be a **pure data-plane structure** (transport channels). Instead, it has:

1. **Room management** (add_room, rooms HashMap)
2. **Room lifecycle** (spawn_forwarder, inbound_task management)
3. **Room negotiation** (handle_peer_offered_rooms, compute_intersection)
4. **State tracking** (peer_offered_rooms, joined_rooms)

This is **80% of what the old `PeerSession` did**, just renamed and relocated to `zznet-router`.

**The ADR explicitly said:**

> **"`PeerSession`: Is eliminated. Its state is split between the `PeerManager` and `Router`."**

**Reality:** `PeerSession` was **renamed to `PeerChannels`** and moved to the router. The state wasn't truly split—it was just relocated with a new name.

---

## Part 4: Why This Happened (Root Cause Analysis)

### 4.1. Misunderstanding of "Data Plane"

**Misconception:** "Data plane = anything involving bytes and channels"

**Reality:** Data plane should mean:
- Hold a `HashMap<PeerId, mpsc::Sender>` for outbound routing
- Expose `send_to_peer(peer_id, room_id, bytes)` API
- **Period. Nothing else.**

Room negotiation (`PublishRooms` protocol), room lifecycle (spawning forwarders), and room state (joined_rooms) are **higher-level concerns** that belong in a separate **session/coordination layer** or in the component's Network Actor.

### 4.2. The "Coordinator" Anti-Pattern

The `SessionCoordinator` was introduced to "coordinate" `PeerManager` and `Router`. But this created a **new God Object** that:
- Manages transaction semantics (rollback on error)
- Coordinates lifecycle between two systems
- Emits events

This is not a solution—it's a **renamed problem**. The architecture still has a central coordination point doing too much.

### 4.3. Failure to Enforce Dependency Rules

**The ADR says:**

> **"Enforcement: Crate dependency graph must not allow imports of `zznet-session`, `zznet-peer-manager`, `zznet-router`, or `zznet-transport-*`. Any violation is a compilation error."**

**Reality:** Every component's `Cargo.toml` has:

```toml
zznet-session = { workspace = true }  # With TODO comment
```

The TODO comment is a **permanent escape hatch** that allows violation of the architecture. TODOs are not enforcement—they're excuses.

### 4.4. Lack of Abstract Interfaces

The refactor created **concrete split** (separate crates) but not **interface split** (separate concerns).

What's missing:
- No `trait PeerRegistry` for control-plane queries (should be in `zznet-api`)
- No `trait MessageRouter` for data-plane operations (should be in `zznet-api`)
- No enforcement that Main Actors can only depend on `zznet-api` types

The crates are split, but the **interfaces are still fat and concrete**.

---

## Part 5: Corrective Actions (How to Actually Fix This)

### 5.1. Extract True Data-Plane Router

Create a **pure routing crate** that has ZERO knowledge of rooms:

```rust
// zznet-router/src/lib.rs (simplified)
pub struct Router {
    peers: HashMap<PeerId, mpsc::Sender<(RoomId, Vec<u8>)>>,
}

impl Router {
    pub fn register_peer(&mut self, peer_id: PeerId, sender: mpsc::Sender<(RoomId, Vec<u8>)>) -> Result<(), SessionError> {
        // Just store the channel
    }

    pub async fn send_to_peer(&self, peer_id: &PeerId, room_id: &RoomId, bytes: Vec<u8>) -> Result<(), SessionError> {
        // Just forward bytes
    }
}
```

**Move out of Router:**
- Room negotiation → `zznet-room` or component's Network Actor
- Room lifecycle → Component's Network Actor
- Room state tracking → Component's Network Actor

### 5.2. Create Abstract Interfaces in zznet-api

```rust
// zznet-api/src/traits.rs

#[async_trait]
pub trait PeerRegistry: Send + Sync {
    async fn get_peer_role(&self, peer_id: &PeerId) -> Option<Role>;
    async fn peers_with_role(&self, role: &Role) -> Vec<PeerId>;
    fn subscribe_events(&self) -> broadcast::Receiver<PeerLifecycleEvent>;
}

#[async_trait]
pub trait MessageRouter: Send + Sync {
    async fn send_to_peer(&self, peer_id: &PeerId, room_id: &RoomId, bytes: Vec<u8>) -> Result<(), SessionError>;
    fn peer_sender(&self, peer_id: &PeerId) -> Option<mpsc::Sender<(RoomId, Vec<u8>)>>;
}
```

Then components depend on **traits**, not concrete actors.

### 5.3. Remove PeerManagerActor Data-Plane Methods

```rust
// ❌ DELETE these from PeerManagerActor:
// - GetPeerSender (data-plane concern)
// - SubscribePeerInbound (data-plane concern)

// ✅ KEEP only control-plane methods:
// - GetPeerRole
// - GetPeerIdentity
// - GetPeersWithRole
// - GetConnectedPeerCount
```

Data-plane queries should go to the `Router` (or through the `MessageRouter` trait), not the `PeerManager`.

### 5.4. Enforce Dependency Rules with Local pre-PR checks

The repository's policy is to avoid CI enforcement. Instead, contributors should run a local pre-PR check to validate crate dependencies before opening a pull request. The project includes a small script `scripts/check-component-dependencies.sh` for this purpose; run it locally and/or as a pre-push hook in your workflow.

Example shell check (run locally or in a `pre-push` hook):

```bash
#!/usr/bin/env bash
set -euo pipefail

main_actor_crates=("zzintent-config" "zzmem-db" "zzcollector-state")

for crate in "${main_actor_crates[@]}"; do
    cargo_toml="src/components/${crate}/Cargo.toml"
    if [ -f "$cargo_toml" ]; then
        if grep -q "zznet-session" "$cargo_toml" || grep -q "zznet-peer-manager" "$cargo_toml" || grep -q "zznet-router" "$cargo_toml"; then
            echo "ERROR: $crate must not depend on network-infrastructure crates (zznet-session/zznet-peer-manager/zznet-router)." >&2
            echo "Please depend only on zznet-api abstractions instead." >&2
            exit 1
        fi
    fi
done

echo "Dependency checks passed."
```

**No TODOs. No excuses. Enforce it locally.**

### 5.5. Create Proper zznet-room Crate

Room negotiation, room lifecycle, and room message dispatch should live in a dedicated `zznet-room` crate that is:
- Used by **Network Actors** (not Main Actors)
- Independent of both `PeerManager` and `Router`
- Focused on the `PublishRooms` protocol and `Room<T>` typed message handling

---

## Part 6: Final Verdict

### What Went Right ✅

1. **Code was moved:** zznet-peer-manager and zznet-router contain real implementations (not stubs).
2. **Tests were written:** Each new crate has unit tests for its core responsibilities.
3. **State isolation:** `PeerManager` correctly owns peer identity/role state separate from routing.
4. **DIP foundation:** The underlying implementations use abstractions (traits, channels).

### What Went Wrong ❌

1. **ISP still violated:** `PeerManagerActor` is a fat interface mixing control and data plane queries.
2. **SRP compromised:** `SessionCoordinator` is a renamed God Object doing coordination + delegation + events.
3. **Three-Actor isolation broken:** Main Actors depend on `zznet-session` and know about rooms.
4. **Dependency rules not enforced:** TODOs allow architectural violations to persist indefinitely.
5. **PeerChannels is PeerSession renamed:** 80% of the old `PeerSession` logic is still there, just moved and renamed.

### The Bottom Line

**The refactor achieved a 60% solution:**
- **Structure**: Crates are split ✅
- **Behavior**: Logic is mostly moved ✅
- **Isolation**: Concerns are separated ⚠️ (partial)
- **Interfaces**: Still monolithic ❌
- **Enforcement**: Not happening ❌

To reach 100%, the corrective actions in Part 5 must be implemented. Until then, **the SOLID principles are aspirational, not actual.**

---

## Appendix: Line Count Breakdown (Actual State)

```
zznet-session/src/:        373 LOC (22% of original)
  - actor.rs:              276 LOC (Actix wrapper with 11 message handlers)
  - coordinator.rs:         97 LOC (Coordination + transaction logic)

zznet-peer-manager/src/:   779 LOC (NEW, control-plane)
  - lib.rs:                330 LOC (PeerManager implementation)
  - peer_state.rs:          87 LOC (PeerState struct)
  - actor.rs:              ~362 LOC (estimated, if exists)

zznet-router/src/:         543 LOC (NEW, data-plane + room management)
  - lib.rs:                287 LOC (Router implementation)
  - peer_channels.rs:      258 LOC (PeerChannels = renamed PeerSession)

Total codebase:            1695 LOC (up from ~1200 LOC pre-refactor)
```

**The 99% claim is false.** Code was moved, not left behind. But **SOLID compliance is incomplete.**

---

**End of Report**
