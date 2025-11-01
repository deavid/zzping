//! Framework-level integration tests for zznet.
//!
//! These tests exercise the zznet framework by manually wiring two complete
//! application stacks that communicate over mock connections. This provides
//! a lower-level validation of the core routing and negotiation logic,
//! separate from the builder-based integration tests.

use actix::prelude::*;
use std::collections::HashSet;
use zznet_api::mock::create_mock_pair;
use zznet_api::types::{Role, RoomId};
use zznet_hello::actor::HelloConfig;
use zznet_hello::connection_manager::ConnectionManager;
use zznet_hello::connection_manager::HandleTransport;
use zznet_router::RouterActor;

use zznet_demo::component_a::{ComponentAActor, ComponentANetworkManager};
use zznet_demo::component_b::ComponentBActor;
use zznet_demo::messages::{
    GetCounter, PublishToA, SendPing, SendPingFromB, SetComponentA, SetNetworkManager, StateUpdate,
    Subscribe,
};

/// Test harness representing a complete zznet application stack.
pub struct AppStack {
    /// Connection manager for HELLO handshake
    connection_manager: Addr<ConnectionManager>,
    /// ComponentA instance
    component_a: Addr<ComponentAActor>,
    /// Optional ComponentB instance
    component_b: Option<Addr<ComponentBActor>>,
}
impl AppStack {
    /// Create a new application stack with the specified role and optional ComponentB.
    pub async fn new(our_role: &str, include_component_b: bool) -> Self {
        Self::new_with_roles(our_role, include_component_b, vec!["collector", "database"]).await
    }

    /// Create a new application stack with specific allowed roles for testing
    pub async fn new_with_roles(
        our_role: &str,
        include_component_b: bool,
        allowed_role_names: Vec<&str>,
    ) -> Self {
        // Create the core actors
        let router = RouterActor::new(vec![RoomId::from("room-a")]).start();

        // Create allowed roles set
        let mut allowed_roles = HashSet::new();
        for role_name in allowed_role_names {
            allowed_roles.insert(Role::new(role_name));
        }

        // Create connection manager
        let connection_manager =
            ConnectionManager::new(router.clone(), our_role.to_string(), allowed_roles.clone())
                .start();

        // Create ComponentA
        let component_a = ComponentAActor::new().start();

        // Create ComponentA's network manager and wire it
        let network_manager =
            ComponentANetworkManager::new(component_a.clone(), router.clone()).start();
        component_a.do_send(SetNetworkManager {
            network_manager: network_manager.clone(),
        });

        // Create ComponentB if requested
        let component_b = if include_component_b {
            let comp_b = ComponentBActor::new().start();
            // Subscribe ComponentB to ComponentA
            component_a.do_send(Subscribe {
                recipient: comp_b.clone().recipient::<StateUpdate>(),
            });
            // Inform ComponentB of ComponentA's address so it can publish to A
            comp_b.do_send(SetComponentA {
                component_a: component_a.clone(),
            });
            Some(comp_b)
        } else {
            None
        };

        Self {
            connection_manager,
            component_a,
            component_b,
        }
    }

    /// Connect this stack to another stack using mock transport.
    pub async fn connect_to(&mut self, other: &mut AppStack) {
        // Create mock connection pair
        let (conn_a, conn_b) = create_mock_pair("test");

        // Get the actual roles from the stacks
        let our_role = self
            .connection_manager
            .send(zznet_hello::messages::GetRole)
            .await
            .unwrap();
        let other_role = other
            .connection_manager
            .send(zznet_hello::messages::GetRole)
            .await
            .unwrap();

        // Create HELLO configs for both sides using actual roles
        let hello_config_a = HelloConfig {
            our_role: our_role.clone(),
            offered_rooms: vec!["room-a".to_string()],
            ..Default::default()
        };

        let hello_config_b = HelloConfig {
            our_role: other_role.clone(),
            offered_rooms: vec!["room-a".to_string()],
            ..Default::default()
        };

        // Send transport to both connection managers
        self.connection_manager.do_send(HandleTransport {
            transport: Box::new(conn_a),
            config: hello_config_a,
        });

        other.connection_manager.do_send(HandleTransport {
            transport: Box::new(conn_b),
            config: hello_config_b,
        });

        // Give some time for the handshake to complete
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[actix::test]
    async fn ping_pong_between_component_a() {
        // Create two app stacks
        let mut stack_a = AppStack::new("collector", false).await;
        let mut stack_b = AppStack::new("database", false).await;

        // Connect them
        stack_a.connect_to(&mut stack_b).await;

        // Send a ping from A to B
        let ping_data = "Hello from A".to_string();
        stack_a.component_a.do_send(SendPing {
            data: ping_data.clone(),
        });

        // Wait for the message to be processed
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Check that component A in stack B received the ping
        let counter = stack_b.component_a.send(GetCounter).await.unwrap();
        assert_eq!(counter, 1);
    }

    #[actix::test]
    async fn component_a_publishes_to_component_b() {
        // Create two app stacks, one with component B
        let mut stack_a = AppStack::new("collector", false).await;
        let mut stack_b = AppStack::new("database", true).await;

        // Connect them
        stack_a.connect_to(&mut stack_b).await;

        // Send a message that will cause A to publish its state
        let publish_data = "State update from A".to_string();
        stack_a.component_a.do_send(PublishToA {
            data: publish_data.clone(),
        });

        // Wait for the message to be processed and published
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Check that component B in stack B received the state update
        let counter_b = stack_b
            .component_b
            .as_ref()
            .unwrap()
            .send(GetCounter)
            .await
            .unwrap();
        assert_eq!(counter_b, 1);
    }

    #[actix::test]
    async fn component_b_sends_message_via_component_a() {
        // Create two app stacks, one with component B
        let mut stack_a = AppStack::new("collector", false).await;
        let mut stack_b = AppStack::new("database", true).await;

        // Connect them
        stack_a.connect_to(&mut stack_b).await;

        // Tell component B on stack B to send a message.
        // This will go B -> A (local) on stack B, then A (stack B) -> A (stack A) (network)
        let ping_from_b_data = "Hello from B via A".to_string();
        stack_b
            .component_b
            .as_ref()
            .unwrap()
            .do_send(SendPingFromB {
                data: ping_from_b_data.clone(),
            });

        // Wait for the message to traverse the stacks
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Check that component A in stack A received the message
        let counter_a = stack_a.component_a.send(GetCounter).await.unwrap();
        assert_eq!(counter_a, 1);

        // Also check that Component A on Stack B, which originated the network message,
        // also updated its own state.
        let counter_a_stack_b = stack_b.component_a.send(GetCounter).await.unwrap();
        assert_eq!(counter_a_stack_b, 1);
    }

    #[actix::test]
    async fn unauthorized_connection_is_rejected() {
        // Create two application stacks with incompatible roles
        let mut stack1 = AppStack::new_with_roles("collector", false, vec!["collector"]).await;
        let mut stack2 = AppStack::new_with_roles("attacker", false, vec!["database"]).await;

        // Attempt to connect them
        stack1.connect_to(&mut stack2).await;

        // Give time for handshake to fail
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        // Verify that a message sent from stack 1 does not arrive at stack 2.
        stack1.component_a.do_send(SendPing {
            data: "should not be received".to_string(),
        });
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let stack2_counter = stack2.component_a.send(GetCounter).await.unwrap();
        assert_eq!(
            stack2_counter, 0,
            "Stack 2 should not have received the message due to authorization failure"
        );
    }
}
