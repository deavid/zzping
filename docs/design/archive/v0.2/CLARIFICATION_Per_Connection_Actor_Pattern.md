# Clarification: Per-Connection Child Actor Pattern

NOTE: Deprecated documentation.


**Date**: 2025-10-06
**Purpose**: Document the per-connection child actor pattern for component-to-network integration

---

## The Core Pattern

**Components spawn child actors, one per connection.**

```
Component Architecture (Database Side):

┌─────────────────────────────────────────────┐
│         CState Component (Parent Actor)     │
│                                             │
│  - Manages overall state                   │
│  - Spawns child actors per connection      │
│  - Maintains list of children              │
│  - Does NOT send/receive network directly  │
│                                             │
│  ┌──────────────┐  ┌──────────────┐       │
│  │  Child for   │  │  Child for   │       │
│  │ Collector-01 │  │ Collector-02 │  ...  │
│  └──────────────┘  └──────────────┘       │
└─────────────────────────────────────────────┘
         │                  │
         │ SessionHandle    │ SessionHandle
         ↓                  ↓
    Connection 1       Connection 2
```

**Key Points:**
- ✅ Parent component spawns **one child actor per connection**
- ✅ Child actor is the **only** entity that sends/receives on that connection
- ✅ Parent maintains a **list of child actors** (implicitly a connection list)
- ✅ Parent communicates with children via **intra-process messages**

---

## Connection Lifecycle and State Management

### When Connection Established

```rust
// Pseudo-code: CState parent actor receives SessionEvent
impl Handler<SessionEvent> for CState {
    fn handle(&mut self, event: SessionEvent) {
        match event {
            SessionEvent::Active { peer_id, session_handle, role, ... } => {
                // Spawn child actor for this connection
                let child = CStateConnectionActor::new(
                    peer_id.clone(),
                    session_handle,
                    role,
                ).start();

                // Track the child
                self.connections.insert(peer_id, child);

                // Child now handles all send/receive for this connection
            }
        }
    }
}
```

**What Happens:**
1. SessionManager notifies component of new connection via `SessionEvent::Active`
2. Component spawns **child actor** for that connection
3. Child receives `SessionHandle` - it's the only one that can use it
4. Parent tracks child in internal list/map

### When Connection Dies

```rust
impl Handler<SessionEvent> for CState {
    fn handle(&mut self, event: SessionEvent) {
        match event {
            SessionEvent::Inactive { peer_id } => {
                // Remove and stop child actor
                if let Some(child) = self.connections.remove(&peer_id) {
                    // Child actor is stopped (dropped)
                    // All its state is destroyed
                }

                // No cleanup needed - child took care of itself
            }
        }
    }
}
```

**What Happens:**
1. Connection dies (network failure, remote close, etc.)
2. SessionManager sends `SessionEvent::Inactive` to component
3. Component **removes child actor** from its list
4. Child actor is **destroyed** - all its state is gone
5. Any in-flight messages are **lost** (acceptable - TCP guarantees were broken)

**Critical Principle:** Connection dies = Child actor dies = State is cleared.

### When Reconnection Happens

**Important:** Reconnection is treated as a **new connection**, even if it's the same remote process.

```rust
// Same collector reconnects
// peer_id might be the same ("collector-01")
// But this is a NEW SessionHandle, NEW child actor

SessionEvent::Active {
    peer_id: "collector-01",  // Same hostname
    session_handle: new_handle,  // Different handle!
    ...
}

// Component spawns a FRESH child actor
// NO state is carried over from old connection
// State must be renegotiated via messages
```

**Why?**
- Old connection had TCP buffers, pending messages, etc.
- That state is lost - no point pretending otherwise
- Explicit renegotiation is more reliable than trying to "resume"

---

## Fire-and-Forget Guarantees

### What "Fire-and-Forget" Means

**Guarantees:**
1. ✅ **Ordering within a room**: Messages sent to same room arrive in order
2. ✅ **TCP delivery**: If connection exists, message is eventually delivered
3. ✅ **No ACKs**: Sender doesn't know if message was received
4. ✅ **No retries**: Component doesn't retry failed sends

**Non-Guarantees:**
1. ❌ **No delivery confirmation**: Sender never knows if message arrived
2. ❌ **Connection loss = message loss**: Disconnect mid-send loses that message
3. ❌ **No at-least-once**: A disconnect can lose messages (at-most-once semantics)
4. ❌ **No deduplication**: Reconnect might cause re-sends if app logic retries

### Message Loss on Disconnect

```rust
// Child actor sends message
child.session_handle.send_to_room(room_id, message);
// Message goes into TCP send buffer

// Connection dies mid-transmission
// Result: Message is LOST

// Child actor is destroyed
// No retry, no buffering, no recovery
// Parent must handle this via application logic
```

**Acceptable because:**
- TCP provides ordering and delivery **while connected**
- Application layer handles **reconnection state sync**
- Components designed to survive message loss (1+ hour partition tolerance)

### State Renegotiation After Reconnect

```rust
// Example: CState after reconnect

// Old connection child actor (destroyed):
// - Had knowledge of collector's role: PRIMARY
// - Had pending status reports
// All lost.

// New connection child actor (fresh):
// - Knows nothing about collector's previous state
// - Database CState must send: "What's your current role?"
// - Collector responds: "I'm PRIMARY with these targets"
// - State is rebuilt from scratch
```

