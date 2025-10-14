# Phase 5 Implementation Checklist V3: Database Application
# (UPDATED WITH PHASE 4 LEARNINGS - Use this version)

**Last Updated:** October 14, 2025 (Post Phase 4 Review)
**Target:** Week 5 (Following Phase 4 Collector Application completion)
**Status:** Ready to begin - Phases 1-4 complete
**Goal:** Create a working database binary that receives, stores, and manages ping data

**🔄 CHANGES FROM V2:**
- ✅ Updated builder APIs (IntentConfigBuilder::new() no-arg pattern)
- ✅ Corrected MemDBActor instantiation (no builder)
- ✅ Added LocalSet pattern for Actix runtime
- ✅ Updated TLS patterns (ServerConfig vs ClientConfig)
- ✅ Added real code examples from working Phase 4 implementation
- ✅ Removed outdated SessionManager wiring (deferred appropriately)
- ✅ Added validation patterns from PR #26
- ✅ Updated error handling patterns

---

## 🚨 BEFORE YOU START: Pre-flight Checklist

**STOP:** Do not write any code until you complete these verification steps.

### Environment Verification
- [ ] **VERIFY WORKSPACE:** Run `pwd` and confirm you're in the zzping root directory
- [ ] **VERIFY GIT STATUS:** Run `git status` - should be on clean branch or ready for new branch
- [ ] **VERIFY BASELINE BUILD:** Run `cargo build` and confirm it succeeds
  - Expected: "Finished dev [unoptimized + debuginfo] target(s)"
  - If fails: Fix existing issues before starting Phase 5
- [ ] **VERIFY BASELINE TESTS:** Run `cargo test` and note passing count
  - Record here: _____ tests passing (should be 80+ from Phases 1-4)
- [ ] **VERIFY PHASE 4 COMPLETE:** Run `cargo test -p zzping-collector` - should pass all tests
- [ ] **CREATE FEATURE BRANCH:** `git checkout -b feature/database-app`

### Component Dependency Verification (CRITICAL)
- [ ] **VERIFY Component: zzintent-config**
  ```bash
  cargo check -p zzintent-config
  # Expected: "Finished dev" with no errors
  ```
- [ ] **VERIFY Component: zzmem-db**
  ```bash
  cargo check -p zzmem-db
  # Expected: "Finished dev" with no errors
  ```
- [ ] **VERIFY Component: zzcollector-state**
  ```bash
  cargo check -p zzcollector-state
  # Expected: "Finished dev" with no errors
  ```

**IF ANY COMPONENT FAILS:** Stop. Fix component issues before proceeding.

