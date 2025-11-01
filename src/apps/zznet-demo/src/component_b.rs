//! ComponentB implementation - a simple local component.
//!
//! ComponentB demonstrates local subscription to ComponentA's state updates.
//! It has no network-facing actors and only receives local messages.

use crate::messages::{GetCounter, PublishToA, SendPingFromB, SetComponentA, StateUpdate};
use actix::{Actor, Addr, Context, Handler, MessageResult};

/// MainActor for ComponentB - handles local state updates.
#[derive(Debug, Default)]
pub struct ComponentBActor {
    /// Current counter value (mirrors ComponentA)
    counter: u64,
    /// Additional data field (mirrors ComponentA)
    data: String,
    /// Optional address of the local ComponentA (used to publish local changes)
    component_a: Option<Addr<super::component_a::ComponentAActor>>,
}

impl ComponentBActor {
    /// Create a new ComponentBActor
    pub fn new() -> Self {
        Self {
            counter: 0,
            data: String::new(),
            component_a: None,
        }
    }

    /// Internal helper to set the ComponentA address
    fn set_component_a_addr(&mut self, addr: Addr<super::component_a::ComponentAActor>) {
        self.component_a = Some(addr);
    }

    /// Update internal state from a received StateUpdate message
    fn update_state(&mut self, state: StateUpdate) {
        self.counter = state.counter;
        self.data = state.data;
    }
}

impl Actor for ComponentBActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        tracing::debug!("ComponentBActor started");
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        tracing::debug!("ComponentBActor stopped");
    }
}

/// Handle setting the ComponentA address so ComponentB can forward local changes.
impl Handler<SetComponentA> for ComponentBActor {
    type Result = ();

    fn handle(&mut self, msg: SetComponentA, _ctx: &mut Self::Context) {
        tracing::debug!("ComponentBActor: SetComponentA received");
        self.set_component_a_addr(msg.component_a);
    }
}

impl Handler<SendPingFromB> for ComponentBActor {
    type Result = ();

    fn handle(&mut self, msg: SendPingFromB, _ctx: &mut Self::Context) {
        if let Some(addr) = &self.component_a {
            // Tell our local ComponentA to publish a message, which will be sent over the network
            addr.do_send(PublishToA { data: msg.data });
        }
    }
}

/// Handle state updates from ComponentA
impl Handler<StateUpdate> for ComponentBActor {
    type Result = ();

    fn handle(&mut self, msg: StateUpdate, _ctx: &mut Self::Context) {
        self.update_state(msg);
    }
}

impl Handler<GetCounter> for ComponentBActor {
    type Result = MessageResult<GetCounter>;

    fn handle(&mut self, _msg: GetCounter, _ctx: &mut Self::Context) -> Self::Result {
        MessageResult(self.counter)
    }
}
