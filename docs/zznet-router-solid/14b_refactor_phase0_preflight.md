# Phase 0 — Preflight: RoomActor<T> API and RoomManager signature

Date: 2025-10-30

Purpose
- Lock the contract for a generic `RoomActor<T>` in `zznet-room` so components can depend on it.
- Precisely define the `RoomManager::create_for_peer` signature change to pass the outbound sender.
- Provide a minimal skeleton implementation you can paste into `zznet-room` to get started.

## Contracts (authoritative)

1) RoomManager signature change (zznet-room)
- Current (found in `src/net/zznet-room/src/room_manager.rs`):
  ```rust
  async fn create_for_peer(
      &self,
      peer_id: PeerId,
      permission: Permission,
      room_id: &RoomId,
  ) -> Result<Option<Recipient<InboundRoomPayload>>, CreateError>;
  ```
- New (Phase 1+): add `outbound_to_peer` so the RoomActor can send bytes back via Router wiring.
  ```rust
  use tokio::sync::mpsc;
  use zznet_api::types::{PeerId, Permission, RoomId};
  use actix::Recipient;

  pub struct InboundRoomPayload {
      pub payload: Vec<u8>,
  }

  async fn create_for_peer(
      &self,
      peer_id: PeerId,
      permission: Permission,
      room_id: &RoomId,
      outbound_to_peer: mpsc::Sender<(RoomId, Vec<u8>)>,
  ) -> Result<Option<Recipient<InboundRoomPayload>>, CreateError>;
  ```
  - Outbound sender type matches `PeerChannels::outbound_sender()` and `OnPeerConnected` fields: `mpsc::Sender<(RoomId, Vec<u8>)>`.
  - Router passes this when invoking managers during `OnPeerConnected`.

2) RoomActor<T> API (zznet-room)
- Goal: centralize (de-)serialization; keep components typed.
- Type bounds and responsibilities:
  - `T: RoomMessageTrait + actix::Message<Result = ()> + Clone + Send + 'static`
  - Inbound: handle `InboundRoomPayload` → decode via `T::deserialize_for_room(&room_id, &bytes)` → forward `T` to component `Recipient<T>`.
  - Outbound: handle typed `T` → serialize via `t.serialize_inner()` → send `(room_id, bytes)` to `outbound_to_peer`.
- Minimal public API surface:
  ```rust
  pub struct RoomActor<T: RoomMessageTrait + actix::Message<Result = ()> + Clone + Send + 'static> {
      room_id: zznet_api::types::RoomId,
      outbound_tx: tokio::sync::mpsc::Sender<(zznet_api::types::RoomId, Vec<u8>)>,
      component_recipient: actix::Recipient<T>,
  }

  impl<T: RoomMessageTrait + actix::Message<Result = ()> + Clone + Send + 'static> actix::Actor for RoomActor<T> {
      type Context = actix::Context<Self>;
  }
  ```

## Paste-ready skeleton (zznet-room)

Add a new module `room_actor.rs` under `src/net/zznet-room/` and re-export it from `lib.rs`.

