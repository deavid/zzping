# Phase 4 — Cleanup, deprecations, and hardening

Date: 2025-10-30

Objective
- Remove obsolete code paths and finalize the transition to RoomActor<T> + TranslatorActor<T>.
- Ensure components no longer depend on InboundRoomPayload or perform any raw (de-)serialization.
- Consolidate serialization concerns under zznet-room; reduce duplicate helpers.

Preconditions
- Phases 1–3 complete; all components have migrated and tests pass.

Cleanup scope
- Components: delete legacy NetworkActor types (or rename final TranslatorActor files and remove old ones).
- Serialization helpers inside component network_messages.rs: keep only what’s needed for tests or mark them #[cfg(test)], moving runtime (de-)serialization to zznet-room.
- Imports: remove zznet_room::room_manager::InboundRoomPayload from component crates.

Steps
1) Remove legacy network actors
- For each component, delete old NetworkActor structs and their Handler<InboundRoomPayload> impls if any remain.
- Ensure TranslatorActor<T> is the only per-peer actor handling network messages.

2) Prune serialization helpers
- In component network_messages.rs:
  - If serialize_inner()/deserialize_for_room() are only used in tests, gate them with #[cfg(test)].
  - If used in production elsewhere, evaluate moving such use to RoomActor<T> or replacing with actix messages between MainActor and TranslatorActor.

3) API tightening and visibility
- In zznet-room, document RoomActor<T> public API clearly; hide any internal helpers not needed by components.
- Consider adding a small helper builder for RoomActor<T> to reduce repetition in RoomManager impls (optional).

4) Grep and code hygiene
- grep -R "InboundRoomPayload" src/components/** → should be zero.
- grep -R "bincode::decode_from_slice" src/components/** → should be zero (allowed in tests only).
- Remove dead code warnings; run clippy and address warnings that affect runtime behavior.

5) Tests and docs
- Update docs in each component explaining the final architecture.
- Keep/expand integration tests in zznet-router/tests to cover multiple rooms and bidirectional traffic.
- Add a README or rustdoc module in zznet-room demonstrating RoomActor<T> usage.

Acceptance criteria
- No component references InboundRoomPayload or bincode::decode_from_slice in runtime paths.
- Old NetworkActor types removed; TranslatorActor<T> is the only network-facing per-peer actor in components.
- Tests green; clippy clean on critical lints; documentation updated.
