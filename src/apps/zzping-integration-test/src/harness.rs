//! Utilities for building deterministic collector/database stacks in tests.

use actix::prelude::*;
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use zzcollector_state::{
    CStateActor, CStateBuilder, CStateConfig, CStatePermissions, SetPinger, SetTcpLock,
};

use std::net::IpAddr;
use zzmem_db::{
    actor::MemDBActor,
    builder::MemDBBuilder,
    config::MemDBConfig,
    messages::{GetHealth, MemDBHealth},
};
use zznet_api::{
    EstablishedConnection, KillSwitch, MockClient, ReconnectConfig, TransportServer,
    create_controlled_pair, maintain_connection, serve_connections,
};
use zzpinger::PingerBuilder;
use zzpinger::SpawnStrategy;
use zzpinger::UpdateCState;
use zzpinger::UpdateIntentConfig;
use zzstorage::actor::{GetStoredBlobs, StorageActor, StorageConfig};
use zztcp_lock::actor::TcpLockActor;

/// A simple TransportServer implementation that yields connections pushed
/// into an internal queue. Tests can send `EstablishedConnection`s into the
/// sender to simulate incoming connections.
pub struct ControlledServer {
    rx: mpsc::Receiver<EstablishedConnection>,
}

impl ControlledServer {
    /// Wrap an incoming connection queue so tests can push server-side sockets
    /// into a controllable `TransportServer` implementation.
    pub fn new(rx: mpsc::Receiver<EstablishedConnection>) -> Self {
        Self { rx }
    }
}

#[async_trait::async_trait]
impl TransportServer for ControlledServer {
    async fn accept(&mut self) -> Result<EstablishedConnection, zznet_api::TransportError> {
        match self.rx.recv().await {
            Some(conn) => Ok(conn),
            None => Err(zznet_api::TransportError::ConnectionClosed(
                std::io::Error::new(std::io::ErrorKind::BrokenPipe, "controlled server closed"),
            )),
        }
    }
}

/// Configuration for the SystemHarness.
#[derive(Default)]
pub struct HarnessConfig {
    /// If Some, enables the CState/TcpLock actors and binds to the given port.
    pub lock_port: Option<u16>,
    /// The collector ID to use when instantiating the `CStateActor`.
    pub collector_id: String,
    /// If provided, this harness will NOT spawn a database.
    /// Instead, it will connect its collector to this existing server input.
    pub existing_db_server: Option<mpsc::Sender<EstablishedConnection>>,
}

/// SystemHarness sets up a minimal collector+database environment for tests.
/// It provides control over transport via a KillSwitch and MockClient queueing.
pub struct SystemHarness {
    /// Client-side transport FIFO used by `maintain_connection`
    pub mock_client: Arc<MockClient>,
    /// The sender for the server-side transport.
    pub server_sender: mpsc::Sender<EstablishedConnection>,
    kill_switch: Option<KillSwitch>,
    /// Address of the collector MemDB actor (we query it for buffered results)
    pub coll_memdb: actix::Addr<MemDBActor>,
    /// Address of the database MemDB actor
    pub db_memdb: actix::Addr<MemDBActor>,
    /// Pinger scheduler address so we can configure intent
    pub scheduler: actix::Addr<zzpinger::PingerSchedulerActor>,
    /// Address of the storage actor
    pub storage: actix::Addr<StorageActor>,
    /// The collector's CStateActor address.
    pub cstate: Option<Addr<CStateActor>>,
    /// The database's CStateActor address.
    pub db_cstate: Option<Addr<CStateActor>>,
}

