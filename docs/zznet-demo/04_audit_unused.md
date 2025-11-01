# ZZNET ARCHITECTURAL AUDIT - CONSOLIDATED REPORT

## PART 1: FILE-BY-FILE AUDIT FINDINGS (FULL DETAIL)

### Audit Progress: File 1 of 18

*   **File:** `apps/zznet-demo/src/component_a.rs`
    *   **Coverage:** `Func: 91.30%`, `Line: 87.97%`
    *   **Finding:** The `Handler<StateUpdate>` implementation for `ComponentAActor` is uncovered.
    *   **Analysis:** This handler implies a bidirectional data flow where local subscribers can push state back to the publisher (`ComponentA`). This violates the intended one-way pub/sub design.
    *   **Decision:** ✅ **DELETE** this handler implementation completely to enforce one-way data flow.

### Audit Progress: File 2 of 18

*   **File:** `apps/zznet-demo/src/component_b.rs`
    *   **Coverage:** `Func: 61.54%`, `Line: 54.10%`
    *   **Finding 1:** `subscribe_to_component_a()` is dead code (manual wiring used).
    *   **Finding 2:** `get_recipient()` is dead code (placeholder, never used).
    *   **Finding 3:** `get_counter()` is redundant API (message-based `GetCounter` is used).
    *   **Finding 4:** `impl Handler<PublishToA>` is dead code (violates passive subscriber role).
    *   **Decision:** ✅ **DELETE** all four items.

### Audit Progress: File 3 of 18

*   **File:** `apps/zznet-demo/src/messages.rs`
    *   **Coverage:** `Func: 50.00%`, `Line: 70.00%`
    *   **Finding:** `RoomMessageTrait::supported_rooms()` is never called by the framework.
    *   **Analysis:** This feature for per-message-type room validation was designed into the trait but never implemented in the framework core. It is architectural cruft.
    *   **Decision:** ✅ **DELETE** the `supported_rooms()` method from the `RoomMessageTrait` definition in `src/net/zznet-room/src/room_message_trait.rs` and from all implementors.

### Audit Progress: File 4 of 18

*   **File:** `net/zznet-api/src/error.rs`
    *   **Coverage:** `Func: 0.00%`, `Line: 0.00%`
    *   **Decision:** ✅ **SKIP.** Benign helper function, lack of coverage is acceptable.

### Audit Progress: File 5 of 18

*   **File:** `net/zznet-api/src/mock.rs`
    *   **Coverage:** `Func: 35.71%`, `Line: 49.15%`
    *   **Finding:** Advanced features (`inject_error`, `MockServer`, `MockClient`) are unused by the demo.
    *   **Decision:** ✅ **KEEP.** This represents a test gap in the demo, not dead code. No action required on this file.

### Audit Progress: File 6 of 18

*   **File:** `net/zznet-api/src/types.rs`
    *   **Coverage:** `Func: 42.86%`, `Line: 40.91%`
    *   **Decision:** ✅ **SKIP.** Uncovered code is benign helpers/legacy structs.

### Audit Progress: File 7 of 18

*   **File:** `net/zznet-hello/src/actor.rs`
    *   **Finding 1 (Timeout):** Handshake timeout logic is untested. -> **Action:** Add a `TODO` for a future negative test suite.
    *   **Finding 2 (Silent Send Fail):** `send_frame_to_io()` fails silently if the I/O task dies. -> **Action:** Add a `FIXME`. The actor **must stop itself** on silent send failure.
    *   **Finding 3 (Silent Handshake Error):** Handshake error paths are logged but actor continues. -> **Action:** Add a `TODO`. Actor should call `handle_error` to stop.
    *   **Finding 4 (TLS Validation):** Certificate validation logic is uncovered. -> **Action:** Add a `TODO` for a future TLS integration test.
    *   **Finding 5 (Over-Engineering):** `handle_error()` complexity for sending `Error` frame. -> **Action:** Add a `TODO` for architectural review of `handle_error` complexity.
    *   **Finding 6 (I/O Task Error):** `spawn_io_task` error paths are untested. -> **Action:** Add a `TODO` for a future negative test using `inject_error`.
    *   **Finding 7 (Dead Code):** `impl Handler<InboundRoomMessage>` is unused, superseded by `SessionBridge`. -> **Action:** **DELETE** the handler and its message type.
    *   **Finding 8 (Dead Code):** `start_hello_actor` is redundant API. -> **Action:** **DELETE** the function.

### Audit Progress: File 8 of 18

