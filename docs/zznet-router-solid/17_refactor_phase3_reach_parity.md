# Phase 3 — Reach parity across all components

Date: 2025-10-30

Objective
- Migrate all remaining components to the new pattern: RoomActor<T> + TranslatorActor<T> + MainActor.
- Achieve feature parity for inbound and outbound across all rooms.
- Keep the workspace compiling green at every step; validate via integration tests.

Preconditions
- Phase 1 complete: RoomActor<T> introduced in zznet-room; RoomManager::create_for_peer accepts outbound_to_peer; RouterActor passes it.
- Phase 2 complete (pilot): zzpinger migrated; translator wired; bincode calls removed from component code paths.

Scope
- Components: zzmem-db, zzcollector-state, zzintent-config.
- Router and transport: no functional changes; use existing outbound/inbound channels.

Per-component migration steps

1) zzmem-db
- Create MemDBTranslatorActor handling Handler<MemDBMessage> and mapping to domain messages (no bytes).
- Update MemDBNetworkManager::create_for_peer to:
  - Construct TranslatorActor and start it; keep Addr.
  - Construct RoomActor<MemDBMessage> with room_id = "mem-db" (adjust to the actual room id), outbound_to_peer, and component_recipient = translator_addr.recipient::<MemDBMessage>().
  - Start RoomActor; return its Recipient<InboundRoomPayload> from create_for_peer.
  - Store RoomActor Addr per peer for outbound sends.
- Remove bincode::decode_from_slice occurrences from component code (translator, manager, legacy network actor).

2) zzcollector-state
- Mirror the zzmem-db steps using CStateMessage and room id (e.g., "collector-state").
- Ensure translator maps network types to domain messages consumed by MainActor.
- Remove bincode usage from component code paths.

3) zzintent-config
- Identify the room id and network message type used; mirror the pattern.
- Implement TranslatorActor<IntentConfigMessage> and wire RoomActor<IntentConfigMessage>.
- Eliminate bincode calls in component code paths.

Router and wiring notes
- RouterActor::OnPeerConnected currently has both outbound_tx and inbound_rx and calls RoomManager::create_for_peer before PeerChannels::build(). Passing outbound_to_peer at creation time is feasible and already used in Phase 1.
- PeerChannelsBuilder continues to accept only Recipient<InboundRoomPayload>; RoomActor<T> provides that recipient and internally holds outbound_to_peer.

Data structures in NetworkManagers
- Maintain per-peer maps for (Addr<TranslatorActor<T>>, Addr<RoomActor<T>>).
- Lifecycle: On peer removal, stop TranslatorActor and allow RoomActor to drop; remove from maps.

Tests to add/update
- Unit tests per translator covering T → domain mapping (and domain → T if applicable).
- Router integration tests (zznet-router/tests/):
  - For each room, feed serialized bytes into inbound_rx and assert MainActor receives the expected domain message via TranslatorActor.
  - For outbound, send a domain-triggered network message via RoomActor<T> and assert bytes appear on outbound_tx (can be captured or mocked).
- Grep checks:
  - No bincode::decode_from_slice in src/components/** except in tests and network_messages.rs.

Acceptance criteria
- All three components fully migrated and working through RoomActor<T> + TranslatorActor<T>.
- Workspace compiles; unit and integration tests pass.
- Inbound path: PeerChannels → RoomActor<T> → TranslatorActor<T> → MainActor verified for each room.
- Outbound path: MainActor → NetworkManager → TranslatorActor<T> → RoomActor<T> → PeerChannels verified for each room.
- Grep check clean for bincode::decode_from_slice in component runtime code.
