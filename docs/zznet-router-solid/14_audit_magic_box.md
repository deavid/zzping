# Audit: The "Magic Box" and Serialization Ownership

Date: 2025-10-30

This report validates the three audit tasks regarding the existence of a generic room actor, and where (de-)serialization currently lives in the codebase.

## Audit Task 1: Existence of a generic "Magic Box" actor in zznet-room

Key question: Does `zznet-room` provide a generic, reusable actor (e.g., `RoomActor<T>`) that owns serialization/deserialization?

Finding: No generic `RoomActor<T>` exists. The crate provides a typed utility `Room<T>` that is not an Actix actor. It manages channels and spawns a Tokio task that performs deserialization and forwards the typed message to a provided `Recipient<T>`.

Evidence:
- File: `src/net/zznet-room/src/room.rs`
	- `pub struct Room<T> { ... }` (not an actor)
	- Outbound serialization: `Room::<T>::send()` uses `bincode::serde::encode_to_vec`.
	- Inbound deserialization: `Room::<T>::handle_message()` uses `bincode::serde::decode_from_slice` and then `handler.send(msg).await`.
	- The inbound task is spawned in the constructor and processes `Vec<u8>` into `T` via `handle_message`.

Conclusion: The hypothesized "Magic Box" actor does not exist. Serialization/deserialization is handled in `Room<T>`'s background task and by component NetworkActors (see Task 2).

## Audit Task 2: Where does (de-)serialization actually happen?

Key question: Which actor currently calls `bincode::decode_from_slice` for inbound network messages?

Finding: Deserialization is performed in each component's per-peer `NetworkActor` within `Handler<InboundRoomPayload>`, not in a central generic actor.

Evidence (non-exhaustive):
- `src/components/zzpinger/src/network_actor.rs`
	- `impl Handler<InboundRoomPayload> for PingerNetworkActor { ... }`
	- Deserialization: `bincode::decode_from_slice(&msg.payload, bincode::config::standard())`
- `src/components/zzmem-db/src/network_actor.rs`
	- Same pattern: decode payload to `MemDBMessage`
- `src/components/zzcollector-state/src/network_actor.rs`
	- Same pattern: decode payload to `CStateMessage`
- `src/net/zznet-room/src/room.rs` also uses `decode_from_slice` but that’s for the internal `Room<T>` channel path and unit tests, not an actor.

Conclusion: The hypothesized deviation is confirmed. Deserialization is owned by each component’s `NetworkActor` (and by `Room<T>` for its typed-recipient path), not by a generic `RoomActor<T>` in `zznet-room`.

## Audit Task 3: What message type does a component MainActor handle?

Key question: What message type does a component’s `MainActor` handle for network communication?

Finding: Main actors handle typed business messages `T` forwarded by their `NetworkActor` (or NetworkManager). Example: `PingerActor` handles `UpdateTargets` directly.

Evidence:
- `src/components/zzpinger/src/actor.rs`
	- `impl Handler<UpdateTargets> for PingerActor { ... }`
	- Its `PingerNetworkActor` translates inbound `PingerMessage::UpdateTargets` into `UpdateTargets` and `do_send`s it to `PingerActor`.

Conclusion: This aligns with the expectation that MainActors handle typed domain messages, with protocol (de-)serialization and translation isolated in NetworkActors.

## Summary

- No generic `RoomActor<T>` exists in `zznet-room`. Instead, `Room<T>` is a channel-based utility that performs (de-)serialization in a background task for its local `Recipient<T>`.
- In the router-driven flow, inbound `(RoomId, Vec<u8>)` is delivered as `InboundRoomPayload` to each component’s `NetworkActor`, which performs `bincode::decode_from_slice` and forwards a typed message to its MainActor.
- MainActors (e.g., `PingerActor`) handle typed domain messages, not raw network payloads.

Notes and optional next steps:
- If a single "Magic Box" actor is desired, a reusable `RoomActor<T>` could be introduced in `zznet-room` to centralize (de-)serialization for the router path. Today, this logic is intentionally pushed to component `NetworkActor`s to keep protocol translation near component boundaries.

