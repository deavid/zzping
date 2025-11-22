//! `ZZNetApplication` implementation for the demo application.
use crate::{
    component_a::{ComponentAActor, ComponentANetworkManager, ComponentAPermissions},
    component_b::ComponentBActor,
    config::DemoAppConfig,
    messages::{SetComponentA, SetNetworkManager, StateUpdate, Subscribe},
};
use actix::prelude::*;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use zznet_api::types::{Role, RoomId};
use zznet_hello::connection_manager::ConnectionManager;
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

        let connection_manager = ConnectionManager::new(
            router.clone().recipient(),
            config.our_role.clone(),
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

        let network_manager = ComponentANetworkManager::new(
            component_a.clone(),
            router.clone(),
            component_a_permissions,
        )
        .start();
        component_a.do_send(SetNetworkManager { network_manager });

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
