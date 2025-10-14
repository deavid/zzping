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

## Day 4: TCP Listener and Connection Acceptance

**GOAL:** Implement TCP listener with TLS acceptor for incoming collector connections.

**KEY CONCEPTS:**
- TcpListener binds to configured address/port
- TlsAcceptor wraps ServerConfig for mTLS
- Accept loop spawns task per connection
- Connection handler is placeholder (Day 5 work)

---

### Pre-Day 4 Checklist

**BEFORE STARTING DAY 4:**
- [ ] Days 1-3 complete and committed
- [ ] All tests passing (12+)
- [ ] Clippy clean with `#[allow(dead_code)]` on StartedComponents
- [ ] TLS loading function tested with real certificates
- [ ] Service runs and accepts shutdown signals

---

### Morning: TCP Listener Setup - [90 min]

#### Step 1: Add Network Imports and Types - [20 min]

**ACTIONS:**
- [ ] Add to `src/apps/zzping-database/src/service.rs`:
  ```rust
  // Add these imports after existing imports
  use tokio::net::{TcpListener, TcpStream};
  use tokio_rustls::TlsAcceptor;
  use std::net::SocketAddr;
  ```

- [ ] **VERIFY:** `cargo check -p zzping-database`

#### Step 2: Implement TCP Listener Creation - [30 min]

**ACTIONS:**
- [ ] Add method to `DatabaseService` in `service.rs`:
  ```rust
  impl DatabaseService {
      // ... existing methods ...

      /// Create TCP listener bound to configured address
      async fn create_listener(&self) -> Result<TcpListener> {
          let bind_addr = format!("{}:{}", self.config.bind_host, self.config.bind_port);

          tracing::info!("Binding TCP listener to {}", bind_addr);

          let listener = TcpListener::bind(&bind_addr)
              .await
              .map_err(|e| DatabaseError::Service(format!(
                  "Failed to bind to {}: {}", bind_addr, e
              )))?;

          let local_addr = listener.local_addr()
              .map_err(|e| DatabaseError::Service(format!("Failed to get local addr: {}", e)))?;

          tracing::info!("TCP listener bound successfully to {}", local_addr);

          Ok(listener)
      }
  }
  ```

- [ ] **VERIFY:** `cargo check -p zzping-database`

#### Step 3: Update run() Method with Accept Loop - [40 min]

**ACTIONS:**
- [ ] Update `DatabaseService::run()` in `service.rs`:
  ```rust
  pub async fn run(self) -> Result<()> {
      tracing::info!("Database service starting");

      // Step 1: Create and start components
      let builders = self.create_builders()?;
      let started = Self::start_components(builders).await?;

      tracing::info!("All components started successfully");

      // Step 2: Load TLS configuration
      tracing::info!("Loading TLS configuration");
      let tls_config = Self::load_tls_config(&self.config.tls)?;
      let acceptor = TlsAcceptor::from(tls_config);
      tracing::info!("TLS acceptor ready");

      // Step 3: Create TCP listener
      let listener = self.create_listener().await?;

      // Step 4: Setup signal handlers
      let mut sigterm = signal(SignalKind::terminate())
          .map_err(|e| DatabaseError::Service(format!("Failed to setup SIGTERM: {}", e)))?;
      let mut sigint = signal(SignalKind::interrupt())
          .map_err(|e| DatabaseError::Service(format!("Failed to setup SIGINT: {}", e)))?;

      tracing::info!("Database service ready - accepting connections");

      // Step 5: Main accept loop with graceful shutdown
      loop {
          tokio::select! {
              // Accept new connection
              accept_result = listener.accept() => {
                  match accept_result {
                      Ok((stream, peer_addr)) => {
                          tracing::info!("Accepted connection from {}", peer_addr);

                          // Clone for move into spawned task
                          let acceptor = acceptor.clone();
                          let started = started.clone();

                          // Spawn connection handler (non-blocking)
                          tokio::spawn(async move {
                              if let Err(e) = Self::handle_connection(stream, peer_addr, acceptor, started).await {
                                  tracing::error!("Connection handler error for {}: {}", peer_addr, e);
                              }
                          });
                      }
                      Err(e) => {
                          tracing::error!("Failed to accept connection: {}", e);
                          // Don't break - keep accepting other connections
                      }
                  }
              }

              // Shutdown signals
              _ = sigterm.recv() => {
                  tracing::info!("Received SIGTERM, shutting down gracefully");
                  break;
              }
              _ = sigint.recv() => {
                  tracing::info!("Received SIGINT (Ctrl+C), shutting down gracefully");
                  break;
              }
          }
      }

      tracing::info!("Database service stopped");
      Ok(())
  }
  ```

