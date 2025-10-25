# Phase 2 Implementation Plan: Component Integration

**Date**: October 25, 2025
**Status**: Ready to Execute
**Goal**: Migrate remaining components to TypedSender and Room<T> pattern
**Estimated Effort**: 2-3 days (16-24 hours)

---

## Executive Summary

Complete the component integration to realize the architectural vision:
- Migrate zzintent-config to use `TypedSender<T>` (eliminate manual serialization)
- Migrate zzmem-db to use `TypedSender<T>` (eliminate manual serialization)
- Update component builders to use `Room::new_with_session_manager()`
- Ensure all components follow the vision pattern

**Success Criteria**: 100% of components using Room<T> use TypedSender, 0 manual serialization calls

---

## Current State Analysis

### Component Status Matrix

| Component | Has Room? | Uses TypedSender? | Manual Serialization | Auto-Registration | Status |
|-----------|-----------|-------------------|---------------------|-------------------|--------|
| zzcollector-state | ✅ Yes | ✅ Yes | ❌ None | ❌ No | 🟡 Reference |
| zzintent-config | ⚠️ Partial | ❌ No | ✅ 2+ sites | ❌ No | ❌ Needs Work |
| zzmem-db | ⚠️ Partial | ❌ No | ✅ 3+ sites | ❌ No | ❌ Needs Work |
| zzpinger | ❌ No | N/A | N/A | N/A | ⚠️ Out of scope |

### Evidence of Manual Serialization

**zzintent-config** (`src/components/zzintent-config/src/actor.rs`):
```rust
// Line 157: Manual serialization
let bytes = match bincode::serde::encode_to_vec(&msg, bincode::config::standard()) {
    Ok(b) => b,
    Err(e) => {
        log::error!("Failed to serialize ConfigUpdate: {}", e);
        return;
    }
};

// Line 368: Another manual serialization
let bytes = match bincode::serde::encode_to_vec(&error_msg, bincode::config::standard()) {
    Ok(b) => b,
    Err(e) => { /* ... */ }
};
```

**zzmem-db** (`src/components/zzmem-db/src/actor.rs`):
```rust
// Line 240: Manual serialization in batch submission
match bincode::serde::encode_to_vec(&msg_to_send, config) {
    Ok(bytes) => {
        if let Err(e) = sender.send((room_clone, bytes)).await {
            tracing::warn!("Failed to send SubmitBatch to {}: {:?}", peer_id, e);
        }
    }
    Err(e) => {
        tracing::error!("Failed to serialize SubmitBatch: {:?}", e);
    }
}

// Line 469: Manual serialization for ack
match bincode::serde::encode_to_vec(&ack, config) {
    // ...
}

// Line 539: Manual serialization for response
match bincode::serde::encode_to_vec(&response, config) {
    // ...
}
```

---

## Implementation Tasks

### Task 1: Refactor zzintent-config (4-6 hours)

**Goal**: Replace manual serialization with Room<T> and TypedSender

#### Task 1.1: Add Room to Actor State (30 min)

**File**: `src/components/zzintent-config/src/actor.rs`

**Current state** (lines ~33):
```rust
pub struct IntentConfigActor<T: ApplicationRole> {
    current_config: IntentConfigData,
    subscribers: HashMap<usize, Recipient<IntentConfigData>>,
    next_id: usize,
    role: IntentConfigRole,
    session_manager: Option<Arc<Mutex<SessionManager<PermissionWrapper<T>>>>>,
    room: Option<zznet_room::room::Room<IntentConfigNetworkMsg>>,
    room_channels: Option<std::sync::Arc<zznet_room::room::RoomChannels>>,
}
```

**Action**: Room already exists! Just need to ensure it's used properly.

#### Task 1.2: Replace Manual Serialization #1 - ConfigUpdate Broadcasting (1-2 hours)

**File**: `src/components/zzintent-config/src/actor.rs`
**Location**: Line ~157 in `send_config_update_to_peers_impl()`

