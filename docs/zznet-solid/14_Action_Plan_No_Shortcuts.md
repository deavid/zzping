# ZZNet SOLID Refactor – Execution Action Plan (No Shortcuts Allowed)

Date: 2025-10-28
Status: Authoritative execution plan
Purpose: Step-by-step, verifiable tasks to complete SOLID refactor with built-in quality gates that prevent lazy AI execution

---

## 0) Non-Negotiable Rules for AI Agents

### Rule 1: Show Your Work
Every task MUST include:
- Actual file paths and line numbers of changes
- Before/after code snippets (not summaries)
- Actual command output from test runs (not "tests pass")
- Grep/search results proving claims (not "I checked")

### Rule 2: No Aspirational Claims
Forbidden phrases:
- ❌ "This should work"
- ❌ "The tests probably pass"
- ❌ "I believe this is correct"
- ❌ "The code looks good"

Required phrases:
- ✅ "I ran `cargo test --package zznet-api` and got: [paste output]"
- ✅ "Grep returned 3 matches at: [list files:lines]"
- ✅ "Build failed with error: [paste actual error]"

### Rule 3: Verify Every Claim
If you claim something is true, you MUST:
1. Run the command that proves it
2. Paste the actual output
3. Show the file/line if it's a code change

### Rule 4: One Task, One Verification
Do not move to the next task until:
- Current task builds (`cargo check`)
- Current task tests pass (`cargo test --package <affected>`)
- Grep confirms no regressions
- You've pasted the proof

### Rule 5: No "TODO" Comments in Committed Code
If you write `// TODO: fix this later`, the task is NOT done.
Either fix it now or document it as a known limitation with a tracking issue.

---

## Phase 1: Interface Segregation (ISP Fix)

**Goal:** Split the fat `PeerManagerActor` interface into two narrow traits in `zznet-api`.

### Task 1.1: Define PeerRegistry Trait

**Location:** `src/net/zznet-api/src/traits.rs` (create if missing)

**What to create:**
```rust
use crate::types::{PeerId, PeerIdentity, PeerLifecycleEvent, Role};
use async_trait::async_trait;
use tokio::sync::broadcast;

/// Control-plane interface for peer state queries and lifecycle events.
///
/// Implementations MUST NOT expose data-plane concerns (channels, routing).
#[async_trait]
pub trait PeerRegistry: Send + Sync {
    /// Get the authenticated role for a peer, if connected and authenticated.
    async fn get_peer_role(&self, peer_id: &PeerId) -> Option<Role>;

    /// Get the full identity information for a peer.
    async fn get_peer_identity(&self, peer_id: &PeerId) -> Option<PeerIdentity>;

    /// Get all peer IDs matching a specific role.
    async fn peers_with_role(&self, role: &Role) -> Vec<PeerId>;

    /// Get all currently registered peer IDs (connected or not).
    async fn peer_ids(&self) -> Vec<PeerId>;

    /// Get count of connected peers.
    async fn connected_peer_count(&self) -> usize;

    /// Check if a specific peer is currently connected.
    async fn is_peer_connected(&self, peer_id: &PeerId) -> bool;

    /// Subscribe to peer lifecycle events (PeerAdded, PeerConnected, etc).
    fn subscribe_events(&self) -> broadcast::Receiver<PeerLifecycleEvent>;
}
```

**Verification checklist:**
- [ ] File created at exact path: `src/net/zznet-api/src/traits.rs`
- [ ] Trait has exactly 7 methods (no more, no less)
- [ ] No methods return channels or room types
- [ ] Run: `cargo check --package zznet-api`
- [ ] Paste output showing SUCCESS
- [ ] Add `pub mod traits;` to `src/net/zznet-api/src/lib.rs`
- [ ] Run: `cargo check --package zznet-api` again
- [ ] Paste output showing SUCCESS

