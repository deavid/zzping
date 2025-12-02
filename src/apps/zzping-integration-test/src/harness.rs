//! Utilities for building deterministic collector/database stacks in tests.

use actix::prelude::*;
use anyhow::Result;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use zzcollector_state::{CStateActor, CStateConfig, SetPinger};

use std::net::IpAddr;
use zzmem_db::{
    actor::MemDBActor,
    builder::MemDBBuilder,
    config::MemDBConfig,
    messages::{GetHealth, MemDBHealth},
};
use zznet_api::{
    create_controlled_pair, maintain_connection, serve_connections, EstablishedConnection,
    KillSwitch, MockClient, ReconnectConfig, TransportServer,
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
    pub collector_id: String,
}

/// SystemHarness sets up a minimal collector+database environment for tests.
/// It provides control over transport via a KillSwitch and MockClient queueing.
pub struct SystemHarness {
    /// Client-side transport FIFO used by `maintain_connection`
    pub mock_client: Arc<MockClient>,
    server_sender: mpsc::Sender<EstablishedConnection>,
    kill_switch: Option<KillSwitch>,
    /// Address of the collector MemDB actor (we query it for buffered results)
    pub coll_memdb: actix::Addr<MemDBActor>,
    /// Address of the database MemDB actor
    pub db_memdb: actix::Addr<MemDBActor>,
    /// Pinger scheduler address so we can configure intent
    pub scheduler: actix::Addr<zzpinger::PingerSchedulerActor>,
    /// Address of the storage actor
    pub storage: actix::Addr<StorageActor>,
}

impl SystemHarness {
    /// Spawn core actors and start client+server lifecycles. Returns an instance
    /// that allows tests to sever/restore the transport and configure pinger.
    pub async fn new(config: HarnessConfig) -> Result<Self> {
        // Routers
        let db_router = zznet_router::RouterActor::new(vec![]).start();
        let coll_router = zznet_router::RouterActor::new(vec![]).start();

        // Database-side Intent and MemDB
        let db_intent_builder = zzintent_config::IntentConfigBuilder::new()
            .config_for_database(std::path::PathBuf::from("/tmp/test_intent.ron"));
        let _db_intent = db_intent_builder.router(db_router.clone()).start()?;

        let storage = StorageActor::new(StorageConfig::Ephemeral).start();
        let db_memdb_builder = MemDBBuilder::new(MemDBConfig::for_database(10000, None))
            .with_storage_actor(storage.clone());
        let db_memdb = db_memdb_builder.router(db_router.clone()).build();

        // Collector-side Intent
        let coll_intent_builder = zzintent_config::IntentConfigBuilder::new().config_for_collector();
        let _coll_intent = coll_intent_builder.router(coll_router.clone()).start()?;

        // Collector-side MemDB
        let coll_memdb = MemDBBuilder::new(MemDBConfig::for_collector(25))
            .router(coll_router.clone())
            .build();

        // --- Pinger wiring ---
        let mock_ping_client =
            zzpinger::MockPingerClient::new_succeeding(std::time::Duration::from_millis(5));

        let clock = zzpinger::TokioAlignedClock::new_arc();
        let pinger_builder = PingerBuilder {
            clock: Some(clock.clone()),
            spawn_strategy: SpawnStrategy::Current,
        };
        let scheduler = pinger_builder.start(
            mock_ping_client,
            coll_memdb.clone().recipient(),
        );
        // --- End Pinger wiring ---

        // --- Collector State and Lock (Optional) ---
        if let Some(port) = config.lock_port {
            let cstate_config = CStateConfig::for_collector(config.collector_id, 1000);
            let cstate_addr = CStateActor::new(cstate_config).start();

            let lock_bind_addr = format!("127.0.0.1:{}", port);
            let lock_actor =
                TcpLockActor::new(cstate_addr.clone().recipient(), lock_bind_addr);
            lock_actor.start();

            cstate_addr.do_send(SetPinger {
                pinger: scheduler.clone().recipient(),
            });
        } else {
            // If lock is disabled, pinger is enabled by default for old tests.
            let _ = scheduler
                .send(zzpinger::UpdateCState { enable: true })
                .await;
        }

        // --- Network Setup ---
        // ... (rest of the network setup is unchanged)
        let db_allowed_roles = {
            let mut s = std::collections::HashSet::new();
            s.insert(zznet_api::Role::new("collector"));
            s
        };

        let db_hello_config = zznet_hello::HelloConfig {
            hostname: "database".to_string(),
            our_role: "database".to_string(),
            offered_rooms: vec!["intent-config".to_string(), "memdb".to_string()],
            handshake_timeout: Duration::from_millis(100),
        };

        let db_cm = zznet_hello::ConnectionManager::new(
            db_router.clone().recipient(),
            db_hello_config,
            db_allowed_roles,
        );
        let db_cm_addr = db_cm.start();

        let (server_tx, server_rx) = mpsc::channel(8);
        let controlled_server = ControlledServer::new(server_rx);
        serve_connections(controlled_server, db_cm_addr.recipient());

        let coll_allowed_roles = {
            let mut s = std::collections::HashSet::new();
            s.insert(zznet_api::Role::new("database"));
            s
        };
        let coll_hello_config = zznet_hello::HelloConfig {
            hostname: "collector".to_string(),
            our_role: "collector".to_string(),
            offered_rooms: vec!["intent-config".to_string(), "memdb".to_string()],
            handshake_timeout: Duration::from_millis(100),
        };

        let coll_cm = zznet_hello::ConnectionManager::new(
            coll_router.clone().recipient(),
            coll_hello_config,
            coll_allowed_roles,
        );
        let coll_cm_addr = coll_cm.start();
        let mock_client = Arc::new(MockClient::new());

        let (server_conn, client_conn, kill_switch) = create_controlled_pair("harness_init");
        let _ = server_tx.clone().try_send(server_conn);
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
            server_sender: server_tx,
            kill_switch: Some(kill_switch),
            coll_memdb,
            db_memdb,
            scheduler,
            storage,
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
            if let Ok(Ok(health)) = res {
                if health.buffer_size >= count as usize {
                    return Ok(());
                }
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
            if let Ok(Ok(health)) = res {
                if health.total_results >= count {
                    return Ok(());
                }
            }

            attempts += 1;
            if attempts > 10_000 {
                anyhow::bail!("timeout waiting for db results");
            }
            tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        }
    }
}
