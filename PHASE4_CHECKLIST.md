# Phase 4 Implementation Checklist: Collector Application

**Target:** Week 4 (Following Phase 3 `zzcollector-state` completion)
**Current Status:** Ready to begin - Phases 1, 2, & 3 complete
**Goal:** Create a working collector binary that integrates all collector-side components

---

## Overview

The collector application is the main binary that:
- Integrates all collector-side components (`zzintent-config`, `zzpinger`, `zzmem-db`, `zzcollector-state`)
- Connects to the database via ZZNet/mTLS
- Implements the three-phase component lifecycle (Builder → Wire → Start)
- Handles configuration from files and dynamic updates from database
- Provides graceful shutdown and cleanup

**What Makes This Different from Old Code:**
- Uses ZZNet (typed messages) instead of gRPC
- Components communicate via SessionManager rooms
- Same-code-different-role pattern for components
- Mock-first testing approach

---

## Week 4 Task List - Copy this and check off as you go!

---

## Day 1: Application Structure and Configuration

### Morning: Create Binary Crate
- [ ] Create directory: `src/apps/zzping-collector/`
- [ ] Create `Cargo.toml`:
  ```toml
  [package]
  name = "zzping-collector"
  version = "0.2.0"
  edition = "2021"

  [[bin]]
  name = "zzping-collector"
  path = "src/main.rs"

  [dependencies]
  actix = "0.13"
  tokio = { version = "1.0", features = ["full"] }
  anyhow = "1.0"
  tracing = "0.1"
  tracing-subscriber = "0.3"
  serde = { version = "1.0", features = ["derive"] }
  ron = "0.8"
  clap = { version = "4.0", features = ["derive"] }

  # ZZNet crates
  zznet-api = { path = "../../net/zznet-api" }
  zznet-session = { path = "../../net/zznet-session" }
  zznet-auth = { path = "../../net/zznet-auth" }
  zznet-builder = { path = "../../net/zznet-builder" }
  zznet-transport-tcp = { path = "../../net/zznet-transport-tcp" }

  # Component crates
  zzintent-config = { path = "../../components/zzintent-config" }
  zzpinger = { path = "../../components/zzpinger" }
  zzmem-db = { path = "../../components/zzmem-db" }
  zzcollector-state = { path = "../../components/zzcollector-state" }

  [dev-dependencies]
  tempfile = "3.0"
  ```
- [ ] Create `src/main.rs` with basic CLI
- [ ] Create `src/lib.rs` for testable logic
- [ ] Create modules: `config.rs`, `service.rs`, `cli.rs`

