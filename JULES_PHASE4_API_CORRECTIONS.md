# Jules Phase 4 API Corrections Guide

**Date:** October 14, 2025
**Purpose:** Provide correct API patterns for implementing `service.rs` in the collector application
**Context:** The PHASE4_CHECKLIST_V2.md examples are outdated; this document provides the actual current APIs

---

## Executive Summary

You're correct that the APIs have evolved. **Continue with your approach** of systematically fixing the code by referencing the actual source. This document provides the correct patterns to speed up your work.

### Key API Changes from Checklist

1. **Generic bounds require `Debug`** - All `TRole` parameters need `+ std::fmt::Debug`
2. **Builder patterns changed** - No-arg constructors, builder methods return owned builders
3. **MemDB requires type parameter** - `MemDBActor<MemDBPermission>`
4. **Pinger returns `PingerHandle`** not `Addr<PingerActor>`
5. **RoomMessageTrait has 4 methods** - `room_id()`, `serialize_inner()`, `deserialize_for_room()`, `supported_rooms()`
6. **IntentConfigBuilder::new()** takes no arguments, use `.role()` to set role

---

## Part 1: Fix Generic Bounds (Add `Debug` Everywhere)

### Problem
```
error: `TRole` doesn't implement `Debug`
```

### Solution
**Every generic function/struct using `TRole: ApplicationRole` must also add `+ std::fmt::Debug`:**

```rust
// ❌ WRONG (from checklist)
fn create_builders<TRole: ApplicationRole>(&self) -> Result<ComponentBuilders<TRole>>

// ✅ CORRECT (actual API)
fn create_builders<TRole: ApplicationRole + std::fmt::Debug>(&self) -> Result<ComponentBuilders<TRole>>
```

**Apply this to:**
- All struct definitions with `TRole`
- All function signatures with `TRole`
- ALL generic parameters in `service.rs`

---

## Part 2: Correct Builder Patterns

### IntentConfigBuilder

```rust
// ❌ WRONG (from checklist)
use zzintent_config::builder::IntentConfigBuilder;
use zzintent_config::role::IntentConfigRole;

let intent_config = IntentConfigBuilder::new(IntentConfigRole::Collector);

// ✅ CORRECT (actual API)
use zzintent_config::builder::IntentConfigBuilder;
use zzintent_config::role::IntentConfigRole;
use zzintent_config::permissions::IntentConfigPermission;

let intent_config = IntentConfigBuilder::<IntentConfigPermission>::new()
    .role(IntentConfigRole::Collector);
```

**Key points:**
- `new()` takes **zero arguments**
- Use `.role(...)` method to set the role
- Type parameter defaults to `IntentConfigPermission` but you may need to specify it explicitly

### PingerBuilder

```rust
// ✅ CORRECT (actual API)
use zzpinger::builder::PingerBuilder;
use zzpinger::api::PingerHandle;
use zzmem_db::actor::MemDBActor;
use zzmem_db::permissions::MemDBPermission;

let pinger_handle: PingerHandle = PingerBuilder::new()
    .memdb_addr(memdb_addr.clone())
    .enabled(true)
    .start()?;  // Returns Result<PingerHandle, PingerError>
```

**Key points:**
- `PingerBuilder::start()` returns `Result<PingerHandle, PingerError>` NOT `Addr<PingerActor>`
- Use `PingerHandle` in your `StartedComponents` struct
- Add error conversion for `PingerError` in your error enum

### CStateBuilder

```rust
// ✅ CORRECT (actual API)
use zzcollector_state::builder::CStateBuilder;
use zzcollector_state::role::CStateRole;
use zzcollector_state::network_messages::CStateMessage;

// Generic signature
let cstate_builder: CStateBuilder<CollectorNetMessage, TRole, SessionManager<CollectorNetMessage, TRole>> =
    CStateBuilder::new(CStateRole::Collector);

// Wire with session manager
let cstate_builder = cstate_builder
    .session_manager(session_manager.clone());

// Build (returns Addr)
let cstate_addr = cstate_builder.build();
```

