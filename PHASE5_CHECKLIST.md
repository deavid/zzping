# Phase 5 Implementation Checklist: Database Application

**Target:** Week 5 (Following Phase 4 Collector Application completion)
**Current Status:** Ready to begin - Phases 1-4 complete
**Goal:** Create a working database binary that receives, stores, and manages ping data

---

## Overview

The database application is the main binary that:
- Receives ping data from multiple collectors
- Manages configuration distribution to collectors
- Tracks collector health and availability
- Persists ping data to disk
- Acts as the authority for the monitoring system

**What Makes This Different from Old Code:**
- Uses ZZNet (typed messages) instead of gRPC
- Components use database role configurations
- SessionManager handles multiple collector connections
- Same component code as collector, different role

---

## Week 5 Task List - Copy this and check off as you go!

---

## Day 1: Application Structure and Configuration

### Morning: Create Binary Crate
- [ ] Create directory: `src/apps/zzping-database/`
- [ ] Create `Cargo.toml`:
  ```toml
  [package]
  name = "zzping-database"
  version = "0.2.0"
  edition = "2021"

  [[bin]]
  name = "zzping-database"
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
  chrono = "0.4"

  # ZZNet crates
  zznet-api = { path = "../../net/zznet-api" }
  zznet-session = { path = "../../net/zznet-session" }
  zznet-auth = { path = "../../net/zznet-auth" }
  zznet-builder = { path = "../../net/zznet-builder" }
  zznet-transport-tcp = { path = "../../net/zznet-transport-tcp" }

  # Component crates
  zzintent-config = { path = "../../components/zzintent-config" }
  zzmem-db = { path = "../../components/zzmem-db" }
  zzcollector-state = { path = "../../components/zzcollector-state" }

  [dev-dependencies]
  tempfile = "3.0"
  ```
- [ ] Create `src/main.rs` with basic CLI
- [ ] Create `src/lib.rs` for testable logic
- [ ] Create modules: `config.rs`, `service.rs`, `cli.rs`, `persistence.rs`

### Afternoon: Configuration Structure
- [ ] In `config.rs`, define:
  ```rust
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct DatabaseConfig {
      /// Server binding address
      pub bind_host: String,
      pub bind_port: u16,

      /// TLS configuration
      pub cert_path: String,
      pub key_path: String,
      pub ca_cert_path: String,

      /// Storage configuration
      pub data_dir: String,
      pub max_results_per_target: usize,
      pub flush_interval_secs: u64,

      /// Collector management
      pub stale_timeout_secs: u64,
      pub max_collectors: Option<usize>,

      /// Intent configuration file (distributed to collectors)
      pub intent_config_path: String,
  }
  ```
- [ ] Implement `DatabaseConfig::load(path: &str) -> Result<Self>`
- [ ] Add validation methods
- [ ] Write config loading tests

### Evening: CLI Interface
- [ ] In `cli.rs`, define:
  ```rust
  #[derive(Parser, Debug)]
  #[command(author, version, about, long_about = None)]
  pub struct Cli {
      /// Path to database configuration file
      #[arg(short, long, default_value = "database.ron")]
      pub config: String,

      /// Enable debug logging
      #[arg(short, long)]
      pub debug: bool,

      /// Initialize data directory and exit
      #[arg(long)]
      pub init: bool,
  }
  ```
- [ ] Set up logging configuration
- [ ] Write CLI parsing tests
- [ ] Commit: "feat(database): Add application structure and configuration"

---

## Day 2: Service Orchestration (Server Side)

### Morning: Service Structure
- [ ] In `service.rs`, create:
  ```rust
  pub struct DatabaseService {
      config: DatabaseConfig,
      // Component handles (created after start)
      intent_config: Option<IntentConfigHandle>,
      memdb: Option<MemDBHandle>,
      cstate: Option<CStateHandle>,
      // SessionManager (server mode)
      session_manager: Option<Rc<SessionManager<...>>>,
      // TLS acceptor
      tls_acceptor: Option<TlsAcceptor>,
      // Shutdown signal
      shutdown_rx: tokio::sync::watch::Receiver<bool>,
  }
  ```
- [ ] Implement `new(config: DatabaseConfig) -> Result<Self>`
- [ ] Set up shutdown signal channel

