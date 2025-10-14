# Jules Phase 4 Architecture Clarification - SessionManager Pattern

**Date:** October 14, 2025
**Purpose:** Clarify the correct SessionManager architecture for the collector application
**Context:** Jules is stuck on SessionManager type mismatches - this explains the actual pattern

---

## The Core Misunderstanding

**PROBLEM:** The previous guide suggested creating ONE `CollectorNetMessage` enum and ONE SessionManager for the entire application.

**REALITY:** Each component manages its OWN SessionManager with its OWN message type!

---

## Correct Architecture

### Pattern 1: Each Component Has Its Own SessionManager

```rust
// IntentConfig component uses IntentConfigMessage
let intent_session = SessionManager::<IntentConfigMessage, PermissionWrapper<T>>::new(rooms);

// CState component uses CStateMessage
let cstate_session = SessionManager::<CStateMessage, PermissionWrapper<T>>::new(rooms);

// They are SEPARATE SessionManagers!
```

### Why This Design?

1. **Component isolation** - Each component only sees its own message types
2. **Type safety** - No need for a giant enum wrapping all messages
3. **Independent evolution** - Components can change without affecting each other
4. **Clear ownership** - Each component owns its network communication

---

## Solution for Jules: Simplified Approach

Since you're implementing the COLLECTOR application (not individual components), you have two options:

### Option A: Stub Out SessionManager for Now (RECOMMENDED)

**This is the quickest path forward:**

```rust
use crate::config::CollectorConfig;
use crate::error::{CollectorError, Result};

use actix::Addr;

// Component imports
use zzintent_config::builder::IntentConfigBuilder;
use zzintent_config::actor::IntentConfigActor;
use zzintent_config::role::IntentConfigRole;
use zzintent_config::permissions::IntentConfigPermission;

use zzpinger::builder::PingerBuilder;
use zzpinger::api::PingerHandle;

use zzmem_db::actor::MemDBActor;
use zzmem_db::permissions::MemDBPermission;
use zzmem_db::role::MemDBRole;

use zzcollector_state::builder::CStateBuilder;
use zzcollector_state::actor::CStateActor;
use zzcollector_state::role::CStateRole;
use zzcollector_state::network_messages::CStateMessage;

/// Builders for all components (before wiring)
struct ComponentBuilders {
    intent_config: IntentConfigBuilder<IntentConfigPermission>,
    pinger: PingerBuilder,
    memdb_addr: Addr<MemDBActor<MemDBPermission>>,
    cstate: CStateBuilder<CStateMessage, IntentConfigPermission, MockSessionManager>,
}

/// Mock session manager for components during Phase 4
/// Real networking will be added in Phase 5
struct MockSessionManager;

impl<TMsg, TRole> zznet_session::session_manager_like::SessionManagerLike<TMsg, TRole>
    for MockSessionManager
where
    TMsg: zznet_session::room_message_trait::RoomMessageTrait,
    TRole: zznet_auth::ApplicationRole,
{
    // Stub implementations - do nothing for now
    fn send_to_peer(&self, _peer_id: &str, _msg: TMsg) -> Result<(), String> {
        Ok(())
    }

    // ... other required methods with stub implementations
}

pub struct CollectorService {
    config: CollectorConfig,
}

impl CollectorService {
    pub fn new(config: CollectorConfig) -> Result<Self> {
        config.validate()?;
        Ok(Self { config })
    }

    pub async fn run(self) -> Result<()> {
        tracing::info!("Collector service starting");

        // Create builders WITHOUT SessionManager (stub for now)
        let builders = self.create_builders()?;

        // Start components
        let _started = Self::start_components(builders).await?;

        // TODO: Add real network connection in Phase 5

        Ok(())
    }

    fn create_builders(&self) -> Result<ComponentBuilders> {
        // Create IntentConfig builder - NO SESSION MANAGER
        let intent_config = IntentConfigBuilder::<IntentConfigPermission>::new()
            .role(IntentConfigRole::Collector);

        // Create Pinger builder
        let pinger = PingerBuilder::new()
            .enabled(true);

        // Create MemDB actor (no builder)
        let memdb_actor = MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Collector);
        let memdb_addr = memdb_actor.start();

        // Wire pinger with memdb
        let pinger = pinger.memdb_addr(memdb_addr.clone());

        // Create CState builder - USE MOCK
        let cstate = CStateBuilder::new(CStateRole::Collector);

        Ok(ComponentBuilders {
            intent_config,
            pinger,
            memdb_addr,
            cstate,
        })
    }

    async fn start_components(
        builders: ComponentBuilders,
    ) -> Result<StartedComponents> {
        // Start IntentConfig
        let intent_addr = builders.intent_config.start()
            .map_err(|e| CollectorError::Component(format!("IntentConfig start failed: {}", e)))?;

        // Start Pinger
        let pinger_handle = builders.pinger.start()?;

        // Start CState
        let cstate_addr = builders.cstate.build();

        Ok(StartedComponents {
            intent_config: intent_addr,
            pinger: pinger_handle,
            memdb_addr: builders.memdb_addr,
            cstate: cstate_addr,
        })
    }
}

/// Started components (running actors)
struct StartedComponents {
    intent_config: Addr<IntentConfigActor<IntentConfigPermission>>,
    pinger: PingerHandle,
    memdb_addr: Addr<MemDBActor<MemDBPermission>>,
    cstate: Addr<CStateActor<CStateMessage, IntentConfigPermission, MockSessionManager>>,
}
```