**Anti-patterns to avoid:**
- ❌ Adding `send_to_peer` to this trait (that's data-plane)
- ❌ Adding `get_peer_sender` to this trait (that's data-plane)
- ❌ Returning `Result<T, SessionError>` when `Option<T>` is sufficient
- ❌ Using synchronous methods when peer state access requires async (DB/lock)

---

### Task 1.2: Define MessageRouter Trait

**Location:** Same file: `src/net/zznet-api/src/traits.rs`

**What to create:**
```rust
use crate::types::{PeerId, RoomId, SessionError};
use tokio::sync::{broadcast, mpsc};

/// Data-plane interface for message routing to peers/rooms.
///
/// Implementations MUST NOT expose control-plane concerns (roles, identity, auth).
#[async_trait]
pub trait MessageRouter: Send + Sync {
    /// Send bytes to a specific peer's room.
    ///
    /// # Errors
    /// - PeerNotFound if peer is not registered
    /// - PeerNotConnected if peer is not in connected state
    /// - RoomNotJoined if peer hasn't negotiated this room
    /// - SendFailed if channel send fails
    async fn send_to_peer(
        &self,
        peer_id: &PeerId,
        room_id: &RoomId,
        bytes: Vec<u8>,
    ) -> Result<(), SessionError>;

    /// Broadcast bytes to multiple peers in a specific room.
    ///
    /// Skips peers that don't have the room joined. Does not fail on partial delivery.
    async fn broadcast_to_peers(
        &self,
        peer_ids: &[PeerId],
        room_id: &RoomId,
        bytes: Vec<u8>,
    ) -> Result<(), SessionError>;

    /// Get a clone of the outbound sender for a peer (if connected).
    ///
    /// Returns None if peer is not connected. Caller can use this for direct sends.
    fn peer_sender(&self, peer_id: &PeerId) -> Option<mpsc::Sender<(RoomId, Vec<u8>)>>;

    /// Subscribe to inbound messages from a peer (if connected).
    ///
    /// Returns None if peer is not connected.
    fn subscribe_peer_inbound(
        &self,
        peer_id: &PeerId,
    ) -> Option<broadcast::Receiver<(RoomId, Vec<u8>)>>;
}
```

**Verification checklist:**
- [ ] Trait added to same file as `PeerRegistry`
- [ ] Trait has exactly 4 methods (no more, no less)
- [ ] No methods reference Role, PeerIdentity, or auth types
- [ ] Run: `cargo check --package zznet-api`
- [ ] Paste output showing SUCCESS
- [ ] Re-export from lib: `pub use traits::{MessageRouter, PeerRegistry};` in `src/net/zznet-api/src/lib.rs`
- [ ] Run: `cargo check --package zznet-api` again
- [ ] Paste output showing SUCCESS

**Anti-patterns to avoid:**
- ❌ Adding `get_peer_role` to this trait (that's control-plane)
- ❌ Adding `peers_with_role` to this trait (that's control-plane)
- ❌ Making `send_to_peer` synchronous (it needs to await on channel send)

---

### Task 1.3: Update zznet-api Dependencies

**Location:** `src/net/zznet-api/Cargo.toml`

**What to add:**
```toml
[dependencies]
async-trait = { workspace = true }
tokio = { workspace = true, features = ["sync"] }
```

**Verification checklist:**
- [ ] Dependencies added to `[dependencies]` section
- [ ] Run: `cargo check --package zznet-api`
- [ ] Paste output showing SUCCESS

---

### Task 1.4: Implement PeerRegistry for PeerManager

**Location:** `src/net/zznet-peer-manager/src/lib.rs`

**What to add (at end of file, before tests):**
```rust
#[async_trait::async_trait]
impl zznet_api::traits::PeerRegistry for PeerManager {
    async fn get_peer_role(&self, peer_id: &PeerId) -> Option<Role> {
        self.get_peer_role(peer_id).cloned()
    }

    async fn get_peer_identity(&self, peer_id: &PeerId) -> Option<PeerIdentity> {
        self.get_peer_identity(peer_id).cloned()
    }

    async fn peers_with_role(&self, role: &Role) -> Vec<PeerId> {
        self.peers_with_role(role)
    }

    async fn peer_ids(&self) -> Vec<PeerId> {
        self.peer_ids()
    }

    async fn connected_peer_count(&self) -> usize {
        self.connected_peer_count()
    }

    async fn is_peer_connected(&self, peer_id: &PeerId) -> bool {
        self.is_peer_connected(peer_id)
    }

    fn subscribe_events(&self) -> broadcast::Receiver<PeerLifecycleEvent> {
        self.subscribe_events()
    }
}
```

**Verification checklist:**
- [ ] Impl block added with exactly 7 methods
- [ ] Update `Cargo.toml` to add: `zznet-api = { workspace = true }` (if not already present)
- [ ] Run: `cargo check --package zznet-peer-manager`
- [ ] Paste output showing SUCCESS
- [ ] Run: `cargo test --package zznet-peer-manager`
- [ ] Paste output showing all tests pass

**Anti-patterns to avoid:**
- ❌ Implementing methods that don't exist in the trait
- ❌ Changing method signatures to not match the trait exactly
- ❌ Adding `unwrap()` calls without error handling

---

### Task 1.5: Implement MessageRouter for Router

**Location:** `src/net/zznet-router/src/lib.rs`

**What to add (at end of file, before tests):**
```rust
#[async_trait::async_trait]
impl zznet_api::traits::MessageRouter for Router {
    async fn send_to_peer(
        &self,
        peer_id: &PeerId,
        room_id: &RoomId,
        bytes: Vec<u8>,
    ) -> Result<(), SessionError> {
        self.send_to_room(peer_id, room_id, bytes).await
    }

    async fn broadcast_to_peers(
        &self,
        peer_ids: &[PeerId],
        room_id: &RoomId,
        bytes: Vec<u8>,
    ) -> Result<(), SessionError> {
        self.broadcast_to_peers(peer_ids, room_id, bytes).await
    }

    fn peer_sender(&self, peer_id: &PeerId) -> Option<mpsc::Sender<(RoomId, Vec<u8>)>> {
        self.peer_sender(peer_id).ok().flatten()
    }

    fn subscribe_peer_inbound(
        &self,
        peer_id: &PeerId,
    ) -> Option<broadcast::Receiver<(RoomId, Vec<u8>)>> {
        self.subscribe_peer_inbound(peer_id).ok().flatten()
    }
}
```

**Verification checklist:**
- [ ] Impl block added with exactly 4 methods
- [ ] Run: `cargo check --package zznet-router`
- [ ] Paste output showing SUCCESS
- [ ] Run: `cargo test --package zznet-router`
- [ ] Paste output showing all tests pass

---

### Task 1.6: Deprecate Mixed-Plane Methods on PeerManagerActor

**Location:** `src/net/zznet-session/src/actor.rs`

**What to change:**

Find these message types and add deprecation:
```rust
#[deprecated(
    since = "0.2.0",
    note = "Use MessageRouter trait instead. This crosses control/data plane boundary."
)]
#[derive(Message)]
#[rtype(result = "Option<mpsc::Sender<(RoomId, Vec<u8>)>>")]
pub struct GetPeerSender {
    pub peer_id: PeerId,
}

#[deprecated(
    since = "0.2.0",
    note = "Use MessageRouter trait instead. This crosses control/data plane boundary."
)]
#[derive(Message)]
#[rtype(result = "Option<broadcast::Receiver<(RoomId, Vec<u8>)>>")]
pub struct SubscribePeerInbound {
    pub peer_id: PeerId,
}
```

**Verification checklist:**
- [ ] Both message types marked with `#[deprecated]`
- [ ] Deprecation note references `MessageRouter`
- [ ] Run: `cargo check --package zznet-session` (expect deprecation warnings)
- [ ] Paste output showing warnings
- [ ] Run: `cargo test --package zznet-session`
- [ ] Paste output showing tests pass

---

## Phase 2: Three-Actor Isolation (Enforce Network Obliviousness)

**Goal:** Remove all network knowledge from Main actors. Main actors should only depend on `zznet-api` types.

### Task 2.1: Audit Main Actor Dependencies

**What to do:** For each component Main actor, check its `Cargo.toml` and struct fields.

**Components to check:**
- `src/components/zzintent-config/`
- `src/components/zzmem-db/`
- `src/components/zzcollector-state/`

**For each, run:**
```bash
grep -n "zznet-session\|zznet-peer-manager\|zznet-router" src/components/*/Cargo.toml
```

**Verification checklist:**
- [ ] Run grep command above
- [ ] Paste full output
- [ ] Count matches: Expected: 3+ (one per component), Actual: ___
- [ ] Document which components violate the rule

**Anti-patterns to avoid:**
- ❌ Saying "I found violations" without pasting the actual grep output
- ❌ Missing any component directory

---

### Task 2.2: Remove PeerManagerActor from IntentConfigActor

**Location:** `src/components/zzintent-config/src/actor.rs`

**What to change:**

Find the struct definition:
```rust
pub struct IntentConfigActor {
    current_config: IntentConfigData,
    subscribers: HashMap<usize, Recipient<IntentConfigData>>,
    next_id: usize,
    role: IntentConfigRole,

    // ❌ DELETE THESE:
    peer_manager: Option<Addr<PeerManagerActor>>,
    room: Option<zznet_room::room::Room<IntentConfigNetworkMsg>>,
    room_channels: Option<std::sync::Arc<zznet_room::room::RoomChannels>>,

    // ✅ KEEP THIS:
    network_manager: Option<Addr<IntentConfigNetworkManager>>,
}
```

**After deletion:**
```rust
pub struct IntentConfigActor {
    current_config: IntentConfigData,
    subscribers: HashMap<usize, Recipient<IntentConfigData>>,
    next_id: usize,
    role: IntentConfigRole,
    network_manager: Option<Addr<IntentConfigNetworkManager>>,
}
```

**Also delete these methods:**
```rust
// ❌ DELETE:
pub fn set_peer_manager(&mut self, peer_manager: Addr<PeerManagerActor>) { ... }
pub fn set_room(&mut self, room: zznet_room::room::Room<IntentConfigNetworkMsg>) { ... }
```

**Verification checklist:**
- [ ] Three fields removed from struct
- [ ] Two setter methods deleted
- [ ] Grep for remaining `PeerManagerActor` usage: `grep -n "PeerManagerActor" src/components/zzintent-config/src/actor.rs`
- [ ] Paste grep output (should show ZERO matches in actor.rs except imports/tests)
- [ ] Run: `cargo check --package zzintent-config` (expect compile errors)
- [ ] Paste errors (these are expected; you'll fix in next task)

**Anti-patterns to avoid:**
- ❌ Commenting out code instead of deleting it
- ❌ Leaving TODO comments like "// TODO: remove this"

---

### Task 2.3: Fix IntentConfig Compilation Errors

**Location:** Various files in `src/components/zzintent-config/src/`

**What to do:**
1. Find all call sites that used the deleted fields/methods
2. Move the logic to `IntentConfigNetworkManager` instead
3. Have the Main actor send messages to NetworkManager, not directly to network infrastructure

**Verification checklist:**
- [ ] Run: `cargo check --package zzintent-config`
- [ ] Paste output showing SUCCESS (not errors)
- [ ] Run: `cargo test --package zzintent-config`
- [ ] Paste output showing tests pass
- [ ] Grep for any remaining direct network usage: `grep -n "PeerManagerActor\|Room<" src/components/zzintent-config/src/actor.rs`
- [ ] Paste output (should be empty or test-only)

---

### Task 2.4: Remove zznet-session from IntentConfig Dependencies

**Location:** `src/components/zzintent-config/Cargo.toml`

**What to change:**

Delete this line:
```toml
zznet-session = { workspace = true }
```

Or if it has comments, delete the entire block:
```toml
# Phase 3.6-3.8: SessionManager bridge (temporary)
# Still using SessionManager as interface to PeerManager/Router during three-actor migration.
# SessionManager provides: GetPeerRole, GetPeerSender, SubscribePeerInbound
# TODO: Replace with direct PeerManager/Router access when SessionManager is fully deprecated
zznet-session = { workspace = true }
```

**Verification checklist:**
- [ ] Dependency removed from `Cargo.toml`
- [ ] Run: `cargo check --package zzintent-config`
- [ ] Paste output showing SUCCESS
- [ ] Grep to confirm removal: `grep "zznet-session" src/components/zzintent-config/Cargo.toml`
- [ ] Paste output (should be empty)

---

### Task 2.5: Repeat for Other Components

**What to do:** Repeat Tasks 2.2, 2.3, 2.4 for:
- `zzmem-db`
- `zzcollector-state`

**Verification checklist (per component):**
- [ ] Struct fields cleaned
- [ ] Compilation succeeds
- [ ] Tests pass
- [ ] Dependency removed from Cargo.toml
- [ ] Paste proof for each

---

## Phase 3: Local Enforcement (No Regressions)

**Goal:** Add local checks and scripts that prevent future violations. This repository does not use automated CI; developers should run these scripts locally as part of pre-PR validation.

### Task 3.1: Create Dependency Check Script

**Location:** `scripts/check-component-dependencies.sh` (create new file)

**What to create:**
```bash
#!/bin/bash
set -e

# Check that Main-actor components do not depend on network infrastructure crates
MAIN_ACTOR_CRATES=(
    "src/components/zzintent-config"
    "src/components/zzmem-db"
    "src/components/zzcollector-state"
)

FORBIDDEN_DEPS=(
    "zznet-session"
    "zznet-peer-manager"
    "zznet-router"
)

VIOLATIONS=0

for crate_path in "${MAIN_ACTOR_CRATES[@]}"; do
    cargo_toml="$crate_path/Cargo.toml"

    if [ ! -f "$cargo_toml" ]; then
        echo "ERROR: $cargo_toml not found"
        exit 1
    fi

    for dep in "${FORBIDDEN_DEPS[@]}"; do
        if grep -q "^$dep\\s*=" "$cargo_toml" || grep -q "^$dep\\s*{" "$cargo_toml"; then
            echo "VIOLATION: $crate_path depends on forbidden crate: $dep"
            VIOLATIONS=$((VIOLATIONS + 1))
        fi
    done
done

if [ $VIOLATIONS -eq 0 ]; then
    echo "✅ All Main-actor components follow dependency rules"
    exit 0
else
    echo "❌ Found $VIOLATIONS dependency violations"
    echo "Main-actor components must only depend on zznet-api (and component-local crates)"
    exit 1
fi
```

**Verification checklist:**
- [ ] File created with exact path: `scripts/check-component-dependencies.sh`
- [ ] Make executable: `chmod +x scripts/check-component-dependencies.sh`
- [ ] Run: `./scripts/check-component-dependencies.sh`
- [ ] Paste output showing either SUCCESS or listing violations
- [ ] If violations found, go back and fix them before proceeding

---

### Task 3.2: Document Local Enforcement Steps

**Location:** Repository documentation and PR template

**What to add:**
1. Ensure `scripts/check-component-dependencies.sh` is present and executable for local use.
2. Add a short developer checklist or PR template entry that instructs contributors to run local checks prior to opening a PR. Example checklist item:
     - `./scripts/check-component-dependencies.sh` passed
     - `cargo test --workspace` passed (locally)
3. Do NOT add a CI job; the project policy is to run these checks locally.

**Verification checklist:**
- [ ] Local enforcement steps documented in the repo (PR template or docs)
- [ ] Running `./scripts/check-component-dependencies.sh` locally produces no violations
- [ ] Developers run local checks before creating PRs (manual policy enforcement)

---

### Task 3.3: Document the Rules

**Location:** `docs/zznet-solid/ARCHITECTURE_RULES.md` (create new file)

**What to create:**
```markdown
# ZZNet Architecture Rules (Enforced)

## Rule 1: Main Actors are Network-Oblivious

**Applies to:** Component Main actors (e.g., IntentConfigActor, MemDbActor, CollectorStateActor)

**Requirement:** Main actors MUST NOT:
- Depend on `zznet-session`, `zznet-peer-manager`, or `zznet-router`
- Import `PeerManagerActor`, `Router`, or `SessionCoordinator`
- Hold fields of type `Room<...>`, `mpsc::Sender<...>` for network channels, or Actix addresses of network infrastructure

**Allowed:** Main actors MAY:
- Depend on `zznet-api` for types (`PeerId`, `Role`, `SessionError`, traits)
- Hold an address to their own `NetworkManager` actor
- Define component-specific message types

**Enforcement:** Local pre-PR script `scripts/check-component-dependencies.sh` fails if violated; run it before opening a PR.

**Rationale:** Business logic must be isolated from network concerns for testability, clarity, and adherence to the Three-Actor pattern.

---

## Rule 2: Interface Segregation (ISP)

**Applies to:** All network infrastructure crates

**Requirement:** Do not expose fat interfaces that mix control-plane and data-plane concerns.

**Preferred:** Define narrow traits in `zznet-api`:
- `PeerRegistry` (control-plane queries, lifecycle events)
- `MessageRouter` (data-plane sends, broadcasts, channel access)

**Enforcement:** Code review; deprecation of mixed methods.

---

## Rule 3: Data-Plane Scope

**Applies to:** `zznet-router`

**Allowed in Router:**
- Channel registration (peer → sender/receiver)
- Room membership and negotiation (PublishRooms)
- Byte routing to room handlers
- Inbound/outbound broadcast/send operations

**NOT allowed in Router:**
- Role queries or auth checks
- Peer identity or lifecycle state
- Business logic

**Enforcement:** `zznet-router` must not import `Role`, `PeerIdentity`, or component types.

---

## Updating These Rules

Changes to these rules require:
1. Update this document
2. Update enforcement scripts
3. Team discussion and consensus
```

**Verification checklist:**
- [ ] File created at exact path
- [ ] Run: `git add docs/zznet-solid/ARCHITECTURE_RULES.md`
- [ ] Commit with message: "docs: add enforced architecture rules"

---

## Phase 4: SessionCoordinator Simplification

**Goal:** Remove cross-plane orchestration from library code.

### Task 4.1: Identify SessionCoordinator Call Sites

**What to do:**
```bash
grep -rn "SessionCoordinator\|\.coordinator\." src/apps/ src/components/ src/net/zznet-hello/
```

**Verification checklist:**
- [ ] Run grep command
- [ ] Paste full output
- [ ] Count call sites: ___
- [ ] List which files call `add_peer` with coordinator

---

### Task 4.2: Replace Coordinator Calls in Apps

**Location:** Each app's `service.rs` or `main.rs` (wherever coordinator is used)

**What to change:**

Before:
```rust
coordinator.add_peer(peer_state, peer_channels)?;
```

After:
```rust
// Explicitly manage control and data planes
peer_manager.add_peer(peer_state)?;

if let Err(e) = router.register_peer(peer_channels) {
    // Rollback control-plane on failure
    let _ = peer_manager.remove_peer(&peer_id);
    return Err(e);
}

// Notify connected
peer_manager.notify_peer_connected(&peer_id);
```

**Verification checklist (per call site):**
- [ ] Call site identified: file:line
- [ ] Coordinator call replaced with explicit two-step
- [ ] Rollback logic included
- [ ] Run: `cargo check --package <app-name>`
- [ ] Paste output showing SUCCESS

---

### Task 4.3: Deprecate SessionCoordinator

**Location:** `src/net/zznet-session/src/coordinator.rs`

**What to add:**
```rust
#[deprecated(
    since = "0.2.0",
    note = "Orchestration belongs in app composition root, not library code. Use PeerManager and Router directly."
)]
pub struct SessionCoordinator {
    // ... existing fields
}
```

**Verification checklist:**
- [ ] Struct marked deprecated
- [ ] Run: `cargo check --workspace` (expect deprecation warnings)
- [ ] Paste warnings showing coordinator usage is deprecated

---

## Phase 5: Final Quality Gates

**Goal:** Ensure everything still works and is documented.

### Task 5.1: Full Workspace Build

**Verification checklist:**
- [ ] Run: `cargo check --workspace`
- [ ] Paste output showing SUCCESS (all packages)
- [ ] Run: `cargo test --workspace`
- [ ] Paste output showing all tests pass
- [ ] Check for any compilation warnings related to refactor
- [ ] Document warnings if any

---

### Task 5.2: Run Architecture Enforcement

**Verification checklist:**
- [ ] Run: `./scripts/check-component-dependencies.sh`
- [ ] Paste output showing ZERO violations
- [ ] Run: `grep -r "#\[deprecated\]" src/net/zznet-session/`
- [ ] Paste output showing deprecated items

---

### Task 5.3: Update Migration Status

**Location:** `docs/zznet-solid/10_migration_status.md`

**What to add (append new section):**
```markdown
# Phase 9 -> Phase 10: SOLID Enforcement Complete

Date: 2025-10-28

## Changes Applied

1. Interface Segregation (ISP)
   - Created `PeerRegistry` and `MessageRouter` traits in `zznet-api`
   - Implemented traits for `PeerManager` and `Router`
   - Deprecated mixed-plane methods on `PeerManagerActor`

2. Three-Actor Isolation
   - Removed `PeerManagerActor` and `Room` fields from Main actors
   - Removed `zznet-session` dependency from component crates
   - All network knowledge now lives in NetworkManager actors

3. Local Enforcement
    - Added `scripts/check-component-dependencies.sh`
    - Local pre-PR script fails on architecture violations
   - Documented rules in `ARCHITECTURE_RULES.md`

4. SessionCoordinator
   - Deprecated in favor of explicit two-step orchestration
   - Rollback logic moved to app composition root

## Verification

All tasks completed per checklist in `14_Action_Plan_No_Shortcuts.md`.
Architecture now enforces SOLID principles in code, not just documentation.
```

**Verification checklist:**
- [ ] Section appended to migration status doc
- [ ] Date and summary accurate
- [ ] Commit with message: "docs: mark Phase 10 complete - SOLID enforcement"

---

## Completion Criteria

The refactor is DONE when ALL of the following are true:

### Code Checks
- [ ] `cargo check --workspace` passes with zero errors
- [ ] `cargo test --workspace` passes with zero failures
- [ ] `./scripts/check-component-dependencies.sh` passes with zero violations

### Architecture Checks
- [ ] `PeerRegistry` and `MessageRouter` traits exist in `zznet-api`
- [ ] Both traits are implemented by their respective crates
- [ ] No Main actor has `PeerManagerActor`, `Router`, or `Room<...>` fields
- [ ] No Main actor crate depends on `zznet-session`, `zznet-peer-manager`, or `zznet-router`

### Documentation Checks
- [ ] `ARCHITECTURE_RULES.md` exists and is accurate
- [ ] `10_migration_status.md` updated with Phase 10 completion
- [ ] All deprecation notices include clear migration paths

### Evidence Collected
- [ ] Every task has pasted terminal output proving completion
- [ ] Every code change has before/after snippets
- [ ] Every grep has full output pasted (not summarized)

---

## Anti-Pattern Hall of Shame

These are examples of what NOT to do:

### ❌ Example 1: Lazy Verification
```
Agent: "I added the PeerRegistry trait and it compiles fine."
```
**Problem:** No proof. No file path. No command output.

**Correct:**
```
Agent: "I added PeerRegistry trait to src/net/zznet-api/src/traits.rs (lines 1-25).

I ran:
$ cargo check --package zznet-api
    Finished dev [unoptimized + debuginfo] target(s) in 0.43s

SUCCESS: zznet-api builds with trait."
```

### ❌ Example 2: Incomplete Change
```
Agent: "I removed PeerManagerActor from IntentConfigActor. It doesn't compile now but that's expected."
```
**Problem:** Task is not done. Leaving broken code is not acceptable.

**Correct:**
```
Agent: "I removed PeerManagerActor from IntentConfigActor and fixed all call sites by moving them to NetworkManager.

I ran:
$ cargo check --package zzintent-config
    Finished dev [unoptimized + debuginfo] target(s) in 1.23s

SUCCESS: Component builds and tests pass."
```

### ❌ Example 3: Skipping Verification
```
Agent: "I updated all three components. Moving to next phase."
```
**Problem:** No evidence. Which components? What changed? Did tests pass?

**Correct:**
```
Agent: "I updated three components:

1. zzintent-config:
   - Removed PeerManagerActor field (actor.rs:25)
   - Removed zznet-session dep (Cargo.toml:36)
   - Tests: [paste test output]

2. zzmem-db:
   - Removed PeerManagerActor field (actor.rs:18)
   - Removed zznet-session dep (Cargo.toml:28)
   - Tests: [paste test output]

3. zzcollector-state:
   - Removed PeerManagerActor field (actor.rs:22)
   - Removed zznet-session dep (Cargo.toml:31)
   - Tests: [paste test output]

Verification:
$ ./scripts/check-component-dependencies.sh
✅ All Main-actor components follow dependency rules
"
```

---

## Final Note to AI Agents

If you're executing this plan, remember:
- **Every task is a contract.** Checkboxes are not suggestions—they're requirements.
- **Proof is mandatory.** If you didn't paste output, you didn't do the task.
- **No shortcuts.** If a task seems too detailed, that's intentional—it prevents mistakes.
- **When stuck, ask.** Better to ask for clarification than to guess and break things.

The human reviewing your work will check:
1. Did you complete every checkbox?
2. Did you paste actual output?
3. Did tests pass?
4. Is the code better than before?

If any answer is "no," the work will be rejected and you'll start over.

**Good luck. Show your work.**