### Afternoon: Configuration Structure
- [ ] In `config.rs`, define:
  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct CollectorConfig {
      /// Unique collector ID (used in mTLS certificate CN)
      pub collector_id: String,

      /// Database connection
      pub database_host: String,
      pub database_port: u16,

      /// TLS configuration
      pub cert_path: String,
      pub key_path: String,
      pub ca_cert_path: String,

      /// Heartbeat configuration
      pub heartbeat_interval_secs: u64,

      /// Initial ping configuration (overridden by database)
      pub default_targets: Vec<TargetConfig>,
      pub default_rate_ms: u64,
      pub default_timeout_ms: u64,

      /// Buffer configuration
      pub memdb_buffer_size: usize,
      pub memdb_batch_size: usize,
  }
  ```
- [ ] Implement `CollectorConfig::load(path: &str) -> Result<Self>`
- [ ] Add validation methods
- [ ] Write config loading tests

### Evening: CLI Interface
- [ ] In `cli.rs`, define:
  ```rust
  #[derive(Parser, Debug)]
  #[command(author, version, about, long_about = None)]
  pub struct Cli {
      /// Path to collector configuration file
      #[arg(short, long, default_value = "collector.ron")]
      pub config: String,

      /// Enable debug logging
      #[arg(short, long)]
      pub debug: bool,

      /// Dry run (don't connect to database)
      #[arg(long)]
      pub dry_run: bool,
  }
  ```
- [ ] Set up logging configuration
- [ ] Write CLI parsing tests
- [ ] Commit: "feat(collector): Add application structure and configuration"

---

## Day 2: Service Orchestration (Phase 1: Builders)

### Morning: Service Structure
- [ ] In `service.rs`, create:
  ```rust
  pub struct CollectorService {
      config: CollectorConfig,
      // Component handles (created after start)
      intent_config: Option<IntentConfigHandle>,
      pinger: Option<PingerHandle>,
      memdb: Option<MemDBHandle>,
      cstate: Option<CStateHandle>,
      // SessionManager
      session_manager: Option<Rc<SessionManager<...>>>,
      // Shutdown signal
      shutdown_rx: tokio::sync::watch::Receiver<bool>,
  }
  ```
- [ ] Implement `new(config: CollectorConfig) -> Result<Self>`
- [ ] Set up shutdown signal channel

### Afternoon: Component Builders (Phase 1)
- [ ] Create component builders in initialization:
  ```rust
  pub fn initialize_components(&self) -> Result<ComponentBuilders> {
      // Create builders (no network yet)
      let intent_config_builder = IntentConfigBuilder::new(
          IntentConfigRole::Collector {
              cache_path: self.get_cache_path(),
          }
      );

      let pinger_builder = PingerBuilder::new();

      let memdb_builder = MemDBBuilder::new(
          MemDBRole::Collector {
              buffer_size: self.config.memdb_buffer_size,
          }
      );

      let cstate_builder = CStateBuilder::new(
          CStateRole::Collector {
              collector_id: self.config.collector_id.clone(),
              heartbeat_interval_secs: self.config.heartbeat_interval_secs,
          }
      );

      Ok(ComponentBuilders {
          intent_config: intent_config_builder,
          pinger: pinger_builder,
          memdb: memdb_builder,
          cstate: cstate_builder,
      })
  }
  ```
- [ ] Write builder creation tests

### Evening: SessionManager Setup
- [ ] Create SessionManager for collector:
  ```rust
  pub fn create_session_manager(
      &self,
      builders: &mut ComponentBuilders,
  ) -> Result<Rc<SessionManager<...>>> {
      let session_manager = SessionManager::new();

      // Register rooms for each component
      intent_config_builder.register_rooms(&session_manager)?;
      memdb_builder.register_rooms(&session_manager)?;
      cstate_builder.register_rooms(&session_manager)?;

      Rc::new(session_manager)
  }
  ```
- [ ] Write SessionManager creation tests
- [ ] Commit: "feat(collector): Implement component builder phase"

---

## Day 3: Component Wiring (Phase 2)

### Morning: Wire Components Together
- [ ] Implement component wiring:
  ```rust
  pub fn wire_components(
      &self,
      builders: &mut ComponentBuilders,
      session_manager: Rc<SessionManager<...>>,
  ) -> Result<()> {
      // Wire IntentConfig → Pinger (targets update)
      let pinger_recipient = pinger_builder.get_target_update_recipient();
      intent_config_builder.subscribe_targets(pinger_recipient);

      // Wire Pinger → MemDB (ping results)
      let memdb_recipient = memdb_builder.get_result_recipient();
      pinger_builder.set_result_sink(memdb_recipient);

      // Wire Pinger → CState (health metrics)
      let cstate_health_recipient = cstate_builder.get_metrics_recipient();
      pinger_builder.set_health_reporter(cstate_health_recipient);

      // Wire MemDB → CState (batch metrics)
      memdb_builder.set_health_reporter(cstate_health_recipient.clone());

      // Wire SessionManager to all components
      intent_config_builder.session_manager(session_manager.clone());
      memdb_builder.session_manager(session_manager.clone());
      cstate_builder.session_manager(session_manager.clone());

      Ok(())
  }
  ```
- [ ] Write wiring tests with mock components

### Afternoon: Component Activation (Phase 3)
- [ ] Implement component activation:
  ```rust
  pub async fn start_components(
      &mut self,
      builders: ComponentBuilders,
  ) -> Result<()> {
      // Start all components (consumes builders)
      self.intent_config = Some(builders.intent_config.start()?);
      self.pinger = Some(builders.pinger.start()?);
      self.memdb = Some(builders.memdb.start()?);
      self.cstate = Some(builders.cstate.start()?);

      tracing::info!("All components started successfully");
      Ok(())
  }
  ```
- [ ] Write component activation tests

### Evening: Full Bootstrap Flow
- [ ] Implement complete bootstrap:
  ```rust
  pub async fn bootstrap(&mut self) -> Result<()> {
      // Phase 1: Create builders
      let mut builders = self.initialize_components()?;

      // Phase 2: Create SessionManager and wire components
      let session_manager = self.create_session_manager(&mut builders)?;
      self.wire_components(&mut builders, session_manager.clone())?;
      self.session_manager = Some(session_manager);

      // Phase 3: Start all components
      self.start_components(builders).await?;

      tracing::info!("Collector service bootstrapped");
      Ok(())
  }
  ```
- [ ] Write end-to-end bootstrap tests
- [ ] Commit: "feat(collector): Implement component wiring and activation"

---

## Day 4: Network Connection

### Morning: TCP Connection Setup
- [ ] Implement database connection:
  ```rust
  pub async fn connect_to_database(&mut self) -> Result<()> {
      let addr = format!("{}:{}", self.config.database_host, self.config.database_port);

      // Load TLS certificates
      let identity = load_identity(
          &self.config.cert_path,
          &self.config.key_path,
      )?;

      let ca_cert = load_ca_cert(&self.config.ca_cert_path)?;

      // Create TLS connector
      let tls_config = create_client_tls_config(identity, ca_cert)?;

      // Connect via ZZNet transport
      let transport = TcpTransport::connect(
          &addr,
          tls_config,
      ).await?;

      // Register with SessionManager
      self.session_manager.as_ref()
          .unwrap()
          .add_connection("database", transport)?;

      tracing::info!("Connected to database at {}", addr);
      Ok(())
  }
  ```
- [ ] Implement TLS helper functions
- [ ] Write connection tests (with mock server)

### Afternoon: Connection Lifecycle
- [ ] Implement reconnection logic:
  ```rust
  pub async fn run_with_reconnect(&mut self) -> Result<()> {
      loop {
          match self.connect_to_database().await {
              Ok(_) => {
                  tracing::info!("Connected to database");
                  // Wait for disconnect or shutdown
                  self.wait_for_disconnect_or_shutdown().await;
              }
              Err(e) => {
                  tracing::error!("Connection failed: {}", e);
                  tokio::time::sleep(Duration::from_secs(5)).await;
              }
          }

          if self.should_shutdown() {
              break;
          }
      }
      Ok(())
  }
  ```
- [ ] Handle graceful disconnection
- [ ] Write reconnection tests

### Evening: Initial Configuration Loading
- [ ] Load default targets at startup:
  ```rust
  pub async fn load_initial_config(&self) -> Result<()> {
      if !self.config.default_targets.is_empty() {
          self.pinger.as_ref()
              .unwrap()
              .update_targets(self.config.default_targets.clone())
              .await?;

          tracing::info!("Loaded {} default targets", self.config.default_targets.len());
      }
      Ok(())
  }
  ```
- [ ] Write initial config tests
- [ ] Commit: "feat(collector): Implement database connection and lifecycle"

---

## Day 5: Main Event Loop and Shutdown

### Morning: Main Run Loop
- [ ] Implement main service loop:
  ```rust
  pub async fn run(&mut self) -> Result<()> {
      // Bootstrap all components
      self.bootstrap().await?;

      // Load initial configuration
      self.load_initial_config().await?;

      // Connect to database (with reconnect)
      self.run_with_reconnect().await?;

      // Shutdown initiated
      self.shutdown().await?;

      tracing::info!("Collector service stopped");
      Ok(())
  }
  ```
- [ ] Write main loop tests

### Afternoon: Graceful Shutdown
- [ ] Implement shutdown sequence:
  ```rust
  pub async fn shutdown(&mut self) -> Result<()> {
      tracing::info!("Initiating graceful shutdown...");

      // 1. Stop accepting new pings
      if let Some(pinger) = &self.pinger {
          pinger.set_enabled(false).await?;
      }

      // 2. Flush any buffered data
      if let Some(memdb) = &self.memdb {
          memdb.flush_buffer().await?;
      }

      // 3. Send final heartbeat
      if let Some(cstate) = &self.cstate {
          cstate.force_heartbeat().await.ok();
      }

      // 4. Close network connection
      if let Some(sm) = &self.session_manager {
          sm.disconnect_all().await;
      }

      // 5. Stop all components
      // (happens automatically when handles drop)

      tracing::info!("Graceful shutdown complete");
      Ok(())
  }
  ```
- [ ] Write shutdown tests

### Evening: Signal Handling
- [ ] Implement signal handling:
  ```rust
  pub async fn setup_signal_handlers(&mut self) -> Result<()> {
      let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
      self.shutdown_rx = shutdown_rx;

      tokio::spawn(async move {
          tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
          tracing::info!("Received Ctrl+C, initiating shutdown...");
          shutdown_tx.send(true).ok();
      });

      Ok(())
  }
  ```
- [ ] Write signal handling tests
- [ ] Commit: "feat(collector): Implement main loop and graceful shutdown"

---

## Day 6: Integration Testing

### Morning: Mock Database Server
- [ ] Create test helper:
  ```rust
  #[cfg(test)]
  mod test_helpers {
      pub struct MockDatabaseServer {
          addr: SocketAddr,
          session_manager: Rc<SessionManager<...>>,
          // Track received messages
          received_heartbeats: Arc<Mutex<Vec<CStateMessage>>>,
          received_batches: Arc<Mutex<Vec<MemDBMessage>>>,
      }

      impl MockDatabaseServer {
          pub async fn start() -> Result<Self> { ... }
          pub fn get_heartbeats(&self) -> Vec<CStateMessage> { ... }
          pub fn get_batches(&self) -> Vec<MemDBMessage> { ... }
          pub async fn send_config(&self, targets: Vec<TargetConfig>) -> Result<()> { ... }
      }
  }
  ```
- [ ] Write mock server implementation

### Afternoon: End-to-End Tests
- [ ] Test full collector with mock database:
  ```rust
  #[tokio::test]
  async fn test_collector_full_flow() {
      // Start mock database
      let mock_db = MockDatabaseServer::start().await?;

      // Create collector config pointing to mock
      let config = create_test_config(mock_db.addr());

      // Start collector
      let mut collector = CollectorService::new(config)?;

      // Run for a few seconds
      tokio::spawn(async move {
          collector.run().await
      });

      tokio::time::sleep(Duration::from_secs(3)).await;

      // Verify heartbeats received
      let heartbeats = mock_db.get_heartbeats();
      assert!(heartbeats.len() >= 1);

      // Send config update
      mock_db.send_config(vec![
          TargetConfig { target: "8.8.8.8".into(), ... }
      ]).await?;

      tokio::time::sleep(Duration::from_secs(2)).await;

      // Verify ping batches received
      let batches = mock_db.get_batches();
      assert!(batches.len() >= 1);
  }
  ```
- [ ] Test configuration updates
- [ ] Test reconnection

### Evening: Error Scenarios
- [ ] Test connection failures
- [ ] Test database unavailable
- [ ] Test malformed configs
- [ ] Test component failures
- [ ] Commit: "test(collector): Add end-to-end integration tests"

---

## Day 7: Documentation and Polish

### Morning: Binary Documentation
- [ ] Create `README.md`:
  ```markdown
  # ZZPing Collector

  ## Overview
  The collector monitors network targets and reports results to the database.

  ## Configuration
  Create a `collector.ron` file:
  [example config]

  ## Running
  ```bash
  ./zzping-collector --config collector.ron
  ```

  ## TLS Certificates
  [certificate setup instructions]

  ## Troubleshooting
  [common issues]
  ```
- [ ] Document configuration options
- [ ] Add example configurations

### Afternoon: User Guide
- [ ] Write deployment guide
- [ ] Document privilege requirements (CAP_NET_RAW)
- [ ] Create systemd service file example
- [ ] Add monitoring/observability guide

### Evening: Final Review
- [ ] Run full test suite
- [ ] Check code coverage
- [ ] Verify all examples work
- [ ] Update main README.md
- [ ] Create PR: "feat(collector): Complete collector application"

---

## Success Criteria

### Functionality
- [ ] Collector connects to database via mTLS
- [ ] All components start successfully
- [ ] Configuration updates flow from database
- [ ] Pings execute based on configuration
- [ ] Results sent to database
- [ ] Heartbeats sent periodically
- [ ] Graceful shutdown works
- [ ] Reconnection works after disconnect

### Code Quality
- [ ] All tests pass
- [ ] >85% code coverage for service.rs
- [ ] No compiler warnings
- [ ] No clippy warnings
- [ ] Follows coding standards

### Documentation
- [ ] README complete
- [ ] Configuration documented
- [ ] Deployment guide written
- [ ] Example configs provided

### Integration
- [ ] Works with mock database
- [ ] All component interactions verified
- [ ] Error scenarios handled
- [ ] Logs are informative

---

## Example Configuration File

```ron
CollectorConfig(
    collector_id: "collector-01",
    database_host: "database.local",
    database_port: 9090,
    cert_path: "certs/collector-01.pem",
    key_path: "certs/collector-01.key",
    ca_cert_path: "certs/ca.pem",
    heartbeat_interval_secs: 5,
    default_targets: [
        TargetConfig(
            target: "8.8.8.8",
            rate_ms: 1000,
            timeout_ms: 500,
        ),
        TargetConfig(
            target: "1.1.1.1",
            rate_ms: 1000,
            timeout_ms: 500,
        ),
    ],
    default_rate_ms: 1000,
    default_timeout_ms: 500,
    memdb_buffer_size: 1000,
    memdb_batch_size: 50,
)
```

---

## Quick Commands Reference

```bash
# Build
cargo build --release --bin zzping-collector

# Run
./target/release/zzping-collector --config collector.ron

# Run with debug logging
./target/release/zzping-collector --config collector.ron --debug

# Dry run (no database connection)
./target/release/zzping-collector --config collector.ron --dry-run

# Run tests
cargo test --package zzping-collector

# Check binary size
ls -lh target/release/zzping-collector
```

---

## Week 4 Completion Checklist

- [ ] All Day 1-7 tasks completed
- [ ] All Success Criteria met
- [ ] PR created and ready for review
- [ ] Binary runs successfully
- [ ] Documentation complete
- [ ] Ready for Phase 5 (Database Application)

---

**Good luck!** 🚀
