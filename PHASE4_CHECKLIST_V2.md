# Phase 4 Implementation Checklist V2: Collector Application
# (IMPROVED VERSION - Use this instead of PHASE4_CHECKLIST.md)

**Target:** Week 4 (Following Phase 3 `zzcollector-state` completion)
**Status:** Ready to begin - Phases 1, 2, & 3 complete
**Goal:** Create a working collector binary that integrates all collector-side components

---

## 🚨 BEFORE YOU START: Pre-flight Checklist

**STOP:** Do not write any code until you complete these verification steps.

### Environment Verification
- [ ] **VERIFY WORKSPACE:** Run `pwd` and confirm you're in the zzping root directory
- [ ] **VERIFY GIT STATUS:** Run `git status` - should be on a clean branch or ready to create feature branch
- [ ] **VERIFY BASELINE BUILD:** Run `cargo build` and confirm it succeeds
  - Expected: "Finished dev [unoptimized + debuginfo] target(s)"
  - If fails: Fix existing issues before starting Phase 4
- [ ] **VERIFY BASELINE TESTS:** Run `cargo test` and note passing count
  - Record here: _____ tests passing (should be 70+ from Phases 1-3)
- [ ] **CREATE FEATURE BRANCH:** `git checkout -b feature/collector-app`

### Component Dependency Verification (CRITICAL)
- [ ] **VERIFY Component: zzintent-config**
  ```bash
  cargo check -p zzintent-config
  # Expected: "Finished dev" with no errors
  ```