*   **File:** `net/zznet-hello/src/connection_manager.rs`
    *   **Finding:** 7 unused handlers/messages: `HandshakePostProcessed`, `GetPeers`, `GetPeerSender`, `SubscribePeerInbound`, `SendToRoom`.
    *   **Analysis:** All are remnants of the old data plane architecture or complex wiring designs.
    *   **Decision:** ✅ **DELETE** all 7 items (`HandshakePostProcessed` struct/handler, `GetPeers` struct/handler, `GetPeerSender` struct/handler, `SubscribePeerInbound` struct/handler, `SendToRoom` struct/handler).

### Audit Progress: File 10 of 18

*   **File:** `net/zznet-hello/src/handshake.rs`
    *   **Finding 1:** `is_failed()` and `state()` are dead code. -> **Action:** **DELETE** both methods.
    *   **Finding 2:** Silent ignore of duplicate HELLO. -> **Action:** **ADD `tracing::warn!`** to the `HandshakeState::Complete` case in `process_hello`.
    *   **Finding 3:** Redundant error frame creation on empty intersection. -> **Action:** **MODIFY** `process_offer` to log a warning and return `Err` instead of creating and returning an `Error` frame.
    *   **Finding 4:** `process_ack()` is dead code. -> **Action:** **DELETE** the method and the corresponding `HandshakeFrame::Ack` variant from `protocol.rs`.

### Audit Progress: File 11 of 18

*   **File:** `net/zznet-hello/src/serialize.rs`
    *   **Finding:** The entire module is dead code, superseded by `RoomActor<T>`.
    *   **Decision:** ✅ **DELETE** the entire file and move the `create_disconnect_frame` logic inline into `HelloActor`.

### Audit Progress: File 16 of 18

*   **File:** `net/zznet-router/src/actor.rs`
    *   **Finding 1:** `OnPeerDisconnected` is uncovered. -> **Action:** **KEEP**, but **ADD `TODO`** for test gap.
    *   **Finding 2:** `HandlePublishRooms` is dead code. -> **Action:** **DELETE** the message and handler.
    *   **Finding 3 & 4:** `PeerJoinedRooms`, `IsRoomJoined` are dead code. -> **Action:** **DELETE** both messages and handlers.
    *   **Finding 5 & 6:** `PeerSender`, `SubscribePeerInbound` are anti-patterns. -> **Action:** **DELETE** both messages and handlers.

### Audit Progress: File 17 of 18

*   **File:** `net/zznet-router/src/peer_channels.rs`
    *   **Finding:** Most public methods are unused remnants of the old data plane API or introspection helpers.
    *   **Decision:** ✅ **DELETE** `handle_publish_rooms`, `outbound_sender`, `subscribe_inbound`, `send_raw_to_room`, `joined_rooms`, `is_room_joined`, and the entire `PeerChannelsTrait` implementation.
    *   **Decision:** ✅ **KEEP** `disconnect()` but add a `TODO` for the test gap.

### Audit Progress: File 18 of 18

*   **File:** `net/zznet-router/src/router.rs`
    *   **Finding:** Many methods (`disconnect_peer`, `peer_mut`, `peer`, `handle_publish_rooms`, `peer_joined_rooms`, `is_room_joined`, `peer_sender`, `subscribe_peer_inbound`) are now dead code due to the cleanup of the `RouterActor` API.
    *   **Decision:**
        *   **KEEP** `disconnect_peer`. Add a `TODO` for the test gap.
        *   **DELETE** all other listed methods (`peer_mut`, `peer`, `handle_publish_rooms`, `peer_joined_rooms`, `is_room_joined`, `peer_sender`, `subscribe_peer_inbound`).

---

## PART 2: FINAL AUDIT REPORT & ACTION PLAN SUMMARY

### Executive Summary

The audit confirmed the success of the new `RoomActor<T>` architecture but revealed pervasive architectural remnants from previous designs, leading to significantly low coverage in core routing crates. The primary finding is that the codebase maintained a large, unused "fat interface" on `RouterActor` and `ConnectionManager` that actively encouraged breaking the new SOLID principles.

The following action plan finalizes the refactor by eliminating these remnants and setting explicit `TODO`/`FIXME` markers for critical test gaps.

### Consolidated Action Plan: Finalizing Cleanup

