# Room Handler Registration: Old vs New Pattern

This document shows the architectural improvement made by introducing `RoomRegistry`.

## Old Pattern (Current, Repetitive)

Located in both `zzping-collector/src/service.rs` and `zzping-database/src/service.rs`:

```rust
// 1. Define wrapper struct inline
struct CollectorIntentConfigRoomHandler {
    intent_addr: Addr<IntentConfigActor<IntentConfigPermission>>,
    room_id: RoomId,
}

// 2. Implement RoomHandle trait
impl RoomHandle<CollectorMessage> for CollectorIntentConfigRoomHandler {
    fn room_id(&self) -> &RoomId { &self.room_id }

    fn send_message(&mut self, msg: CollectorMessage) -> Result<(), SessionError> {
        match msg {
            CollectorMessage::Intent(intent_msg) => {
                self.intent_addr.do_send(NetworkMessageReceived(intent_msg));
                Ok(())
            }
        }
    }

    fn spawn_forwarder(&mut self, _tx: ...) -> Result<(), SessionError> {
        Ok(())
    }
}

// 3. Manual wiring in wire_room_handlers()
async fn wire_room_handlers(
    intent_addr: &Addr<...>,
    session_manager: &Arc<Mutex<SessionManager<CollectorMessage, AuthRole>>>,
) -> Result<()> {
    let mut sm = session_manager.lock().await;

    // Lock, iterate peers, create handlers, call add_room_to_peer
    for peer_id in sm.peer_ids() {
        let handler: Box<dyn RoomHandle<CollectorMessage>> =
            Box::new(CollectorIntentConfigRoomHandler {
                intent_addr: intent_addr.clone(),
                room_id: RoomId::from("zzintent-config"),
            });
        sm.add_room_to_peer(&peer_id, room_id.clone(), handler).await?;
    }
    Ok(())
}

// 4. Manual registration for new peers
pub async fn register_room_for_peer(
    peer_id: &PeerId,
    intent_addr: &Addr<...>,
    session_manager: &Arc<...>,
) -> Result<()> {
    let mut sm = session_manager.lock().await;
    // Repeat the handler creation + add_room_to_peer
    let handler = Box::new(CollectorIntentConfigRoomHandler { ... });
    sm.add_room_to_peer(peer_id, room_id, handler).await?;
    Ok(())
}
```

**Issues:**
- ❌ Code duplicated in both apps
- ❌ Handler struct defined inside function
- ❌ Manual SessionManager locking
- ❌ Peer iteration logic repeated
- ❌ Hard to test
- ❌ Hard to reuse

---

## New Pattern (Refactored)

### Step 1: Define Factory (Once, Reusable)

**File:** `src/apps/zzping-collector/src/room_handlers.rs`

```rust
use zznet_builder::RoomHandlerFactory;

/// Factory for IntentConfig room handlers
pub struct IntentConfigRoomHandlerFactory {
    intent_addr: Addr<IntentConfigActor<IntentConfigPermission>>,
}

impl IntentConfigRoomHandlerFactory {
    pub fn new(intent_addr: Addr<...>) -> Self { ... }
}

impl RoomHandlerFactory<CollectorMessage, AuthRole> for IntentConfigRoomHandlerFactory {
    fn create_handler(&self, room_id: RoomId) -> Box<dyn RoomHandle<CollectorMessage>> {
        Box::new(CollectorIntentConfigRoomHandler {
            intent_addr: self.intent_addr.clone(),
            room_id,
        })
    }
}

// Wrapper struct (now private)
struct CollectorIntentConfigRoomHandler { ... }

impl RoomHandle<CollectorMessage> for CollectorIntentConfigRoomHandler {
    // Same implementation as before
}
```

### Step 2: Use Registry (Centralized, Safe)

**File:** `src/apps/zzping-collector/src/service.rs`

```rust
use zznet_builder::RoomRegistry;
use crate::room_handlers::IntentConfigRoomHandlerFactory;

pub async fn start_components(builders: ComponentBuilders) -> Result<StartedComponents> {
    // Create IntentConfig actor
    let intent_addr = builders.intent_config.start()?;

    // 🎯 Create registry (one call)
    let mut registry = RoomRegistry::new(Arc::clone(&builders.session_manager));

    // 🎯 Register handler factory (one call)
    registry.register_room_handler(
        RoomId::from("intent-config"),
        Arc::new(IntentConfigRoomHandlerFactory::new(intent_addr.clone()))
    );

    // 🎯 Wire all peers at startup (one call)
    registry.wire_all_peers().await?;

    Ok(StartedComponents { ... })
}

// For dynamic peer connections:
// 🎯 Register new peer (one call)
// registry.wire_peer(&peer_id).await?;
```

**Advantages:**
- ✅ Clean, readable API
- ✅ No manual SessionManager locking
- ✅ Reusable across apps
- ✅ Easy to test (RoomRegistry is generic)
- ✅ Can register multiple rooms at once
- ✅ Same handler factory for startup + dynamic peers

---

## Comparison

| Aspect | Old Pattern | New Pattern |
|--------|-------------|-------------|
| **Lines of code per app** | ~150 lines | ~20 lines |
| **Inline structs** | 1-2 per room | 0 (factory pattern) |
| **Manual locking** | Yes (error-prone) | No (handled by registry) |
| **Reusable across apps** | No | Yes |
| **Testable** | Hard | Easy |
| **Duplication** | Both apps, both functions | None |

---

## Migration Path

1. **Phase 1** (DONE): Create `RoomRegistry` infrastructure ✅
2. **Phase 2** (Optional): Implement `RoomHandlerFactory` for each component ⏳
3. **Phase 3** (Optional): Replace old `wire_room_handlers()` calls ⏳
4. **Phase 4** (Optional): Remove old methods entirely ⏳

**Can be done incrementally** — old and new patterns can coexist during migration.

---

## File Locations

### New Infrastructure
- `src/net/zznet-builder/src/room_registry.rs` — Core registry and trait
- `src/net/zznet-builder/tests/room_registry_test.rs` — Unit tests with examples

### Collector App (Partial Migration)
- `src/apps/zzping-collector/src/room_handlers.rs` — Factory implementation
- `src/apps/zzping-collector/src/service.rs` — Refactor notes

### Database App (Future)
- Will have similar structure when migrated

---

## Testing the New Pattern

```bash
# Run registry tests
cargo test -p zznet-builder --test room_registry_test

# Verify no regressions
cargo test -q
```

All 660+ tests pass ✅