**Key points:**
- `CStateBuilder::new(role)` takes the role as argument
- `CStateBuilder` is generic over `<TMsg, TRole, SM>`
- `.build()` returns `Addr<CStateActor<...>>` directly (no `?` needed)

### MemDBActor

```rust
// ❌ WRONG (from checklist)
use zzmem_db::builder::MemDBBuilder;  // This doesn't exist!

// ✅ CORRECT (actual API) - No builder, use Actor::start
use zzmem_db::actor::MemDBActor;
use zzmem_db::permissions::MemDBPermission;
use zzmem_db::role::MemDBRole;

let memdb_actor = MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Collector);
let memdb_addr = memdb_actor.start();
```

**Key points:**
- There is NO `MemDBBuilder` - use `MemDBActor::new_with_role()` directly
- Must specify type parameter: `MemDBActor<MemDBPermission>`
- Call `.start()` to get `Addr<MemDBActor<MemDBPermission>>`

---

## Part 3: RoomMessageTrait Implementation

### Problem
```
error[E0046]: not all trait items implemented, missing: `room_id`, `serialize_inner`, `deserialize_for_room`, `supported_rooms`
```

### Solution
Implement all 4 required methods (not just `get_room_id`):

```rust
use zznet_session::room_message_trait::{RoomMessageTrait, SerializationError, DeserializationError};
use zznet_session::types::RoomId;
use serde::{Serialize, Deserialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CollectorNetMessage {
    CState(CStateMessage),
    // Add other message types as needed
}

impl RoomMessageTrait for CollectorNetMessage {
    /// Get the room ID for this message (NEW METHOD NAME)
    fn room_id(&self) -> RoomId {
        match self {
            CollectorNetMessage::CState(msg) => msg.room_id(),
        }
    }

    /// Serialize the inner message (not the enum wrapper)
    fn serialize_inner(&self) -> Result<Vec<u8>, SerializationError> {
        match self {
            CollectorNetMessage::CState(msg) => msg.serialize_inner(),
        }
    }

    /// Deserialize from room ID + bytes
    fn deserialize_for_room(room_id: &RoomId, bytes: &[u8]) -> Result<Self, DeserializationError> {
        // Determine which variant based on room_id
        if room_id == &RoomId::from("cstate") {
            let msg = CStateMessage::deserialize_for_room(room_id, bytes)?;
            Ok(CollectorNetMessage::CState(msg))
        } else {
            Err(DeserializationError::UnknownRoom(room_id.clone()))
        }
    }

    /// List all rooms this application supports
    fn supported_rooms() -> Vec<RoomId> {
        vec![
            RoomId::from("cstate"),
            // Add other rooms as needed
        ]
    }
}
```

**Key changes from checklist:**
- Method name is `room_id()` NOT `get_room_id()`
- Must implement all 4 methods
- Add proper error handling for unknown rooms

---

## Part 4: Error Handling

### Add PingerError Conversion

```rust
// In src/error.rs
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CollectorError {
    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Service error: {0}")]
    Service(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Pinger error: {0}")]
    Pinger(#[from] zzpinger::error::PingerError),  // ADD THIS

    #[error("Component error: {0}")]
    Component(String),
}
```

---

## Part 5: Complete service.rs Structure

Here's the corrected structure for your `service.rs`:

```rust
use crate::config::CollectorConfig;
use crate::error::{CollectorError, Result};

use actix::{Addr, Actor};
use std::sync::Arc;

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

// Network imports
use zznet_session::session_manager::SessionManager;
use zznet_session::room_message_trait::{RoomMessageTrait, SerializationError, DeserializationError};
use zznet_session::types::RoomId;
use zznet_auth::role::ApplicationRole;

use serde::{Serialize, Deserialize};

/// Application message enum (wraps all component messages)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum CollectorNetMessage {
    CState(CStateMessage),
}

impl RoomMessageTrait for CollectorNetMessage {
    fn room_id(&self) -> RoomId {
        match self {
            CollectorNetMessage::CState(msg) => msg.room_id(),
        }
    }

    fn serialize_inner(&self) -> Result<Vec<u8>, SerializationError> {
        match self {
            CollectorNetMessage::CState(msg) => msg.serialize_inner(),
        }
    }

    fn deserialize_for_room(room_id: &RoomId, bytes: &[u8]) -> Result<Self, DeserializationError> {
        if room_id == &RoomId::from("cstate") {
            let msg = CStateMessage::deserialize_for_room(room_id, bytes)?;
            Ok(CollectorNetMessage::CState(msg))
        } else {
            Err(DeserializationError::UnknownRoom(room_id.clone()))
        }
    }

    fn supported_rooms() -> Vec<RoomId> {
        vec![RoomId::from("cstate")]
    }
}

/// Builders for all components (before wiring)
struct ComponentBuilders<TRole: ApplicationRole + std::fmt::Debug> {
    intent_config: IntentConfigBuilder<TRole>,
    pinger: PingerBuilder,
    memdb_addr: Addr<MemDBActor<MemDBPermission>>,
    cstate: CStateBuilder<CollectorNetMessage, TRole, SessionManager<CollectorNetMessage, TRole>>,
}

/// Wired components (builders with session manager attached)
struct WiredComponents<TRole: ApplicationRole + std::fmt::Debug> {
    intent_config: IntentConfigBuilder<TRole>,
    pinger: PingerBuilder,
    memdb_addr: Addr<MemDBActor<MemDBPermission>>,
    cstate: CStateBuilder<CollectorNetMessage, TRole, SessionManager<CollectorNetMessage, TRole>>,
}

/// Started components (running actors)
struct StartedComponents<TRole: ApplicationRole + std::fmt::Debug> {
    intent_config: Addr<IntentConfigActor<TRole>>,
    pinger: PingerHandle,
    memdb_addr: Addr<MemDBActor<MemDBPermission>>,
    cstate: Addr<CStateActor<CollectorNetMessage, TRole, SessionManager<CollectorNetMessage, TRole>>>,
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

        // Step 1: Create builders
        let builders = self.create_builders::<IntentConfigPermission>()?;

        // Step 2: Create session manager
        let session_manager = Self::create_session_manager::<IntentConfigPermission>().await?;

        // Step 3: Wire components
        let wired = Self::wire_components(builders, session_manager)?;

        // Step 4: Start components
        let _started = Self::start_components(wired).await?;

        // TODO: Connect to database, start main loop, etc.

        Ok(())
    }

    fn create_builders<TRole: ApplicationRole + std::fmt::Debug>(&self) -> Result<ComponentBuilders<TRole>> {
        // Create IntentConfig builder
        let intent_config = IntentConfigBuilder::<TRole>::new()
            .role(IntentConfigRole::Collector);

        // Create Pinger builder
        let pinger = PingerBuilder::new()
            .enabled(true);

        // Create MemDB actor (no builder)
        let memdb_actor = MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Collector);
        let memdb_addr = memdb_actor.start();

        // Create CState builder
        let cstate = CStateBuilder::new(CStateRole::Collector);

        Ok(ComponentBuilders {
            intent_config,
            pinger,
            memdb_addr,
            cstate,
        })
    }

    async fn create_session_manager<TRole: ApplicationRole + std::fmt::Debug>() -> Result<Arc<SessionManager<CollectorNetMessage, TRole>>> {
        // Get list of rooms from message trait
        let offered_rooms = CollectorNetMessage::supported_rooms();

        let session_manager = SessionManager::new(offered_rooms);
        Ok(Arc::new(session_manager))
    }

    fn wire_components<TRole: ApplicationRole + std::fmt::Debug>(
        builders: ComponentBuilders<TRole>,
        session_manager: Arc<SessionManager<CollectorNetMessage, TRole>>,
    ) -> Result<WiredComponents<TRole>> {
        // Wire IntentConfig
        let intent_config = builders.intent_config
            .session_manager((*session_manager).clone());  // Clone the SessionManager, not Arc

        // Wire Pinger (memdb already set via addr)
        let pinger = builders.pinger
            .memdb_addr(builders.memdb_addr.clone());

        // Wire CState
        let cstate = builders.cstate
            .session_manager(session_manager.clone());

        Ok(WiredComponents {
            intent_config,
            pinger,
            memdb_addr: builders.memdb_addr,
            cstate,
        })
    }

    async fn start_components<TRole: ApplicationRole + std::fmt::Debug>(
        wired: WiredComponents<TRole>,
    ) -> Result<StartedComponents<TRole>> {
        // Start IntentConfig - need to call .start() method
        let intent_addr = wired.intent_config.start();

        // Start Pinger
        let pinger_handle = wired.pinger.start()?;

        // Start CState
        let cstate_addr = wired.cstate.build();

        Ok(StartedComponents {
            intent_config: intent_addr,
            pinger: pinger_handle,
            memdb_addr: wired.memdb_addr,
            cstate: cstate_addr,
        })
    }
}
```