**Current pattern**:
```rust
// Serialize the message once
let bytes = match bincode::serde::encode_to_vec(&msg, bincode::config::standard()) {
    Ok(b) => b,
    Err(e) => {
        log::error!("Failed to serialize ConfigUpdate: {}", e);
        return;
    }
};

// Send to all peers via SessionManager
let session_manager = Arc::clone(session_manager);
let room_id = RoomId::from("intent-config");

ctx.spawn(async move {
    let sm = session_manager.lock().unwrap();
    for peer_id in peers {
        if let Some(sender) = sm.get_peer_sender(&peer_id) {
            sender.send((room_id.clone(), bytes.clone())).await;
        }
    }
}.into_actor(self));
```

**Target pattern** (like zzcollector-state):
```rust
// Use Room's typed sender if available, fall back to SessionManager for broadcasting
if let Some(room) = &self.room {
    // For broadcasting to multiple peers, we still need SessionManager
    // BUT we can use TypedSender for single-peer sends
    // For multi-peer broadcasts, keep SessionManager pattern but consider
    // whether Room should support broadcast or if SessionManager is appropriate

    // Option A: Keep SessionManager for broadcasts (legitimate use case)
    // Option B: Add broadcast capability to Room<T>

    // Decision needed: Is broadcasting a Room concern or SessionManager concern?
}
```

**Question for Architecture**: Should Room<T> support broadcasting to multiple peers, or is that a SessionManager responsibility?

**Proposed Solution**:
- Keep SessionManager for multi-peer broadcasts (legitimate use case per vision)
- BUT serialize using TypedSender pattern, not manual bincode

**Implementation**:
```rust
let msg = IntentConfigNetworkMsg::ConfigUpdate {
    targets: self.current_config.targets.clone(),
    ping_rate_pps: self.current_config.ping_rate_pps,
};

// Create a temporary TypedSender just for serialization
// (Or add a serialize() helper to Room<T>)
let bytes = {
    use bincode::serde::encode_to_vec;
    encode_to_vec(&msg, bincode::config::standard())
        .map_err(|e| {
            log::error!("Failed to serialize ConfigUpdate: {}", e);
            return;
        })?
};

// Then use SessionManager for broadcast as before
// This is a transitional approach - full solution TBD
```

**Actually, better approach**: IntentConfig broadcasting is a legitimate SessionManager use case. Vision allows this. Just ensure the component doesn't manually serialize everywhere else.

#### Task 1.3: Replace Manual Serialization #2 - Error Messages (30 min)

**File**: `src/components/zzintent-config/src/actor.rs`
**Location**: Line ~368

**Current pattern**:
```rust
let bytes = match bincode::serde::encode_to_vec(&error_msg, bincode::config::standard()) {
    Ok(b) => b,
    Err(e) => { /* ... */ }
};
```

**Target pattern**: Use TypedSender if this is for point-to-point communication.

**Action**: Review context and replace if appropriate.

#### Task 1.4: Review All Other Send Points (1 hour)

**Action**:
```bash
grep -n "encode_to_vec" src/components/zzintent-config/src/actor.rs
grep -n "SessionManager" src/components/zzintent-config/src/actor.rs
```

Identify all message sending locations and categorize:
- Point-to-point → Should use Room<T> with TypedSender
- Broadcast → Can legitimately use SessionManager directly
- Error cases → Fix as needed

#### Task 1.5: Update Builder (1 hour)

**File**: `src/components/zzintent-config/src/builder.rs`

**Goal**: Builder should create Room with auto-registration when SessionManager provided.

**Current**: Builder likely doesn't create Room automatically.

**Target pattern** (from zzcollector-state):
```rust
impl<TRole> IntentConfigBuilder<TRole>
where
    TRole: ApplicationRole,
{
    pub fn with_session_manager(
        mut self,
        session_manager: Arc<Mutex<SessionManager<PermissionWrapper<TRole>>>>,
    ) -> Self {
        self.session_manager = Some(session_manager);
        self
    }

    pub fn build(self) -> Addr<IntentConfigActor<TRole>> {
        let session_manager = self.session_manager.clone();

        IntentConfigActor::create(move |ctx| {
            let mut actor = IntentConfigActor::new_with_role(self.role);

            // If we have a session manager, create and register Room
            if let Some(sm) = session_manager {
                match Room::new_with_session_manager(
                    "intent-config".to_string(),
                    ctx.address().recipient(),
                    sm,
                ) {
                    Ok(room) => {
                        actor.room = Some(room);
                        log::info!("IntentConfig Room auto-registered");
                    }
                    Err(e) => {
                        log::error!("Failed to register IntentConfig Room: {}", e);
                    }
                }
            }

            actor
        })
    }
}
```

