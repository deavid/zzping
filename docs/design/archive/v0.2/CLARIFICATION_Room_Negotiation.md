# Clarification: Room Negotiation and Partial Connections

NOTE: Deprecated documentation.


**Date**: 2025-10-06
**Purpose**: Clarify what happens when room negotiation has partial intersection

---

## The Question

**What happens when room negotiation produces a partial match?**

Example scenario:
```
Collector offers: ["mem-db", "c-state", "intent-config"]
Database offers: ["mem-db", "c-state"]

Intersection: ["mem-db", "c-state"]
```

---

## The Answer: Partial Connection is Success

**Result: Connection SUCCEEDS with available rooms.**

- ✅ Connection is established
- ✅ Rooms with intersection are activated: `["mem-db", "c-state"]`
- ✅ Room without intersection is NOT activated: `"intent-config"`

**This is not an error.** It's the expected behavior.

---

## Component Perspective: No Connection

**From IntentConfig component's perspective:**

```rust
// IntentConfig component on database
impl IntentConfig {
    connections: HashMap<PeerId, Addr<IntentConfigChildActor>>,

    // When collector connects...
    fn handle_session_event(&mut self, event: SessionEvent) {
        // IntentConfig receives SessionEvent::Active
        // But "intent-config" room was NOT in intersection
        // So IntentConfig does NOT spawn a child actor
        //
        // Result: self.connections remains empty for this peer
    }
}
```

**IntentConfig's view:**
- "I have zero connections"
- "That collector doesn't exist from my perspective"
- "Not broken, just not connected"

**This is fine.** IntentConfig works with 0 to N connections by design.

---

## How Components Know About Rooms

**Components discover rooms via child actor spawning:**

```rust
// When connection negotiates rooms successfully
SessionEvent::Active {
    peer_id: "collector-01",
    rooms: vec!["mem-db", "c-state"],  // Only successful rooms
    ...
}

// Each component receives this event
// Component checks: "Is my room in the list?"

// MemDB component
if rooms.contains("mem-db") {
    // Spawn child actor for this connection
    let child = MemDBChildActor::new(...).start();
    self.connections.insert(peer_id, child);
}

// CState component
if rooms.contains("c-state") {
    // Spawn child actor for this connection
    let child = CStateChildActor::new(...).start();
    self.connections.insert(peer_id, child);
}

// IntentConfig component
if rooms.contains("intent-config") {
    // Room NOT in list
    // Don't spawn child actor
    // This peer doesn't exist for IntentConfig
}
```

**Simple rule:** Room in `SessionEvent` → spawn child. Room not in event → no child.

---

## Required Rooms vs Optional Rooms

**Problem:** What if a component NEEDS a room but doesn't get it?

**Example:**
```
Collector MUST have "intent-config" room to receive ping targets
Database forgot to offer "intent-config"
Collector connects with only ["mem-db", "c-state"]
Result: Collector can't receive config → can't ping anything
```

**Solution:** Optional `require_all_rooms` flag on connection.

### Option 1: Require All Rooms (Collector/Client Behavior)

```rust
ClientBuilder::new()
    .connect_to("database:9001")
    .offer_rooms(vec!["mem-db", "c-state", "intent-config"])
    .require_all_rooms(true)  // ← Connection fails if any room missing
    .connect()
    .await?;
```

**Behavior:**
- If intersection != offered rooms → connection rejected
- Example: Offered 3, got 2 → connection fails
- Collector exits or retries

**Use case:** Collectors and clients that need all their rooms to function.

### Option 2: Accept Partial Match (Database Behavior)

```rust
ServerBuilder::new()
    .bind("0.0.0.0:9001")
    .offer_rooms(vec!["mem-db", "c-state", "intent-config"])
    .require_all_rooms(false)  // ← Accept partial match (default)
    .start()
    .await?;
```

**Behavior:**
- Accept any non-empty intersection
- Example: Offered 3, got 1 → connection succeeds
- Database is flexible about what clients support

**Use case:** Database accepts diverse clients (GUI with only monitoring, CLI with only admin, collector with all rooms).

---

## Default Behavior

**Default: `require_all_rooms = false`** (accept partial match)

**Reasoning:**
- More flexible for evolving systems
- Old clients can connect to new server (with new rooms)
- New clients can connect to old server (missing new rooms)

