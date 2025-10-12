use actix::prelude::*;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::task::JoinHandle;

use actix::prelude::Recipient;
use zzmem_db::messages::StorePingResult;
use zzmem_db::permissions::MemDBPermission;

use crate::error::PingerError;
use crate::messages::{GetHealth, PingerHealth, SetPingingEnabled, TargetConfig, UpdateTargets};
use crate::pinger::{RealPingBackend, TargetPinger};

/// Manages ping operations for multiple targets concurrently. Uses per-target tasks to maintain rate limits and submits results to MemDB. Designed for testability with injectable backends.
pub struct PingerActor {
    targets: HashMap<String, TargetPinger>,
    total_pings_sent: Arc<AtomicU64>,
    total_responses: Arc<AtomicU64>,
    pinging_enabled: bool,
    memdb_addr: Option<Recipient<StorePingResult>>,
    ping_tasks: HashMap<String, JoinHandle<()>>,
    backend: Arc<dyn crate::pinger::PingBackend>,
}

impl PingerActor {
    /// Creates a new actor with default MockBackend. Safe for testing as it avoids real ICMP.
    pub fn new() -> Self {
        Self {
            targets: HashMap::new(),
            total_pings_sent: Arc::new(AtomicU64::new(0)),
            total_responses: Arc::new(AtomicU64::new(0)),
            pinging_enabled: true,
            memdb_addr: None,
            ping_tasks: HashMap::new(),
            backend: Arc::new(crate::pinger::MockBackend::new(None)),
        }
    }

    /// Configures targets with RealPingBackend for production use. Creates TargetPinger instances with actual ICMP capability.
    pub fn with_targets(mut self, configs: Vec<TargetConfig>) -> Self {
        for cfg in configs {
            // Production wiring: use real backend when constructing from configuration
            let backend = Arc::new(RealPingBackend::new());
            self.targets.insert(
                cfg.target.clone(),
                TargetPinger::new_with_backend(cfg.target, cfg.rate_ms, cfg.timeout_ms, backend),
            );
        }
        self
    }

    /// Configures targets with custom backend for testing. Allows injection of MockBackend to avoid real network calls.
    pub fn with_targets_with_backend(
        mut self,
        configs: Vec<TargetConfig>,
        backend: Arc<dyn crate::pinger::PingBackend>,
    ) -> Self {
        self.backend = backend.clone();
        for cfg in configs {
            self.targets.insert(
                cfg.target.clone(),
                TargetPinger::new_with_backend(
                    cfg.target,
                    cfg.rate_ms,
                    cfg.timeout_ms,
                    Arc::clone(&backend),
                ),
            );
        }
        self
    }

    /// Sets initial pinging state. Controls whether ping tasks start immediately on actor startup.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.pinging_enabled = enabled;
        self
    }

    /// Configures MemDB recipient for result submission. Uses Addr.recipient() for loose coupling.
    pub fn with_memdb_addr(
        mut self,
        addr: Addr<zzmem_db::actor::MemDBActor<MemDBPermission>>,
    ) -> Self {
        // store only the recipient for the StorePingResult message to decouple typing
        self.memdb_addr = Some(addr.recipient());
        self
    }

    /// Allows direct recipient injection for testing. Bypasses Addr requirement for mock actors.
    pub fn with_memdb_recipient(mut self, recipient: Recipient<StorePingResult>) -> Self {
        self.memdb_addr = Some(recipient);
        self
    }

    /// Start ping tasks for all targets
    fn start_ping_tasks(&mut self) {
        for (target, pinger) in &self.targets {
            // don't start a task if one already exists for this target
            if self.ping_tasks.contains_key(target) {
                continue;
            }

            let pinger_clone = pinger.clone();
            let memdb_addr = self.memdb_addr.clone();
            let total_pings_sent = Arc::clone(&self.total_pings_sent);
            let total_responses = Arc::clone(&self.total_responses);

            let handle = tokio::spawn(async move {
                loop {
                    // perform one ping and submit
                    PingerActor::ping_once_and_submit(
                        pinger_clone.clone(),
                        memdb_addr.clone(),
                        Arc::clone(&total_pings_sent),
                        Arc::clone(&total_responses),
                    )
                    .await;
                    tokio::time::sleep(std::time::Duration::from_millis(pinger_clone.rate_ms()))
                        .await;
                }
            });

            self.ping_tasks.insert(target.clone(), handle);
        }
    }

    /// Ping loop for a single target
    // helper to perform one ping and optionally submit the result to MemDB
    pub(crate) async fn ping_once_and_submit(
        pinger: TargetPinger,
        memdb_addr: Option<Recipient<StorePingResult>>,
        total_pings_sent: Arc<AtomicU64>,
        total_responses: Arc<AtomicU64>,
    ) {
        let result = pinger.ping().await;
        total_pings_sent.fetch_add(1, Ordering::Relaxed);

        if result.rtt_us.is_some() {
            total_responses.fetch_add(1, Ordering::Relaxed);
        }

        if let Some(memdb) = memdb_addr {
            match memdb.send(StorePingResult { result }).await {
                Ok(Ok(())) => { /* success */ }
                Ok(Err(e)) => {
                    tracing::warn!("memdb returned error when storing ping result: {:?}", e)
                }
                Err(e) => {
                    tracing::warn!("failed to send StorePingResult to memdb recipient: {}", e)
                }
            }
        }
    }

    // memdb integration will be added later; placeholder omitted for now
}