#### Task 1.6: Testing (1 hour)

1. Run component tests: `cargo nextest run -p zzintent-config`
2. Run integration tests that use zzintent-config
3. Verify no manual serialization remains:
   ```bash
   grep "encode_to_vec" src/components/zzintent-config/src/actor.rs
   ```
4. Verify Room is created and registered

**Expected outcome**:
- Tests pass
- No `bincode::serde::encode_to_vec` in actor.rs (except for legitimate broadcast cases)
- Room auto-registration working

---

### Task 2: Refactor zzmem-db (4-6 hours)

**Goal**: Replace manual serialization with Room<T> and TypedSender

#### Task 2.1: Analyze Current Architecture (30 min)

**File**: `src/components/zzmem-db/src/actor.rs`

Current state:
- Has session_manager field
- Has manual serialization in 3+ locations
- Doesn't have Room field yet

**Questions to answer**:
1. Does MemDB need point-to-point Room communication?
2. Does MemDB broadcast to multiple peers?
3. What are the message sending patterns?

**Action**: Map out all message sends in the component.

#### Task 2.2: Add Room to Actor State (30 min)

**File**: `src/components/zzmem-db/src/actor.rs`

**Add to struct**:
```rust
pub struct MemDBActor<T: ApplicationRole> {
    // ... existing fields ...

    /// Room for typed network messaging (Room<T> architecture)
    room: Option<zznet_room::room::Room<MemDBMessage>>,
}
```

**Update constructor and setter**:
```rust
impl<T: ApplicationRole> MemDBActor<T> {
    pub fn new(/* ... */) -> Self {
        Self {
            // ... existing fields ...
            room: None,
        }
    }

    pub fn with_room(mut self, room: Room<MemDBMessage>) -> Self {
        self.room = Some(room);
        self
    }
}
```

#### Task 2.3: Replace Manual Serialization #1 - SubmitBatch (1-2 hours)

**File**: `src/components/zzmem-db/src/actor.rs`
**Location**: Line ~240

**Current pattern**:
```rust
match bincode::serde::encode_to_vec(&msg_to_send, config) {
    Ok(bytes) => {
        if let Err(e) = sender.send((room_clone, bytes)).await {
            tracing::warn!("Failed to send SubmitBatch to {}: {:?}", peer_id, e);
        }
    }
    Err(e) => {
        tracing::error!("Failed to serialize SubmitBatch: {:?}", e);
    }
}
```

**Target pattern**:
```rust
if let Some(room) = &self.room {
    let sender = room.typed_sender();
    let msg_to_send = batch.clone();

    actix::spawn(async move {
        if let Err(e) = sender.send(msg_to_send).await {
            tracing::warn!("Failed to send SubmitBatch: {}", e);
        }
    });
} else {
    tracing::warn!("MemDB has no room configured, cannot send batch");
}
```

**Note**: This appears to be broadcasting to multiple peers. May need to keep SessionManager pattern for multi-peer sends, but use TypedSender for serialization.

#### Task 2.4: Replace Manual Serialization #2 - Acks (30 min)

**File**: `src/components/zzmem-db/src/actor.rs`
**Location**: Line ~469

Same pattern as above - replace with TypedSender.

#### Task 2.5: Replace Manual Serialization #3 - Query Responses (30 min)

**File**: `src/components/zzmem-db/src/actor.rs`
**Location**: Line ~539

Same pattern as above - replace with TypedSender.

#### Task 2.6: Update Builder (1 hour)

**File**: `src/components/zzmem-db/src/builder.rs` (if exists) or wherever actor is constructed

Add Room creation with auto-registration similar to IntentConfig (Task 1.5).

#### Task 2.7: Testing (1 hour)

1. Run component tests: `cargo nextest run -p zzmem-db`
2. Run integration tests
3. Verify no manual serialization remains
4. Test batch submission, acks, and query responses