| Component/File | Action | Category & Rationale |
| :--- | :--- | :--- |
| **`zznet-hello`** / `serialize.rs` | **DELETE FILE.** | Architectural Remnant: Superseded by `RoomActor<T>`. |
| **`zznet-hello`** / `actor.rs` | **DELETE** `start_hello_actor` and `Handler<InboundRoomMessage>`. | Architectural Remnant: Bypassed by `SessionBridge` and redundant API. |
| **`zznet-hello`** / `connection_manager.rs` | **DELETE 7 UNUSED HANDLERS** (`GetPeers`, `SendToRoom`, `GetPeerSender`, etc.) | Architectural Remnant: Old data plane API (ISP violation). |
| **`zznet-hello`** / `handshake.rs` | **DELETE** `is_failed()`, `state()`, `process_ack()`, and `Ack` protocol frame. **SIMPLIFY** `process_offer`. | Protocol Simplification: Removes dead code and simplifies handshake to HELLO -> OFFER. |
| **`zznet-router`** / `actor.rs` | **DELETE 5 UNUSED MESSAGES** (`HandlePublishRooms`, `PeerSender`, etc.). | Architectural Remnant: Removes unused query/anti-pattern APIs. |
| **`zznet-router`** / `peer_channels.rs` | **DELETE ALL PUBLIC ACCESSORS/SENDERS** (`outbound_sender`, `send_raw_to_room`, `joined_rooms`, and `PeerChannelsTrait` impl). | Architectural Remnant: Enforces pure internal data structure; access must go via `RouterActor`. |
| **`zznet-router`** / `router.rs` | **DELETE 7 REMNANT METHODS** (`peer`, `peer_mut`, `handle_publish_rooms`, etc.). | Architectural Remnant: Removes internal helpers for deleted public APIs. |
| **`zznet-demo`** / `component_a.rs` | **DELETE** `Handler<StateUpdate>`. | Design Fix: Enforces one-way data flow. |
| **`zznet-room`** / `room_message_trait.rs` | **DELETE** `supported_rooms()` from trait and implementors. | Design Fix: Feature was not needed/implemented. |

### Missing Test Cases & Required `TODO`s

The following lines must be added to the cleaned code to address the critical gaps revealed by the audit:

| Location | Issue | Required Action |
| :--- | :--- | :--- |
| **`net/zznet-hello/src/actor.rs`** | Silent I/O send failure. | **FIXME:** Actor must call `ctx.stop()` inside `send_frame_to_io` on failure. |
| **`net/zznet-router/src/actor.rs`** | Disconnection lifecycle uncovered. | **TODO:** Add integration test that simulates peer disconnection to cover `OnPeerDisconnected` handler. |
| **`net/zznet-hello/src/actor.rs`** | Handshake error path uncovered. | **TODO:** Add test using mock transport's `inject_error` to verify actor termination. |
| **`net/zznet-hello/src/actor.rs`** | TLS Validation uncovered. | **TODO:** Add TLS validation test case. |
| **`net/zznet-router/src/router.rs`** | Disconnection lifecycle uncovered. | **TODO:** Add test case to cover `disconnect_peer()` method. |


--------------------

# ZZNET ARCHITECTURAL AUDIT & CONSOLIDATED ACTION PLAN

**Date:** 2025-11-01
**Status:** FINALIZED - Ready for Execution

## Executive Summary

This audit, driven by a coverage report from the `zznet-demo` integration test, is now complete. The investigation has successfully identified a clear delta between the "Ground Truth 3.0" architectural vision and the current implementation.

**Key Findings:**
1.  **The Core Architecture Works:** The "happy path" of the new SOLID architecture is functional. The `RoomActor<T>` pattern for type-safe, serialized messaging is correctly implemented and exercised.
2.  **Massive Architectural Remnants:** The codebase is filled with dead code from previous refactoring efforts. Entire modules, actor handlers, and public API methods are unused and obsolete.
3.  **Critical Test Gaps:** The `zznet-demo` is a simplistic "happy path" test. It fails to cover critical lifecycle events (disconnection), error handling, and timeout logic.
4.  **Major Framework Gaps:** The demo completely bypasses two of the most important high-level crates: `zznet-builder` and `zznet-auth`, indicating they are not being validated by this test suite.

This document provides a definitive, file-by-file action plan to purge the architectural remnants and flag the critical test gaps that must be addressed to achieve a clean, robust, and maintainable framework.

---

## PART 1: FILE-BY-FILE AUDIT FINDINGS (FULL DETAIL)

