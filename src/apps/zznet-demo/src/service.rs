//! `ZZNetApplication` implementation for the demo application.
use crate::{
    component_a::{ComponentAActor, ComponentAPermissions},
    component_a_spec::ComponentASpec,
    component_b::ComponentBActor,
    config::DemoAppConfig,
    messages::{SetComponentA, StateUpdate, Subscribe},
};
use actix::prelude::*;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use zznet_api::{Role, RoomId};
use zznet_hello::ConnectionManager;
use zznet_router::RouterActor;

/// Demo service that implements `ZZNetApplication`.
pub struct DemoAppService {
    /// The router actor.
    pub router: Addr<RouterActor>,
    /// The connection manager actor.
    pub connection_manager: Addr<ConnectionManager>,
    /// The ComponentA actor.
    pub component_a: Addr<ComponentAActor>,
    /// The ComponentB actor.
    pub component_b: Option<Addr<ComponentBActor>>,
}

impl DemoAppService {
    /// Create a new demo service from configuration.
    pub fn new(config: DemoAppConfig) -> Result<Self> {
        let router = RouterActor::new(
            config
                .offered_rooms
                .iter()
                .map(|r| RoomId::from(r.as_str()))
                .collect(),
        )
        .start();

        let allowed_roles: HashSet<Role> =
            config.allowed_roles.iter().map(|r| Role::new(r)).collect();

        let hello_config = zznet_hello::HelloConfig {
            hostname: "demo".to_string(),
            our_role: config.our_role.clone(),
            offered_rooms: config.offered_rooms.clone(),
            handshake_timeout: std::time::Duration::from_secs(30),
        };

        let connection_manager = ConnectionManager::new(
            router.clone().recipient(),
            hello_config,
            allowed_roles,
        )
        .start();

        let component_a = ComponentAActor::new().start();

        // Create permissions policy for ComponentA (demo: allow all roles full access)
        let mut component_a_permissions = HashMap::new();
        component_a_permissions.insert("client".to_string(), ComponentAPermissions::full_access());
        component_a_permissions.insert("server".to_string(), ComponentAPermissions::full_access());
        // Add test roles used in integration tests
        component_a_permissions.insert("app1".to_string(), ComponentAPermissions::full_access());
        component_a_permissions.insert("app2".to_string(), ComponentAPermissions::full_access());
        component_a_permissions.insert(
            "collector".to_string(),
            ComponentAPermissions::full_access(),
        );
        component_a_permissions
            .insert("database".to_string(), ComponentAPermissions::full_access());

        // Get event bus from ComponentAActor via message
        let component_a_clone = component_a.clone();
        let router_clone = router.clone();
        actix::spawn(async move {
            if let Ok(event_bus) = component_a_clone.send(crate::messages::GetEventBus).await {
                let _network_manager =
                    zznet_component::GenericNetworkManager::<ComponentASpec>::new(
                        component_a_clone,
                        router_clone,
                        event_bus,
                        component_a_permissions,
                    )
                    .start();
            }
        });

        let component_b = if config.include_component_b {
            let comp_b = ComponentBActor::new().start();
            component_a.do_send(Subscribe {
                recipient: comp_b.clone().recipient::<StateUpdate>(),
            });
            comp_b.do_send(SetComponentA {
                component_a: component_a.clone(),
            });
            Some(comp_b)
        } else {
            None
        };

        Ok(Self {
            router,
            connection_manager,
            component_a,
            component_b,
        })
    }
}
