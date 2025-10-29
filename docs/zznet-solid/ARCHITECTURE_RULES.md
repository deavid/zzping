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

**Enforcement:** CI script `scripts/check-component-dependencies.sh` fails if violated.

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