**Pattern:** Every reconnect triggers full state sync negotiation.

---

## Component Message Passing

### Parent → Child Communication

**Parent cannot send directly to network.** Parent must ask child to send.

```rust
// Parent actor (CState)
impl CState {
    fn broadcast_config_update(&mut self, config: Config) {
        // Parent wants to send to all collectors
        // Must go through child actors

        for (peer_id, child) in &self.connections {
            // Send message to child actor
            child.do_send(SendToCollector {
                room: "intent-config",
                message: ConfigUpdate { config: config.clone() },
            });
        }
    }
}

// Child actor (CStateConnectionActor)
impl Handler<SendToCollector> for CStateConnectionActor {
    fn handle(&mut self, msg: SendToCollector) {
        // Child has the SessionHandle
        self.session_handle.send_to_room(msg.room, msg.message);
    }
}
```

**Why this indirection?**
- SessionHandle is owned by child (one per connection)
- Parent doesn't have access to SessionHandle
- Forces explicit per-connection logic

### Child → Parent Communication

**Child receives network messages, forwards to parent for business logic.**

```rust
// Child actor receives message from network
impl Handler<NetworkMessage> for CStateConnectionActor {
    fn handle(&mut self, msg: NetworkMessage) {
        match msg {
            NetworkMessage::RoleStatusReport { role, targets } => {
                // Forward to parent for processing
                self.parent.do_send(CollectorStatusUpdate {
                    peer_id: self.peer_id.clone(),
                    role,
                    targets,
                });
            }
        }
    }
}

// Parent processes business logic
impl Handler<CollectorStatusUpdate> for CState {
    fn handle(&mut self, msg: CollectorStatusUpdate) {
        // Update internal state
        self.collector_states.insert(msg.peer_id, msg.role);

        // Maybe trigger handoff logic, etc.
    }
}
```

**Why this pattern?**
- Child handles connection-specific logic (send/receive)
- Parent handles business logic (state management, decisions)
- Clean separation of concerns

---

## Simplified Components (No Per-Connection State)

**Not all components need per-connection children.**

### Example: IntentConfig on Collector (Client Side)

```rust
// Collector has ONE connection to database
// IntentConfig doesn't need per-connection children

impl IntentConfigComponent {
    fn handle_network_message(&mut self, msg: IntentConfigMessage) {
        match msg {
            IntentConfigMessage::ConfigUpdate { targets, ping_rate } => {
                // Update local state
                self.current_config = targets;

                // Broadcast to Pinger (intra-process)
                self.pinger.do_send(UpdateConfig { targets, ping_rate });
            }
        }
    }
}
```

**Why simpler?**
- Collector is a **client** (one connection)
- Component doesn't manage multiple peers
- No need for per-connection state tracking

### Example: IntentConfig on Database (Server Side)

```rust
// Database has MANY connections from collectors
// IntentConfig needs to track which collectors to notify

impl IntentConfigComponent {
    // Parent actor maintains connection list
    connections: HashMap<PeerId, Addr<IntentConfigConnectionActor>>,

    fn handle_config_change(&mut self, new_config: Config) {
        // Persist config
        self.persist(new_config.clone());

        // Broadcast to all connected collectors
        for (peer_id, child) in &self.connections {
            child.do_send(SendConfigUpdate {
                config: new_config.clone(),
            });
        }
    }
}
```

**Why more complex?**
- Database is a **server** (many connections)
- Needs to track which collectors are online
- Needs per-connection children to send to each

---

## Summary: The Pattern

### Per-Connection Child Actor Pattern

1. ✅ **Component spawns child actor per connection**
   - Child receives `SessionHandle`
   - Child is sole sender/receiver for that connection

2. ✅ **Parent maintains list of children**
   - `HashMap<PeerId, Addr<ChildActor>>`
   - List doubles as "who's connected" tracker

3. ✅ **Connection dies → Child destroyed**
   - All state for that connection is lost
   - No cleanup needed (child handles it)

4. ✅ **Reconnection = fresh start**
   - New child actor spawned
   - State renegotiated from scratch

5. ✅ **Parent ↔ Child via messages**
   - Parent cannot send to network directly
   - Parent sends messages to child
   - Child forwards to network via SessionHandle

6. ✅ **Fire-and-forget semantics**
   - Ordering guaranteed within room (while connected)
   - No ACKs, no retries
   - Disconnect loses in-flight messages (acceptable)

### When to Use This Pattern

**Use per-connection children when:**
- Component is server-side (many connections)
- Component needs per-connection state
- Example: CState, MemDB on database

**Don't use when:**
- Component is client-side (one connection)
- Component is stateless
- Example: IntentConfig on collector

---

## Open Questions (Deferred to Implementation)

1. **Child actor lifecycle details**: How is child notified to stop? Actix stop message? Drop on remove?
2. **State sync protocol**: What messages are used to renegotiate state after reconnect?
3. **Error handling**: What if child actor panics? Does parent restart it?
4. **Testing**: How to mock per-connection children for unit tests?

These are **implementation details** that will emerge during development.