impl SystemHarness {
    /// Spawn core actors and start client+server lifecycles. Returns an instance
    /// that allows tests to sever/restore the transport and configure pinger.
    pub async fn new(config: HarnessConfig) -> Result<Self> {
        let (server_sender, db_memdb, storage, db_cstate) = if let Some(tx) =
            config.existing_db_server
        {
            // SATELLITE MODE: We are connecting to an existing DB.
            let storage = StorageActor::new(StorageConfig::Ephemeral).start();
            let db_memdb = MemDBBuilder::new(MemDBConfig::for_database(1, None)).build();
            (tx, db_memdb, storage, None)
        } else {
            // PRIMARY MODE: Spawn the full Database stack.
            let db_router = zznet_router::RouterActor::new(vec![]).start();

            let storage = StorageActor::new(StorageConfig::Ephemeral).start();
            let db_memdb_builder = MemDBBuilder::new(MemDBConfig::for_database(10000, None))
                .with_storage_actor(storage.clone());
            let db_memdb = db_memdb_builder.router(db_router.clone()).build();

            let db_intent_builder = zzintent_config::IntentConfigBuilder::new()
                .config_for_database(std::path::PathBuf::from("/tmp/test_intent.ron"));
            let _db_intent = db_intent_builder.router(db_router.clone()).start()?;

            let db_cstate = if config.lock_port.is_some() {
                let cstate_config = CStateConfig::for_database(1000, Some(10));
                // Allow collectors to send heartbeats to the database
                let mut permissions_map = HashMap::new();
                permissions_map.insert("collector".to_string(), CStatePermissions::for_collector());
                Some(
                    CStateBuilder::new(cstate_config)
                        .router(db_router.clone())
                        .permissions_map(permissions_map)
                        .build(),
                )
            } else {
                None
            };

            let db_hello_config = zznet_hello::HelloConfig {
                hostname: "database".to_string(),
                our_role: "database".to_string(),
                offered_rooms: vec![
                    "intent-config".to_string(),
                    "memdb".to_string(),
                    "cstate".to_string(),
                ],
                handshake_timeout: Duration::from_millis(100),
            };
            let db_allowed_roles = {
                let mut s = std::collections::HashSet::new();
                s.insert(zznet_api::Role::new("collector"));
                s
            };
            let db_cm = zznet_hello::ConnectionManager::new(
                db_router.clone().recipient(),
                db_hello_config,
                db_allowed_roles,
            )
            .start();

            let (server_tx, server_rx) = mpsc::channel(8);
            let controlled_server = ControlledServer::new(server_rx);
            serve_connections(controlled_server, db_cm.recipient());

            (server_tx, db_memdb, storage, db_cstate)
        };

        // 2. Setup Collector Stack (Always)
        let coll_router = zznet_router::RouterActor::new(vec![]).start();

        let coll_intent_builder =
            zzintent_config::IntentConfigBuilder::new().config_for_collector();
        let _coll_intent = coll_intent_builder.router(coll_router.clone()).start()?;

        let coll_memdb = MemDBBuilder::new(MemDBConfig::for_collector(25))
            .router(coll_router.clone())
            .build();

        let mock_ping_client =
            zzpinger::MockPingerClient::new_succeeding(std::time::Duration::from_millis(5));

        let clock = zzpinger::TokioAlignedClock::new_arc();
        let pinger_builder = PingerBuilder {
            clock: Some(clock.clone()),
            spawn_strategy: SpawnStrategy::Current,
        };
        let scheduler = pinger_builder.start(mock_ping_client, coll_memdb.clone().recipient());

        // 3. Setup CState & Lock
        let cstate_addr = if let Some(port) = config.lock_port {
            let cstate_config = CStateConfig::for_collector(config.collector_id.clone(), 1000);
            // Allow database to send commands to the collector (for mastership control)
            let mut coll_permissions_map = HashMap::new();
            // Database peers need permissions to send control messages (PrepareToSwap, SetMastership)
            // These are handled as regular CStateMessage, so we use deny_all() but the network actor
            // will still forward them. The deny_all just means the DB can't send heartbeats or query.
            coll_permissions_map.insert("database".to_string(), CStatePermissions::deny_all());
            let cstate_addr = CStateBuilder::new(cstate_config)
                .router(coll_router.clone())
                .permissions_map(coll_permissions_map)
                .build();

            let lock_bind_addr = format!("127.0.0.1:{}", port);
            let lock_actor =
                TcpLockActor::new(cstate_addr.clone().recipient(), lock_bind_addr).start();

            cstate_addr.do_send(SetPinger {
                pinger: scheduler.clone().recipient(),
            });
            cstate_addr.do_send(SetTcpLock {
                tcp_lock: lock_actor.recipient(),
            });
            Some(cstate_addr)
        } else {
            let _ = scheduler
                .send(zzpinger::UpdateCState { enable: true })
                .await;
            None
        };

        // 4. Connect
        let mock_client = Arc::new(MockClient::new());

        let coll_allowed_roles = {
            let mut s = std::collections::HashSet::new();
            s.insert(zznet_api::Role::new("database"));
            s
        };
        let coll_hello_config = zznet_hello::HelloConfig {
            hostname: "collector".to_string(),
            our_role: "collector".to_string(),
            offered_rooms: vec![
                "intent-config".to_string(),
                "memdb".to_string(),
                "cstate".to_string(),
            ],
            handshake_timeout: Duration::from_millis(100),
        };

        let coll_cm = zznet_hello::ConnectionManager::new(
            coll_router.clone().recipient(),
            coll_hello_config,
            coll_allowed_roles,
        );
        let coll_cm_addr = coll_cm.start();

        let (server_conn, client_conn, kill_switch) = create_controlled_pair("harness");
        let _ = server_sender.clone().try_send(server_conn);
        mock_client.push_connection(client_conn).await;

        let reconnect_config = ReconnectConfig {
            retry_delay: Duration::from_millis(1),
        };
        maintain_connection(
            mock_client.clone(),
            coll_cm_addr.recipient(),
            reconnect_config,
        );

        tokio::time::sleep(std::time::Duration::from_millis(1)).await;

        Ok(Self {
            mock_client,
            server_sender,
            kill_switch: Some(kill_switch),
            coll_memdb,
            db_memdb,
            scheduler,
            storage,
            cstate: cstate_addr,
            db_cstate,
        })
    }