| File | Finding Summary | Decision / Action |
| :--- | :--- | :--- |
| `apps/zznet-demo/src/component_a.rs` | `Handler<StateUpdate>` is uncovered and implies an incorrect bidirectional design. | ✅ **DELETE** this handler. |
| `apps/zznet-demo/src/component_b.rs` | `subscribe_to_component_a`, `get_recipient`, `get_counter`, `Handler<PublishToA>` are all dead or redundant code. | ✅ **DELETE** all four items. |
| `apps/zznet-demo/src/messages.rs` | `RoomMessageTrait::supported_rooms()` is dead code; the feature was never implemented in the framework. | ✅ **DELETE** from trait definition and all implementors. |
| `net/zznet-api/src/error.rs` | `impl From<io::Error>` is uncovered. | ✅ **SKIP.** Benign helper. |
| `net/zznet-api/src/mock.rs` | Advanced features (`inject_error`, `MockServer`) are unused by the simple demo. | ✅ **KEEP.** This is a test gap, not dead code. |
| `net/zznet-api/src/types.rs` | Benign helpers are uncovered. | ✅ **SKIP.** |
| `net/zznet-hello/src/actor.rs` | Multiple issues: untested timeout/error paths, silent failures, and two major pieces of dead code. | ✅ **DELETE** `Handler<InboundRoomMessage>` and `start_hello_actor`. **ADD** `FIXME`/`TODO`s for silent failures and test gaps. |
| `net/zznet-hello/src/connection_manager.rs`| 7 unused handlers/messages (`GetPeers`, `SendToRoom`, etc.) from old data plane API. | ✅ **DELETE** all 7 dead items. |
| `net/zznet-hello/src/error.rs` | Error types uncovered due to happy-path test. | ✅ **SKIP.** |
| `net/zznet-hello/src/handshake.rs` | `is_failed()`, `state()`, `process_ack()` are dead code. Error handling is over-engineered or insufficient. | ✅ **DELETE** dead code, **ADD `warn!`** for duplicate HELLO, **SIMPLIFY** empty intersection logic. |
| `net/zznet-hello/src/protocol.rs` | Well-covered. | ✅ **SKIP.** (Will be modified by `handshake.rs` cleanup). |
| `net/zznet-hello/src/serialize.rs`| Entire module is dead code, superseded by `RoomActor<T>`. | ✅ **DELETE** the file. Move `create_disconnect_frame` logic inline. |
| `net/zznet-hello/src/session_bridge.rs`| Well-covered. | ✅ **SKIP.** |
| `net/zznet-room/src/actor.rs` | Well-covered. | ✅ **SKIP.** |
| `net/zznet-room/src/room_message_trait.rs`| Benign helpers uncovered. | ✅ **SKIP.** |
| `net/zznet-router/src/actor.rs` | 5 unused handlers from obsolete API patterns (`HandlePublishRooms`, `PeerSender`, etc.). `OnPeerDisconnected` is necessary but untested. | ✅ **DELETE** the 5 dead handlers. **KEEP** `OnPeerDisconnected` but add `TODO` for test gap. |
| `net/zznet-router/src/peer_channels.rs`| Most public methods are dead code from the old API. `disconnect()` is necessary but untested. | ✅ **DELETE** all dead methods and the `PeerChannelsTrait` impl. **KEEP** `disconnect()` but add `TODO`. |
| `net/zznet-router/src/router.rs` | Most methods are dead code, being internal helpers for the now-deleted actor messages. `disconnect_peer` is necessary but untested. | ✅ **DELETE** all dead methods. **KEEP** `disconnect_peer` but add `TODO`. |

---

## PART 2: GAPS IDENTIFIED BY MISSING FILES

The following framework crates were **completely absent** from the coverage report, indicating the `zznet-demo` test does not exercise them at all.

| Missing Crate | Analysis (Critical Finding) | Decision / Action |
| :--- | :--- | :--- |
| **`zznet-builder`** | The demo **does not use the intended public API** for building applications. It uses manual, low-level wiring, which fails to validate the primary entry point for developers. | 🟡 **PRIORITY:** The `zznet-demo` test harness must be refactored to use `AppBuilder` to provide realistic coverage. |
| **`zznet-auth`** | The demo **does not use the real authorization framework**. It uses a simplistic, hardcoded `HashSet` of roles, leaving the entire `AclManager` and its supporting logic untested. | 🔵 **DEFERRED:** Per owner request, `zznet-auth` is out of scope for this cleanup. It will be addressed after the core framework is finalized. |
| **`zznet-transport-tcp`** | The demo correctly uses the mock transport by design. | ✅ **EXPECTED.** This is not a flaw. A separate, dedicated test (`connectivity_integration_test.rs`) is responsible for validating the real TCP transport. |

---

## PART 3: CONSOLIDATED ACTION PLAN (FOR EXECUTION)

### Section A: Deletions (Architectural Remnants & Dead Code)

Execute these deletions to remove obsolete code.

1.  **`zznet-demo` Cleanup:**
    *   In `apps/zznet-demo/src/component_a.rs`, **DELETE** the `impl Handler<StateUpdate> for ComponentAActor` block.
    *   In `apps/zznet-demo/src/component_b.rs`, **DELETE** the methods `subscribe_to_component_a`, `get_recipient`, `get_counter`, and the `impl Handler<PublishToA> for ComponentBActor` block.