impl Default for PingerActor {
    fn default() -> Self {
        Self::new()
    }
}

impl Actor for PingerActor {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Self::Context) {
        tracing::info!("PingerActor started with {} targets", self.targets.len());
        // If pinging was enabled before start (via builder), kick off tasks now.
        if self.pinging_enabled {
            self.start_ping_tasks();
        }
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        // Cancel all ping tasks
        for (_target, handle) in self.ping_tasks.drain() {
            handle.abort();
        }
        tracing::info!("PingerActor stopped");
    }
}

impl Handler<UpdateTargets> for PingerActor {
    type Result = Result<(), PingerError>;

    fn handle(&mut self, msg: UpdateTargets, _ctx: &mut Context<Self>) -> Self::Result {
        for cfg in &msg.targets {
            if cfg.target.is_empty() {
                return Err(PingerError::InvalidTarget("Target cannot be empty".into()));
            }
            if cfg.rate_ms == 0 {
                return Err(PingerError::InvalidTarget("Rate must be > 0".into()));
            }
            if cfg.timeout_ms == 0 {
                return Err(PingerError::InvalidTarget("Timeout must be > 0".into()));
            }
        }

        // Collect old targets to identify removed ones
        let old_targets: std::collections::HashSet<String> = self.targets.keys().cloned().collect();

        self.targets.clear();
        for cfg in msg.targets {
            self.targets.insert(
                cfg.target.clone(),
                TargetPinger::new_with_backend(
                    cfg.target,
                    cfg.rate_ms,
                    cfg.timeout_ms,
                    Arc::clone(&self.backend),
                ),
            );
        }

        // Cancel tasks for removed targets
        let new_targets: std::collections::HashSet<String> = self.targets.keys().cloned().collect();
        for removed in old_targets.difference(&new_targets) {
            if let Some(handle) = self.ping_tasks.remove(removed) {
                handle.abort();
            }
        }

        // Start new ping tasks if pinging is enabled
        if self.pinging_enabled {
            self.start_ping_tasks();
        }

        tracing::info!("Updated targets: {}", self.targets.len());
        Ok(())
    }
}

impl Handler<SetPingingEnabled> for PingerActor {
    type Result = ();

    fn handle(&mut self, msg: SetPingingEnabled, _ctx: &mut Context<Self>) -> Self::Result {
        self.pinging_enabled = msg.enabled;

        if msg.enabled {
            // Start ping tasks
            self.start_ping_tasks();
        } else {
            // Cancel all ping tasks
            for (_target, handle) in self.ping_tasks.drain() {
                handle.abort();
            }
        }

        tracing::info!("Pinging enabled = {}", self.pinging_enabled);
    }
}

impl Handler<GetHealth> for PingerActor {
    type Result = actix::MessageResult<GetHealth>;

