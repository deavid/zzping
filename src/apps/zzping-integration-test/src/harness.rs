//! Utilities for building deterministic collector/database stacks in tests.

use actix::prelude::*;
use anyhow::Result;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;

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

/// SystemHarness sets up a minimal collector+database environment for tests.
/// It provides control over transport via a KillSwitch and MockClient queueing.
pub struct SystemHarness {
    /// Client-side transport FIFO used by `maintain_connection`
    pub mock_client: Arc<MockClient>,
    server_sender: mpsc::Sender<EstablishedConnection>,
    kill_switch: Option<KillSwitch>,
    /// Address of the collector MemDB actor (we query it for buffered results)
    coll_memdb: actix::Addr<MemDBActor>,
    /// Address of the database MemDB actor
    db_memdb: actix::Addr<MemDBActor>,
    /// Pinger scheduler address so we can configure intent
    scheduler: actix::Addr<zzpinger::PingerSchedulerActor>,
    /// Address of the storage actor
    storage: actix::Addr<StorageActor>,
}

impl SystemHarness {
    /// Spawn core actors and start client+server lifecycles. Returns an instance
    /// that allows tests to sever/restore the transport and configure pinger.
    pub async fn new() -> Result<Self> {
        // Routers
        let db_router = zznet_router::RouterActor::new(vec![]).start();
        let coll_router = zznet_router::RouterActor::new(vec![]).start();

        // Intent, MemDB, CState for database
        let intent_builder = zzintent_config::IntentConfigBuilder::new()
            .config_for_database(std::path::PathBuf::from("/tmp/test_intent.ron"));
        let _db_intent = intent_builder.router(db_router.clone()).start()?;

        let storage = StorageActor::new(StorageConfig::Ephemeral).start();
        let memdb_builder =
            MemDBBuilder::new(MemDBConfig::for_database(10000, None))
                .with_storage_actor(storage.clone());
        let db_memdb = memdb_builder.router(db_router.clone()).build();

        let cstate_builder = zzcollector_state::CStateBuilder::new(
            zzcollector_state::CStateConfig::for_database(5000, Some(100)),
        );
        let _db_cstate = cstate_builder.router(db_router.clone()).build();

        // Collector: Intent + MemDB
        let intent_builder = zzintent_config::IntentConfigBuilder::new().config_for_collector();
        let _coll_intent = intent_builder.router(coll_router.clone()).start()?;

        // Use a smaller collector buffer in tests so batches flush quickly
        // and Act I can verify DB ingestion before the disaster phase.
        let coll_memdb = MemDBBuilder::new(MemDBConfig::for_collector(25))
            .router(coll_router.clone())
            .build();

        // Connection managers
        let db_allowed_roles = {
            let mut s = std::collections::HashSet::new();
            s.insert(zznet_api::Role::new("collector"));
            s.insert(zznet_api::Role::new("client-ro"));
            s.insert(zznet_api::Role::new("client-admin"));
            s
        };

        let db_hello_config = zznet_hello::HelloConfig {
            hostname: "database".to_string(),
            our_role: "database".to_string(),
            offered_rooms: vec![
                "intent-config".to_string(),
                "memdb".to_string(),
                "query".to_string(),
            ],
            handshake_timeout: Duration::from_millis(1),
        };

        let db_cm = zznet_hello::ConnectionManager::new(
            db_router.clone().recipient(),
            db_hello_config,
            db_allowed_roles,
        );
        let db_cm_addr = db_cm.start();

        // Controlled server channel - tests will push server-side EstablishedConnection here
        let (server_tx, server_rx) = mpsc::channel(8);
        let controlled_server = ControlledServer::new(server_rx);

        // Start serving loop which will accept from our ControlledServer
        serve_connections(controlled_server, db_cm_addr.recipient());

        // Collector side connection manager
        let coll_allowed_roles = {
            let mut s = std::collections::HashSet::new();
            s.insert(zznet_api::Role::new("database"));
            s.insert(zznet_api::Role::new("collector"));
            s
        };
        let coll_hello_config = zznet_hello::HelloConfig {
            hostname: "collector".to_string(),
            our_role: "collector".to_string(),
            offered_rooms: vec!["intent-config".to_string(), "memdb".to_string()],
            handshake_timeout: Duration::from_millis(1),
        };

        let coll_cm = zznet_hello::ConnectionManager::new(
            coll_router.clone().recipient(),
            coll_hello_config,
            coll_allowed_roles,
        );
        let coll_cm_addr = coll_cm.start();

        // Create a MockClient (queue-backed) and an initial connection pair
        let mock_client = Arc::new(MockClient::new());

        // Create the initial controlled pair and push into server queue and client queue
        let (server_conn, client_conn, kill_switch) = create_controlled_pair("harness_init");

        // send server side into controlled server
        let _ = server_tx.clone().try_send(server_conn);

        // push client side into mock client queue
        mock_client.push_connection(client_conn).await;

        // Start client maintain loop
        let reconnect_config = ReconnectConfig {
            retry_delay: Duration::from_millis(1),
        };
        maintain_connection(
            mock_client.clone(),
            coll_cm_addr.recipient(),
            reconnect_config,
        );

        // Give the networking/subscription plumbing a virtual tick so that
        // NetworkActors have a chance to start and subscribe to the event bus
        // before the pinger begins emitting results. This uses tokio virtual
        // time (tests call `tokio::time::pause()`), so it's hermetic.
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;

        // --- Pinger wiring ---
        // Use the public MockPingerClient from the zzpinger crate for deterministic behavior.
        let mock_ping_client =
            zzpinger::MockPingerClient::new_succeeding(std::time::Duration::from_millis(5));

        // Create a tokio-aligned clock and pass it to the pinger so its
        // SystemTime calculations follow the test's virtual time.
        let clock = zzpinger::TokioAlignedClock::new_arc();

        let pinger_builder = PingerBuilder {
            clock: Some(clock.clone()),
            spawn_strategy: SpawnStrategy::Current,
        };

        // Start the pinger on the current Arbiter so it runs on the test thread.
        let scheduler = pinger_builder.start_on_arbiter(
            actix::Arbiter::current(),
            mock_ping_client,
            coll_memdb.clone().recipient(),
        );

        // Ensure the scheduler actor is responsive before returning the harness.
        // Send a no-op UpdateCState (disabled) to warm up the actor and backend.
        let _ = scheduler
            .send(zzpinger::UpdateCState { enable: false })
            .await;

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

    /// Sever the currently-active transport connection using the KillSwitch.
    pub fn sever_connection(&mut self) {
        if let Some(k) = &self.kill_switch {
            k.sever();
            // drop reference to signal that it's gone
            self.kill_switch = None;
        }
    }

    /// Restore a connection by creating a new controlled pair and wiring it
    /// into the server accept queue and the client's connect queue.
    pub async fn restore_connection(&mut self) {
        let (server_conn, client_conn, kill_switch) = create_controlled_pair("harness_restore");
        // send the server side to the ControlledServer accept queue
        let _ = self.server_sender.send(server_conn).await;
        // push the client-side into the mock client so maintain_connection will obtain it
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
    /// Uses 1ms polling delay (compatible with virtual time).
    pub async fn wait_for_pings(&self, count: u64) -> anyhow::Result<()> {
        let mut attempts = 0u32;
        loop {
            let res = self.coll_memdb.send(GetHealth).await;
            if let Ok(Ok(health)) = res {
                // Accept either flushed results (total_results) or buffered results
                // since collector may buffer before flushing to the database.
                tracing::debug!(
                    "harness.wait_for_pings: attempt={} total_results={} buffer_size={}",
                    attempts,
                    health.total_results,
                    health.buffer_size
                );
                if health.total_results >= count || (health.buffer_size as u64) >= count {
                    tracing::debug!(
                        "harness.wait_for_pings: threshold reached (attempt={}): total_results={} buffer_size={}",
                        attempts,
                        health.total_results,
                        health.buffer_size
                    );
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
        let start = Instant::now();
        loop {
            let res = self.db_memdb.send(GetHealth).await;
            if let Ok(Ok(health)) = res {
                tracing::debug!(
                    "harness.wait_for_db_results: attempt={} total_results={}",
                    attempts,
                    health.total_results
                );
                if health.total_results >= count {
                    let metrics = DbWaitMetrics::from_elapsed(
                        attempts,
                        health.total_results,
                        start.elapsed(),
                    );
                    tracing::debug!(
                        attempts = metrics.attempts,
                        total_results = metrics.total_results,
                        elapsed_ms = metrics.elapsed_ms,
                        "harness.wait_for_db_results: threshold reached"
                    );
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

#[derive(Debug, PartialEq, Eq)]
struct DbWaitMetrics {
    attempts: u32,
    total_results: u64,
    elapsed_ms: u128,
}

impl DbWaitMetrics {
    fn from_elapsed(attempts: u32, total_results: u64, elapsed: Duration) -> Self {
        Self {
            attempts,
            total_results,
            elapsed_ms: elapsed.as_millis(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_wait_metrics_converts_elapsed_to_millis() {
        let metrics = DbWaitMetrics::from_elapsed(3, 42, Duration::from_micros(1_500));
        assert_eq!(metrics.attempts, 3);
        assert_eq!(metrics.total_results, 42);
        assert_eq!(metrics.elapsed_ms, 1); // 1500µs should truncate to 1ms
    }

    #[test]
    fn db_wait_metrics_handles_large_elapsed_values() {
        let metrics = DbWaitMetrics::from_elapsed(10, 100, Duration::from_secs(2));
        assert_eq!(metrics.elapsed_ms, 2_000);
    }
}