2.  **`zznet-room` Trait Simplification:**
    *   In `src/net/zznet-room/src/room_message_trait.rs`, **DELETE** the `supported_rooms()` method from the `RoomMessageTrait` definition.
    *   **DELETE** the `supported_rooms()` implementation from all structs that implement this trait (e.g., `ComponentAMessage`, `PingerMessage`, `MemDBMessage`, etc.).

3.  **`zznet-hello` Cleanup:**
    *   **DELETE** the file `src/net/zznet-hello/src/serialize.rs`.
    *   In `src/net/zznet-hello/src/actor.rs`, **DELETE** the function `start_hello_actor` and the `impl Handler<InboundRoomMessage> for HelloActor` block.
    *   In `src/net/zznet-hello/src/session_messages.rs`, **DELETE** the `InboundRoomMessage` struct.
    *   In `src/net/zznet-hello/src/connection_manager.rs`, **DELETE** the following structs and their `Handler` implementations: `HandshakePostProcessed`, `GetPeers`, `GetPeerSender`, `SubscribePeerInbound`, `SendToRoom`.
    *   In `src/net/zznet-hello/src/handshake.rs`, **DELETE** the methods `is_failed`, `state`, and `process_ack`.
    *   In `src/net/zznet-hello/src/protocol.rs`, **DELETE** the `HandshakeFrame::Ack` variant.

4.  **`zznet-router` & `zznet-api` Cleanup:**
    *   In `src/net/zznet-router/src/actor.rs`, **DELETE** the following message structs and their `Handler` implementations: `HandlePublishRooms`, `PeerJoinedRooms`, `IsRoomJoined`, `PeerSender`, `SubscribePeerInbound`.
    *   In `src/net/zznet-router/src/peer_channels.rs`, **DELETE** the methods: `handle_publish_rooms`, `outbound_sender`, `subscribe_inbound`, `send_raw_to_room`, `joined_rooms`, `is_room_joined`.
    *   In `src/net/zznet-router/src/peer_channels.rs`, **DELETE** the entire `impl PeerChannelsTrait for PeerChannels` block.
    *   In `src/net/zznet-api/src/types.rs`, **DELETE** the `PeerChannelsTrait` definition.
    *   In `src/net/zznet-router/src/router.rs`, **DELETE** the methods: `peer_mut`, `peer`, `handle_publish_rooms`, `peer_joined_rooms`, `is_room_joined`, `peer_sender`, `subscribe_peer_inbound`.

### Section B: Modifications & `TODO`s (Flaws & Gaps)

Add these comments and code changes to address identified issues.

1.  **`net/zznet-hello/src/actor.rs`:**
    *   **FIXME:** In `send_frame_to_io`, if `self.io_tx.send()` fails, call `ctx.stop()` immediately to terminate the zombie actor.
    *   **TODO:** In `handle_handshake_frame`, if `self.handshake.process_frame()` returns `Err`, call `self.handle_error()` to terminate the actor.
    *   **TODO:** Add a negative test using `inject_error` on the mock transport to verify that transport send/recv errors cause the `HelloActor` to terminate.
    *   **TODO:** Add a test case that uses a TLS-enabled transport to cover certificate validation logic in `complete_handshake`.
    *   **TODO (Architectural Review):** Add this comment above `handle_error`: `Is sending an Error frame necessary? This adds complexity. Consider simplifying to just log and stop.`

2.  **`net/zznet-hello/src/handshake.rs`:**
    *   In `process_hello`, in the `HandshakeState::Complete` branch, **ADD** a `tracing::warn!` for receiving a duplicate HELLO frame.
    *   In `process_offer`, **MODIFY** the `if intersection.is_empty()` block to log a `warn!` and return an `Err(HelloError::NoCommonRooms)`, removing the logic that creates and sends an `Error` frame.

3.  **`net/zznet-router/`:**
    *   In `src/net/zznet-router/src/actor.rs`, add this **TODO** above the `Handler<OnPeerDisconnected>` impl: `// TODO: Add an integration test that simulates peer disconnection to cover this handler.`
    *   In `src/net/zznet-router/src/peer_channels.rs`, add this **TODO** above the `disconnect` method: `// TODO: This is called by RouterActor::OnPeerDisconnected. An integration test is needed.`
    *   In `src/net/zznet-router/src/router.rs`, add this **TODO** above the `disconnect_peer` method: `// TODO: This is called by RouterActor::OnPeerDisconnected. An integration test is needed.`