---

### Task 3: Update All Component Builders (2-3 hours)

**Goal**: Ensure all component builders create Room with auto-registration when SessionManager is provided.

#### Task 3.1: Create Builder Pattern Template (30 min)

Create a standardized pattern that all builders follow:

```rust
pub struct ComponentBuilder<TRole>
where
    TRole: ApplicationRole,
{
    // Component-specific config
    session_manager: Option<Arc<Mutex<SessionManager<TRole>>>>,
}

impl<TRole> ComponentBuilder<TRole>
where
    TRole: ApplicationRole,
{
    pub fn with_session_manager(
        mut self,
        session_manager: Arc<Mutex<SessionManager<TRole>>>,
    ) -> Self {
        self.session_manager = Some(session_manager);
        self
    }

    pub fn build(self) -> Addr<ComponentActor<TRole>> {
        let session_manager = self.session_manager.clone();
        let room_id = "component-room-id".to_string();

        ComponentActor::create(move |ctx| {
            let mut actor = ComponentActor::new(/* config */);

            // Auto-register Room if SessionManager provided
            if let Some(sm) = session_manager {
                match Room::new_with_session_manager(
                    room_id,
                    ctx.address().recipient(),
                    sm,
                ) {
                    Ok(room) => {
                        log::info!("Component Room auto-registered with SessionManager");
                        actor = actor.with_room(room);
                    }
                    Err(e) => {
                        log::error!("Failed to auto-register Room: {}", e);
                        // Continue without Room - component may still work
                    }
                }
            }

            actor
        })
    }
}
```

#### Task 3.2: Update CStateBuilder (30 min)

**File**: `src/components/zzcollector-state/src/builder.rs`

Currently, CStateBuilder accepts session_manager but doesn't create Room automatically.

**Action**: Add Room creation logic to the `build()` method.

#### Task 3.3: Update IntentConfigBuilder (30 min)

Apply the template pattern.

#### Task 3.4: Update MemDBBuilder (30 min)

Apply the template pattern.

#### Task 3.5: Testing (30 min)

Test that all builders:
1. Accept `.with_session_manager()`
2. Create and register Room automatically
3. Pass Room to actor via `.with_room()`
4. Handle errors gracefully

---

### Task 4: Verification & Testing (2-4 hours)

#### Task 4.1: Component-Level Tests (1 hour)

Run all component tests:
```bash
cargo nextest run -p zzcollector-state
cargo nextest run -p zzintent-config
cargo nextest run -p zzmem-db
cargo nextest run -p zzpinger
```

All should pass.

#### Task 4.2: Integration Tests (1 hour)

Run full test suite:
```bash
cargo nextest run --no-fail-fast
```

Verify no regressions in existing functionality.

#### Task 4.3: Code Audit (1 hour)

**Verify no manual serialization**:
```bash
grep -r "bincode::serde::encode_to_vec" src/components/
```

Should only show:
- Test files (acceptable)
- Legitimate broadcast scenarios where SessionManager is appropriate

**Verify Room usage**:
```bash
grep -r "typed_sender" src/components/
```

Should show usage in all components that send messages.

**Verify auto-registration**:
```bash
grep -r "new_with_session_manager" src/components/
```

Should show usage in all component builders.

#### Task 4.4: Documentation Check (30 min)

Update component documentation to reflect new patterns:
- How to create components with auto-registration
- When to use Room vs SessionManager directly
- Examples of TypedSender usage

---

## Architecture Decisions Needed

### Decision 1: Broadcasting Pattern

**Question**: Should Room<T> support broadcasting to multiple peers, or is that SessionManager's responsibility?

**Options**:

