//! Generic room actor for centralized (de-)serialization.

use crate::room_message_trait::RoomMessageTrait;
use actix::prelude::*;
use tokio::sync::mpsc;
use zznet_api::messages::InboundRoomPayload;
use zznet_api::protocol::{Frame, RoomFrame};
use zznet_api::types::{RoomId, TransportFrame};

/// Actix actor that centralizes (de-)serialization for a network room.
///
/// `RoomActor<T>` owns the byte boundary for a specific room:
/// - inbound `InboundRoomPayload` bytes are decoded via `RoomMessageTrait`
/// - outbound typed messages are serialized, wrapped in RoomFrame, and sent to transport
pub struct RoomActor<T>
where
    T: RoomMessageTrait + actix::Message<Result = ()> + Send + 'static,
{
    room_id: RoomId,
    transport_tx: mpsc::Sender<TransportFrame>,
    component_recipient: Recipient<T>,
}

impl<T> RoomActor<T>
where
    T: RoomMessageTrait + actix::Message<Result = ()> + Send + 'static,
{
    /// Build a room actor bound to a specific room id and component recipient.
    pub fn new(
        room_id: RoomId,
        transport_tx: mpsc::Sender<TransportFrame>,
        component_recipient: Recipient<T>,
    ) -> Self {
        Self {
            room_id,
            transport_tx,
            component_recipient,
        }
    }
}

impl<T> Actor for RoomActor<T>
where
    T: RoomMessageTrait + actix::Message<Result = ()> + Send + 'static,
{
    type Context = Context<Self>;
}

impl<T> Handler<InboundRoomPayload> for RoomActor<T>
where
    T: RoomMessageTrait + actix::Message<Result = ()> + Send + 'static,
{
    type Result = ();

    fn handle(&mut self, msg: InboundRoomPayload, _ctx: &mut Context<Self>) -> Self::Result {
        match T::deserialize_for_room(&self.room_id, &msg.payload) {
            Ok(typed) => {
                self.component_recipient.do_send(typed);
            }
            Err(error) => {
                tracing::warn!(
                    "RoomActor inbound decode failed for room {}: {}",
                    self.room_id,
                    error
                );
            }
        }
    }
}

impl<T> Handler<T> for RoomActor<T>
where
    T: RoomMessageTrait + actix::Message<Result = ()>,
{
    type Result = ();

    fn handle(&mut self, msg: T, ctx: &mut Context<Self>) -> Self::Result {
        let msg_room_id = msg.room_id();
        if msg_room_id != self.room_id {
            tracing::error!(
                "RoomActor outbound message room mismatch: expected {}, got {}",
                self.room_id,
                msg_room_id
            );
        }

        match msg.serialize_inner() {
            Ok(payload_bytes) => {
                // Wrap the payload in a RoomFrame and then a Frame
                let room_frame = RoomFrame::Message {
                    from_room: self.room_id.as_str().to_string(),
                    to_room: self.room_id.as_str().to_string(),
                    payload: payload_bytes,
                };
                let frame = Frame::Room(room_frame);

                match frame.serialize() {
                    Ok(frame_bytes) => {
                        let transport_tx = self.transport_tx.clone();
                        let transport_frame = TransportFrame::new(frame_bytes);

                        if let Err(error) = transport_tx.try_send(transport_frame) {
                            tracing::error!("RoomActor transport send failed: {:?}", error);
                            ctx.stop();
                            // TODO: Ensure transport connection teardown propagates when this actor stops.
                        }
                    }
                    Err(error) => {
                        tracing::error!(
                            "RoomActor frame serialization failed for room {}: {}",
                            self.room_id,
                            error
                        );
                        ctx.stop();
                    }
                }
            }
            Err(error) => {
                tracing::error!(
                    "RoomActor outbound encode failed for room {}: {}",
                    self.room_id,
                    error
                );
                ctx.stop();
            }
        }
    }
}