    fn handle(&mut self, _msg: GetHealth, _ctx: &mut Context<Self>) -> Self::Result {
        actix::MessageResult(PingerHealth {
            active_targets: self.targets.len(),
            total_pings_sent: self.total_pings_sent.load(Ordering::Relaxed),
            total_responses: self.total_responses.load(Ordering::Relaxed),
            enabled: self.pinging_enabled,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix::Actor;
    use std::sync::atomic::AtomicU64;
    use std::sync::{Arc, Mutex};
    use zzmem_db::messages::MemDBError;
    use zzmem_db::network_messages::PingResult as MemPingResult;

    #[actix::test]
    async fn test_pinger_actor_creation() {
        let actor = PingerActor::new();

        assert!(actor.targets.is_empty());
        assert!(actor.pinging_enabled);
        assert_eq!(actor.total_pings_sent.load(Ordering::Relaxed), 0);
        assert_eq!(actor.total_responses.load(Ordering::Relaxed), 0);
    }

    #[actix::test]
    async fn test_pinger_actor_with_targets() {
        let targets = vec![TargetConfig {
            target: "8.8.8.8".to_string(),
            rate_ms: 1000,
            timeout_ms: 5000,
        }];

        let actor = PingerActor::new().with_targets(targets);

        assert_eq!(actor.targets.len(), 1);
        assert!(actor.targets.contains_key("8.8.8.8"));
    }

    #[actix::test]
    async fn test_update_targets_validation() {
        let addr = PingerActor::new().start();

        let invalid_targets = vec![TargetConfig {
            target: "".to_string(),
            rate_ms: 1000,
            timeout_ms: 5000,
        }];

        let res = addr
            .send(UpdateTargets {
                targets: invalid_targets,
            })
            .await;
        // Expect the message to return Err(PingerError)
        assert!(matches!(res, Ok(Err(_))));
    }

    #[actix::test]
    async fn test_get_health() {
        let addr = PingerActor::new().start();

        let health = addr.send(GetHealth).await.unwrap();

        assert_eq!(health.active_targets, 0);
        assert_eq!(health.total_pings_sent, 0);
        assert_eq!(health.total_responses, 0);
        assert!(health.enabled);
    }

    #[actix::test]
    async fn test_ping_once_and_submit_sends_to_memdb() {
        // Mock MemDB actor which collects StorePingResult messages
        struct MockMemDB {
            results: Arc<Mutex<Vec<MemPingResult>>>,
        }

        impl Actor for MockMemDB {
            type Context = Context<Self>;
        }

        impl Handler<zzmem_db::messages::StorePingResult> for MockMemDB {
            type Result = Result<(), MemDBError>;

            fn handle(
                &mut self,
                msg: zzmem_db::messages::StorePingResult,
                _ctx: &mut Context<Self>,
            ) -> Self::Result {
                self.results.lock().unwrap().push(msg.result);
                Ok(())
            }
        }

        let collected = Arc::new(Mutex::new(Vec::new()));
        let mock = MockMemDB {
            results: collected.clone(),
        }
        .start();

        // Create a pinger with a MockBackend that returns a known RTT
        use crate::pinger::MockBackend;
        use crate::pinger::TargetPinger;
        use std::sync::Arc as StdArc;

        let backend = StdArc::new(MockBackend::new(Some(4321)));
        let pinger = TargetPinger::new_with_backend("127.0.0.1".to_string(), 1000, 1000, backend);

        let total_pings = Arc::new(AtomicU64::new(0));
        let total_responses = Arc::new(AtomicU64::new(0));

        PingerActor::ping_once_and_submit(
            pinger,
            Some(mock.recipient()),
            total_pings.clone(),
            total_responses.clone(),
        )
        .await;

        let results = collected.lock().unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].rtt_us, Some(4321));
        assert_eq!(total_pings.load(Ordering::Relaxed), 1);
        assert_eq!(total_responses.load(Ordering::Relaxed), 1);
    }

    #[actix::test]
    async fn test_builder_integration_sends_store_ping_result() {
        // Mock MemDB actor which collects StorePingResult messages
        struct MockMemDB {
            results: Arc<Mutex<Vec<MemPingResult>>>,
        }

        impl Actor for MockMemDB {
            type Context = Context<Self>;
        }

        impl Handler<zzmem_db::messages::StorePingResult> for MockMemDB {
            type Result = Result<(), MemDBError>;

            fn handle(
                &mut self,
                msg: zzmem_db::messages::StorePingResult,
                _ctx: &mut Context<Self>,
            ) -> Self::Result {
                self.results.lock().unwrap().push(msg.result);
                Ok(())
            }
        }

        let collected = Arc::new(Mutex::new(Vec::new()));
        let mock = MockMemDB {
            results: collected.clone(),
        }
        .start();

        // Build the pinger via PingerBuilder, inject MockBackend that returns known RTT
        use crate::builder::PingerBuilder;
        use crate::pinger::MockBackend;
        use std::sync::Arc as StdArc;

        let backend = StdArc::new(MockBackend::new(Some(7777)));

        let targets = vec![TargetConfig {
            target: "127.0.0.1".to_string(),
            rate_ms: 10,
            timeout_ms: 1000,
        }];

        let handle = PingerBuilder::new()
            .backend(backend)
            .memdb_recipient(mock.recipient())
            .targets(targets)
            .enabled(true)
            .start()
            .expect("failed to start pinger");

        // Allow some time for the background task to run at least once
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Verify that the mock MemDB received at least one StorePingResult
        let results = collected.lock().unwrap();
        assert!(!results.is_empty(), "expected at least one StorePingResult");
        assert_eq!(results[0].rtt_us, Some(7777));

        // Stop the actor by dropping the handle (actor stops when process ends in tests)
        drop(handle);
    }

    #[actix::test]
    async fn test_per_target_cancellation_on_update() {
        // Custom backend that counts pings per target
        struct CountingBackend {
            counts: Arc<Mutex<HashMap<String, u64>>>,
            rtt: Option<u32>,
        }

        impl CountingBackend {
            fn new(counts: Arc<Mutex<HashMap<String, u64>>>, rtt: Option<u32>) -> Self {
                Self { counts, rtt }
            }
        }

        impl crate::pinger::PingBackend for CountingBackend {
            fn ping<'a>(
                &'a self,
                target: &'a str,
                _sequence: u32,
                _timeout_ms: u64,
            ) -> futures::future::BoxFuture<'a, Option<u32>> {
                let target = target.to_string();
                let counts = Arc::clone(&self.counts);
                let rtt = self.rtt;
                Box::pin(async move {
                    let mut map = counts.lock().unwrap();
                    *map.entry(target).or_insert(0) += 1;
                    rtt
                })
            }
        }