---

## Fixing the Three Errors Jules Hit

### Error 1: `anyhow::Error` Conversion

```rust
// In error.rs, add:
#[derive(Error, Debug)]
pub enum CollectorError {
    // ... existing variants ...

    #[error("Component error: {0}")]
    Component(String),  // Use this for anyhow errors
}

// In service.rs, convert anyhow errors:
let intent_addr = builders.intent_config.start()
    .map_err(|e| CollectorError::Component(format!("IntentConfig start failed: {}", e)))?;
```

### Error 2: IntentConfigBuilder Type Mismatch

```rust
// ❌ WRONG - trying to use generic TRole
fn create_builders<TRole: ApplicationRole + std::fmt::Debug>(&self) -> Result<ComponentBuilders<TRole>> {
    let intent_config = IntentConfigBuilder::<TRole>::new()  // Generic doesn't work!

// ✅ CORRECT - use concrete type
fn create_builders(&self) -> Result<ComponentBuilders> {
    let intent_config = IntentConfigBuilder::<IntentConfigPermission>::new()
        .role(IntentConfigRole::Collector);
```

### Error 3: SessionManager Type Mismatch

**The core issue:** IntentConfigBuilder expects `SessionManager<IntentConfigMessage, ...>` but you're trying to give it `SessionManager<CollectorNetMessage, ...>`.

**Solution:** DON'T wire SessionManager for IntentConfig yet. Leave it as `None` (optional). The component will work without network communication for Phase 4.

```rust
// Just create the builder, don't call .session_manager() on it
let intent_config = IntentConfigBuilder::<IntentConfigPermission>::new()
    .role(IntentConfigRole::Collector);
// That's it! No .session_manager() call needed.
```

---

## Revised Step-by-Step for Jules

### Step 1: Simplify - Remove Generic TRole

**Change this:**
```rust
struct ComponentBuilders<TRole: ApplicationRole + std::fmt::Debug> {
    intent_config: IntentConfigBuilder<TRole>,
    // ...
}
```

**To this:**
```rust
struct ComponentBuilders {
    intent_config: IntentConfigBuilder<IntentConfigPermission>,
    pinger: PingerBuilder,
    memdb_addr: Addr<MemDBActor<MemDBPermission>>,
    // CState can work without SessionManager for now
}
```

### Step 2: Remove All SessionManager Wiring

**Delete these calls:**
```rust
// DELETE this
let intent_config = builders.intent_config
    .session_manager((*session_manager).clone());

// DELETE this
let cstate = builders.cstate
    .session_manager(session_manager.clone());
```

**Keep builders as-is:**
```rust
fn create_builders(&self) -> Result<ComponentBuilders> {
    let intent_config = IntentConfigBuilder::<IntentConfigPermission>::new()
        .role(IntentConfigRole::Collector);

    let pinger = PingerBuilder::new()
        .enabled(true);

    let memdb_actor = MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Collector);
    let memdb_addr = memdb_actor.start();

    let pinger = pinger.memdb_addr(memdb_addr.clone());

    Ok(ComponentBuilders {
        intent_config,
        pinger,
        memdb_addr,
    })
}
```

### Step 3: Fix Error Conversion

```rust
// In error.rs
#[error("Component error: {0}")]
Component(String),

// In start_components()
let intent_addr = builders.intent_config.start()
    .map_err(|e| CollectorError::Component(format!("IntentConfig: {}", e)))?;
```

### Step 4: Compile and Verify

```bash
cargo check --bin zzping-collector
cargo build --bin zzping-collector
cargo test -p zzping-collector
```

---

## Why This Works

1. **IntentConfigBuilder** can start WITHOUT a SessionManager (it's optional)
2. **PingerBuilder** only needs MemDB address (network not required yet)
3. **MemDBActor** stores results locally (doesn't need network in Collector role for Phase 4)
4. **CStateBuilder** - we can skip it entirely for Phase 4 or stub it

This gives you a **working collector application** that:
- ✅ Loads configuration
- ✅ Starts all components
- ✅ Pinger can ping and store results in MemDB
- ✅ Compiles and runs

**Network communication will be added in Phase 5** when you implement the database application.

---

## Alternative: If You Must Have SessionManager

If the checklist absolutely requires SessionManager wiring, create a mock:

```rust
// In service.rs
use std::sync::Arc;

/// Mock SessionManager that does nothing (for Phase 4 development)
#[derive(Clone)]
struct MockSessionManager;

// Then use it for CState:
let cstate = CStateBuilder::new(CStateRole::Collector)
    .session_manager(Arc::new(MockSessionManager));
```

But **I recommend skipping SessionManager entirely for Phase 4** and adding it in Phase 5.

---

## Summary

**Jules, do this:**

1. Remove all generic `TRole` parameters - use concrete `IntentConfigPermission`
2. DON'T call `.session_manager()` on any builders
3. Add `Component(String)` variant to `CollectorError`
4. Use `.map_err()` to convert `anyhow::Error` to `CollectorError::Component`
5. Test that it compiles

This gets Phase 4 working. Phase 5 will add real networking.

**You're almost there!** These three errors are the last hurdle.