---

## Part 6: Step-by-Step Fix Checklist

Work through these in order:

### [ ] Step 1: Fix all generic bounds
- Add `+ std::fmt::Debug` to every `TRole: ApplicationRole`
- This fixes ~15 errors at once

### [ ] Step 2: Fix IntentConfigBuilder
- Change `IntentConfigBuilder::new(role)` to `IntentConfigBuilder::new().role(role)`
- Add type parameter if needed: `IntentConfigBuilder::<IntentConfigPermission>::new()`

### [ ] Step 3: Fix MemDB
- Remove import of `MemDBBuilder` (doesn't exist)
- Use `MemDBActor::<MemDBPermission>::new_with_role(role)`
- Call `.start()` to get address

### [ ] Step 4: Fix Pinger
- Change return type from `Addr<PingerActor>` to `PingerHandle`
- Add `#[from] zzpinger::error::PingerError` to error enum
- Keep `start()?` call

### [ ] Step 5: Fix RoomMessageTrait
- Rename `get_room_id()` to `room_id()`
- Add `serialize_inner()` method
- Add `deserialize_for_room()` static method
- Add `supported_rooms()` static method

### [ ] Step 6: Fix CState builder
- `build()` returns `Addr` directly (no `?`)
- Ensure generic parameters match

### [ ] Step 7: Run cargo check
- Fix any remaining import errors
- Fix any remaining type mismatches

---

## Part 7: Common Patterns

### Pattern: Cloning SessionManager

```rust
// SessionManager is in an Arc, but builders need owned SessionManager
let session_manager = Arc::new(SessionManager::new(rooms));

// ❌ WRONG - can't move out of Arc
builder.session_manager(session_manager)

// ✅ CORRECT - clone the inner SessionManager
builder.session_manager((*session_manager).clone())

// OR for CState which takes Arc
cstate_builder.session_manager(session_manager.clone())  // Clones the Arc
```

### Pattern: Type Inference

Sometimes Rust needs help with type parameters:

```rust
// Explicit type annotation
let builders: ComponentBuilders<IntentConfigPermission> = self.create_builders()?;

// OR specify in function call
let builders = self.create_builders::<IntentConfigPermission>()?;
```

---

## Part 8: Quick Reference

| Component | Builder Call | Start Method | Return Type |
|-----------|-------------|--------------|-------------|
| IntentConfig | `IntentConfigBuilder::new().role(r)` | `.start()` | `Addr<IntentConfigActor<T>>` |
| Pinger | `PingerBuilder::new()` | `.start()?` | `Result<PingerHandle, PingerError>` |
| MemDB | `MemDBActor::new_with_role(r)` | `.start()` | `Addr<MemDBActor<T>>` |
| CState | `CStateBuilder::new(r)` | `.build()` | `Addr<CStateActor<...>>` |

---

## Debugging Tips

1. **Fix bounds first** - Add `+ std::fmt::Debug` everywhere, this cascades through most errors
2. **One component at a time** - Comment out other components, fix one completely
3. **Check return types** - Pinger returns `PingerHandle`, others return `Addr`
4. **Use explicit types** - When compiler can't infer, add type annotations
5. **Check examples** - Look at `examples/message-exchange-example/` for working patterns

---

## Next Steps

1. Apply the corrections from Parts 1-5 systematically
2. Run `cargo check --bin zzping-collector` after each major fix
3. Once it compiles, move to testing (write tests for service initialization)
4. Report back if you hit any blocking issues

You're on the right track - the checklist is just outdated. Keep going!
