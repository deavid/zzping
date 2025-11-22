# Clarification: Cross-Room Message Ordering (Non-Issue)

NOTE: Deprecated documentation.


**Date**: 2025-10-06
**Purpose**: Clarify that cross-room message ordering is intentionally undefined

---

## The Question

**What about message ordering across different rooms?**

Example scenario:
```
Collector receives new config via "intent-config" room
Collector immediately sends ping results via "mem-db" room

Database perspective:
- Message A arrives on "intent-config": Add target 192.168.1.1
- Message B arrives on "mem-db": Ping result for 192.168.1.1

What if B arrives before A?
```

---

## The Answer: Non-Issue by Design

**Components must be self-sufficient and not rely on cross-room ordering.**

This is **not a gap** - it's an **architectural principle**.

---

## Why This Is Not A Problem

### Principle: Component Independence

**Each component is responsible for handling its own consistency.**

```rust
// Database MemDB component receives ping for unknown target
impl MemDB {
    fn handle_ping_result(&mut self, result: PingResult) {
        // Component doesn't care if target is "known" by IntentConfig
        // MemDB just stores the data

        self.store_ping(result);
        // No dependency on IntentConfig state
    }
}
```

**Key insight:** MemDB doesn't need to know what targets IntentConfig says are "active". It just stores whatever pings arrive.

### Principle: No Cross-Component State Dependencies

**Components should NOT assume:**
- "IntentConfig has already told everyone about this target"
- "CState has already informed everyone of role change"
- "Component X processed message before Component Y"

**Components SHOULD:**
- Handle messages independently
- Maintain their own consistent state
- Be robust to message arrival in any order

---

## Actual Example: The "Unknown Target" Scenario

### Scenario

```
1. Admin adds target 192.168.1.1 via GUI
2. IntentConfig broadcasts to collector (via "intent-config" room)
3. Collector Pinger immediately starts pinging 192.168.1.1
4. Ping results flow to MemDB (via "mem-db" room)

Race condition:
- Message on "mem-db" room might arrive at database first
- Message on "intent-config" room arrives second
```

### Why This Is Fine

**Database MemDB perspective:**
```rust
// Ping result arrives first (before config update)
fn handle_ping_result(&mut self, result: PingResult) {
    // Store it regardless of whether we "know" about target
    self.store(result);

    // No error, no rejection, just store
    // IntentConfig state is irrelevant to MemDB
}
```

**Database IntentConfig perspective:**
```rust
// Config update arrives second
fn handle_config_update(&mut self, config: Config) {
    // Update our config
    self.current_config = config;

    // MemDB already has some data? That's fine!
    // We're not responsible for MemDB's data
}
```

**Result:** Both components have consistent state. No coordination needed.

---

## Implementation Reality

### Typical Behavior (But Not Guaranteed)

**In practice, messages often arrive in order because:**
- Both rooms share the same TCP connection
- TCP preserves byte-stream ordering
- Serialization happens sequentially

**Example:**
```rust
// Collector sends two messages
intent_config.send_update(new_config);  // Serialized first
memdb.send_batch(ping_data);            // Serialized second

// Database receives:
// 1. Bytes for intent-config message
// 2. Bytes for mem-db message
// Likely arrive in this order (same TCP stream)
```

### Why We Don't Rely On It

**Reasons not to depend on cross-room ordering:**

1. **Implementation flexibility**: Future SessionManager might multiplex differently
2. **Concurrency**: Messages might be serialized in parallel threads
3. **Buffering**: Rooms might have different buffer sizes/policies
4. **Clarity**: Explicit dependencies are better than implicit assumptions

**Design principle:** If ordering matters, use the same room or explicit coordination.

---

## When Ordering DOES Matter

### Within Same Room: Ordering Guaranteed

```rust
// CState sends two messages on "c-state" room
cstate.send(RoleUpdate { role: PRIMARY });
cstate.send(StatusReport { targets: [...] });

// Guaranteed to arrive in order at database
// Database CState sees PRIMARY before StatusReport
```

**This is guaranteed** - same room = same message queue = ordered delivery.

### Across Rooms: No Guarantee

```rust
// Collector sends on different rooms
intent_config.send(ConfigAck { ... });  // "intent-config" room
cstate.send(RoleUpdate { ... });         // "c-state" room

// Database might receive:
// - RoleUpdate first, then ConfigAck
// - ConfigAck first, then RoleUpdate
// - Simultaneously (if multi-threaded)
```

**This is undefined** - different rooms = no ordering guarantee.

---

## Design Pattern: Event Sourcing for Coordination

**If components need coordination, use explicit messages within same room.**

### Anti-Pattern (Cross-Room Dependency)

```rust
// WRONG: Assuming IntentConfig has updated before sending pings
fn start_pinging(&mut self) {
    // Assumes database has already received config update
    self.memdb.send(PingBatch { ... });  // Different room!
}
```

### Correct Pattern (Self-Contained State)

```rust
// RIGHT: Include necessary context in the message
fn send_ping_batch(&mut self, batch: PingBatch) {
    self.memdb.send(PingBatch {
        data: batch.data,
        config_version: self.current_config.version,  // ← Explicit context
    });
}
```

Now MemDB can validate or store config version without depending on IntentConfig.

---

## FAQ

**Q: What if I NEED cross-component ordering?**

**A:** Redesign so you don't. Options:
1. **Embed context**: Include necessary info in each message
2. **Use same room**: If two messages must be ordered, use same room
3. **Explicit coordination**: Use request-response pattern within same room
4. **Rethink split**: Maybe those two concerns should be in same component

**Q: Can TCP reorder messages across rooms?**

**A:** No, TCP preserves byte order. But SessionManager might demultiplex concurrently, and components process messages in parallel. Don't rely on TCP ordering across rooms.

**Q: What about timestamps for ordering?**

**A:** Timestamps don't solve the problem - they just expose it. If you need to compare timestamps across components, you're creating a distributed systems problem. Avoid it.

---

## Summary

### The Principle

**Components must be self-sufficient.**
- No cross-room ordering guarantees
- No cross-component state dependencies
- Each component handles its own consistency

### The Guarantee

**Within same room:**
- ✅ Messages arrive in order
- ✅ FIFO queue semantics
- ✅ Reliable sequencing

**Across different rooms:**
- ❌ No ordering guarantee
- ❌ May arrive in any order
- ❌ May be processed concurrently

### The Practice

**Most implementations will deliver cross-room messages in order** (same TCP connection, sequential serialization). But **don't rely on it** - it's an implementation detail, not a guarantee.

### The Design

**If your component breaks when messages arrive out of order, redesign it.**

This is not a limitation - it's a feature. It forces components to be robust and independent.