**Option A: SessionManager for broadcasts** (Recommended)
- Pros: Clear separation - Room is point-to-point, SessionManager is multi-peer
- Cons: Components still need SessionManager reference for broadcasts
- Vision alignment: Medium (SessionManager doesn't touch bytes, but components call it directly)

**Option B: Add broadcast to Room<T>**
- Pros: Components never touch SessionManager
- Cons: More complex Room API, unclear abstraction boundary
- Vision alignment: High (components fully abstracted from transport)

**Recommendation**: Option A - Keep SessionManager for broadcasts. This is a legitimate architectural boundary. Components can use Room for point-to-point and SessionManager for broadcasts.

### Decision 2: Serialization in Broadcast Code

**Question**: When components broadcast via SessionManager, should they:

**Option A: Manually serialize** (Current)
```rust
let bytes = bincode::serde::encode_to_vec(&msg, config)?;
session_manager.send_to_all(room_id, bytes);
```

**Option B: SessionManager serializes** (Vision-aligned)
```rust
session_manager.send_to_all(room_id, msg); // Takes typed message
```

**Option C: Use helper function**
```rust
let bytes = Room::<MsgType>::serialize(&msg)?; // Static helper
session_manager.send_to_all(room_id, bytes);
```

**Recommendation**: Option C for now (pragmatic), Option B for full vision (requires SessionManager API change).

### Decision 3: Builder Pattern Consistency

**Question**: Should all component builders follow identical patterns?

**Answer**: Yes. Benefits:
- Easier to learn and use
- Less room for errors
- Better testability
- Clear conventions

**Action**: Create a trait or macro to enforce consistent builder patterns.

---

## Risk Assessment

### Technical Risks

| Risk | Probability | Impact | Mitigation |
|------|------------|--------|------------|
| Breaking existing tests | Medium | Medium | Run tests after each component |
| Message format changes | Low | High | Use same serialization (bincode) |
| Performance regression | Low | Medium | Benchmark before/after |
| Broadcast pattern unclear | Medium | Medium | Make architecture decision first |

### Execution Risks

| Risk | Probability | Impact | Mitigation |
|------|------------|--------|------------|
| Longer than estimated | Medium | Low | Break into smaller tasks |
| Merge conflicts | Low | Low | Work on separate components |
| Unclear requirements | Low | High | Document decisions as we go |

---

## Success Criteria

### Functional Requirements

- ✅ All components using Room<T> use TypedSender
- ✅ No manual `bincode::serde::encode_to_vec` in component actors (except legitimate cases)
- ✅ Component builders create Room with auto-registration
- ✅ All existing tests pass
- ✅ No functional regressions

### Code Quality

- ✅ Consistent pattern across all components
- ✅ Clear documentation of broadcast vs point-to-point
- ✅ Builder pattern is consistent
- ✅ Error handling is appropriate

### Vision Alignment

- ✅ Components work with typed messages (not bytes)
- ✅ Serialization is transparent (handled by framework)
- ✅ Minimal boilerplate in component code
- ✅ Clear architectural boundaries

---

## Estimated Timeline

### Day 1 (8 hours)
- **Morning** (4h): Task 1.1-1.4 - Refactor zzintent-config
- **Afternoon** (4h): Task 1.5-1.6 - Update builder and test

### Day 2 (8 hours)
- **Morning** (4h): Task 2.1-2.4 - Refactor zzmem-db
- **Afternoon** (4h): Task 2.5-2.7 - Complete zzmem-db and test

### Day 3 (8 hours)
- **Morning** (4h): Task 3 - Update all builders
- **Afternoon** (4h): Task 4 - Verification and testing

**Total**: 3 days (24 hours)
**Buffer**: Add 20% for unknowns = ~3.5 days total

---

## Rollback Strategy

If issues arise:

1. **Component-level rollback**: Each component can be rolled back independently
2. **Commits**: Make atomic commits per component for easy revert
3. **Feature flags**: Could add if needed (probably overkill)
4. **Testing**: Extensive testing before merging

---

## Next Steps

### Immediate (Before Starting)

1. **Architecture decision**: Confirm broadcast pattern (Decision 1)
2. **Review plan**: Get sign-off on approach
3. **Setup branch**: Create feature branch for work
4. **Baseline tests**: Confirm all tests pass before starting

### During Execution

1. Work on one component at a time
2. Commit frequently with clear messages
3. Run tests after each component
4. Document any deviations from plan
5. Update this plan with findings

### After Completion

1. Update Phase 3 plan (application migration)
2. Create component development guide
3. Update architecture documentation
4. Celebrate! 🎉

---

**Ready to start?** Review architecture decisions and proceed with Task 1.