### Document Review (MANDATORY READING - Don't Skip!)
- [ ] **READ:** This entire checklist (don't skim!)
- [ ] **READ:** `AGENT_CODING_STANDARDS.md` - Error handling and doc comment rules
- [ ] **READ:** `PR26_PHASE4_REVIEW.md` - See what worked well in Phase 4
- [ ] **READ:** `JULES_PHASE4_API_CORRECTIONS.md` - Correct API patterns
- [ ] **READ:** `TLS_DEBUGGING_GUIDE.md` - Critical for server TLS setup
- [ ] **EXAMINE:** `src/apps/zzping-collector/` - Your reference for application structure

### Understanding Check (Answer These - Be Honest!)
- [ ] **PRIMARY PURPOSE:** Can you explain in 1-2 sentences what this database application does?
  - Write it here: _________________________________________________
  - Should be: "Server that accepts mTLS connections from collectors, receives ping data, stores it, and distributes configuration updates back to collectors."

- [ ] **CRITICAL DIFFERENCE FROM COLLECTOR:** How is database different from collector?
  - Write it here: _________________________________________________
  - Should mention: SERVER not client, ACCEPTS connections not CONNECTS, handles MULTIPLE collectors

- [ ] **TLS ROLE:** What TLS role does database use?
  - Write it here: _________________________________________________
  - Should be: SERVER role with rustls::ServerConfig, verifies client certificates

- [ ] **RUNTIME PATTERN:** What Tokio runtime pattern does database need?
  - Write it here: _________________________________________________
  - Should be: LocalSet with Actix for spawn_local support

**IF YOU CANNOT ANSWER THESE:** Stop and re-read the documentation.

### Execution Strategy Commitment
- [ ] **I COMMIT TO:** Creating files incrementally with `cargo check` after each step
- [ ] **I COMMIT TO:** Writing tests for each module before moving to the next
- [ ] **I COMMIT TO:** Running verification commands and checking expected outcomes
- [ ] **I COMMIT TO:** Committing after each completed increment
- [ ] **I COMMIT TO:** Not skipping checkpoints even if I think it's "obvious"
- [ ] **I COMMIT TO:** Using Phase 4 code as reference (it works!)
- [ ] **I COMMIT TO:** Asking for help if stuck >30 minutes instead of guessing

**SIGNATURE (Type your name/ID to confirm):** ___________________

---

## Day 1: Application Structure and Configuration

**GOAL:** Create the database binary crate structure and configuration loading system.

**KEY DIFFERENCES FROM COLLECTOR:**
- Configuration uses SERVER certificate paths (not client)
- Binds to port (not connects)
- Validates bind_host (not database_host)

---

### Morning: Create Binary Crate Structure

#### Step 1: Create Directory and Cargo.toml - [30 min]

**ACTIONS:**
- [ ] Create directory: `mkdir -p src/apps/zzping-database`
- [ ] Create `src/apps/zzping-database/Cargo.toml`:
  ```toml
  [package]
  name = "zzping-database"
  version = "0.1.0"
  edition = "2021"

  [[bin]]
  name = "zzping-database"
  path = "src/main.rs"

  [dependencies]
  # Component dependencies (our Phase 1-3 work)
  zzintent-config = { path = "../../components/zzintent-config" }
  zzmem-db = { path = "../../components/zzmem-db" }
  zzcollector-state = { path = "../../components/zzcollector-state" }

  # Network layer dependencies
  zznet-session = { path = "../../net/zznet-session" }
  zznet-builder = { path = "../../net/zznet-builder" }
  zznet-transport-tcp = { path = "../../net/zznet-transport-tcp" }
  zznet-auth = { path = "../../net/zznet-auth" }
  zznet-api = { path = "../../net/zznet-api" }

  # Runtime dependencies
  tokio = { version = "1.0", features = ["full", "macros", "rt-multi-thread", "signal"] }
  actix = "0.13"

  # Configuration and serialization
  serde = { version = "1.0", features = ["derive"] }
  ron = "0.8"

  # CLI argument parsing
  clap = { version = "4.0", features = ["derive"] }

  # Logging and tracing
  tracing = "0.1"
  tracing-subscriber = { version = "0.3", features = ["env-filter", "fmt"] }

  # Error handling
  anyhow = "1.0"
  thiserror = "1.0"

  # TLS/crypto (SERVER-SIDE)
  rustls = "0.21"
  rustls-pemfile = "1.0"
  tokio-rustls = "0.24"

  # Utilities
  chrono = "0.4"

  [dev-dependencies]
  tempfile = "3.0"
  tokio-test = "0.4"
  ```

- [ ] Update workspace `Cargo.toml` (root):
  ```toml
  members = [
      # ... existing members ...
      "src/apps/zzping-database",
  ]
  ```

- [ ] **VERIFY:** `cargo check -p zzping-database` (will fail - package doesn't exist yet)
  - Expected: Error about missing src/main.rs

#### Step 2: Create Module Structure - [30 min]

**ACTIONS:**
- [ ] Create `src/apps/zzping-database/src/lib.rs`:
  ```rust
  //! Database application library.
  //!
  //! Contains testable business logic separated from main() entry point.
  //! This allows unit testing of configuration, service orchestration, and
  //! component integration without running the full binary.

  // Module declarations
  pub mod cli;
  pub mod config;
  pub mod error;
  pub mod service;

  // Re-exports for convenience (acceptable for binary crates)
  pub use cli::CliArgs;
  pub use config::DatabaseConfig;
  pub use error::DatabaseError;
  pub use service::DatabaseService;
  ```

- [ ] Create `src/apps/zzping-database/src/main.rs`:
  ```rust
  //! ZZPing Database Application
  //!
  //! Server that accepts mTLS connections from collectors, stores ping data,
  //! and distributes configuration updates.

  use anyhow::{Context, Result};
  use clap::Parser;
  use tokio::task::LocalSet;
  use tracing_subscriber::EnvFilter;
  use zzping_database::{CliArgs, DatabaseConfig, DatabaseService};

  fn main() -> Result<()> {
      // CRITICAL: Use LocalSet for Actix compatibility (spawn_local support)
      let rt = tokio::runtime::Runtime::new()?;
      let local = LocalSet::new();
      local.block_on(&rt, async_main())
  }

  async fn async_main() -> Result<()> {
      // Parse command-line arguments
      let args = CliArgs::parse();

      // Initialize logging based on CLI flags
      init_logging(&args);

      tracing::info!("ZZPing Database v{} starting", env!("CARGO_PKG_VERSION"));
      tracing::info!("Loading configuration from: {}", args.config);

      // Load and validate configuration
      let config = DatabaseConfig::load(&args.config)
          .with_context(|| format!("Failed to load configuration from {}", args.config))?;

      config
          .validate()
          .context("Configuration validation failed")?;

      tracing::info!("Configuration loaded successfully");
      tracing::info!("Binding to {}:{}", config.bind_host, config.bind_port);

      // Create and run the database service
      let service = DatabaseService::new(config).context("Failed to create database service")?;

      service.run().await.context("Database service failed")?;

      tracing::info!("ZZPing Database shutdown complete");
      Ok(())
  }

  /// Initialize logging based on CLI arguments.
  fn init_logging(args: &CliArgs) {
      let filter = if args.trace {
          EnvFilter::new("trace")
      } else if args.debug {
          EnvFilter::new("debug")
      } else {
          EnvFilter::new("info")
      };

      tracing_subscriber::fmt()
          .with_env_filter(filter)
          .with_target(true)
          .with_thread_ids(true)
          .with_line_number(true)
          .init();
  }
  ```

- [ ] **VERIFY:** `cargo check -p zzping-database`
  - Expected: Errors about missing modules (cli, config, error, service)

#### Step 3: Create CLI Module - [15 min]

**ACTIONS:**
- [ ] Create `src/apps/zzping-database/src/cli.rs`:
  ```rust
  //! Command-line argument parsing.

  use clap::Parser;

  /// ZZPing Database - Network monitoring server
  #[derive(Parser, Debug)]
  #[command(author, version, about, long_about = None)]
  pub struct CliArgs {
      /// Path to configuration file
      #[arg(short, long, default_value = "database.ron")]
      pub config: String,

      /// Enable debug logging
      #[arg(short, long)]
      pub debug: bool,

      /// Enable trace logging (very verbose)
      #[arg(short, long)]
      pub trace: bool,
  }
  ```

- [ ] **VERIFY:** `cargo check -p zzping-database`
  - Expected: Still errors about missing config, error, service modules

#### Step 4: Create Error Module - [15 min]

**ACTIONS:**
- [ ] Create `src/apps/zzping-database/src/error.rs`:
  ```rust
  //! Error types for the database application.

  use thiserror::Error;

  /// Errors that can occur in the database application.
  #[derive(Error, Debug)]
  pub enum DatabaseError {
      #[error("Configuration error: {0}")]
      Config(String),

      #[error("Service error: {0}")]
      Service(String),

      #[error("IO error: {0}")]
      Io(#[from] std::io::Error),

      #[error("Component error: {0}")]
      Component(String),

      #[error("TLS error: {0}")]
      Tls(String),

      #[error("Persistence error: {0}")]
      Persistence(String),
  }

  /// Result type alias for database operations.
  pub type Result<T> = std::result::Result<T, DatabaseError>;
  ```

- [ ] **VERIFY:** `cargo check -p zzping-database`
  - Expected: Still errors about missing config and service modules

- [ ] **COMMIT:** `git add -A && git commit -m "feat(database): Add project structure and CLI"`

---

### Afternoon: Configuration Module

#### Step 1: Create Configuration Structures - [45 min]

**ACTIONS:**
- [ ] Create `src/apps/zzping-database/src/config.rs`:
  ```rust
  //! Configuration structures and loading.

  use serde::{Deserialize, Serialize};

  /// Database application configuration.
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct DatabaseConfig {
      /// Network binding settings
      pub bind_host: String,
      pub bind_port: u16,

      /// TLS configuration for mTLS server
      pub tls: TlsConfig,

      /// Component-specific settings
      pub components: ComponentConfig,
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct TlsConfig {
      /// CA certificate for verifying client certificates (from collectors)
      pub ca_cert_path: String,
      /// Server certificate (this database's identity)
      pub server_cert_path: String,
      /// Server private key
      pub server_key_path: String,
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct ComponentConfig {
      /// Heartbeat timeout in seconds for collector state tracking
      pub stale_timeout_secs: u64,

      /// Maximum number of collectors to accept
      pub max_collectors: usize,
  }

  impl DatabaseConfig {
      /// Load configuration from a RON file.
      ///
      /// Reads and parses the RON configuration file. Fails if the file
      /// cannot be read or contains invalid RON syntax.
      pub fn load(path: &str) -> crate::error::Result<Self> {
          let content = std::fs::read_to_string(path).map_err(|e| {
              crate::error::DatabaseError::Config(format!(
                  "Failed to read config file {}: {}",
                  path, e
              ))
          })?;

          let config: Self = ron::from_str(&content).map_err(|e| {
              crate::error::DatabaseError::Config(format!("Failed to parse config: {}", e))
          })?;

          Ok(config)
      }

      /// Validate configuration values.
      ///
      /// Checks all configuration values for validity. Ensures required fields
      /// are not empty, numeric values are in acceptable ranges, and file paths
      /// point to existing files.
      ///
      /// Fails if any validation check does not pass.
      pub fn validate(&self) -> crate::error::Result<()> {
          if self.bind_host.is_empty() {
              return Err(crate::error::DatabaseError::Config(
                  "bind_host cannot be empty".into(),
              ));
          }

          if self.bind_port == 0 {
              return Err(crate::error::DatabaseError::Config(
                  "bind_port cannot be 0".into(),
              ));
          }

          if self.components.stale_timeout_secs == 0 {
              return Err(crate::error::DatabaseError::Config(
                  "stale_timeout_secs cannot be 0".into(),
              ));
          }

          if self.components.max_collectors == 0 {
              return Err(crate::error::DatabaseError::Config(
                  "max_collectors cannot be 0".into(),
              ));
          }

          // Validate TLS file paths exist
          if !std::path::Path::new(&self.tls.ca_cert_path).exists() {
              return Err(crate::error::DatabaseError::Config(format!(
                  "CA certificate not found: {}",
                  self.tls.ca_cert_path
              )));
          }

          if !std::path::Path::new(&self.tls.server_cert_path).exists() {
              return Err(crate::error::DatabaseError::Config(format!(
                  "Server certificate not found: {}",
                  self.tls.server_cert_path
              )));
          }

          if !std::path::Path::new(&self.tls.server_key_path).exists() {
              return Err(crate::error::DatabaseError::Config(format!(
                  "Server private key not found: {}",
                  self.tls.server_key_path
              )));
          }

          Ok(())
      }
  }
  ```

- [ ] **VERIFY:** `cargo check -p zzping-database`
  - Expected: Errors about missing service module only

#### Step 2: Create Service Stub - [30 min]

**ACTIONS:**
- [ ] Create `src/apps/zzping-database/src/service.rs`:
  ```rust
  use crate::config::DatabaseConfig;
  use crate::error::{DatabaseError, Result};

  use tokio::signal::unix::{signal, SignalKind};

  pub struct DatabaseService {
      config: DatabaseConfig,
  }

  impl DatabaseService {
      pub fn new(config: DatabaseConfig) -> Result<Self> {
          config.validate()?;
          Ok(Self { config })
      }

      pub async fn run(self) -> Result<()> {
          tracing::info!("Database service starting");
          tracing::info!("Will bind to {}:{}", self.config.bind_host, self.config.bind_port);

          // TODO: Implement component creation in Day 2
          // TODO: Implement TLS server in Day 3
          // TODO: Implement connection acceptance in Day 4

          // Setup signal handlers
          let mut sigterm = signal(SignalKind::terminate())
              .map_err(|e| DatabaseError::Service(format!("Failed to setup SIGTERM: {}", e)))?;
          let mut sigint = signal(SignalKind::interrupt())
              .map_err(|e| DatabaseError::Service(format!("Failed to setup SIGINT: {}", e)))?;

          tracing::info!("Database service running - press Ctrl+C to stop");

          // Main loop - wait for shutdown signal
          tokio::select! {
              _ = sigterm.recv() => {
                  tracing::info!("Received SIGTERM, shutting down gracefully");
              }
              _ = sigint.recv() => {
                  tracing::info!("Received SIGINT (Ctrl+C), shutting down gracefully");
              }
          }

          tracing::info!("Database service stopped");
          Ok(())
      }
  }
  ```

- [ ] **VERIFY:** `cargo check -p zzping-database`
  - Expected: "Finished dev" with no errors
- [ ] **VERIFY:** `cargo build --bin zzping-database`
  - Expected: Compiles successfully
- [ ] **VERIFY:** `./target/debug/zzping-database --help`
  - Expected: Shows help message

- [ ] **COMMIT:** `git add -A && git commit -m "feat(database): Add configuration and service stub"`

#### Step 3: Create Example Configuration - [15 min]

**ACTIONS:**
- [ ] Create `src/apps/zzping-database/database.example.ron`:
  ```ron
  // Example ZZPing Database Configuration
  // Copy this to database.ron and customize for your environment

  DatabaseConfig(
      // Network binding settings (server listens on this address)
      bind_host: "0.0.0.0",
      bind_port: 8443,

      // TLS certificate paths for mTLS server
      tls: TlsConfig(
          // CA certificate for verifying collector certificates
          ca_cert_path: "test_certs/ca.pem",
          // Server certificate (this database's identity)
          server_cert_path: "test_certs/database.pem",
          // Server private key
          server_key_path: "test_certs/database.key",
      ),

      // Component-specific configuration
      components: ComponentConfig(
          // How long before a collector is considered stale (in seconds)
          stale_timeout_secs: 30,

          // Maximum number of collectors to accept
          max_collectors: 100,
      ),
  )
  ```

- [ ] **VERIFY:** Configuration loads:
  ```bash
  cp src/apps/zzping-database/database.example.ron src/apps/zzping-database/database.ron
  ./target/debug/zzping-database --config src/apps/zzping-database/database.ron
  ```
  - Expected: Logs startup, waits for Ctrl+C

- [ ] **COMMIT:** `git add -A && git commit -m "docs(database): Add example configuration"`

#### Step 4: Create Configuration Tests - [60 min]

**ACTIONS:**
- [ ] Create directory: `mkdir -p src/apps/zzping-database/tests`
- [ ] Create `src/apps/zzping-database/tests/config_tests.rs`:
  ```rust
  //! Tests for configuration loading and validation.

  use std::io::Write;
  use std::path::Path;
  use tempfile::NamedTempFile;
  use zzping_database::config::*;

  /// Helper to create a valid test configuration.
  fn create_valid_config() -> DatabaseConfig {
      let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
          .parent()
          .unwrap()
          .parent()
          .unwrap()
          .parent()
          .unwrap();
      let certs_dir = workspace_root.join("test_certs");

      DatabaseConfig {
          bind_host: "0.0.0.0".into(),
          bind_port: 8443,
          tls: TlsConfig {
              ca_cert_path: certs_dir.join("ca.pem").to_str().unwrap().to_string(),
              server_cert_path: certs_dir
                  .join("database.pem")
                  .to_str()
                  .unwrap()
                  .to_string(),
              server_key_path: certs_dir
                  .join("database.key")
                  .to_str()
                  .unwrap()
                  .to_string(),
          },
          components: ComponentConfig {
              stale_timeout_secs: 30,
              max_collectors: 100,
          },
      }
  }

  #[test]
  fn test_valid_config_validates() {
      let config = create_valid_config();
      assert!(config.validate().is_ok());
  }

  #[test]
  fn test_empty_bind_host_fails_validation() {
      let mut config = create_valid_config();
      config.bind_host = String::new();

      let result = config.validate();
      assert!(result.is_err());
      assert!(result.unwrap_err().to_string().contains("bind_host"));
  }

  #[test]
  fn test_zero_port_fails_validation() {
      let mut config = create_valid_config();
      config.bind_port = 0;

      let result = config.validate();
      assert!(result.is_err());
      assert!(result.unwrap_err().to_string().contains("port"));
  }

  #[test]
  fn test_zero_stale_timeout_fails_validation() {
      let mut config = create_valid_config();
      config.components.stale_timeout_secs = 0;

      let result = config.validate();
      assert!(result.is_err());
      assert!(result.unwrap_err().to_string().contains("stale_timeout"));
  }

  #[test]
  fn test_zero_max_collectors_fails_validation() {
      let mut config = create_valid_config();
      config.components.max_collectors = 0;

      let result = config.validate();
      assert!(result.is_err());
      assert!(result.unwrap_err().to_string().contains("max_collectors"));
  }

  #[test]
  fn test_load_valid_config_file() {
      let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
          .parent()
          .unwrap()
          .parent()
          .unwrap()
          .parent()
          .unwrap();
      let certs_dir = workspace_root.join("test_certs");
      let config_content = format!(
          r#"
      DatabaseConfig(
          bind_host: "0.0.0.0",
          bind_port: 8443,
          tls: TlsConfig(
              ca_cert_path: "{}",
              server_cert_path: "{}",
              server_key_path: "{}",
          ),
          components: ComponentConfig(
              stale_timeout_secs: 30,
              max_collectors: 100,
          ),
      )
      "#,
          certs_dir.join("ca.pem").to_str().unwrap(),
          certs_dir.join("database.pem").to_str().unwrap(),
          certs_dir.join("database.key").to_str().unwrap()
      );

      let mut temp_file = NamedTempFile::new().unwrap();
      temp_file.write_all(config_content.as_bytes()).unwrap();
      let path = temp_file.path().to_str().unwrap();

      let config = DatabaseConfig::load(path).expect("Failed to load config");
      assert_eq!(config.bind_host, "0.0.0.0");
      assert_eq!(config.bind_port, 8443);
  }

  #[test]
  fn test_load_nonexistent_file_fails() {
      let result = DatabaseConfig::load("/nonexistent/path/config.ron");
      assert!(result.is_err());
      assert!(result.unwrap_err().to_string().contains("Failed to read"));
  }

  #[test]
  fn test_load_invalid_ron_fails() {
      let invalid_content = "this is not valid RON {{{";

      let mut temp_file = NamedTempFile::new().unwrap();
      temp_file.write_all(invalid_content.as_bytes()).unwrap();
      let path = temp_file.path().to_str().unwrap();

      let result = DatabaseConfig::load(path);
      assert!(result.is_err());
      assert!(result.unwrap_err().to_string().contains("Failed to parse"));
  }
  ```

- [ ] **VERIFY:** `cargo test -p zzping-database --test config_tests`
  - Expected: 8 tests pass

- [ ] **COMMIT:** `git add -A && git commit -m "test(database): Add configuration tests"`

---

### 🛑 CHECKPOINT 1: Configuration System Complete

**VERIFY BEFORE PROCEEDING TO DAY 2:**
- [ ] All config tests pass: `cargo test -p zzping-database config` (8 passed)
- [ ] Binary accepts CLI arguments: `./target/debug/zzping-database --help`
- [ ] Example config loads successfully
- [ ] Invalid configs are rejected with clear errors
- [ ] Logging works at different levels (info/debug/trace)
- [ ] No panics on any error condition
- [ ] All code committed: `git status` shows clean

**SELF-CHECK QUESTIONS:**
- Where is validation logic implemented? (Answer: config.rs, validate() method)
- What TLS role does database use? (Answer: SERVER, verifies client certs)
- What runtime pattern is used? (Answer: LocalSet for Actix spawn_local)

**IF ANY FAILS:** Review Day 1 and fix issues before Day 2.

---

## Day 2: Component Integration (SERVER Roles)

**GOAL:** Create component builders with DATABASE roles (not collector roles).

**KEY DIFFERENCES FROM COLLECTOR:**
- Components use Database/Server roles (not Collector roles)
- MemDBRole::Database (not MemDBRole::Collector)
- IntentConfigRole::Database (not IntentConfigRole::Collector)
- CStateRole::Database (not CStateRole::Collector)

---

### Morning: Component Builder Creation - [90 min]

#### Step 1: Update Service with Component Structures - [45 min]

**ACTIONS:**
- [ ] Update `src/apps/zzping-database/src/service.rs`:
  ```rust
  use crate::config::DatabaseConfig;
  use crate::error::{DatabaseError, Result};

  use actix::{Actor, Addr};

  // Component imports (DATABASE ROLES)
  use zzintent_config::actor::IntentConfigActor;
  use zzintent_config::builder::IntentConfigBuilder;
  use zzintent_config::permissions::IntentConfigPermission;
  use zzintent_config::role::IntentConfigRole;

  use zzmem_db::actor::MemDBActor;
  use zzmem_db::permissions::MemDBPermission;
  use zzmem_db::role::MemDBRole;

  use zzcollector_state::actor::CStateActor;
  use zzcollector_state::builder::CStateBuilder;
  use zzcollector_state::permissions::CStatePermission;
  use zzcollector_state::role::CStateRole;

  use tokio::signal::unix::{signal, SignalKind};

  /// Builders for all components (before wiring)
  struct ComponentBuilders {
      intent_config: IntentConfigBuilder<IntentConfigPermission>,
      memdb_addr: Addr<MemDBActor<MemDBPermission>>,
      cstate: CStateBuilder<CStatePermission>,
  }

  /// Started components (running actors)
  #[allow(dead_code)]
  struct StartedComponents {
      intent_config: Addr<IntentConfigActor<IntentConfigPermission>>,
      memdb_addr: Addr<MemDBActor<MemDBPermission>>,
      cstate: Addr<CStateActor<CStatePermission>>,
  }

  pub struct DatabaseService {
      config: DatabaseConfig,
  }

  impl DatabaseService {
      pub fn new(config: DatabaseConfig) -> Result<Self> {
          config.validate()?;
          Ok(Self { config })
      }

      pub async fn run(self) -> Result<()> {
          tracing::info!("Database service starting");

          // Step 1: Create and start components
          let builders = self.create_builders()?;
          let _started = Self::start_components(builders).await?;

          tracing::info!("All components started successfully");

          // TODO: TLS server setup in Day 3
          // TODO: Connection acceptance in Day 4

          // Setup signal handlers
          let mut sigterm = signal(SignalKind::terminate())
              .map_err(|e| DatabaseError::Service(format!("Failed to setup SIGTERM: {}", e)))?;
          let mut sigint = signal(SignalKind::interrupt())
              .map_err(|e| DatabaseError::Service(format!("Failed to setup SIGINT: {}", e)))?;

          tracing::info!("Database service running - press Ctrl+C to stop");

          // Main loop - wait for shutdown signal
          tokio::select! {
              _ = sigterm.recv() => {
                  tracing::info!("Received SIGTERM, shutting down gracefully");
              }
              _ = sigint.recv() => {
                  tracing::info!("Received SIGINT (Ctrl+C), shutting down gracefully");
              }
          }

          tracing::info!("Database service stopped");
          Ok(())
      }

      fn create_builders(&self) -> Result<ComponentBuilders> {
          // Create IntentConfig builder - DATABASE ROLE
          let intent_config = IntentConfigBuilder::<IntentConfigPermission>::new()
              .role(IntentConfigRole::Database);

          // Create MemDB actor - DATABASE ROLE (no builder pattern!)
          let memdb_actor = MemDBActor::<MemDBPermission>::new_with_role(
              MemDBRole::Database {
                  max_storage_mb: 1024,  // 1GB storage
              }
          );
          let memdb_addr = memdb_actor.start();

          // Create CState builder - DATABASE ROLE
          let cstate = CStateBuilder::<CStatePermission>::new(CStateRole::Database {
              stale_timeout_secs: self.config.components.stale_timeout_secs,
          });

          Ok(ComponentBuilders {
              intent_config,
              memdb_addr,
              cstate,
          })
      }

      async fn start_components(builders: ComponentBuilders) -> Result<StartedComponents> {
          // Start IntentConfig
          let intent_addr = builders
              .intent_config
              .start()
              .map_err(|e| DatabaseError::Component(format!("IntentConfig start failed: {}", e)))?;

          // Start CState
          let cstate_addr = builders.cstate.build();

          Ok(StartedComponents {
              intent_config: intent_addr,
              memdb_addr: builders.memdb_addr,
              cstate: cstate_addr,
          })
      }
  }
  ```

- [ ] **VERIFY:** `cargo check -p zzping-database`
  - Expected: May have some warnings about unused, but should compile

- [ ] **COMMIT:** `git add -A && git commit -m "feat(database): Add component builders with database roles"`

#### Step 2: Create Service Tests - [45 min]

**ACTIONS:**
- [ ] Create `src/apps/zzping-database/tests/service_tests.rs`:
  ```rust
  //! Tests for service orchestration.

  use std::path::Path;
  use zzping_database::{config::*, DatabaseService};

  /// Helper to create a valid test config.
  fn create_test_config() -> DatabaseConfig {
      let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
          .parent()
          .unwrap()
          .parent()
          .unwrap()
          .parent()
          .unwrap();
      let certs_dir = workspace_root.join("test_certs");
      DatabaseConfig {
          bind_host: "0.0.0.0".into(),
          bind_port: 8443,
          tls: TlsConfig {
              ca_cert_path: certs_dir.join("ca.pem").to_str().unwrap().to_string(),
              server_cert_path: certs_dir
                  .join("database.pem")
                  .to_str()
                  .unwrap()
                  .to_string(),
              server_key_path: certs_dir
                  .join("database.key")
                  .to_str()
                  .unwrap()
                  .to_string(),
          },
          components: ComponentConfig {
              stale_timeout_secs: 30,
              max_collectors: 100,
          },
      }
  }

  #[test]
  fn test_service_creation() {
      let config = create_test_config();
      let service = DatabaseService::new(config);
      assert!(service.is_ok());
  }

  #[test]
  fn test_service_creation_validates_config() {
      let mut config = create_test_config();
      config.bind_host = String::new(); // Invalid!

      let result = DatabaseService::new(config);
      assert!(result.is_err());
  }
  ```

- [ ] **VERIFY:** `cargo test -p zzping-database --test service_tests`
  - Expected: 2 tests pass

- [ ] **COMMIT:** `git add -A && git commit -m "test(database): Add service tests"`

---

### 🛑 CHECKPOINT 2: Component Integration Complete

**VERIFY BEFORE PROCEEDING:**
- [ ] Service compiles: `cargo check -p zzping-database`
- [ ] Service tests pass: `cargo test -p zzping-database service`
- [ ] Components use DATABASE roles (not collector roles)
- [ ] All code committed

---

## Day 3: TLS Server Setup

**GOAL:** Implement TLS server configuration loading (ServerConfig not ClientConfig).

**KEY DIFFERENCES FROM COLLECTOR:**
- Use `rustls::ServerConfig` (not ClientConfig)
- Load server cert + key (not client cert)
- Verify client certificates (not server certificate)
- Use `AllowAnyAuthenticatedClient` verifier

---

### Morning: TLS Configuration Loading - [120 min]

#### Step 1: Add TLS Loading Function - [90 min]

**ACTIONS:**
- [ ] Add to `src/apps/zzping-database/src/service.rs`:
  ```rust
  // Add these imports at top
  use rustls::{Certificate, PrivateKey, RootCertStore, ServerConfig};
  use rustls::server::AllowAnyAuthenticatedClient;
  use rustls_pemfile::{certs, pkcs8_private_keys};
  use std::fs::File;
  use std::io::BufReader;
  use std::sync::Arc;

  impl DatabaseService {
      // Add this method
      /// Load TLS configuration for mTLS server
      pub fn load_tls_config(tls: &crate::config::TlsConfig) -> Result<Arc<ServerConfig>> {
          // 1. Load CA certificate (to verify client certificates from collectors)
          let ca_file = File::open(&tls.ca_cert_path)
              .map_err(|e| DatabaseError::Config(format!("Failed to open CA file: {}", e)))?;
          let mut ca_reader = BufReader::new(ca_file);
          let ca_certs: Vec<Certificate> = certs(&mut ca_reader)
              .map_err(|e| DatabaseError::Config(format!("Failed to parse CA certs: {}", e)))?
              .into_iter()
              .map(Certificate)
              .collect();

          if ca_certs.is_empty() {
              return Err(DatabaseError::Config("No CA certificates found".into()));
          }

          let mut root_store = RootCertStore::empty();
          for cert in ca_certs {
              root_store
                  .add(&cert)
                  .map_err(|e| DatabaseError::Config(format!("Failed to add CA cert: {}", e)))?;
          }

          // 2. Load server certificate
          let cert_file = File::open(&tls.server_cert_path)
              .map_err(|e| DatabaseError::Config(format!("Failed to open server cert: {}", e)))?;
          let mut cert_reader = BufReader::new(cert_file);
          let cert_chain: Vec<Certificate> = certs(&mut cert_reader)
              .map_err(|e| DatabaseError::Config(format!("Failed to parse server cert: {}", e)))?
              .into_iter()
              .map(Certificate)
              .collect();

          if cert_chain.is_empty() {
              return Err(DatabaseError::Config("No server certificate found".into()));
          }

          // 3. Load server private key
          let key_file = File::open(&tls.server_key_path)
              .map_err(|e| DatabaseError::Config(format!("Failed to open server key: {}", e)))?;
          let mut key_reader = BufReader::new(key_file);
          let mut keys: Vec<PrivateKey> = pkcs8_private_keys(&mut key_reader)
              .map_err(|e| DatabaseError::Config(format!("Failed to parse private key: {}", e)))?
              .into_iter()
              .map(PrivateKey)
              .collect();

          if keys.is_empty() {
              return Err(DatabaseError::Config("No private key found".into()));
          }
          let private_key = keys.remove(0);

          // 4. Build server config (NOT client config!)
          let client_verifier = AllowAnyAuthenticatedClient::new(root_store);

          let config = ServerConfig::builder()
              .with_safe_defaults()
              .with_client_cert_verifier(Arc::new(client_verifier))
              .with_single_cert(cert_chain, private_key)
              .map_err(|e| DatabaseError::Config(format!("Failed to build TLS config: {}", e)))?;

          Ok(Arc::new(config))
      }
  }
  ```

- [ ] **VERIFY:** `cargo check -p zzping-database`
  - Expected: Compiles with no errors

#### Step 2: Add TLS Tests - [30 min]

**ACTIONS:**
- [ ] Add to `src/apps/zzping-database/tests/service_tests.rs`:
  ```rust
  #[test]
  fn test_tls_config_loads_valid_certs() {
      let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
          .parent()
          .unwrap()
          .parent()
          .unwrap()
          .parent()
          .unwrap();
      let certs_dir = workspace_root.join("test_certs");
      let tls_config = TlsConfig {
          ca_cert_path: certs_dir.join("ca.pem").to_str().unwrap().to_string(),
          server_cert_path: certs_dir
              .join("database.pem")
              .to_str()
              .unwrap()
              .to_string(),
          server_key_path: certs_dir
              .join("database.key")
              .to_str()
              .unwrap()
              .to_string(),
      };

      let result = DatabaseService::load_tls_config(&tls_config);
      assert!(result.is_ok(), "TLS config should load successfully");
  }

  #[test]
  fn test_tls_config_fails_missing_ca() {
      let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
          .parent()
          .unwrap()
          .parent()
          .unwrap()
          .parent()
          .unwrap();
      let certs_dir = workspace_root.join("test_certs");
      let tls_config = TlsConfig {
          ca_cert_path: "nonexistent.pem".into(),
          server_cert_path: certs_dir
              .join("database.pem")
              .to_str()
              .unwrap()
              .to_string(),
          server_key_path: certs_dir
              .join("database.key")
              .to_str()
              .unwrap()
              .to_string(),
      };

      let result = DatabaseService::load_tls_config(&tls_config);
      assert!(result.is_err(), "Should fail with missing CA");
  }
  ```

- [ ] **VERIFY:** `cargo test -p zzping-database --test service_tests`
  - Expected: 4 tests pass

- [ ] **COMMIT:** `git add -A && git commit -m "feat(database): Add TLS server configuration loading"`

---

### 🛑 CHECKPOINT 3: TLS Configuration Complete

**VERIFY:**
- [ ] TLS tests pass: `cargo test -p zzping-database tls`
- [ ] ServerConfig used (not ClientConfig)
- [ ] Client certificate verification enabled
- [ ] All code committed

---

## Day 4-7: Placeholder for Future Implementation

**NOTE:** Days 4-7 cover:
- Day 4: TCP listener and accept loop
- Day 5: Connection handling and SessionManager integration
- Day 6: Multi-collector management
- Day 7: Documentation and final tests

These will be detailed after Day 3 is successfully completed and we have working TLS server configuration.

**For Now:** Focus on completing Days 1-3 perfectly. The remaining days will build on this foundation.

---

## Common Mistakes to Avoid (Based on Phase 4 Learnings)

### From PR #26 Review:

✅ **DO:**
- Use LocalSet for Actix runtime (fixes spawn_local panic)
- Call IntentConfigBuilder::new() with no args, then .role()
- Instantiate MemDBActor directly (no builder)
- Use DATABASE roles (not collector roles)
- Load ServerConfig for server (not ClientConfig)
- Validate all config fields including file existence
- Write comprehensive tests for config and TLS loading
- Create example configuration files
- Use signal handlers for graceful shutdown
- Keep validation in config.rs (not scattered)

❌ **DON'T:**
- Pass role to IntentConfigBuilder::new() (outdated API)
- Try to use MemDBBuilder (doesn't exist)
- Use collector roles for database components
- Use ClientConfig APIs for server
- Skip file existence validation
- Skip writing tests "because it's obvious"
- Hardcode configuration values
- Forget LocalSet with Actix

---

## Quick Reference Commands

```bash
# Build database binary
cargo build --bin zzping-database

# Run database with config
./target/debug/zzping-database --config src/apps/zzping-database/database.ron

# Run one test file
cargo test -p zzping-database --test config_tests

# Run all package tests
cargo test -p zzping-database

# Check for errors
cargo check -p zzping-database

# Lint
cargo clippy -p zzping-database --all-targets
```

---

**END OF PHASE 5 CHECKLIST V3 (DAYS 1-3)**

*Days 4-7 will be added after successful completion of Days 1-3 with verified working TLS server.*