    // ... (rest of the harness methods are unchanged)
    /// Sever the currently-active transport connection using the KillSwitch.
    pub fn sever_connection(&mut self) {
        if let Some(k) = &self.kill_switch {
            k.sever();
            self.kill_switch = None;
        }
    }

    /// Restore a connection by creating a new controlled pair and wiring it
    /// into the server accept queue and the client's connect queue.
    pub async fn restore_connection(&mut self) {
        let (server_conn, client_conn, kill_switch) = create_controlled_pair("harness_restore");
        let _ = self.server_sender.send(server_conn).await;
        self.mock_client.push_connection(client_conn).await;
        self.kill_switch = Some(kill_switch);
    }

    /// Configure pinger intent (targets + pings per second) and enable/disable.
    pub async fn configure_intent(&self, targets: Vec<IpAddr>, pings_per_second: u16) {
        let msg = UpdateIntentConfig {
            targets,
            pings_per_second,
        };
        let _ = self.scheduler.send(msg).await;
    }

    /// Enable or disable the collector's pinger component via CState updates.
    pub async fn enable_pinger(&self, enable: bool) {
        let _ = self.scheduler.send(UpdateCState { enable }).await;
    }

    /// Wait until the collector MemDB reports at least `count` total_results.
    pub async fn wait_for_pings(&self, count: u64) -> anyhow::Result<()> {
        let mut attempts = 0u32;
        loop {
            let res = self.coll_memdb.send(GetHealth).await;
            if let Ok(Ok(health)) = res
                && health.buffer_size >= count as usize
            {
                return Ok(());
            }
            attempts += 1;
            if attempts > 10_000 {
                anyhow::bail!("timeout waiting for pings");
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    }

    /// Query collector MemDB health
    pub async fn collector_health(&self) -> anyhow::Result<MemDBHealth> {
        let res = self.coll_memdb.send(GetHealth).await?;
        res.map_err(|e| anyhow::anyhow!("MemDB health error: {:?}", e))
    }

    /// Query database MemDB health
    pub async fn database_health(&self) -> anyhow::Result<MemDBHealth> {
        let res = self.db_memdb.send(GetHealth).await?;
        res.map_err(|e| anyhow::anyhow!("MemDB health error: {:?}", e))
    }

    /// Get the stored blobs from the storage actor.
    pub async fn get_stored_blobs(&self) -> anyhow::Result<Vec<Vec<u8>>> {
        let res = self.storage.send(GetStoredBlobs).await?;
        res.map_err(|e| anyhow::anyhow!("Storage error: {:?}", e))
    }

    /// Wait until the database MemDB reports at least `count` total_results.
    pub async fn wait_for_db_results(&self, count: u64) -> anyhow::Result<()> {
        let mut attempts = 0u32;
        let _start = Instant::now();
        loop {
            let res = self.db_memdb.send(GetHealth).await;
            if let Ok(Ok(health)) = res
                && health.total_results >= count
            {
                return Ok(());
            }

            attempts += 1;
            if attempts > 10_000 {
                anyhow::bail!("timeout waiting for db results");
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    }
}