**Override for critical components:**
```rust
// Collector NEEDS all its rooms
ClientBuilder::new()
    .require_all_rooms(true)
    .connect()
```

---

## Component Startup Impact

**Question:** Does partial room negotiation affect component startup?

**Answer:** No, components don't start/stop based on connections.

**Component lifecycle:**
```rust
// In main.rs - components start regardless of network state
let memdb = MemDBComponent::new(config).start();
let cstate = CStateComponent::new(config).start();
let intent_config = IntentConfigComponent::new(config).start();

// All components are running
// They work with 0 to N connections

// Network connects later (or never)
// Components adapt to available connections
```

**Why this works:**
- Components designed for 0 to N connections
- No connection? Component works with local state
- Connection arrives? Component spawns child actor
- Connection offers partial rooms? Some components get children, others don't

---

## Error Scenarios

### Scenario 1: Empty Intersection (Total Failure)

```
Collector offers: ["room-a", "room-b"]
Database offers: ["room-x", "room-y"]
Intersection: [] (EMPTY)
```

**Result: Connection FAILS.**

**Reasoning:**
- No shared rooms = no communication possible
- Connection would be useless
- Fail fast at handshake

### Scenario 2: Partial Intersection (Partial Success)

```
Collector offers: ["mem-db", "c-state", "intent-config"]
Database offers: ["mem-db", "c-state"]
Intersection: ["mem-db", "c-state"] (PARTIAL)
```

**Result: Connection SUCCEEDS** (unless `require_all_rooms=true`).

**Reasoning:**
- Shared rooms exist = communication possible
- Some components can work
- Let application decide if this is sufficient

### Scenario 3: Perfect Match

```
Collector offers: ["mem-db", "c-state"]
Database offers: ["mem-db", "c-state", "intent-config", "health"]
Intersection: ["mem-db", "c-state"] (collector's full set)
```

**Result: Connection SUCCEEDS.**

**Reasoning:**
- Collector got all its rooms (from its perspective, perfect match)
- Database has extra rooms (not used, but that's fine)

---

## Monitoring and Debugging

**How to know if room negotiation succeeded?**

### Logging

```rust
// After HELLO completes
log::info!(
    "Connection to {} established with rooms: {:?}",
    peer_id,
    negotiated_rooms
);

// Per-component
log::debug!(
    "IntentConfig: No connection to {} (room not negotiated)",
    peer_id
);
```

### Metrics (Future)

```rust
// Metric: successful_rooms_per_connection
metrics.gauge("rooms_negotiated", negotiated_rooms.len());

// Metric: component_connections
metrics.gauge("intent_config_connections", self.connections.len());
```

### Testing

```rust
#[test]
fn test_partial_room_negotiation() {
    let collector_rooms = vec!["mem-db", "c-state", "intent-config"];
    let database_rooms = vec!["mem-db", "c-state"];

    let intersection = negotiate_rooms(&collector_rooms, &database_rooms);

    assert_eq!(intersection, vec!["mem-db", "c-state"]);
    // Connection succeeds (non-empty intersection)
}
```

---

## Summary

### Room Negotiation Rules

1. ✅ **Partial match = success** (by default)
2. ✅ **Empty intersection = failure**
3. ✅ **Components unaware of rooms they don't get**
4. ✅ **Optional `require_all_rooms` flag for critical components**
5. ✅ **Components work with 0 to N connections** (no startup impact)

### Component Behavior

- **Component spawns child** when its room is in negotiated list
- **Component has no child** when its room is NOT in list
- **Component doesn't know about** connections it doesn't handle
- **Not an error** - just "no connection for this component"

### Configuration Pattern

```rust
// Database (flexible, accepts partial)
ServerBuilder::new()
    .offer_rooms(vec!["mem-db", "c-state", "intent-config"])
    .require_all_rooms(false)  // Default
    .start()

// Collector (strict, needs all rooms)
ClientBuilder::new()
    .offer_rooms(vec!["mem-db", "c-state", "intent-config"])
    .require_all_rooms(true)  // Fails if any missing
    .connect()
```

---

## Open Questions (Deferred)

1. **Room version negotiation**: What if "mem-db" room has v1 vs v2 protocol?
2. **Dynamic room addition**: Can new rooms be added after connection? (Probably not - reconnect required)
3. **Room deprecation**: How to sunset old rooms without breaking old clients?

These are **forward compatibility concerns** deferred to future protocol versions.