        let ping_counts = Arc::new(Mutex::new(HashMap::new()));

        let backend = Arc::new(CountingBackend::new(Arc::clone(&ping_counts), Some(1000)));

        let targets = vec![
            TargetConfig {
                target: "target1".to_string(),
                rate_ms: 10,
                timeout_ms: 1000,
            },
            TargetConfig {
                target: "target2".to_string(),
                rate_ms: 10,
                timeout_ms: 1000,
            },
        ];

        use crate::builder::PingerBuilder;

        let handle = PingerBuilder::new()
            .backend(backend)
            .targets(targets)
            .enabled(true)
            .start()
            .expect("failed to start pinger");

        // Wait for some pings to accumulate
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let initial_counts = ping_counts.lock().unwrap().clone();
        assert!(
            initial_counts.get("target1").unwrap_or(&0) > &0,
            "target1 should have pings"
        );
        assert!(
            initial_counts.get("target2").unwrap_or(&0) > &0,
            "target2 should have pings"
        );

        // Update targets to remove target1
        let new_targets = vec![TargetConfig {
            target: "target2".to_string(),
            rate_ms: 10,
            timeout_ms: 1000,
        }];

        handle
            .update_targets(new_targets)
            .await
            .expect("update failed");

        // Wait again
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let final_counts = ping_counts.lock().unwrap().clone();
        let initial_target1 = *initial_counts.get("target1").unwrap_or(&0);
        let final_target1 = *final_counts.get("target1").unwrap_or(&0);
        let initial_target2 = *initial_counts.get("target2").unwrap_or(&0);
        let final_target2 = *final_counts.get("target2").unwrap_or(&0);