### Afternoon: Component Builders (Database Roles)
- [ ] Create component builders with database roles:
  ```rust
  pub fn initialize_components(&self) -> Result<ComponentBuilders> {
      let intent_config_builder = IntentConfigBuilder::new(
          IntentConfigRole::Database {
              config_path: self.config.intent_config_path.clone(),
          }
      );

      let memdb_builder = MemDBBuilder::new(
          MemDBRole::Database {
              max_results_per_target: self.config.max_results_per_target,
          }
      );

      let cstate_builder = CStateBuilder::new(
          CStateRole::Database {
              stale_timeout_secs: self.config.stale_timeout_secs,
              max_collectors: self.config.max_collectors,
          }
      );

      Ok(ComponentBuilders {
          intent_config: intent_config_builder,
          memdb: memdb_builder,
          cstate: cstate_builder,
      })
  }
  ```
- [ ] Write builder creation tests

### Evening: SessionManager Server Setup
- [ ] Create SessionManager in server mode:
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

      tracing::info!("SessionManager configured with {} rooms",
          session_manager.room_count());

      Rc::new(session_manager)
  }
  ```
- [ ] Write SessionManager creation tests
- [ ] Commit: "feat(database): Implement server-side component setup"

---

## Day 3: TLS Server and Connection Handling

### Morning: TLS Server Setup
- [ ] Implement TLS acceptor:
  ```rust
  pub async fn setup_tls_server(&mut self) -> Result<()> {
      // Load server certificates
      let identity = load_identity(
          &self.config.cert_path,
          &self.config.key_path,
      )?;

      let ca_cert = load_ca_cert(&self.config.ca_cert_path)?;

      // Create TLS acceptor with client auth
      let tls_config = create_server_tls_config(identity, ca_cert)?;
      self.tls_acceptor = Some(TlsAcceptor::from(tls_config));

      tracing::info!("TLS server configured");
      Ok(())
  }
  ```
- [ ] Implement certificate loading helpers
- [ ] Write TLS setup tests

### Afternoon: TCP Listener
- [ ] Implement connection acceptance:
  ```rust
  pub async fn run_server(&mut self) -> Result<()> {
      let addr = format!("{}:{}", self.config.bind_host, self.config.bind_port);
      let listener = TcpListener::bind(&addr).await?;

      tracing::info!("Database server listening on {}", addr);

      loop {
          tokio::select! {
              result = listener.accept() => {
                  match result {
                      Ok((stream, peer_addr)) => {
                          self.handle_connection(stream, peer_addr).await;
                      }
                      Err(e) => {
                          tracing::error!("Accept error: {}", e);
                      }
                  }
              }
              _ = self.shutdown_rx.changed() => {
                  if *self.shutdown_rx.borrow() {
                      tracing::info!("Shutdown signal received");
                      break;
                  }
              }
          }
      }

      Ok(())
  }
  ```
- [ ] Write listener tests

### Evening: Connection Handler
- [ ] Implement per-connection handler:
  ```rust
  async fn handle_connection(
      &self,
      stream: TcpStream,
      peer_addr: SocketAddr,
  ) {
      tracing::info!("New connection from {}", peer_addr);

      // Perform TLS handshake
      let tls_stream = match self.tls_acceptor
          .as_ref()
          .unwrap()
          .accept(stream)
          .await
      {
          Ok(s) => s,
          Err(e) => {
              tracing::error!("TLS handshake failed: {}", e);
              return;
          }
      };

      // Extract client identity from certificate
      let client_cn = extract_client_cn(&tls_stream)?;
      tracing::info!("Client authenticated as: {}", client_cn);

      // Create transport and register with SessionManager
      let transport = TcpTransport::from_stream(tls_stream);

      if let Some(sm) = &self.session_manager {
          if let Err(e) = sm.add_connection(&client_cn, transport) {
              tracing::error!("Failed to register connection: {}", e);
          }
      }
  }
  ```
- [ ] Write connection handler tests
- [ ] Commit: "feat(database): Implement TLS server and connection handling"

---

## Day 4: Data Persistence

### Morning: Persistence Layer
- [ ] In `persistence.rs`, create:
  ```rust
  pub struct PersistenceManager {
      data_dir: PathBuf,
      flush_interval: Duration,
      memdb_handle: MemDBHandle,
  }

  impl PersistenceManager {
      pub fn new(data_dir: PathBuf, flush_interval: Duration) -> Self { ... }

      pub async fn start_flush_loop(&self) {
          let mut interval = tokio::time::interval(self.flush_interval);
          loop {
              interval.tick().await;
              if let Err(e) = self.flush_to_disk().await {
                  tracing::error!("Flush failed: {}", e);
              }
          }
      }

      async fn flush_to_disk(&self) -> Result<()> {
          // Query all data from MemDB
          let data = self.memdb_handle.query_all().await?;

          // Write to disk in daily files
          for (target, results) in data {
              self.write_target_data(&target, &results).await?;
          }

          Ok(())
      }
  }
  ```
- [ ] Implement file format (e.g., RON, JSON, or binary)
- [ ] Write persistence tests

### Afternoon: File Management
- [ ] Implement file rotation:
  ```rust
  fn get_file_path(&self, target: &str, date: NaiveDate) -> PathBuf {
      let filename = format!("{}-{}.ron",
          sanitize_target_name(target),
          date.format("%Y%m%d")
      );
      self.data_dir.join(filename)
  }

  async fn write_target_data(
      &self,
      target: &str,
      results: &[PingResult],
  ) -> Result<()> {
      let today = Local::now().date_naive();
      let file_path = self.get_file_path(target, today);

      // Atomic write: write to temp file, then rename
      let temp_path = file_path.with_extension("tmp");
      let serialized = ron::to_string(&results)?;
      tokio::fs::write(&temp_path, serialized).await?;
      tokio::fs::rename(&temp_path, &file_path).await?;

      Ok(())
  }
  ```
- [ ] Implement old file cleanup
- [ ] Write file management tests

### Evening: Data Recovery
- [ ] Implement startup data loading:
  ```rust
  pub async fn load_existing_data(&self) -> Result<()> {
      let entries = tokio::fs::read_dir(&self.data_dir).await?;
      let today = Local::now().date_naive();

      while let Some(entry) = entries.next_entry().await? {
          let path = entry.path();

          // Only load today's files
          if let Some(date) = extract_date_from_filename(&path) {
              if date == today {
                  self.load_file(&path).await?;
              }
          }
      }

      tracing::info!("Loaded existing data from {}", self.data_dir.display());
      Ok(())
  }
  ```
- [ ] Write recovery tests
- [ ] Commit: "feat(database): Implement data persistence"

---

## Day 5: Configuration Management

### Morning: Intent Config Loading
- [ ] Load intent configuration at startup:
  ```rust
  pub async fn load_intent_config(&self) -> Result<IntentConfigData> {
      let path = &self.config.intent_config_path;

      if !Path::new(path).exists() {
          tracing::warn!("Intent config not found, creating default");
          return Ok(IntentConfigData::default());
      }

      let content = tokio::fs::read_to_string(path).await?;
      let config = ron::from_str(&content)?;

      tracing::info!("Loaded intent configuration from {}", path);
      Ok(config)
  }
  ```
- [ ] Write config loading tests

### Afternoon: Dynamic Config Updates
- [ ] Implement config update API:
  ```rust
  pub async fn update_intent_config(
      &self,
      new_config: IntentConfigData,
  ) -> Result<()> {
      // Validate new config
      new_config.validate()?;

      // Update IntentConfig component
      self.intent_config
          .as_ref()
          .unwrap()
          .update_config(new_config.clone())
          .await?;

      // Persist to disk
      let serialized = ron::to_string_pretty(&new_config, Default::default())?;
      tokio::fs::write(&self.config.intent_config_path, serialized).await?;

      tracing::info!("Intent configuration updated");
      Ok(())
  }
  ```
- [ ] Write config update tests

### Evening: Config Distribution
- [ ] Verify config distribution to collectors:
  ```rust
  #[tokio::test]
  async fn test_config_distribution() {
      let db = DatabaseService::new(test_config())?;
      db.bootstrap().await?;

      // Start mock collector
      let collector = start_mock_collector().await?;

      // Update config
      let new_config = IntentConfigData {
          targets: vec!["8.8.8.8".into(), "1.1.1.1".into()],
      };
      db.update_intent_config(new_config.clone()).await?;

      // Wait for distribution
      tokio::time::sleep(Duration::from_secs(1)).await;

      // Verify collector received it
      let collector_config = collector.get_config().await?;
      assert_eq!(collector_config.targets, new_config.targets);
  }
  ```
- [ ] Write distribution tests
- [ ] Commit: "feat(database): Implement configuration management"

---

## Day 6: Integration and Testing

### Morning: Full Bootstrap Flow
- [ ] Implement complete database startup:
  ```rust
  pub async fn run(&mut self) -> Result<()> {
      tracing::info!("Starting database service...");

      // Phase 1: Initialize components
      let mut builders = self.initialize_components()?;

      // Phase 2: Create SessionManager and wire components
      let session_manager = self.create_session_manager(&mut builders)?;
      self.wire_components(&mut builders, session_manager.clone())?;
      self.session_manager = Some(session_manager);

      // Phase 3: Start all components
      self.start_components(builders).await?;

      // Load intent configuration
      let intent_config = self.load_intent_config().await?;
      self.intent_config.as_ref().unwrap()
          .update_config(intent_config).await?;

      // Load existing data
      self.load_existing_data().await?;

      // Setup TLS server
      self.setup_tls_server().await?;

      // Start persistence loop
      tokio::spawn(self.start_persistence_loop());

      // Run server
      self.run_server().await?;

      // Shutdown
      self.shutdown().await?;

      tracing::info!("Database service stopped");
      Ok(())
  }
  ```
- [ ] Write bootstrap tests

### Afternoon: Multi-Collector Test
- [ ] Test multiple collectors:
  ```rust
  #[tokio::test]
  async fn test_multiple_collectors() {
      let db = start_database().await?;

      // Start 3 collectors
      let collector1 = start_collector("collector-01", &db.addr()).await?;
      let collector2 = start_collector("collector-02", &db.addr()).await?;
      let collector3 = start_collector("collector-03", &db.addr()).await?;

      // Wait for registration
      tokio::time::sleep(Duration::from_secs(2)).await;

      // Query active collectors
      let collectors = db.query_collectors().await?;
      assert_eq!(collectors.len(), 3);

      // Update config
      db.update_config(new_targets()).await?;

      // Verify all collectors received it
      tokio::time::sleep(Duration::from_secs(1)).await;
      assert!(collector1.has_config_update());
      assert!(collector2.has_config_update());
      assert!(collector3.has_config_update());
  }
  ```
- [ ] Test collector disconnection/reconnection
- [ ] Test data persistence across restart

### Evening: Error Scenarios
- [ ] Test invalid certificates
- [ ] Test disk full scenarios
- [ ] Test corrupted data files
- [ ] Test component failures
- [ ] Commit: "test(database): Add comprehensive integration tests"

---

## Day 7: Documentation and Polish

### Morning: Binary Documentation
- [ ] Create `README.md`:
  ```markdown
  # ZZPing Database

  ## Overview
  The database receives ping data from collectors and manages configuration.

  ## Configuration
  Create a `database.ron` file:
  [example config]

  ## Running
  ```bash
  # Initialize data directory
  ./zzping-database --init

  # Start server
  ./zzping-database --config database.ron
  ```

  ## Certificate Setup
  [TLS certificate instructions]

  ## Data Storage
  [file format and location]
  ```
- [ ] Document configuration options
- [ ] Add example configurations

### Afternoon: Operations Guide
- [ ] Write backup/restore procedures
- [ ] Document monitoring metrics
- [ ] Create systemd service file
- [ ] Add security best practices

### Evening: Final Review
- [ ] Run full test suite
- [ ] Check code coverage
- [ ] Verify all examples work
- [ ] Update main documentation
- [ ] Create PR: "feat(database): Complete database application"

---

## Success Criteria

### Functionality
- [ ] Server accepts mTLS connections from collectors
- [ ] All components start successfully
- [ ] Multiple collectors can connect
- [ ] Configuration distributes to collectors
- [ ] Ping data received and stored
- [ ] Collector health tracked
- [ ] Data persists to disk
- [ ] Data survives restarts
- [ ] Graceful shutdown works

### Code Quality
- [ ] All tests pass
- [ ] >85% code coverage
- [ ] No compiler warnings
- [ ] No clippy warnings
- [ ] Follows coding standards

### Documentation
- [ ] README complete
- [ ] Configuration documented
- [ ] Operations guide written
- [ ] Example configs provided

### Integration
- [ ] Works with real collectors
- [ ] Multi-collector scenarios work
- [ ] Persistence tested
- [ ] Error scenarios handled

---

## Example Configuration File

```ron
DatabaseConfig(
    bind_host: "0.0.0.0",
    bind_port: 9090,
    cert_path: "certs/database.pem",
    key_path: "certs/database.key",
    ca_cert_path: "certs/ca.pem",
    data_dir: "/var/lib/zzping/data",
    max_results_per_target: 10000,
    flush_interval_secs: 60,
    stale_timeout_secs: 15,
    max_collectors: Some(100),
    intent_config_path: "/etc/zzping/intent.ron",
)
```

---

## Quick Commands Reference

```bash
# Build
cargo build --release --bin zzping-database

# Initialize data directory
./target/release/zzping-database --init --config database.ron

# Run
./target/release/zzping-database --config database.ron

# Run with debug logging
./target/release/zzping-database --config database.ron --debug

# Run tests
cargo test --package zzping-database

# Check data files
ls -lh /var/lib/zzping/data/
```

---

## Week 5 Completion Checklist

- [ ] All Day 1-7 tasks completed
- [ ] All Success Criteria met
- [ ] PR created and ready for review
- [ ] Binary runs successfully
- [ ] Documentation complete
- [ ] Ready for Phase 6 (Full Integration)

---

**Good luck!** 🚀