**IMPORTANT CHANGES:**
- `started` is now used (no longer unused!)
- Accept loop is non-blocking with tokio::select!
- Each connection spawns separate task
- Shutdown signals stop accept loop

- [ ] **VERIFY:** `cargo check -p zzping-database`
  - Expected: Error about `handle_connection` not existing (we'll add it next)

---

### Afternoon: Connection Handler Stub - [60 min]

#### Step 1: Make StartedComponents Cloneable - [15 min]

**ACTIONS:**
- [ ] Update `StartedComponents` in `service.rs`:
  ```rust
  /// Started components (running actors)
  /// These addresses are cloned for each connection handler
  #[derive(Clone)]
  struct StartedComponents {
      intent_config: Addr<IntentConfigActor<IntentConfigPermission>>,
      memdb_addr: Addr<MemDBActor<MemDBPermission>>,
      cstate: Addr<CStateActor<DatabaseMessage, DatabaseRole, SessionManager<DatabaseMessage, DatabaseRole>>>,
  }
  ```

**KEY CHANGE:** Removed `#[allow(dead_code)]` and added `#[derive(Clone)]`

- [ ] **VERIFY:** `cargo check -p zzping-database`

#### Step 2: Add Connection Handler Stub - [45 min]

**ACTIONS:**
- [ ] Add to `DatabaseService` in `service.rs`:
  ```rust
  impl DatabaseService {
      // ... existing methods ...

      /// Handle a single collector connection
      ///
      /// This function:
      /// 1. Performs TLS handshake
      /// 2. Extracts client certificate
      /// 3. Validates client role
      /// 4. Creates connection handler (Day 5)
      async fn handle_connection(
          stream: TcpStream,
          peer_addr: SocketAddr,
          acceptor: TlsAcceptor,
          _components: StartedComponents,  // Will use in Day 5
      ) -> Result<()> {
          tracing::debug!("Starting TLS handshake with {}", peer_addr);

          // Perform TLS handshake
          let tls_stream = acceptor
              .accept(stream)
              .await
              .map_err(|e| DatabaseError::Tls(format!("TLS handshake failed with {}: {}", peer_addr, e)))?;

          tracing::info!("TLS handshake successful with {}", peer_addr);

          // Extract client certificate info
          let (_io, session) = tls_stream.into_inner();
          let peer_certs = session.peer_certificates();

          match peer_certs {
              Some(certs) if !certs.is_empty() => {
                  tracing::info!(
                      "Client {} presented {} certificate(s)",
                      peer_addr,
                      certs.len()
                  );

                  // TODO Day 5: Extract CN from certificate
                  // TODO Day 5: Validate role
                  // TODO Day 5: Create ConnectionHandler
                  // TODO Day 5: Run message loop

                  // For now, just log and return
                  tracing::info!("Connection handler stub for {} - will implement in Day 5", peer_addr);
                  Ok(())
              }
              _ => {
                  let msg = format!("Client {} did not present certificate", peer_addr);
                  tracing::error!("{}", msg);
                  Err(DatabaseError::Tls(msg))
              }
          }
      }
  }
  ```

- [ ] **VERIFY:** `cargo check -p zzping-database`
- [ ] **VERIFY:** `cargo clippy -p zzping-database -- -D warnings`
  - Expected: Should pass now (StartedComponents fields used!)

- [ ] **COMMIT:** `git add -A && git commit -m "feat(database): Add TCP listener and connection accept loop"`

---

### Evening: Integration Testing - [30 min]

#### Step 1: Manual Test with Collector

**ACTIONS:**
- [ ] Start database in one terminal:
  ```bash
  RUST_LOG=debug ./target/debug/zzping-database \
    --config src/apps/zzping-database/database.ron
  ```

- [ ] Start collector in another terminal:
  ```bash
  RUST_LOG=debug ./target/debug/zzping-collector \
    --config src/apps/zzping-collector/collector.ron
  ```

- [ ] **VERIFY in database logs:**
  ```
  INFO Database service ready - accepting connections
  INFO Accepted connection from 127.0.0.1:XXXXX
  INFO TLS handshake successful with 127.0.0.1:XXXXX
  INFO Client 127.0.0.1:XXXXX presented 1 certificate(s)
  INFO Connection handler stub for 127.0.0.1:XXXXX - will implement in Day 5
  ```

- [ ] **VERIFY in collector logs:**
  ```
  INFO Connecting to database at 127.0.0.1:8443
  INFO TLS handshake successful
  ```

**Expected Behavior:**
- ✅ Database accepts connection
- ✅ TLS handshake succeeds
- ✅ Certificate validation works
- ⚠️ Connection closes immediately (stub returns)
- ✅ Database continues accepting new connections

- [ ] **COMMIT:** `git add -A && git commit -m "test(database): Verify TCP listener with collector"`

---

### 🛑 CHECKPOINT 4: Connection Acceptance Complete

**VERIFY:**
- [ ] Clippy passes (no dead code warnings)
- [ ] Database accepts connections
- [ ] TLS handshake succeeds
- [ ] Certificate extraction works
- [ ] Multiple connections can be accepted
- [ ] Graceful shutdown still works
- [ ] All code committed

---

## Day 5: Connection Handler and Message Routing

**GOAL:** Implement per-connection handler that routes messages to components.

**KEY CONCEPTS:**
- Each connection gets a ConnectionHandler struct
- ConnectionHandler owns TLS stream
- Reads/writes messages using NetworkMessage trait
- Routes incoming messages to appropriate components
- Handles connection lifecycle

---

### Morning: ConnectionHandler Structure - [120 min]

#### Step 1: Define DatabaseMessage and DatabaseRole - [45 min]

**ACTIONS:**
- [ ] Add to `service.rs` (replace placeholder types):
  ```rust
  use serde::{Deserialize, Serialize};
  use zznet_session::{
      room_message_trait::{DeserializationError, RoomMessageTrait, SerializationError},
      session_manager::SessionManager,
      types::RoomId,
  };
  use zznet_auth::{error::AuthError, role::ApplicationRole};

  // Component message imports
  use zzintent_config::network_messages::IntentConfigMessage;
  use zzmem_db::network_messages::MemDBMessage;
  use zzcollector_state::network_messages::CStateMessage;

  /// Application roles for database
  #[derive(Debug, Clone, PartialEq, Eq, Copy, Serialize, Deserialize)]
  pub enum DatabaseRole {
      Database,
      Collector,
      Admin,
  }

  impl ApplicationRole for DatabaseRole {
      fn as_str(&self) -> &'static str {
          match self {
              DatabaseRole::Database => "database",
              DatabaseRole::Collector => "collector",
              DatabaseRole::Admin => "admin",
          }
      }

      fn from_cn(cn: &str) -> std::result::Result<Self, AuthError> {
          // Extract role from CN (format: "role-name" or "name-role")
          let cn_lower = cn.to_lowercase();

          if cn_lower.contains("database") {
              Ok(DatabaseRole::Database)
          } else if cn_lower.contains("collector") {
              Ok(DatabaseRole::Collector)
          } else if cn_lower.contains("admin") {
              Ok(DatabaseRole::Admin)
          } else {
              Err(AuthError::UnknownRole(cn.to_string()))
          }
      }

      fn can_connect_to(&self, other: &Self) -> bool {
          match (self, other) {
              // Collectors connect to database
              (DatabaseRole::Collector, DatabaseRole::Database) => true,
              // Database accepts collectors
              (DatabaseRole::Database, DatabaseRole::Collector) => true,
              // Admin can connect to anything
              (DatabaseRole::Admin, _) => true,
              (_, DatabaseRole::Admin) => true,
              // Same role can connect (testing)
              (a, b) if a == b => true,
              _ => false,
          }
      }

      fn can_access_room(&self, room_id: &str) -> bool {
          match self {
              DatabaseRole::Admin => true,  // Admin has full access
              DatabaseRole::Database => true,  // Database has full access
              DatabaseRole::Collector => {
                  // Collectors can access their own rooms
                  room_id.starts_with("collector_") ||
                  room_id.starts_with("ping_") ||
                  room_id.starts_with("config_")
              }
          }
      }
  }

  /// Network messages for database application
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub enum DatabaseMessage {
      Intent(IntentConfigMessage),
      MemDB(MemDBMessage),
      CState(CStateMessage),
  }

  impl From<IntentConfigMessage> for DatabaseMessage {
      fn from(msg: IntentConfigMessage) -> Self {
          DatabaseMessage::Intent(msg)
      }
  }

  impl From<MemDBMessage> for DatabaseMessage {
      fn from(msg: MemDBMessage) -> Self {
          DatabaseMessage::MemDB(msg)
      }
  }

  impl From<CStateMessage> for DatabaseMessage {
      fn from(msg: CStateMessage) -> Self {
          DatabaseMessage::CState(msg)
      }
  }

  impl RoomMessageTrait for DatabaseMessage {
      fn room_id(&self) -> RoomId {
          match self {
              DatabaseMessage::Intent(msg) => msg.room_id(),
              DatabaseMessage::MemDB(msg) => msg.room_id(),
              DatabaseMessage::CState(msg) => msg.room_id(),
          }
      }

      fn serialize_inner(&self) -> std::result::Result<Vec<u8>, SerializationError> {
          ron::to_string(self)
              .map(|s| s.into_bytes())
              .map_err(|e| SerializationError::Failed(e.to_string()))
      }

      fn deserialize_for_room(
          room_id: &RoomId,
          bytes: &[u8],
      ) -> std::result::Result<Self, DeserializationError> {
          // Try each message type
          if let Ok(msg) = IntentConfigMessage::deserialize_for_room(room_id, bytes) {
              return Ok(DatabaseMessage::Intent(msg));
          }
          if let Ok(msg) = MemDBMessage::deserialize_for_room(room_id, bytes) {
              return Ok(DatabaseMessage::MemDB(msg));
          }
          if let Ok(msg) = CStateMessage::deserialize_for_room(room_id, bytes) {
              return Ok(DatabaseMessage::CState(msg));
          }
          Err(DeserializationError::Failed(
              "Failed to deserialize message for any known type".to_string(),
          ))
      }

      fn supported_rooms() -> Vec<RoomId> {
          let mut rooms = Vec::new();
          rooms.extend(IntentConfigMessage::supported_rooms());
          rooms.extend(MemDBMessage::supported_rooms());
          rooms.extend(CStateMessage::supported_rooms());
          rooms
      }
  }
  ```

- [ ] Update `ComponentBuilders` and `StartedComponents` types:
  ```rust
  struct ComponentBuilders {
      intent_config: IntentConfigBuilder<IntentConfigPermission>,
      memdb_addr: Addr<MemDBActor<MemDBPermission>>,
      cstate:
          CStateBuilder<DatabaseMessage, DatabaseRole, SessionManager<DatabaseMessage, DatabaseRole>>,
  }

  #[derive(Clone)]
  struct StartedComponents {
      intent_config: Addr<IntentConfigActor<IntentConfigPermission>>,
      memdb_addr: Addr<MemDBActor<MemDBPermission>>,
      cstate:
          Addr<CStateActor<DatabaseMessage, DatabaseRole, SessionManager<DatabaseMessage, DatabaseRole>>>,
  }
  ```

- [ ] **VERIFY:** `cargo check -p zzping-database`

- [ ] **COMMIT:** `git add -A && git commit -m "feat(database): Implement DatabaseRole and DatabaseMessage"`

#### Step 2: Create ConnectionHandler - [75 min]

**ACTIONS:**
- [ ] Add to `service.rs`:
  ```rust
  use tokio::io::{AsyncReadExt, AsyncWriteExt};
  use tokio_rustls::server::TlsStream;

  /// Per-connection handler for collector connections
  struct ConnectionHandler {
      peer_addr: SocketAddr,
      peer_role: DatabaseRole,
      stream: TlsStream<TcpStream>,
      components: StartedComponents,
  }

  impl ConnectionHandler {
      /// Create new connection handler
      fn new(
          peer_addr: SocketAddr,
          peer_role: DatabaseRole,
          stream: TlsStream<TcpStream>,
          components: StartedComponents,
      ) -> Self {
          Self {
              peer_addr,
              peer_role,
              stream,
              components,
          }
      }

      /// Run the connection message loop
      async fn run(mut self) -> Result<()> {
          tracing::info!(
              "Connection handler started for {} (role: {:?})",
              self.peer_addr,
              self.peer_role
          );

          // TODO: Implement message read/write loop
          // For now, just keep connection alive briefly
          tokio::time::sleep(tokio::time::Duration::from_secs(5)).await;

          tracing::info!("Connection handler stopping for {}", self.peer_addr);
          Ok(())
      }

      /// Route incoming message to appropriate component
      async fn route_message(&self, msg: DatabaseMessage) -> Result<()> {
          tracing::debug!("Routing message to component: {:?}", msg);

          match msg {
              DatabaseMessage::Intent(intent_msg) => {
                  // TODO: Send to IntentConfig actor
                  tracing::debug!("Would route to IntentConfig: {:?}", intent_msg);
              }
              DatabaseMessage::MemDB(memdb_msg) => {
                  // TODO: Send to MemDB actor
                  tracing::debug!("Would route to MemDB: {:?}", memdb_msg);
              }
              DatabaseMessage::CState(cstate_msg) => {
                  // TODO: Send to CState actor
                  tracing::debug!("Would route to CState: {:?}", cstate_msg);
              }
          }

          Ok(())
      }
  }
  ```

- [ ] **VERIFY:** `cargo check -p zzping-database`

- [ ] **COMMIT:** `git add -A && git commit -m "feat(database): Add ConnectionHandler structure"`

---

### Afternoon: Integrate ConnectionHandler - [60 min]

#### Step 1: Update handle_connection to Use Handler - [30 min]

**ACTIONS:**
- [ ] Update `handle_connection` in `service.rs`:
  ```rust
  async fn handle_connection(
      stream: TcpStream,
      peer_addr: SocketAddr,
      acceptor: TlsAcceptor,
      components: StartedComponents,
  ) -> Result<()> {
      tracing::debug!("Starting TLS handshake with {}", peer_addr);

      // Perform TLS handshake
      let tls_stream = acceptor
          .accept(stream)
          .await
          .map_err(|e| DatabaseError::Tls(format!("TLS handshake failed with {}: {}", peer_addr, e)))?;

      tracing::info!("TLS handshake successful with {}", peer_addr);

      // Extract client certificate and determine role
      let peer_role = Self::extract_client_role(&tls_stream, peer_addr)?;

      tracing::info!(
          "Client {} authenticated as role: {:?}",
          peer_addr,
          peer_role
      );

      // Create and run connection handler
      let handler = ConnectionHandler::new(peer_addr, peer_role, tls_stream, components);
      handler.run().await?;

      tracing::info!("Connection closed for {}", peer_addr);
      Ok(())
  }
  ```

- [ ] Add helper method:
  ```rust
  /// Extract client role from certificate
  fn extract_client_role(
      tls_stream: &TlsStream<TcpStream>,
      peer_addr: SocketAddr,
  ) -> Result<DatabaseRole> {
      let (_io, session) = tls_stream.get_ref();
      let peer_certs = session.peer_certificates();

      match peer_certs {
          Some(certs) if !certs.is_empty() => {
              // For now, assume first cert and use simple CN extraction
              // TODO: Proper X.509 parsing

              // Placeholder: All authenticated clients are collectors
              tracing::debug!(
                  "Client {} presented {} certificate(s) - assuming Collector role",
                  peer_addr,
                  certs.len()
              );
              Ok(DatabaseRole::Collector)
          }
          _ => {
              Err(DatabaseError::Tls(format!(
                  "Client {} did not present certificate",
                  peer_addr
              )))
          }
      }
  }
  ```

- [ ] **VERIFY:** `cargo check -p zzping-database`
- [ ] **VERIFY:** `cargo clippy -p zzping-database -- -D warnings`

- [ ] **COMMIT:** `git add -A && git commit -m "feat(database): Integrate ConnectionHandler with role extraction"`

---

### Evening: Test Multi-Collector - [30 min]

#### Step 1: Test Multiple Simultaneous Connections

**ACTIONS:**
- [ ] Start database:
  ```bash
  RUST_LOG=debug ./target/debug/zzping-database \
    --config src/apps/zzping-database/database.ron
  ```

- [ ] Start 3 collectors in separate terminals:
  ```bash
  # Terminal 1
  RUST_LOG=info ./target/debug/zzping-collector --config src/apps/zzping-collector/collector.ron

  # Terminal 2
  RUST_LOG=info ./target/debug/zzping-collector --config src/apps/zzping-collector/collector.ron

  # Terminal 3
  RUST_LOG=info ./target/debug/zzping-collector --config src/apps/zzping-collector/collector.ron
  ```

- [ ] **VERIFY in database logs:**
  ```
  INFO Accepted connection from 127.0.0.1:XXXXX
  INFO TLS handshake successful with 127.0.0.1:XXXXX
  INFO Client 127.0.0.1:XXXXX authenticated as role: Collector
  INFO Connection handler started for 127.0.0.1:XXXXX (role: Collector)

  INFO Accepted connection from 127.0.0.1:YYYYY
  INFO TLS handshake successful with 127.0.0.1:YYYYY
  INFO Client 127.0.0.1:YYYYY authenticated as role: Collector
  INFO Connection handler started for 127.0.0.1:YYYYY (role: Collector)

  INFO Accepted connection from 127.0.0.1:ZZZZZ
  ...
  ```

**Expected Behavior:**
- ✅ Database accepts all 3 connections
- ✅ Each connection has separate handler
- ✅ Connections stay alive for 5 seconds
- ✅ All close cleanly

- [ ] **COMMIT:** `git add -A && git commit -m "test(database): Verify multi-collector support"`

---

### 🛑 CHECKPOINT 5: Connection Handling Complete

**VERIFY:**
- [ ] Multiple collectors can connect simultaneously
- [ ] Role extraction works
- [ ] ConnectionHandler lifecycle works
- [ ] Clean connection shutdown
- [ ] All code committed

---

## Day 6: Message Loop and Component Routing

**GOAL:** Implement full message read/write loop and route messages to components.

**KEY CONCEPTS:**
- Read messages from TLS stream
- Deserialize using DatabaseMessage
- Route to components via Actix messages
- Handle component responses
- Write responses back to stream

---

### Morning: Message Reading - [90 min]

#### Step 1: Implement Message Frame Reading - [60 min]

**ACTIONS:**
- [ ] Update `ConnectionHandler::run()` in `service.rs`:
  ```rust
  async fn run(mut self) -> Result<()> {
      tracing::info!(
          "Connection handler started for {} (role: {:?})",
          self.peer_addr,
          self.peer_role
      );

      let mut buffer = vec![0u8; 8192]; // 8KB buffer

      loop {
          // Read message length (4 bytes, big-endian)
          let mut len_bytes = [0u8; 4];
          match self.stream.read_exact(&mut len_bytes).await {
              Ok(_) => {}
              Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                  tracing::info!("Client {} disconnected", self.peer_addr);
                  break;
              }
              Err(e) => {
                  tracing::error!("Failed to read message length from {}: {}", self.peer_addr, e);
                  break;
              }
          }

          let msg_len = u32::from_be_bytes(len_bytes) as usize;

          if msg_len == 0 {
              tracing::warn!("Received zero-length message from {}", self.peer_addr);
              continue;
          }

          if msg_len > buffer.len() {
              tracing::debug!("Resizing buffer from {} to {} bytes", buffer.len(), msg_len);
              buffer.resize(msg_len, 0);
          }

          // Read message body
          match self.stream.read_exact(&mut buffer[..msg_len]).await {
              Ok(_) => {
                  tracing::debug!("Received {} bytes from {}", msg_len, self.peer_addr);

                  // Deserialize and handle message
                  if let Err(e) = self.handle_message(&buffer[..msg_len]).await {
                      tracing::error!("Failed to handle message from {}: {}", self.peer_addr, e);
                      // Continue processing other messages
                  }
              }
              Err(e) => {
                  tracing::error!("Failed to read message body from {}: {}", self.peer_addr, e);
                  break;
              }
          }
      }

      tracing::info!("Connection handler stopping for {}", self.peer_addr);
      Ok(())
  }
  ```

- [ ] Add message handling:
  ```rust
  /// Handle a single received message
  async fn handle_message(&self, data: &[u8]) -> Result<()> {
      // Try to deserialize as DatabaseMessage
      let msg_str = std::str::from_utf8(data)
          .map_err(|e| DatabaseError::Service(format!("Invalid UTF-8 in message: {}", e)))?;

      tracing::debug!("Received message: {}", msg_str);

      // For now, just parse and log
      // TODO: Actual deserialization and routing
      tracing::debug!("Message handling placeholder for {}", self.peer_addr);

      Ok(())
  }
  ```

- [ ] **VERIFY:** `cargo check -p zzping-database`

- [ ] **COMMIT:** `git add -A && git commit -m "feat(database): Implement message frame reading"`

---

### Afternoon: Component Message Routing - [90 min]

#### Step 1: Implement Full Message Routing - [60 min]

**ACTIONS:**
- [ ] Update `route_message` to actually send to components:
  ```rust
  /// Route incoming message to appropriate component
  async fn route_message(&self, msg: DatabaseMessage) -> Result<()> {
      tracing::debug!("Routing message: {:?}", msg);

      match msg {
          DatabaseMessage::Intent(intent_msg) => {
              tracing::debug!("Routing to IntentConfig: {:?}", intent_msg);
              // self.components.intent_config.send(intent_msg).await
              //     .map_err(|e| DatabaseError::Component(format!("IntentConfig send failed: {}", e)))?;
              // For now, just log
              tracing::info!("Would send to IntentConfig component");
          }
          DatabaseMessage::MemDB(memdb_msg) => {
              tracing::debug!("Routing to MemDB: {:?}", memdb_msg);
              // self.components.memdb_addr.send(memdb_msg).await
              //     .map_err(|e| DatabaseError::Component(format!("MemDB send failed: {}", e)))?;
              // For now, just log
              tracing::info!("Would send to MemDB component");
          }
          DatabaseMessage::CState(cstate_msg) => {
              tracing::debug!("Routing to CState: {:?}", cstate_msg);
              // self.components.cstate.send(cstate_msg).await
              //     .map_err(|e| DatabaseError::Component(format!("CState send failed: {}", e)))?;
              // For now, just log
              tracing::info!("Would send to CState component");
          }
      }

      Ok(())
  }
  ```

**NOTE:** Actual component message sending is commented out pending proper message handler implementation in components. This is Day 6-7 integration work.

- [ ] **VERIFY:** `cargo check -p zzping-database`

- [ ] **COMMIT:** `git add -A && git commit -m "feat(database): Add component routing infrastructure"`

---

### 🛑 CHECKPOINT 6: Message Routing Infrastructure Complete

**VERIFY:**
- [ ] Message framing works
- [ ] Messages can be read from stream
- [ ] Routing infrastructure in place
- [ ] Connection stays alive during message exchange
- [ ] All code committed

---

## Day 7: Documentation and Final Integration

**GOAL:** Complete documentation, write README, verify end-to-end functionality.

---

### Morning: Documentation - [90 min]

#### Step 1: Write README.md - [60 min]

**ACTIONS:**
- [ ] Create `src/apps/zzping-database/README.md`:
  ```markdown
  # ZZPing Database Server

  Network monitoring database server that accepts mTLS connections from collectors,
  stores ping data, and distributes configuration updates.

  ## Features

  - **mTLS Server:** Accepts secure connections from authenticated collectors
  - **Multi-Collector:** Handles multiple simultaneous collector connections
  - **Component Integration:** Routes messages to IntentConfig, MemDB, and CState components
  - **Graceful Shutdown:** Handles SIGTERM/SIGINT signals cleanly

  ## Configuration

  Copy `database.example.ron` to `database.ron` and customize:

  ```ron
  DatabaseConfig(
      bind_host: "0.0.0.0",
      bind_port: 8443,
      tls: TlsConfig(
          ca_cert_path: "test_certs/ca.pem",
          server_cert_path: "test_certs/database.pem",
          server_key_path: "test_certs/database.key",
      ),
      components: ComponentConfig(
          stale_timeout_secs: 30,
          max_collectors: 100,
      ),
  )
  ```

  ## Running

  ```bash
  # Build
  cargo build --bin zzping-database

  # Run with default config
  ./target/debug/zzping-database

  # Run with custom config
  ./target/debug/zzping-database --config /path/to/database.ron

  # Enable debug logging
  RUST_LOG=debug ./target/debug/zzping-database

  # Enable trace logging
  ./target/debug/zzping-database --trace
  ```

  ## Testing

  ```bash
  # Run all tests
  cargo test -p zzping-database

  # Run specific test suite
  cargo test -p zzping-database --test config_tests
  cargo test -p zzping-database --test service_tests
  ```

  ## Architecture

  - **main.rs:** Entry point with LocalSet for Actix runtime
  - **config.rs:** Configuration structures and validation
  - **service.rs:** Core service with TCP listener and connection handling
  - **cli.rs:** Command-line argument parsing
  - **error.rs:** Error types

  ## TLS Certificates

  The database requires:
  - **CA certificate:** For verifying collector client certificates
  - **Server certificate:** Database's own identity
  - **Server private key:** For TLS encryption

  See `test_certs/` for test certificates (DO NOT USE IN PRODUCTION).

  ## Troubleshooting

  **Connection refused:**
  - Check bind_host/bind_port in config
  - Verify port is not already in use: `netstat -ln | grep 8443`

  **TLS handshake failed:**
  - Verify certificates exist and are readable
  - Check certificate validity: `openssl x509 -in database.pem -text -noout`
  - Ensure collector certificate is signed by same CA

  **Component failures:**
  - Check component logs for errors
  - Verify all Phase 1-3 components are built: `cargo build`
  ```

- [ ] **COMMIT:** `git add -A && git commit -m "docs(database): Add comprehensive README"`

#### Step 2: Update Module Documentation - [30 min]

**ACTIONS:**
- [ ] Review and enhance doc comments in all modules
- [ ] Ensure all public items have doc comments
- [ ] Add examples where helpful

- [ ] **VERIFY:** `cargo doc -p zzping-database --no-deps --open`
  - Review generated documentation

- [ ] **COMMIT:** `git add -A && git commit -m "docs(database): Enhance module documentation"`

---

### Afternoon: Final Testing and Review - [120 min]

#### Step 1: Comprehensive Test Run - [45 min]

**ACTIONS:**
- [ ] Clean build:
  ```bash
  cargo clean -p zzping-database
  cargo build -p zzping-database
  ```

- [ ] Run all tests:
  ```bash
  cargo test -p zzping-database
  ```
  - **Expected:** All tests pass (12+ tests)

- [ ] Run clippy:
  ```bash
  cargo clippy -p zzping-database -- -D warnings
  ```
  - **Expected:** No errors, no warnings

- [ ] Run fmt check:
  ```bash
  cargo fmt -p zzping-database -- --check
  ```
  - **Expected:** All code formatted

#### Step 2: End-to-End Manual Test - [45 min]

**ACTIONS:**
- [ ] Test 1: Basic startup and shutdown
  ```bash
  ./target/debug/zzping-database &
  sleep 2
  kill -TERM $!
  ```
  - **Expected:** Clean startup and shutdown logs

- [ ] Test 2: Invalid configuration
  ```bash
  echo "invalid RON {{{" > /tmp/bad-config.ron
  ./target/debug/zzping-database --config /tmp/bad-config.ron
  ```
  - **Expected:** Clear error message, exit cleanly

- [ ] Test 3: Missing certificates
  ```bash
  # Create config with nonexistent certs
  # Run database
  # Expect: Clear error about missing certificate files
  ```

- [ ] Test 4: Multi-collector stress test
  - Start database
  - Start 5 collectors simultaneously
  - **Expected:** All connect successfully
  - Stop all collectors
  - **Expected:** Database continues running

- [ ] Test 5: Graceful shutdown under load
  - Start database
  - Start 3 collectors
  - Send SIGTERM to database
  - **Expected:** Clean shutdown, all connections close

#### Step 3: Code Review Checklist - [30 min]

**ACTIONS:**
- [ ] **Standards Compliance:**
  - [ ] All public items have doc comments
  - [ ] Error types use thiserror
  - [ ] No unwrap() in production code
  - [ ] Proper error context with anyhow
  - [ ] LocalSet pattern for Actix

- [ ] **API Patterns:**
  - [ ] IntentConfigBuilder::new().role()
  - [ ] MemDBActor::new_with_role()
  - [ ] DATABASE component roles
  - [ ] ServerConfig for TLS

- [ ] **Testing:**
  - [ ] At least 12 tests
  - [ ] Config validation tested
  - [ ] TLS loading tested
  - [ ] Service creation tested

- [ ] **Code Quality:**
  - [ ] No clippy warnings
  - [ ] Code formatted
  - [ ] No dead code warnings
  - [ ] Clear error messages

- [ ] **Documentation:**
  - [ ] README exists
  - [ ] Example config exists
  - [ ] All modules documented
  - [ ] Troubleshooting section

- [ ] **COMMIT:** `git add -A && git commit -m "test(database): Complete Phase 5 verification"`

---

### 🛑 FINAL CHECKPOINT: Phase 5 Complete

**VERIFY ALL:**
- [ ] All tests pass (12+)
- [ ] Clippy clean
- [ ] Documentation complete
- [ ] Multi-collector support works
- [ ] TLS handshake succeeds
- [ ] Graceful shutdown works
- [ ] Example config works
- [ ] README comprehensive

**CREATE PR:**
```bash
git push origin feature/database-app-1
# Create PR: "feat(database): Implement Phase 5 (Complete Database Server)"
```

---

## Common Issues and Solutions

### Issue: StartedComponents Dead Code Warning

**Problem:** Clippy complains about unused struct fields.

**Solution:** Add `#[allow(dead_code)]` if fields will be used in future, or make sure they're actually used in connection handlers.

### Issue: TLS Handshake Fails

**Problem:** "TLS handshake failed" error.

**Solution:**
1. Verify certificates exist: `ls -la test_certs/`
2. Check certificate validity: `openssl x509 -in database.pem -text -noout`
3. Ensure collector cert signed by same CA
4. Check RUST_LOG=debug for detailed TLS errors

### Issue: Port Already in Use

**Problem:** "Address already in use" error.

**Solution:**
```bash
# Find process using port
lsof -i :8443
# Kill if needed
kill -9 <PID>
```

### Issue: Collector Can't Connect

**Problem:** Collector times out connecting.

**Solution:**
1. Check database is running: `ps aux | grep zzping-database`
2. Check bind address (0.0.0.0 vs 127.0.0.1)
3. Check firewall rules
4. Verify port matches in both configs

---

## Phase 5 Summary

**What You Built:**
- ✅ Complete database server application
- ✅ Configuration system with validation
- ✅ Component integration (IntentConfig, MemDB, CState)
- ✅ TLS server with mTLS
- ✅ TCP listener and connection acceptance
- ✅ Per-connection handlers
- ✅ Message routing infrastructure
- ✅ Multi-collector support
- ✅ Graceful shutdown
- ✅ Comprehensive tests (12+)
- ✅ Full documentation

**Quality Metrics:**
- Test count: 12+ (exceeds baseline)
- Code quality: Production-ready
- Documentation: Complete
- Standards compliance: 100%

**Next Phase (Phase 6):**
- End-to-end integration testing
- Certificate rotation
- 24-hour stability testing
- Performance baseline
- Final documentation
- MVP acceptance

---

**END OF PHASE 5 CHECKLIST V3 (COMPLETE)**

*Last Updated: October 14, 2025 (Post-Review)*
*Status: Ready for implementation*

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
