# Architectural Review Notes: `src/components/zznet/src/connection_manager.rs`

This document records the findings from the architectural review of `src/components/zznet/src/connection_manager.rs`.

### Summary of Findings

This file is a critical component of the existing `zznet` implementation, but its design is fundamentally at odds with the new architectural vision for `zznet` and the `zzchorale` framework. It embodies the imperative, request-response communication model and uses `oneshot` channels, both of which are explicitly forbidden by the new "Pure Actor Model" and "Declarative Architecture" principles. The file also exhibits several code smells, including a "god function" and a lack of comprehensive documentation.

The entire file requires a complete redesign to align with the new gRPC/Tonic-based "Session Provisioning" model, where inter-process connections are managed declaratively as "Rooms" rather than imperatively requested "Channels."

### List of Deviations and Notes

1.  **Primary Deviation (Architecture): Imperative Channel Request/Response**
    *   **Finding:** The `ConnectionCommand::RequestChannel` variant and the `Connection::request_channel` method implement an imperative request-response pattern for establishing communication channels. This is further evidenced by the `pending_requests` HashMap in `ConnectionActor` and the handling of `ControlMsg::ChannelOpened` (client-side).
    *   **Impact:** This directly violates the "Pure Actor Model (Fire-and-Forget)" principle, which forbids blocking request-response patterns. It also conflicts with the new declarative "Room" architecture for `zznet`, where "Rooms" are provisioned automatically, not requested.
    *   **Principle Violated:** Pure Actor Model; Declarative Architecture for `zznet`.
    *   **Recommendation:** Remove `ConnectionCommand::RequestChannel`, `Connection::request_channel`, `ConnectionActor::pending_requests`, and all associated logic for handling channel requests and responses. The new `zznet` will provision "Rooms" declaratively.

2.  **Primary Deviation (Architecture): Obsolete Serialization and Transport**
    *   **Finding:** The file extensively uses `rmp_serde` for serialization and deserialization of frames, and `tokio::io::{ReadHalf, WriteHalf, split}` for stream handling.
    *   **Impact:** This is part of the custom TCP protocol that is being replaced by gRPC/Tonic and TLS.
    *   **Principle Violated:** Adherence to the new `ZZPing_Network_protocol.md`.
    *   **Recommendation:** This entire serialization and transport layer will be superseded by the gRPC/Tonic implementation.

3.  **Design Smell (Clarity/Maintainability): "God Function" `ConnectionActor::run`**
    *   **Finding:** The `ConnectionActor::run` method is a single, large `async` function containing a `loop` with a `tokio::select!` that handles all incoming commands and network events. This makes it difficult to read, understand, and test individual pieces of logic.
    *   **Impact:** Violates the Single Responsibility Principle and the "Keep It Simple (KISS)" principle. Makes the code hard to maintain and prone to errors.
    *   **Principle Violated:** Single Responsibility Principle; Avoid Deep Nesting ("Arrow Code"); Framework-Enforced Uniformity & Testability.
    *   **Recommendation:** The core `select!` loop logic should be managed by the `zzchorale` framework. Each branch of the `select!` (handling a specific command or network event) should be extracted into its own dedicated, testable function.

4.  **Naming Convention (Clarity): Ambiguous "Channel" Terminology**
    *   **Finding:** The terms `ChannelId`, `ConnectionEvent::ChannelOpened`, and `struct Channel` are used to refer to inter-process communication entities.
    *   **Impact:** This creates confusion with intra-process `zzchorale` channels, violating the "Clear Conventions" principle and the `GEMINI_SESSION_SUMMARY.md` decision to rename inter-process connections to "Rooms."
    *   **Principle Violated:** Clear Conventions; Naming Collision (from `architectural_review/src/common/zznet-lib/src/facade.rs.md`).
    *   **Recommendation:** Rename `ChannelId` to `RoomId`, `ConnectionEvent::ChannelOpened` to `ConnectionEvent::ClientRoomOpened`, and `struct Channel` to `struct ClientRoom`.

5.  **Documentation Note: Missing Docstrings**
    *   **Finding:** The `ConnectionCommand` enum is missing a docstring.
    *   **Impact:** Reduces code clarity and maintainability.
    *   **Principle Violated:** All public items MUST have docstrings (from `AGENT_CODING_STANDARDS.md`).
    *   **Recommendation:** Add a comprehensive docstring to the `ConnectionCommand` enum, explaining its purpose and the role of its variants.

6.  **Design Smell (Testability): `FIXME` comment for `channels_by_name`**
    *   **Finding:** The `ConnectionActor` contains a `FIXME` comment indicating that `channels_by_name` is populated but never used on the client side.
    *   **Impact:** Suggests dead code or an incomplete feature, adding to cognitive load.
    *   **Recommendation:** Re-evaluate the necessity of `channels_by_name` in the context of the new architecture. If it's not needed, remove it. If it is, clarify its purpose.

7.  **Design Smell (Safety): Missing Timeout for `request_channel`**
    *   **Finding:** The `request_channel` method has a `TODO` comment about adding a timeout.
    *   **Impact:** Without a timeout, a `oneshot::Sender` could be held indefinitely, leading to resource leaks and potential deadlocks if the remote peer never responds.
    *   **Recommendation:** While `request_channel` is to be removed, this highlights a general principle: all blocking or waiting operations should have timeouts.

### Conclusion

`src/components/zznet/src/connection_manager.rs` is a prime example of the "snowflake" architecture and imperative design that the new `zznet` and `zzchorale` frameworks aim to replace. Its current form is incompatible with the established architectural principles. The recommended path forward is to completely rewrite this component to adhere to the gRPC/Tonic-based "Session Provisioning" model, leveraging the `zzchorale` framework for actor management and lifecycle.
