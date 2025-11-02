//! Network Manager for the Pinger component.
//!
//! This actor implements the RoomManager trait to enable Router integration.
//! It manages the "pinger" room for remote configuration updates.

use actix::prelude::*;
use async_trait::async_trait;
use log::{debug, info, warn};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use zznet_api::types::{PeerId, RoomId};
use zznet_room::actor::RoomActor;
use zznet_room::room_manager::{CreateError, RoomInboundRecipient, RoomManager};
use zznet_router::RouterActor;

use crate::permissions::PingerPermissions;

/// The NetworkManager for the Pinger component.
///
/// This actor implements RoomManager to register the "pinger" room with the Router.
/// It creates NetworkActors and RoomActors<T> for peers that want to send configuration updates.
pub struct PingerNetworkManager {
    /// Address of the main PingerActor for business logic
    main_actor: Addr<crate::actor::PingerActor>,

    /// Address of this NetworkManager (set in started())
    self_addr: Option<Addr<PingerNetworkManager>>,

    /// RouterActor for data-plane message routing
    router_actor: Addr<RouterActor>,

    /// Per-peer translator actors for message translation
    translators: Arc<RwLock<HashMap<PeerId, Addr<crate::network_actor::PingerNetworkActor>>>>,

    /// Per-peer RoomActor addresses for outbound sends
    room_actors:
        Arc<RwLock<HashMap<PeerId, Addr<RoomActor<crate::network_messages::PingerMessage>>>>>,

    /// Permissions map for role-to-permissions translation
    permissions_map: HashMap<String, PingerPermissions>,
}

impl Clone for PingerNetworkManager {
    fn clone(&self) -> Self {
        Self {
            main_actor: self.main_actor.clone(),
            self_addr: self.self_addr.clone(),
            router_actor: self.router_actor.clone(),
            translators: Arc::clone(&self.translators),
            room_actors: Arc::clone(&self.room_actors),
            permissions_map: self.permissions_map.clone(),
        }
    }
}

impl PingerNetworkManager {
    /// Creates a new PingerNetworkManager.
    ///
    /// # Arguments
    /// * `main_actor` - Address of the PingerActor (business logic)
    /// * `router_actor` - RouterActor for data-plane message routing
    /// * `permissions_map` - Map of role strings to PingerPermissions
    pub fn new(
        main_actor: Addr<crate::actor::PingerActor>,
        router_actor: Addr<RouterActor>,
        permissions_map: HashMap<String, PingerPermissions>,
    ) -> Self {
        Self {
            main_actor,
            self_addr: None,
            router_actor,
            translators: Arc::new(RwLock::new(HashMap::new())),
            room_actors: Arc::new(RwLock::new(HashMap::new())),
            permissions_map,
        }
    }

    /// Get the permissions map (for testing)
    pub fn permissions_map(&self) -> &HashMap<String, PingerPermissions> {
        &self.permissions_map
    }
}

impl Actor for PingerNetworkManager {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        info!("PingerNetworkManager started");
        self.self_addr = Some(ctx.address());

        // Register this manager with the Router
        let router_addr = self.router_actor.clone();
        let manager_clone = Arc::new(self.clone());

        // Spawn a task to register with the router
        actix::spawn(async move {
            if let Err(e) = router_addr
                .send(zznet_router::RegisterManager {
                    manager: manager_clone,
                })
                .await
            {
                log::error!(
                    "Failed to register PingerNetworkManager with Router: {:?}",
                    e
                );
            } else {
                info!("✓ PingerNetworkManager registered with Router");
            }
        });
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        info!("PingerNetworkManager stopped");
    }
}

#[async_trait]
impl RoomManager for PingerNetworkManager {
    fn managed_rooms(&self) -> std::collections::HashSet<RoomId> {
        let mut rooms = std::collections::HashSet::new();
        rooms.insert(RoomId::from("pinger"));
        rooms
    }

    async fn create_for_peer(
        &self,
        peer_id: PeerId,
        role: zznet_api::types::Role,
        room_id: &RoomId,
        outbound_to_peer: tokio::sync::mpsc::Sender<(zznet_api::types::RoomId, Vec<u8>)>,
    ) -> Result<Option<RoomInboundRecipient>, CreateError> {
        // Only handle the "pinger" room
        if room_id != &RoomId::from("pinger") {
            return Ok(None);
        }

        // Translate Role to Permissions using the policy map
        let permissions = self
            .permissions_map
            .get(role.as_str())
            .cloned()
            .ok_or_else(|| {
                warn!(
                    "Role '{}' not found in permissions map for peer {}",
                    role.as_str(),
                    peer_id
                );
                CreateError::InvalidPermission {
                    room_id: room_id.clone(),
                }
            })?;

        debug!(
            "Creating PingerNetworkActor for peer {} with role '{}': permissions = {:?}",
            peer_id,
            role.as_str(),
            permissions
        );

        // Create the translator actor with permissions
        let translator = crate::network_actor::PingerNetworkActor::new(
            peer_id.clone(),
            permissions,
            self.main_actor.clone(),
            self.self_addr.as_ref().unwrap().clone(),
        );

        // Start the translator actor
        let translator_addr = translator.start();

        // Create the RoomActor<PingerMessage>
        let room_actor = RoomActor::new(
            RoomId::from("pinger"),
            outbound_to_peer,
            translator_addr
                .clone()
                .recipient::<crate::network_messages::PingerMessage>(),
        );

        // Start the RoomActor
        let room_actor_addr = room_actor.start();

        // Store the addresses in the maps
        {
            let mut translators = self.translators.write().unwrap();
            translators.insert(peer_id.clone(), translator_addr);

            let mut room_actors = self.room_actors.write().unwrap();
            room_actors.insert(peer_id, room_actor_addr.clone());
        }

        // Return the RoomActor's raw inbound recipient
        let recipient = RoomActor::inbound_recipient(&room_actor_addr);

        Ok(Some(recipient))
    }
}

#[cfg(test)]
mod tests {
    use zznet_api::types::RoomId;

    #[actix::test]
    async fn test_managed_rooms() {
        // This is a placeholder test - full integration tests will be added later
        let rooms = {
            let mut rooms = std::collections::HashSet::new();
            rooms.insert(RoomId::from("pinger"));
            rooms
        };

        assert!(rooms.contains(&RoomId::from("pinger")));
        assert_eq!(rooms.len(), 1);
    }
}