        // target1 should not have increased (task cancelled)
        assert_eq!(
            final_target1, initial_target1,
            "target1 pings should not increase after removal"
        );

        // target2 should have increased
        assert!(
            final_target2 > initial_target2,
            "target2 pings should continue"
        );

        drop(handle);
    }

    #[actix::test]
    async fn test_pinger_handle_api() {
        let targets1 = vec![TargetConfig {
            target: "api_test1".to_string(),
            rate_ms: 100,
            timeout_ms: 1000,
        }];

        use crate::builder::PingerBuilder;

        let handle = PingerBuilder::new()
            .targets(targets1.clone())
            .enabled(true)
            .start()
            .expect("failed to start pinger");

        // Test get_health
        let health = handle.get_health().await.expect("get_health failed");
        assert_eq!(health.active_targets, 1);
        assert!(health.enabled);

        // Test update_targets
        let targets2 = vec![
            TargetConfig {
                target: "api_test1".to_string(),
                rate_ms: 100,
                timeout_ms: 1000,
            },
            TargetConfig {
                target: "api_test2".to_string(),
                rate_ms: 100,
                timeout_ms: 1000,
            },
        ];
        handle
            .update_targets(targets2)
            .await
            .expect("update_targets failed");

        let health = handle.get_health().await.expect("get_health failed");
        assert_eq!(health.active_targets, 2);

        // Test set_enabled(false)
        handle.set_enabled(false).await.expect("set_enabled failed");

        let health = handle.get_health().await.expect("get_health failed");
        assert!(!health.enabled);

        // Test set_enabled(true)
        handle.set_enabled(true).await.expect("set_enabled failed");

        let health = handle.get_health().await.expect("get_health failed");
        assert!(health.enabled);

        drop(handle);
    }

    #[actix::test]
    async fn test_ping_once_and_submit_handles_memdb_handler_error() {
        // MemDB actor that returns an error from the handler
        struct ErroringMemDB;

        impl Actor for ErroringMemDB {
            type Context = Context<Self>;
        }

        impl Handler<zzmem_db::messages::StorePingResult> for ErroringMemDB {
            type Result = Result<(), MemDBError>;

            fn handle(
                &mut self,
                _msg: zzmem_db::messages::StorePingResult,
                _ctx: &mut Context<Self>,
            ) -> Self::Result {
                Err(MemDBError::InternalError("simulated".into()))
            }
        }

        let mem = ErroringMemDB.start();

        // Use MockBackend that returns a RTT so responses increment
        let backend = Arc::new(crate::pinger::MockBackend::new(Some(5555)));
        let pinger = TargetPinger::new_with_backend("127.0.0.1".to_string(), 1000, 1000, backend);

        let total_pings = Arc::new(AtomicU64::new(0));
        let total_responses = Arc::new(AtomicU64::new(0));

        // This should not panic and should increment counters even though memdb handler returns error
        PingerActor::ping_once_and_submit(
            pinger,
            Some(mem.recipient()),
            total_pings.clone(),
            total_responses.clone(),
        )
        .await;

        assert_eq!(total_pings.load(Ordering::Relaxed), 1);
        assert_eq!(total_responses.load(Ordering::Relaxed), 1);
    }

    #[actix::test]
    async fn test_ping_once_and_submit_handles_backend_timeout() {
        // No memdb in this test

        // Use MockBackend that simulates a timeout (None)
        let backend = Arc::new(crate::pinger::MockBackend::new(None));
        let pinger = TargetPinger::new_with_backend("127.0.0.1".to_string(), 1000, 1000, backend);

        let total_pings = Arc::new(AtomicU64::new(0));
        let total_responses = Arc::new(AtomicU64::new(0));

        PingerActor::ping_once_and_submit(
            pinger,
            None,
            total_pings.clone(),
            total_responses.clone(),
        )
        .await;

        // Ping attempted, but no response recorded
        assert_eq!(total_pings.load(Ordering::Relaxed), 1);
        assert_eq!(total_responses.load(Ordering::Relaxed), 0);
    }
}
