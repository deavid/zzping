//! ComponentB implementation - a simple local component.
//!
//! ComponentB demonstrates local subscription to ComponentA's state updates.
//! It has no network-facing actors and only receives local messages.

use crate::messages::{GetCounter, PublishToA, SendPingFromB, SetComponentA, StateUpdate};
use actix::{Actor, Addr, Context, Handler, MessageResult, Recipient};

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

    /// Subscribe to ComponentA's state updates
    pub fn subscribe_to_component_a(
        &self,
        component_a_addr: Addr<super::component_a::ComponentAActor>,
    ) {
        // Send subscription message to ComponentA
        component_a_addr.do_send(super::messages::Subscribe {
            recipient: self.get_recipient(),
        });
    }

    /// Get recipient for state updates
    fn get_recipient(&self) -> Recipient<StateUpdate> {
        // This is a bit tricky in Actix - we need the actor's address
        // In practice, this would be called after the actor is started
        // For now, we'll handle this in the test harness
        unimplemented!("get_recipient should be called from test harness with actor address")
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

    /// Get the current counter value (for testing)
    pub fn get_counter(&self) -> u64 {
        self.counter
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

/// Handle requests from tests/components to publish a local value to ComponentA.
impl Handler<PublishToA> for ComponentBActor {
    type Result = ();

    fn handle(&mut self, msg: PublishToA, _ctx: &mut Self::Context) -> Self::Result {
        tracing::debug!("ComponentBActor: PublishToA({})", msg.data);
        // This is a local trigger, so we just update our own state
        // and forward to ComponentA to handle the network broadcast.
        self.counter += 1;
        self.data = msg.data.clone();

        // Forward to ComponentA if available
        if let Some(ref addr) = self.component_a {
            addr.do_send(StateUpdate {
                counter: self.counter,
                data: self.data.clone(),
            });
        } else {
            tracing::warn!("ComponentBActor: ComponentA addr not set, cannot forward");
        }
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