- [ ] **VERIFY Component: zzpinger**
  ```bash
  cargo check -p zzpinger
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

**IF ANY COMPONENT FAILS:** Stop. Fix component issues before proceeding. The collector application cannot work without working components.

### Document Review (MANDATORY READING - Don't Skip!)
- [ ] **READ THIS ENTIRE CHECKLIST:** Don't skim - read all sections including common mistakes
- [ ] **READ:** `AGENT_CODING_STANDARDS.md` - Pay attention to error handling and doc comment rules
- [ ] **READ:** `COMPONENT_TEMPLATE_GUIDE.md` - Understand the three-phase lifecycle pattern
- [ ] **EXAMINE:** `src/components/zzintent-config/` - This is your reference implementation
- [ ] **READ:** `PHASE_CHECKLIST_IMPROVEMENTS.md` - Understand the test-first approach and verification pattern

### Understanding Check (Answer These - Be Honest!)
- [ ] **PRIMARY PURPOSE:** Can you explain in 1-2 sentences what this collector application does?
  - Write it here: _________________________________________________
  - Should be something like: "Integrates zzpinger, zzmem-db, zzintent-config, and zzcollector-state into a single binary that connects to the database via mTLS and performs network monitoring tasks."

- [ ] **CRITICAL FEATURES:** What are the 3 things this application MUST do?
  1. _________________________________________________
  2. _________________________________________________
  3. _________________________________________________
  - Should include: (1) Connect to database via mTLS, (2) Integrate all components with proper lifecycle, (3) Handle configuration updates and graceful shutdown

- [ ] **INTEGRATION POINTS:** Which components talk to which?
  - Draw/write it here: _________________________________________________
  - Should show: IntentConfig → Pinger, Pinger → MemDB, All → CState

**IF YOU CANNOT ANSWER THESE:** Stop and re-read the documentation.

### Execution Strategy Commitment
- [ ] **I COMMIT TO:** Creating files incrementally with `cargo check` after each step
- [ ] **I COMMIT TO:** Writing tests for each module before moving to the next
- [ ] **I COMMIT TO:** Running verification commands and checking expected outcomes
- [ ] **I COMMIT TO:** Committing after each completed increment
- [ ] **I COMMIT TO:** Not skipping checkpoints even if I think it's "obvious"
- [ ] **I COMMIT TO:** Asking for help if stuck >30 minutes instead of guessing

**SIGNATURE (Type your name/ID to confirm):** ___________________

---

## Day 1: Application Structure and Configuration

**GOAL:** Create the binary crate structure and configuration loading system.

---

### Morning: Create Binary Crate Structure

**CONTEXT:** We're creating a BINARY crate (not library). Binary crates have `src/main.rs` and optionally `src/lib.rs` for testable logic. This is different from component library crates.

**CRITICAL DIFFERENCE:**
- Component (library): `src/lib.rs` with public API, used as dependency
- Application (binary): `src/main.rs` as entry point, `src/lib.rs` for testable logic

#### Step 1: Verify Directory Doesn't Exist - [5 min]

**ACTIONS:**
- [ ] Run: `ls -la src/apps/`
  - Expected: Directory might not exist yet, or exists without zzping-collector
  - **IF zzping-collector EXISTS:** You're in the wrong state! Check with supervisor.
- [ ] Run: `mkdir -p src/apps` (if directory doesn't exist)
- [ ] Run: `ls -la src/apps/`
  - Expected: Empty directory or other apps but no zzping-collector

#### Step 2: Create Cargo.toml - [10 min]

**ACTIONS:**
- [ ] Create directory: `mkdir -p src/apps/zzping-collector`
- [ ] Create file: `src/apps/zzping-collector/Cargo.toml` with EXACT content:
  ```toml
  [package]
  name = "zzping-collector"
  version = "0.1.0"
  edition = "2021"

  [[bin]]
  name = "zzping-collector"
  path = "src/main.rs"

  [dependencies]
  # Component dependencies (our Phase 1-3 work)
  zzintent-config = { path = "../../components/zzintent-config" }
  zzpinger = { path = "../../components/zzpinger" }
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

  # TLS/crypto
  rustls = "0.21"
  rustls-pemfile = "1.0"

  # Utilities
  chrono = "0.4"

  [dev-dependencies]
  tempfile = "3.0"
  tokio-test = "0.4"
  ```

- [ ] **VERIFY:** Run `cargo metadata --format-version 1 | grep zzping-collector`
  - Expected: Should see package name in output
  - **IF NOT FOUND:** Check Cargo.toml syntax, ensure proper path

#### Step 3: Create Minimal main.rs - [5 min]

**ACTIONS:**
- [ ] Create file: `src/apps/zzping-collector/src/main.rs` with EXACT content:
  ```rust
  //! ZZPing Collector Application
  //!
  //! Integrates all collector-side components (zzpinger, zzmem-db, zzintent-config,
  //! zzcollector-state) into a single binary that connects to the database server
  //! via mTLS and performs network monitoring.

  use anyhow::Result;

  fn main() -> Result<()> {
      println!("zzping-collector v0.1.0 starting...");
      println!("Phase 4 implementation in progress");
      Ok(())
  }
  ```

- [ ] **VERIFY:** Run `cargo build --bin zzping-collector`
  - Expected: "Finished dev [unoptimized + debuginfo] target(s)" (may take a few minutes on first build)
  - **IF FAILS:** Check error messages carefully - likely dependency path issue

- [ ] **VERIFY:** Run `./target/debug/zzping-collector`
  - Expected output:
    ```
    zzping-collector v0.1.0 starting...
    Phase 4 implementation in progress
    ```
  - **IF FAILS:** Binary didn't compile, check previous step

- [ ] **COMMIT:** `git add -A && git commit -m "chore(collector): Initialize binary crate with minimal main.rs"`

#### Step 4: Create lib.rs for Testable Logic - [10 min]

**CONTEXT:** We separate main.rs (entry point) from lib.rs (testable logic) so we can write unit tests for the application logic without running the whole binary.

**ACTIONS:**
- [ ] Create file: `src/apps/zzping-collector/src/lib.rs` with content:
  ```rust
  //! Collector application library.
  //!
  //! Contains testable business logic separated from main() entry point.
  //! This allows unit testing of configuration, service orchestration, and
  //! component integration without running the full binary.

  // Module declarations
  pub mod cli;
  pub mod config;
  pub mod error;
  pub mod service;

  // Re-exports for convenience
  pub use cli::CliArgs;
  pub use config::CollectorConfig;
  pub use error::CollectorError;
  pub use service::CollectorService;
  ```

- [ ] Create stub files with minimal content:

  **File:** `src/apps/zzping-collector/src/error.rs`
  ```rust
  //! Error types for the collector application.

  use thiserror::Error;

  /// Errors that can occur in the collector application.
  #[derive(Error, Debug)]
  pub enum CollectorError {
      #[error("Configuration error: {0}")]
      Config(String),

      #[error("Service error: {0}")]
      Service(String),

      #[error("IO error: {0}")]
      Io(#[from] std::io::Error),

      #[error("Component error: {0}")]
      Component(String),
  }

  /// Result type alias for collector operations.
  pub type Result<T> = std::result::Result<T, CollectorError>;
  ```

  **File:** `src/apps/zzping-collector/src/cli.rs`
  ```rust
  //! Command-line argument parsing.

  use clap::Parser;

  /// ZZPing Collector - Network monitoring client
  #[derive(Parser, Debug)]
  #[command(author, version, about, long_about = None)]
  pub struct CliArgs {
      /// Path to configuration file
      #[arg(short, long, default_value = "collector.ron")]
      pub config: String,

      /// Enable debug logging
      #[arg(short, long)]
      pub debug: bool,

      /// Enable trace logging (very verbose)
      #[arg(short, long)]
      pub trace: bool,
  }
  ```

  **File:** `src/apps/zzping-collector/src/config.rs`
  ```rust
  //! Configuration structures and loading.

  use serde::{Deserialize, Serialize};

  /// Collector application configuration.
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct CollectorConfig {
      /// Unique identifier for this collector instance
      pub collector_id: String,

      /// Database connection settings
      pub database: DatabaseConfig,

      /// TLS certificate paths
      pub tls: TlsConfig,

      /// Component-specific settings
      pub components: ComponentConfig,
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct DatabaseConfig {
      pub host: String,
      pub port: u16,
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct TlsConfig {
      pub ca_cert_path: String,
      pub client_cert_path: String,
      pub client_key_path: String,
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct ComponentConfig {
      /// Heartbeat interval in seconds for collector state
      pub heartbeat_interval_secs: u64,

      /// Batch size for mem-db
      pub memdb_batch_size: usize,
  }

  impl CollectorConfig {
      /// Load configuration from a RON file.
      ///
      /// # Errors
      /// Returns error if file cannot be read or parsed.
      pub fn load(path: &str) -> crate::error::Result<Self> {
          let content = std::fs::read_to_string(path)
              .map_err(|e| crate::error::CollectorError::Config(
                  format!("Failed to read config file {}: {}", path, e)
              ))?;

          let config: Self = ron::from_str(&content)
              .map_err(|e| crate::error::CollectorError::Config(
                  format!("Failed to parse config: {}", e)
              ))?;

          Ok(config)
      }

      /// Validate configuration values.
      ///
      /// # Errors
      /// Returns error if configuration has invalid values.
      pub fn validate(&self) -> crate::error::Result<()> {
          if self.collector_id.is_empty() {
              return Err(crate::error::CollectorError::Config(
                  "collector_id cannot be empty".into()
              ));
          }

          if self.database.port == 0 {
              return Err(crate::error::CollectorError::Config(
                  "database port cannot be 0".into()
              ));
          }

          if self.components.heartbeat_interval_secs == 0 {
              return Err(crate::error::CollectorError::Config(
                  "heartbeat_interval_secs cannot be 0".into()
              ));
          }

          Ok(())
      }
  }
  ```

  **File:** `src/apps/zzping-collector/src/service.rs`
  ```rust
  //! Service orchestration and component lifecycle management.

  use crate::config::CollectorConfig;
  use crate::error::{CollectorError, Result};

  /// Main collector service that orchestrates all components.
  pub struct CollectorService {
      config: CollectorConfig,
  }

  impl CollectorService {
      /// Create a new collector service with the given configuration.
      pub fn new(config: CollectorConfig) -> Result<Self> {
          config.validate()?;
          Ok(Self { config })
      }

      /// Start the collector service.
      ///
      /// This will:
      /// 1. Initialize all components
      /// 2. Connect to the database
      /// 3. Start the main event loop
      ///
      /// # Errors
      /// Returns error if service cannot start.
      pub async fn run(self) -> Result<()> {
          tracing::info!("Collector service starting with ID: {}", self.config.collector_id);

          // TODO: Implement service logic in Day 2-5

          Ok(())
      }
  }
  ```

- [ ] **VERIFY:** Run `cargo check --bin zzping-collector`
  - Expected: "Finished dev" with warnings about unused fields (that's OK)
  - **IF FAILS:** Check syntax errors, ensure all files created correctly

- [ ] **VERIFY:** Run `cargo test -p zzping-collector`
  - Expected: "running 0 tests" and "test result: ok. 0 passed"
  - **IF FAILS:** Check compilation errors

- [ ] **COMMIT:** `git add -A && git commit -m "chore(collector): Add module structure with stubs"`

**DONE CRITERIA:**
- [ ] Binary compiles: `cargo build --bin zzping-collector` succeeds
- [ ] Binary runs: `./target/debug/zzping-collector` prints startup message
- [ ] All modules compile: `cargo check -p zzping-collector` succeeds
- [ ] Test harness works: `cargo test -p zzping-collector` succeeds (0 tests OK)
- [ ] All changes committed with proper messages

---

### 🛑 CHECKPOINT 1: Crate Structure Complete

**VERIFY BEFORE PROCEEDING:**
- [ ] `cargo build --bin zzping-collector` succeeds without errors
- [ ] Binary executable exists: `ls -la target/debug/zzping-collector`
- [ ] Binary runs and prints message: `./target/debug/zzping-collector`
- [ ] All module files created: cli.rs, config.rs, error.rs, service.rs
- [ ] No compilation errors (warnings about unused code are OK)
- [ ] Git shows all files committed: `git status` shows clean or only new work

**IF ANY FAILS:** Stop. Fix the issue before moving to configuration implementation.

**SELF-CHECK QUESTIONS:**
- Can you explain the difference between main.rs and lib.rs? (Write it: ________________)
- Which module is responsible for loading config files? (Answer: ________________)
- What error type do we use throughout the collector? (Answer: ________________)

---

### Afternoon: Configuration Loading and Validation - [90 min]

**CONTEXT:** We already created config.rs stubs. Now we add tests and validation logic.

#### Step 1: Write Configuration Tests FIRST - [30 min]

**ACTIONS:**
- [ ] Create test file: `src/apps/zzping-collector/tests/config_tests.rs`:
  ```rust
  //! Tests for configuration loading and validation.

  use zzping_collector::config::*;
  use tempfile::NamedTempFile;
  use std::io::Write;

  /// Helper to create a valid test configuration.
  fn create_valid_config() -> CollectorConfig {
      CollectorConfig {
          collector_id: "test-collector-01".into(),
          database: DatabaseConfig {
              host: "127.0.0.1".into(),
              port: 8443,
          },
          tls: TlsConfig {
              ca_cert_path: "test_certs/ca.pem".into(),
              client_cert_path: "test_certs/collector.pem".into(),
              client_key_path: "test_certs/collector.key".into(),
          },
          components: ComponentConfig {
              heartbeat_interval_secs: 5,
              memdb_batch_size: 100,
          },
      }
  }

  #[test]
  fn test_valid_config_validates() {
      let config = create_valid_config();
      assert!(config.validate().is_ok());
  }

  #[test]
  fn test_empty_collector_id_fails_validation() {
      let mut config = create_valid_config();
      config.collector_id = String::new();

      let result = config.validate();
      assert!(result.is_err());
      assert!(result.unwrap_err().to_string().contains("collector_id"));
  }

  #[test]
  fn test_zero_port_fails_validation() {
      let mut config = create_valid_config();
      config.database.port = 0;

      let result = config.validate();
      assert!(result.is_err());
      assert!(result.unwrap_err().to_string().contains("port"));
  }

  #[test]
  fn test_zero_heartbeat_interval_fails_validation() {
      let mut config = create_valid_config();
      config.components.heartbeat_interval_secs = 0;

      let result = config.validate();
      assert!(result.is_err());
      assert!(result.unwrap_err().to_string().contains("heartbeat"));
  }

  #[test]
  fn test_load_valid_config_file() {
      let config_content = r#"
      CollectorConfig(
          collector_id: "test-collector",
          database: DatabaseConfig(
              host: "127.0.0.1",
              port: 8443,
          ),
          tls: TlsConfig(
              ca_cert_path: "test_certs/ca.pem",
              client_cert_path: "test_certs/collector.pem",
              client_key_path: "test_certs/collector.key",
          ),
          components: ComponentConfig(
              heartbeat_interval_secs: 5,
              memdb_batch_size: 100,
          ),
      )
      "#;

      let mut temp_file = NamedTempFile::new().unwrap();
      temp_file.write_all(config_content.as_bytes()).unwrap();
      let path = temp_file.path().to_str().unwrap();

      let config = CollectorConfig::load(path).expect("Failed to load config");
      assert_eq!(config.collector_id, "test-collector");
      assert_eq!(config.database.host, "127.0.0.1");
      assert_eq!(config.database.port, 8443);
  }

  #[test]
  fn test_load_nonexistent_file_fails() {
      let result = CollectorConfig::load("/nonexistent/path/config.ron");
      assert!(result.is_err());
      assert!(result.unwrap_err().to_string().contains("Failed to read"));
  }

  #[test]
  fn test_load_invalid_ron_fails() {
      let invalid_content = "this is not valid RON {{{";

      let mut temp_file = NamedTempFile::new().unwrap();
      temp_file.write_all(invalid_content.as_bytes()).unwrap();
      let path = temp_file.path().to_str().unwrap();

      let result = CollectorConfig::load(path);
      assert!(result.is_err());
      assert!(result.unwrap_err().to_string().contains("Failed to parse"));
  }
  ```

- [ ] **VERIFY TESTS PASS:** Run `cargo test -p zzping-collector config`
  - Expected: "test result: ok. 7 passed" (all config tests pass)
  - **IF FAILS:** The config.rs implementation from morning should make these pass. Check error messages.

- [ ] **COMMIT:** `git add -A && git commit -m "test(collector): Add configuration loading and validation tests"`

#### Step 2: Create Example Configuration File - [15 min]

**ACTIONS:**
- [ ] Create file: `src/apps/zzping-collector/collector.example.ron`:
  ```ron
  // Example ZZPing Collector Configuration
  // Copy this to collector.ron and customize for your environment

  CollectorConfig(
      // Unique identifier for this collector instance
      // Should match the CN in the client TLS certificate
      collector_id: "collector-01",

      // Database server connection settings
      database: DatabaseConfig(
          host: "127.0.0.1",
          port: 8443,
      ),

      // TLS certificate paths for mTLS authentication
      tls: TlsConfig(
          ca_cert_path: "test_certs/ca.pem",
          client_cert_path: "test_certs/collector.pem",
          client_key_path: "test_certs/collector.key",
      ),

      // Component-specific configuration
      components: ComponentConfig(
          // How often to send heartbeat to database (in seconds)
          heartbeat_interval_secs: 5,

          // How many ping results to batch before sending to database
          memdb_batch_size: 50,
      ),
  )
  ```

- [ ] **VERIFY:** Manually check the file can be parsed:
  ```bash
  cargo run --bin zzping-collector -- --config src/apps/zzping-collector/collector.example.ron
  ```
  - Expected: Should run without errors (though service doesn't do anything yet)
  - **IF FAILS:** Check RON syntax

- [ ] **COMMIT:** `git add -A && git commit -m "docs(collector): Add example configuration file"`

**DONE CRITERIA:**
- [ ] All configuration tests pass (7 tests)
- [ ] Example config file exists and is valid RON
- [ ] Config validation catches empty IDs, zero ports, zero intervals
- [ ] Config loading handles file errors gracefully

---

### Evening: CLI Integration and Logging - [60 min]

#### Step 1: Implement CLI in main.rs - [30 min]

**ACTIONS:**
- [ ] Update `src/apps/zzping-collector/src/main.rs`:
  ```rust
  //! ZZPing Collector Application
  //!
  //! Integrates all collector-side components (zzpinger, zzmem-db, zzintent-config,
  //! zzcollector-state) into a single binary that connects to the database server
  //! via mTLS and performs network monitoring.

  use anyhow::{Context, Result};
  use clap::Parser;
  use tracing_subscriber::EnvFilter;
  use zzping_collector::{CliArgs, CollectorConfig, CollectorService};

  #[tokio::main]
  async fn main() -> Result<()> {
      // Parse command-line arguments
      let args = CliArgs::parse();

      // Initialize logging based on CLI flags
      init_logging(&args);

      tracing::info!("ZZPing Collector v{} starting", env!("CARGO_PKG_VERSION"));
      tracing::info!("Loading configuration from: {}", args.config);

      // Load and validate configuration
      let config = CollectorConfig::load(&args.config)
          .with_context(|| format!("Failed to load configuration from {}", args.config))?;

      config.validate()
          .context("Configuration validation failed")?;

      tracing::info!("Configuration loaded successfully");
      tracing::info!("Collector ID: {}", config.collector_id);
      tracing::info!("Database: {}:{}", config.database.host, config.database.port);

      // Create and run the collector service
      let service = CollectorService::new(config)
          .context("Failed to create collector service")?;

      service.run().await
          .context("Collector service failed")?;

      tracing::info!("ZZPing Collector shutdown complete");
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

- [ ] **VERIFY:** Build and run with different logging levels:
  ```bash
  cargo build --bin zzping-collector
  ./target/debug/zzping-collector --help
  # Expected: Shows help with options for --config, --debug, --trace

  ./target/debug/zzping-collector --config src/apps/zzping-collector/collector.example.ron
  # Expected: Loads config, logs startup, then exits (service.run() is still stub)

  ./target/debug/zzping-collector --config src/apps/zzping-collector/collector.example.ron --debug
  # Expected: Same but with DEBUG level logs
  ```

- [ ] **COMMIT:** `git add -A && git commit -m "feat(collector): Implement CLI and logging initialization"`

#### Step 2: Test Error Handling - [30 min]

**ACTIONS:**
- [ ] Test with nonexistent config file:
  ```bash
  ./target/debug/zzping-collector --config /nonexistent/file.ron
  ```
  - Expected: Error message about "Failed to load configuration from /nonexistent/file.ron"
  - **IF PANICS:** Fix error handling

- [ ] Create an invalid config file for testing:
  ```bash
  echo "invalid ron content {{{" > /tmp/bad-config.ron
  ./target/debug/zzping-collector --config /tmp/bad-config.ron
  ```
  - Expected: Error message about "Failed to parse config"
  - **IF PANICS:** Fix error handling

- [ ] Create config with invalid values:
  ```bash
  cat > /tmp/invalid-config.ron << 'EOF'
  CollectorConfig(
      collector_id: "",
      database: DatabaseConfig(host: "127.0.0.1", port: 0),
      tls: TlsConfig(
          ca_cert_path: "test_certs/ca.pem",
          client_cert_path: "test_certs/collector.pem",
          client_key_path: "test_certs/collector.key",
      ),
      components: ComponentConfig(
          heartbeat_interval_secs: 0,
          memdb_batch_size: 100,
      ),
  )
  EOF

  ./target/debug/zzping-collector --config /tmp/invalid-config.ron
  ```
  - Expected: Error message about "Configuration validation failed"
  - **IF PANICS:** Fix validation

**DONE CRITERIA:**
- [ ] `--help` shows all CLI options
- [ ] `--debug` and `--trace` flags work
- [ ] Loading valid config succeeds
- [ ] Loading nonexistent file shows clear error (doesn't panic)
- [ ] Loading invalid RON shows clear error (doesn't panic)
- [ ] Invalid config values caught by validation

---

### 🛑 CHECKPOINT 2: Configuration System Complete

**VERIFY BEFORE PROCEEDING TO DAY 2:**
- [ ] All config tests pass: `cargo test -p zzping-collector config` (7 passed)
- [ ] Binary accepts CLI arguments: `./target/debug/zzping-collector --help`
- [ ] Example config loads successfully
- [ ] Invalid configs are rejected with clear errors
- [ ] Logging works at different levels (info/debug/trace)
- [ ] No panics on any error condition
- [ ] All code committed: `git status` shows clean

**SELF-CHECK QUESTIONS:**
- Where is validation logic implemented? (Answer: config.rs, validate() method)
- What happens if config file doesn't exist? (Answer: Returns CollectorError::Config with context)
- How do you enable debug logging? (Answer: --debug flag)

**IF ANY FAILS:** Review Day 1 and fix issues before Day 2.

---

## Day 2: Component Integration (Phase 1: Builders)

**GOAL:** Create component builders and prepare for wiring.

**CONTEXT:** The three-phase lifecycle pattern:
1. **Builder Phase:** Create builder objects for each component
2. **Wire Phase:** Connect components together via SessionManager
3. **Start Phase:** Actually start the actor systems

Today we focus on Phase 1 (Builders). Tomorrow we'll do Phase 2 (Wiring) and Phase 3 (Starting).

---

### Morning: Component Builder Creation - [90 min]

#### Step 1: Add Component Builder Fields to Service - [30 min]

**ACTIONS:**
- [ ] Update `src/apps/zzping-collector/src/service.rs`:
  ```rust
  //! Service orchestration and component lifecycle management.

  use crate::config::CollectorConfig;
  use crate::error::{CollectorError, Result};
  use actix::Addr;
  use std::sync::Arc;

  // Component imports
  use zzintent_config::IntentConfigBuilder;
  use zzpinger::PingerBuilder;
  use zzmem_db::MemDBBuilder;
  use zzcollector_state::CStateBuilder;

  // Network imports
  use zznet_api::RoomId;
  use zznet_session::SessionManager;
  use zznet_auth::ApplicationRole;

  /// Main collector service that orchestrates all components.
  pub struct CollectorService {
      config: CollectorConfig,
  }

  impl CollectorService {
      /// Create a new collector service with the given configuration.
      pub fn new(config: CollectorConfig) -> Result<Self> {
          config.validate()?;
          Ok(Self { config })
      }

      /// Bootstrap Phase 1: Create component builders.
      ///
      /// This creates builder objects for each component but doesn't start them yet.
      /// Builders are used to configure components before wiring them together.
      fn create_builders(&self) -> Result<ComponentBuilders> {
          tracing::info!("Phase 1: Creating component builders");

          // TODO: Create actual builders in next step

          Ok(ComponentBuilders {
              // Will add fields here
          })
      }

      /// Start the collector service.
      ///
      /// This will:
      /// 1. Initialize all components
      /// 2. Connect to the database
      /// 3. Start the main event loop
      ///
      /// # Errors
      /// Returns error if service cannot start.
      pub async fn run(self) -> Result<()> {
          tracing::info!("Collector service starting with ID: {}", self.config.collector_id);

          // Phase 1: Create builders
          let builders = self.create_builders()?;
          tracing::info!("Component builders created");

          // TODO: Phase 2 (wire) and Phase 3 (start) in next steps

          Ok(())
      }
  }

  /// Container for component builders (Phase 1).
  struct ComponentBuilders {
      // Will add fields in next step
  }
  ```

- [ ] **VERIFY:** `cargo check -p zzping-collector`
  - Expected: Compiles with warnings about unused (that's OK)

#### Step 2: Implement Builder Creation - [60 min]

**CRITICAL:** Different components need different role configurations!

**ACTIONS:**
- [ ] Update `create_builders()` and add builder fields:
  ```rust
  use zzintent_config::{IntentConfigBuilder, IntentConfigRole};
  use zzpinger::{PingerBuilder, PingerRole};
  use zzmem_db::{MemDBBuilder, MemDBRole};
  use zzcollector_state::{CStateBuilder, CStateRole};

  /// Container for component builders (Phase 1).
  struct ComponentBuilders<TRole: ApplicationRole> {
      intent_config: IntentConfigBuilder<TRole>,
      pinger: PingerBuilder,
      memdb: MemDBBuilder<TRole>,
      cstate: CStateBuilder<TRole>,
  }

  impl CollectorService {
      // ... existing code ...

      /// Bootstrap Phase 1: Create component builders.
      fn create_builders<TRole: ApplicationRole>(&self) -> Result<ComponentBuilders<TRole>> {
          tracing::info!("Phase 1: Creating component builders");

          // IntentConfig: Collector role (receives config from database)
          let intent_config_role = IntentConfigRole::Collector;
          let intent_config = IntentConfigBuilder::new(intent_config_role);
          tracing::debug!("Created IntentConfigBuilder with Collector role");

          // Pinger: Active role (performs pings)
          let pinger_role = PingerRole::Active;
          let pinger = PingerBuilder::new(pinger_role);
          tracing::debug!("Created PingerBuilder with Active role");

          // MemDB: Collector role (buffers and sends data to database)
          let memdb_role = MemDBRole::Collector {
              batch_size: self.config.components.memdb_batch_size,
              flush_interval_secs: 10, // Flush every 10 seconds
          };
          let memdb = MemDBBuilder::new(memdb_role);
          tracing::debug!("Created MemDBBuilder with Collector role");

          // CState: Collector role (sends heartbeats to database)
          let cstate_role = CStateRole::Collector {
              collector_id: self.config.collector_id.clone(),
              heartbeat_interval_secs: self.config.components.heartbeat_interval_secs,
          };
          let cstate = CStateBuilder::new(cstate_role);
          tracing::debug!("Created CStateBuilder with Collector role");

          Ok(ComponentBuilders {
              intent_config,
              pinger,
              memdb,
              cstate,
          })
      }
  }
  ```

- [ ] **VERIFY:** `cargo check -p zzping-collector`
  - Expected: Should compile (with warnings about unused, that's OK)
  - **IF FAILS:** Common errors:
    - Missing imports: Add use statements for builder types
    - Wrong role types: Check component documentation for role enums
    - Generic type errors: Make sure `<TRole: ApplicationRole>` is on both struct and impl

- [ ] **COMMIT:** `git add -A && git commit -m "feat(collector): Implement component builder creation"`

**COMMON MISTAKES TO AVOID:**

**❌ MISTAKE 1:** Forgetting generic type parameter on impl block
```rust
// WRONG:
impl CollectorService {
    fn create_builders(&self) -> Result<ComponentBuilders<TRole>> { ... }
}

// CORRECT:
impl CollectorService {
    fn create_builders<TRole: ApplicationRole>(&self) -> Result<ComponentBuilders<TRole>> { ... }
}
```

**❌ MISTAKE 2:** Using wrong role for collector side
```rust
// WRONG: Database roles on collector!
let memdb_role = MemDBRole::Database { ... };  // NO!

// CORRECT: Collector roles
let memdb_role = MemDBRole::Collector { ... };  // YES!
```

**❌ MISTAKE 3:** Not matching role to component purpose
- IntentConfig: `Collector` (receives config)
- Pinger: `Active` (performs pings)
- MemDB: `Collector` (sends data)
- CState: `Collector` (reports status)

---

### Afternoon: SessionManager Setup - [90 min]

#### Step 1: Add SessionManager Creation - [45 min]

**CONTEXT:** SessionManager is the communication hub. All components register their room handlers with it.

**ACTIONS:**
- [ ] Update service.rs to add SessionManager creation:
  ```rust
  use tokio::sync::mpsc;

  impl CollectorService {
      /// Bootstrap Phase 1.5: Create SessionManager (before wiring).
      ///
      /// SessionManager is created before wiring so components can register
      /// their room handlers with it.
      async fn create_session_manager<TRole: ApplicationRole>() -> Result<Arc<SessionManager<TRole>>> {
          tracing::info!("Creating SessionManager for collector");

          // Create SessionManager in client mode
          // Note: We don't connect yet - that happens in Phase 3
          let session_manager = SessionManager::new();

          tracing::info!("SessionManager created");
          Ok(Arc::new(session_manager))
      }

      /// Start the collector service.
      pub async fn run(self) -> Result<()> {
          tracing::info!("Collector service starting with ID: {}", self.config.collector_id);

          // Phase 1: Create builders
          let builders = self.create_builders()?;
          tracing::info!("Component builders created");

          // Phase 1.5: Create SessionManager
          let session_manager = Self::create_session_manager().await?;
          tracing::info!("SessionManager created");

          // TODO: Phase 2 (wire) and Phase 3 (start)

          Ok(())
      }
  }
  ```

- [ ] **VERIFY:** `cargo check -p zzping-collector`
  - Expected: Should compile

**Note:** Actual SessionManager API might differ. Check `zznet-session` documentation if this doesn't compile. The key is creating the SessionManager before wiring components to it.

#### Step 2: Write Test for Builder Creation - [45 min]

**ACTIONS:**
- [ ] Create test file: `src/apps/zzping-collector/tests/service_tests.rs`:
  ```rust
  //! Tests for service orchestration.

  use zzping_collector::*;

  /// Helper to create a valid test config.
  fn create_test_config() -> CollectorConfig {
      CollectorConfig {
          collector_id: "test-collector".into(),
          database: DatabaseConfig {
              host: "127.0.0.1".into(),
              port: 8443,
          },
          tls: TlsConfig {
              ca_cert_path: "test_certs/ca.pem".into(),
              client_cert_path: "test_certs/collector.pem".into(),
              client_key_path: "test_certs/collector.key".into(),
          },
          components: ComponentConfig {
              heartbeat_interval_secs: 5,
              memdb_batch_size: 50,
          },
      }
  }

  #[test]
  fn test_service_creation() {
      let config = create_test_config();
      let service = CollectorService::new(config);
      assert!(service.is_ok());
  }

  #[test]
  fn test_service_creation_validates_config() {
      let mut config = create_test_config();
      config.collector_id = String::new(); // Invalid!

      let result = CollectorService::new(config);
      assert!(result.is_err());
  }

  // Note: Can't easily test run() yet since it's async and starts actors
  // We'll add integration tests later
  ```

- [ ] **VERIFY:** `cargo test -p zzping-collector service`
  - Expected: 2 tests pass

- [ ] **COMMIT:** `git add -A && git commit -m "feat(collector): Add SessionManager creation"`

**DONE CRITERIA:**
- [ ] Component builders create successfully
- [ ] Each builder has correct role configuration
- [ ] SessionManager created before wiring
- [ ] Service creation tests pass

---

### 🛑 CHECKPOINT 3: Builders Phase Complete

**VERIFY:**
- [ ] All builders create with correct roles
- [ ] SessionManager created successfully
- [ ] Service tests pass: `cargo test -p zzping-collector`
- [ ] No compilation errors or warnings (except unused)
- [ ] Code compiles: `cargo check -p zzping-collector`

**SELF-CHECK:**
- What role does MemDB use on collector side? (Answer: Collector role)
- When is SessionManager created? (Answer: After builders, before wiring)
- Why separate builder creation from wiring? (Answer: Allows configuration before connection)

---

## Day 3: Component Wiring and Starting

**GOAL:** Wire components to SessionManager and start them.

---

### Morning: Wire Components to SessionManager - [2 hours]

**CONTEXT:** "Wiring" means connecting component builders to the SessionManager so they can communicate. This is Phase 2 of the three-phase lifecycle.

#### Step 1: Understand Wiring Pattern - [15 min]

**READ THIS CAREFULLY:**

Each component builder has a method like:
```rust
builder.with_session_manager(session_manager.clone())
```

This:
1. Gives the component access to SessionManager
2. Allows the component to register room handlers
3. Prepares the component for network communication

**IMPORTANT:** Components are NOT started yet! We're just wiring them together.

#### Step 2: Implement Component Wiring - [90 min]

**ACTIONS:**
- [ ] Update `service.rs` to add wiring phase:
  ```rust
  impl CollectorService {
      /// Bootstrap Phase 2: Wire components to SessionManager.
      ///
      /// This connects each component to the SessionManager, allowing them
      /// to register room handlers and prepare for communication.
      fn wire_components<TRole: ApplicationRole>(
          builders: ComponentBuilders<TRole>,
          session_manager: Arc<SessionManager<TRole>>,
      ) -> Result<WiredComponents<TRole>> {
          tracing::info!("Phase 2: Wiring components to SessionManager");

          // Wire IntentConfig
          let intent_config = builders.intent_config
              .with_session_manager(session_manager.clone())
              .map_err(|e| CollectorError::Component(
                  format!("Failed to wire IntentConfig: {}", e)
              ))?;
          tracing::debug!("Wired IntentConfig to SessionManager");

          // Wire Pinger (may not need SessionManager, check component API)
          let pinger = builders.pinger; // Pinger might not wire to SessionManager directly
          tracing::debug!("Pinger prepared (no SessionManager needed)");

          // Wire MemDB
          let memdb = builders.memdb
              .with_session_manager(session_manager.clone())
              .map_err(|e| CollectorError::Component(
                  format!("Failed to wire MemDB: {}", e)
              ))?;
          tracing::debug!("Wired MemDB to SessionManager");

          // Wire CState
          let cstate = builders.cstate
              .with_session_manager(session_manager.clone())
              .map_err(|e| CollectorError::Component(
                  format!("Failed to wire CState: {}", e)
              ))?;
          tracing::debug!("Wired CState to SessionManager");

          Ok(WiredComponents {
              intent_config,
              pinger,
              memdb,
              cstate,
              session_manager,
          })
      }
  }

  /// Container for wired components (Phase 2).
  struct WiredComponents<TRole: ApplicationRole> {
      intent_config: IntentConfigBuilder<TRole>,
      pinger: PingerBuilder,
      memdb: MemDBBuilder<TRole>,
      cstate: CStateBuilder<TRole>,
      session_manager: Arc<SessionManager<TRole>>,
  }
  ```

- [ ] Update `run()` method to call wiring:
  ```rust
  pub async fn run(self) -> Result<()> {
      tracing::info!("Collector service starting with ID: {}", self.config.collector_id);

      // Phase 1: Create builders
      let builders = self.create_builders()?;
      tracing::info!("Component builders created");

      // Phase 1.5: Create SessionManager
      let session_manager = Self::create_session_manager().await?;
      tracing::info!("SessionManager created");

      // Phase 2: Wire components
      let wired = Self::wire_components(builders, session_manager)?;
      tracing::info!("Components wired to SessionManager");

      // TODO: Phase 3 (start components)

      Ok(())
  }
  ```

- [ ] **VERIFY:** `cargo check -p zzping-collector`
  - Expected: Should compile
  - **IF FAILS:** Common errors:
    - Method not found: Check component builder API, might be named differently
    - Type mismatch: Ensure SessionManager generic type matches builders

**COMMON MISTAKES:**

**❌ MISTAKE 1:** Forgetting to clone SessionManager
```rust
// WRONG: Moves SessionManager, can't use for next component
builder.with_session_manager(session_manager)

// CORRECT: Clones Arc, can reuse
builder.with_session_manager(session_manager.clone())
```

**❌ MISTAKE 2:** Not all components need SessionManager
- Pinger might not need it (check component docs)
- Only components that communicate over network need wiring

**❌ MISTAKE 3:** Wiring in wrong order
- Order usually doesn't matter, but some components might depend on others being wired first
- If you see errors, try wiring in this order: IntentConfig → MemDB → CState → Pinger

---

### Afternoon: Start Components (Phase 3) - [2 hours]

**CONTEXT:** Now we actually start the actor systems. This is Phase 3 - the final phase of component lifecycle.

#### Step 1: Implement Component Starting - [90 min]

**ACTIONS:**
- [ ] Update `service.rs` to add starting phase:
  ```rust
  use actix::System;

  impl CollectorService {
      /// Bootstrap Phase 3: Start all components.
      ///
      /// This actually starts the actor systems, spawning background tasks
      /// and beginning component operation.
      async fn start_components<TRole: ApplicationRole>(
          wired: WiredComponents<TRole>,
      ) -> Result<StartedComponents<TRole>> {
          tracing::info!("Phase 3: Starting components");

          // Start IntentConfig
          let intent_addr = wired.intent_config.start()
              .map_err(|e| CollectorError::Component(
                  format!("Failed to start IntentConfig: {}", e)
              ))?;
          tracing::info!("IntentConfig started");

          // Start Pinger
          let pinger_addr = wired.pinger.start()
              .map_err(|e| CollectorError::Component(
                  format!("Failed to start Pinger: {}", e)
              ))?;
          tracing::info!("Pinger started");

          // Start MemDB
          let memdb_addr = wired.memdb.start()
              .map_err(|e| CollectorError::Component(
                  format!("Failed to start MemDB: {}", e)
              ))?;
          tracing::info!("MemDB started");

          // Start CState
          let cstate_addr = wired.cstate.start()
              .map_err(|e| CollectorError::Component(
                  format!("Failed to start CState: {}", e)
              ))?;
          tracing::info!("CState started");

          Ok(StartedComponents {
              intent_config: intent_addr,
              pinger: pinger_addr,
              memdb: memdb_addr,
              cstate: cstate_addr,
              session_manager: wired.session_manager,
          })
      }
  }

  /// Container for started components (Phase 3).
  struct StartedComponents<TRole: ApplicationRole> {
      intent_config: Addr<IntentConfigActor<TRole>>,
      pinger: Addr<PingerActor>,
      memdb: Addr<MemDBActor<TRole>>,
      cstate: Addr<CStateActor<TRole>>,
      session_manager: Arc<SessionManager<TRole>>,
  }
  ```

- [ ] Update `run()` to call starting:
  ```rust
  pub async fn run(self) -> Result<()> {
      tracing::info!("Collector service starting with ID: {}", self.config.collector_id);

      // Phase 1: Create builders
      let builders = self.create_builders()?;
      tracing::info!("Component builders created");

      // Phase 1.5: Create SessionManager
      let session_manager = Self::create_session_manager().await?;
      tracing::info!("SessionManager created");

      // Phase 2: Wire components
      let wired = Self::wire_components(builders, session_manager)?;
      tracing::info!("Components wired to SessionManager");

      // Phase 3: Start components
      let components = Self::start_components(wired).await?;
      tracing::info!("All components started successfully");

      // TODO: Connection and main loop in Day 4-5

      Ok(())
  }
  ```

- [ ] **VERIFY:** `cargo check -p zzping-collector`
  - Expected: Should compile
  - **IF FAILS:** Check actor type names match component exports

- [ ] **COMMIT:** `git add -A && git commit -m "feat(collector): Implement component wiring and starting"`

**DONE CRITERIA:**
- [ ] All components wire to SessionManager
- [ ] All components start successfully
- [ ] Proper error handling with context
- [ ] Logging at each phase

---

### 🛑 CHECKPOINT 4: Component Lifecycle Complete

**VERIFY:**
- [ ] Code compiles: `cargo check -p zzping-collector`
- [ ] Three phases implemented: Builders → Wire → Start
- [ ] All components have addresses after starting
- [ ] Error handling throughout (no unwrap)
- [ ] Logging shows progress through phases

**SELF-CHECK:**
- What are the three phases? (Answer: Builder, Wire, Start)
- Why wire before starting? (Answer: Components need SessionManager reference before actors spawn)
- Which components need SessionManager? (Answer: IntentConfig, MemDB, CState - not Pinger)

---

## Day 4 & 5: Database Connection and Main Loop

[TO BE CONTINUED - Following same pattern with verification steps, common mistakes, and checkpoints]

---

## Day 6 & 7: Testing and Documentation

[TO BE CONTINUED]

---

## Success Criteria Checklist

Before creating PR, verify ALL of these:

### Functionality
- [ ] Collector connects to database via mTLS
- [ ] All components start successfully in correct order
- [ ] Configuration updates flow from database through IntentConfig to Pinger
- [ ] Pings execute based on configuration
- [ ] Results flow from Pinger → MemDB → Database
- [ ] Heartbeats send periodically from CState
- [ ] Graceful shutdown works (all actors stop cleanly)
- [ ] Reconnection works after network disconnect

### Code Quality
- [ ] All tests pass: `cargo test -p zzping-collector`
- [ ] No compiler warnings: `cargo build --bin zzping-collector 2>&1 | grep warning`
- [ ] No clippy warnings: `cargo clippy -p zzping-collector -- -D warnings`
- [ ] Code formatted: `cargo fmt --check -p zzping-collector`
- [ ] >85% code coverage for service.rs
- [ ] No `.unwrap()` on fallible operations
- [ ] All errors have context (using `.context()`)
- [ ] Follows `AGENT_CODING_STANDARDS.md`

### Documentation
- [ ] README.md created with:
  - [ ] Purpose and overview
  - [ ] Configuration documentation
  - [ ] Usage examples
  - [ ] Deployment instructions
- [ ] All public items have doc comments
- [ ] Example configuration file included
- [ ] Integration guide written

### Integration
- [ ] Works with mock database (tests)
- [ ] All component interactions verified
- [ ] Error scenarios handled gracefully
- [ ] Logs are informative at INFO level

---

## If You Get Stuck

### Common Issues and Solutions

1. **"cannot find type `XYZ` in this scope"**
   - Add missing import: `use path::to::XYZ;`
   - Check component exports in their lib.rs

2. **"trait bounds not satisfied"**
   - Add `Unpin` bound: `T: ApplicationRole + Unpin`
   - Add `Send` bound if used in tokio::spawn

3. **"Rc<T> cannot be sent between threads safely"**
   - Replace `Rc` with `Arc` for thread-safe sharing
   - Components used in async must be `Send`

4. **"method `start` not found"**
   - Check builder API, might be named `build()` or similar
   - Ensure you called `with_session_manager()` first

5. **Components start but don't communicate**
   - Verify room names match exactly
   - Check room handlers registered before connection
   - Add debug logging to message handlers

### Getting Help

If stuck >30 minutes:
1. Read the error message carefully
2. Check `PHASE_CHECKLIST_IMPROVEMENTS.md` for similar issues
3. Review PR25_REVIEW.md for patterns of mistakes
4. Examine working component code (zzintent-config)
5. Ask for help with specific error message and context

---

**END OF IMPROVED PHASE 4 CHECKLIST**
