# 04 — Plan: Finalizing the SOLID Refactor

**Date:** 2025-10-29
**Status:** Proposed

## 1. Motivation

This document outlines the plan to address the final action items identified in the `03_Skeptical_Audit_Report.md`. The goal is to complete the transition to a pure actor-based, unidirectional architecture by removing the last remnants of the old patterns and ensuring all documentation is up to date.

## 2. Scope and Success Criteria

- **Purge legacy APIs:** Remove all deprecated, data-plane-related messages and trait implementations from the control-plane actor (`PeerManagerActor`).
- **Complete actor-first transition:** Replace the final `Arc<dyn PeerRegistry>` trait object with actor messaging, fully committing to the actor model for inter-service communication.
- **Finalize documentation:** Update all related documentation to reflect the completed and audited state of the architecture.
- **Quality gates:** All changes must pass the full suite of builds, lints, and tests.

---

## 3. High-Level Milestones

1.  **Purge Legacy Data-Plane Messages from `PeerManagerActor`** (High Priority)
2.  **Replace `Arc<dyn PeerRegistry>` with Actor Messaging** (Medium Priority)
3.  **Documentation Cleanup** (Low Priority)

---

## Milestone 1: Purge Legacy Data-Plane Messages from `PeerManagerActor`

This milestone focuses on enforcing the unidirectional "club sandwich" architecture by removing the deprecated APIs that allow components to query the `PeerManager` for data-plane information.

**Files to Modify:**
- `src/net/zznet-peer-manager/src/actor.rs`
- `src/net/zznet-api/src/traits.rs` (to remove the methods from the trait)
- Any files that show compiler errors after the removal.

**Changes:**
1.  **Remove Deprecated Messages:** In `src/net/zznet-peer-manager/src/actor.rs`, identify and delete the `actix::Message` structs and their corresponding `Handler` implementations for any data-plane operations. This includes, but is not limited to:
    - `GetPeerSender`
    - `SubscribePeerInbound`
    - `GetJoinedRooms`
2.  **Update `MessageRouter` Trait:** In `src/net/zznet-api/src/traits.rs`, remove the corresponding method definitions from the `MessageRouter` trait to ensure no new implementations can use them.
3.  **Fix Compile Errors:** Run a full workspace check (`cargo check --all-targets`) and fix any resulting compilation errors. Since these APIs are deprecated, there should be no active call sites, but this step ensures complete cleanup.

**Acceptance Criteria:**
- The codebase compiles successfully.
- A global search for `GetPeerSender`, `SubscribePeerInbound`, and `GetJoinedRooms` yields no results within the `src` directory.
- The `MessageRouter` trait in `zznet-api` is clean of any data-plane query methods.

---

## Milestone 2: Replace `Arc<dyn PeerRegistry>` with Actor Messaging

This milestone completes the transition to a pure actor model by removing the last `Arc<dyn ...>` dependency from the component network managers.

**Files to Modify:**
- `src/components/zzintent-config/src/network_manager.rs`
- `src/components/zzmem-db/src/network_manager.rs`
- (And any other component `network_manager.rs` files using `Arc<dyn PeerRegistry>`)
- `src/apps/zzping-collector/src/service.rs`
- `src/apps/zzping-database/src/service.rs`
- `src/net/zznet-peer-manager/src/actor.rs`

**Changes:**
1.  **Add `IsPeerConnected` Message:** In `src/net/zznet-peer-manager/src/actor.rs`, create a new message `IsPeerConnected { peer_id: PeerId }` that returns a `bool`. Implement the handler to check the internal state.
2.  **Remove `Arc<dyn PeerRegistry>`:** In each component's `NetworkManager` (e.g., `IntentConfigNetworkManager`), remove the `peer_registry: Arc<dyn PeerRegistry>` field.
3.  **Add `PeerManagerActor` Address:** Add a `peer_manager: Addr<PeerManagerActor>` field to each `NetworkManager`.
4.  **Update Call Sites:** In the `spawn_network_actor` method of each `NetworkManager`, replace the direct call `self.peer_registry.is_peer_connected(...)` with an asynchronous message send: `self.peer_manager.send(IsPeerConnected { ... }).await`.
5.  **Update Application Wiring:** In `CollectorService` and `DatabaseService`, update the component builders to accept the `Addr<PeerManagerActor>` instead of the `Arc<Mutex<PeerManager>>`.

**Acceptance Criteria:**
- The `Arc<dyn PeerRegistry>` type is no longer present in any component `NetworkManager`.
- Components now communicate with the `PeerManager` exclusively through actor messages.
- The application compiles and passes all integration tests, proving the new communication path works correctly.

---

## Milestone 3: Documentation Cleanup

This final milestone ensures the project's documentation accurately reflects the finished architecture.

**Files to Modify:**
- `docs/zznet-router-solid/README.md`
- `docs/zznet-router-solid/02_Plan_ZZNet_Router_SOLID_Refactor.md`
- Any other relevant architectural documents.

**Changes:**
1.  **Update `README.md`:** Review `docs/zznet-router-solid/README.md` and remove any language that suggests the refactor is in-progress. Update diagrams and code snippets to match the final implementation.
2.  **Finalize Plan Document:** Mark the `02_Plan_ZZNet_Router_SOLID_Refactor.md` as fully complete and archived.
3.  **Review Other Docs:** Briefly scan other design documents to ensure they don't contain stale information that contradicts the new, audited architecture.

**Acceptance Criteria:**
- The `zznet-router-solid` documentation is clean, accurate, and reflects the production-ready state of the code.
- There is no conflicting or outdated architectural information in the `docs` folder regarding the router and peer management systems.