```rust
// src/net/zznet-room/src/room_actor.rs
use actix::prelude::*;
use tokio::sync::mpsc;
use zznet_api::types::RoomId;
use crate::room_manager::InboundRoomPayload;
use crate::room_message_trait::RoomMessageTrait;

pub struct RoomActor<T>
where
    T: RoomMessageTrait + Message<Result = ()> + Clone + Send + 'static,
{
    room_id: RoomId,
    outbound_tx: mpsc::Sender<(RoomId, Vec<u8>)>,
    component_recipient: Recipient<T>,
}

impl<T> RoomActor<T>
where
    T: RoomMessageTrait + Message<Result = ()> + Clone + Send + 'static,
{
    pub fn new(
        room_id: RoomId,
        outbound_tx: mpsc::Sender<(RoomId, Vec<u8>)>,
        component_recipient: Recipient<T>,
    ) -> Self {
        Self { room_id, outbound_tx, component_recipient }
    }
}

impl<T> Actor for RoomActor<T>
where
    T: RoomMessageTrait + Message<Result = ()> + Clone + Send + 'static,
{
    type Context = Context<Self>;
}

impl<T> Handler<InboundRoomPayload> for RoomActor<T>
where
    T: RoomMessageTrait + Message<Result = ()> + Clone + Send + 'static,
{
    type Result = ();

    fn handle(&mut self, msg: InboundRoomPayload, _ctx: &mut Context<Self>) -> Self::Result {
        match T::deserialize_for_room(&self.room_id, &msg.payload) {
            Ok(typed) => {
                self.component_recipient.do_send(typed);
            }
            Err(e) => {
                tracing::warn!("RoomActor inbound decode failed for room {}: {}", self.room_id, e);
            }
        }
    }
}

impl<T> Handler<T> for RoomActor<T>
where
    T: RoomMessageTrait + Message<Result = ()> + Clone + Send + 'static,
{
    type Result = ();

    fn handle(&mut self, msg: T, _ctx: &mut Context<Self>) -> Self::Result {
        match msg.serialize_inner() {
            Ok(bytes) => {
                // Non-async send to keep Handler synchronous; warn if channel is full/closed
                if let Err(e) = self.outbound_tx.try_send((self.room_id.clone(), bytes)) {
                    tracing::warn!("RoomActor outbound try_send failed for room {}: {:?}", self.room_id, e);
                }
            }
            Err(e) => {
                tracing::warn!("RoomActor outbound encode failed for room {}: {}", self.room_id, e);
            }
        }
    }
}
```

And in `lib.rs`:
```rust
// src/net/zznet-room/src/lib.rs
pub mod room_actor; // add
pub use room_actor::RoomActor; // re-export
```

## Router call-site adjustment

In `zznet-router/src/actor.rs`, when iterating managers inside `OnPeerConnected` handler, pass the `outbound_tx` to `create_for_peer`:

```rust
// Pseudocode where the signature is updated
if let Ok(Some(room_recipient)) = manager
    .create_for_peer(peer_id.clone(), permission.clone(), &room_id, outbound_tx.clone())
    .await
{
    builder.add_room(room_id.clone(), room_recipient)?;
}
```

This is feasible because `OnPeerConnected` already has `outbound_tx` prior to calling `PeerChannelsBuilder::build(...)`.

## Component manager usage (example)

In a component’s `NetworkManager::create_for_peer`, construct both translator and room actor, then return the `Recipient<InboundRoomPayload>` exposed by the room actor:

```rust
// inside component NetworkManager::create_for_peer (new signature)
let translator_addr = TranslatorActor::new(self.main_actor.clone()).start();

let room_actor = zznet_room::RoomActor::<PingerMessage>::new(
    RoomId::from("pinger"),
    outbound_to_peer.clone(),
    translator_addr.recipient::<PingerMessage>(),
)
.start();

let recipient = room_actor.recipient::<InboundRoomPayload>();
Ok(Some(recipient))
```

Outbound sends later can use `room_actor.do_send(PingerMessage::...)` via a per-peer map stored in the NetworkManager.

## Success criteria (for Phase 1 readiness)
- Compiles after adding `RoomActor<T>` and the new `create_for_peer` parameter.
- Router builds `PeerChannels` and registers peers as before; only the manager signature changes.
- A smoke test can send an inbound serialized frame `(room_id, bytes)` and observe the translator receive typed `T`.
- A smoke test can `do_send(T)` to the RoomActor and observe bytes arrive on `outbound_tx`.

## Notes and constraints
- Using `try_send` keeps `Handler<T>` synchronous; if backpressure is likely, consider switching to an async handler pattern (ResponseActFuture) to `await` on `send`.
- `T` must implement `actix::Message<Result = ()>` so that it can be sent to both the RoomActor and the TranslatorActor via Actix messaging.
- `InboundRoomPayload` remains the router-facing message type and is not used by component translators.
