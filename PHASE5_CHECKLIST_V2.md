# Phase 5 Implementation Checklist V2: Database Application
# (IMPROVED VERSION - Use this instead of PHASE5_CHECKLIST.md)

**Target:** Week 5 (Following Phase 4 Collector Application completion)
**Status:** Ready to begin - Phases 1-4 complete
**Goal:** Create a working database binary that receives, stores, and manages ping data

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

### Application Dependency Verification
- [ ] **VERIFY Collector App (Phase 4) Complete:**
  ```bash
  cargo check -p zzping-collector
  # Expected: "Finished dev" with no errors

  cargo test -p zzping-collector
  # Expected: Tests pass
  ```
- [ ] **IF COLLECTOR FAILS:** Phase 4 might not be complete. Verify with supervisor.

### Document Review (MANDATORY READING - Don't Skip!)
- [ ] **READ THIS ENTIRE CHECKLIST:** Don't skim - read all sections including common mistakes
- [ ] **READ:** `AGENT_CODING_STANDARDS.md` - Pay attention to error handling and doc comment rules
- [ ] **READ:** `COMPONENT_TEMPLATE_GUIDE.md` - Understand the three-phase lifecycle pattern
- [ ] **READ:** `PHASE4_CHECKLIST_V2.md` - Review the collector pattern (database is similar but server-side)
- [ ] **READ:** `TLS_DEBUGGING_GUIDE.md` - Critical for understanding server TLS setup
- [ ] **EXAMINE:** `src/apps/zzping-collector/` - Your reference for application structure

### Understanding Check (Answer These - Be Honest!)
- [ ] **PRIMARY PURPOSE:** Can you explain in 1-2 sentences what this database application does?
  - Write it here: _________________________________________________
  - Should be: "Server that accepts mTLS connections from collectors, receives ping data, stores it, and distributes configuration updates back to collectors."

- [ ] **CRITICAL DIFFERENCE FROM COLLECTOR:** How is database different from collector?
  - Write it here: _________________________________________________
  - Should mention: SERVER not client, ACCEPTS connections not CONNECTS, handles MULTIPLE collectors not just one database

- [ ] **CRITICAL FEATURES:** What are the 3 things this application MUST do?
  1. _________________________________________________
  2. _________________________________________________
  3. _________________________________________________
  - Should include: (1) Accept mTLS connections from multiple collectors, (2) Store ping data and distribute config, (3) Track collector health

- [ ] **TLS ROLE:** What TLS role does database use?
  - Write it here: _________________________________________________
  - Should be: SERVER role with server certificate, requires client certificates from collectors

**IF YOU CANNOT ANSWER THESE:** Stop and re-read the documentation.

### Execution Strategy Commitment
- [ ] **I COMMIT TO:** Creating files incrementally with `cargo check` after each step
- [ ] **I COMMIT TO:** Writing tests for each module before moving to the next
- [ ] **I COMMIT TO:** Running verification commands and checking expected outcomes
- [ ] **I COMMIT TO:** Committing after each completed increment
- [ ] **I COMMIT TO:** Not skipping checkpoints even if I think it's "obvious"
- [ ] **I COMMIT TO:** Reading TLS_DEBUGGING_GUIDE.md when working on network code
- [ ] **I COMMIT TO:** Asking for help if stuck >30 minutes instead of guessing

**SIGNATURE (Type your name/ID to confirm):** ___________________

---

## Day 1: Application Structure and Configuration

**GOAL:** Create the database binary crate structure and configuration loading system.

**CRITICAL CONTEXT:** The database is a SERVER application. This is fundamentally different from the collector (client). Key differences:

| Aspect | Collector (Client) | Database (Server) |
|--------|-------------------|-------------------|
| TLS Role | Client certificate | Server certificate + CA for verifying clients |
| Connection | Connects to 1 server | Accepts from N clients |
| SessionManager | Client mode | Server mode |
| Lifecycle | Connect → Run | Bind → Accept loop → Handle connections |

---

### Morning: Create Binary Crate Structure

**CONTEXT:** Similar to collector, but with server-specific considerations.

#### Step 1: Verify Directory Doesn't Exist - [5 min]

**ACTIONS:**
- [ ] Run: `ls -la src/apps/`
  - Expected: Should see zzping-collector but NOT zzping-database
  - **IF zzping-database EXISTS:** You're in the wrong state! Check with supervisor.
- [ ] Run: `ls -la src/apps/`
  - Expected: Directory exists with zzping-collector

#### Step 2: Create Cargo.toml - [15 min]

**ACTIONS:**
- [ ] Create directory: `mkdir -p src/apps/zzping-database`
- [ ] Create file: `src/apps/zzping-database/Cargo.toml` with EXACT content:
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
  tokio = { version = "1.0", features = ["full", "macros", "rt-multi-thread", "signal", "net"] }
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

  # TLS/crypto (SERVER side)
  rustls = "0.21"
  rustls-pemfile = "1.0"

  # Utilities
  chrono = "0.4"

  [dev-dependencies]
  tempfile = "3.0"
  tokio-test = "0.4"
  ```

- [ ] **VERIFY:** Run `cargo metadata --format-version 1 | grep zzping-database`
  - Expected: Should see package name in output
  - **IF NOT FOUND:** Check Cargo.toml syntax, ensure proper path

#### Step 3: Create Minimal main.rs - [5 min]

**ACTIONS:**
- [ ] Create file: `src/apps/zzping-database/src/main.rs` with EXACT content:
  ```rust
  //! ZZPing Database Application
  //!
  //! Server that accepts connections from collectors via mTLS, receives ping data,
  //! stores it in memory, and distributes configuration updates to collectors.

  use anyhow::Result;

  fn main() -> Result<()> {
      println!("zzping-database v0.1.0 starting...");
      println!("Phase 5 implementation in progress");
      Ok(())
  }
  ```

- [ ] **VERIFY:** Run `cargo build --bin zzping-database`
  - Expected: "Finished dev [unoptimized + debuginfo] target(s)" (may take a few minutes on first build)
  - **IF FAILS:** Check error messages carefully - likely dependency path issue

- [ ] **VERIFY:** Run `./target/debug/zzping-database`
  - Expected output:
    ```
    zzping-database v0.1.0 starting...
    Phase 5 implementation in progress
    ```
  - **IF FAILS:** Binary didn't compile, check previous step

- [ ] **COMMIT:** `git add -A && git commit -m "chore(database): Initialize binary crate with minimal main.rs"`

#### Step 4: Create lib.rs for Testable Logic - [15 min]

**CONTEXT:** Same pattern as collector - separate main.rs (entry point) from lib.rs (testable logic).

**ACTIONS:**
- [ ] Create file: `src/apps/zzping-database/src/lib.rs` with content:
  ```rust
  //! Database application library.
  //!
  //! Contains testable business logic separated from main() entry point.
  //! This allows unit testing of configuration, service orchestration, and
  //! component integration without running the full server.

  // Module declarations
  pub mod cli;
  pub mod config;
  pub mod error;
  pub mod service;
  pub mod tls;
  pub mod persistence;

  // Re-exports for convenience
  pub use cli::CliArgs;
  pub use config::DatabaseConfig;
  pub use error::DatabaseError;
  pub use service::DatabaseService;
  ```

- [ ] Create stub files with minimal content:

  **File:** `src/apps/zzping-database/src/error.rs`
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

      #[error("TLS error: {0}")]
      Tls(String),

      #[error("Component error: {0}")]
      Component(String),

      #[error("Persistence error: {0}")]
      Persistence(String),
  }

  /// Result type alias for database operations.
  pub type Result<T> = std::result::Result<T, DatabaseError>;
  ```

  **File:** `src/apps/zzping-database/src/cli.rs`
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

      /// Initialize data directory and exit
      #[arg(long)]
      pub init: bool,
  }
  ```

  **File:** `src/apps/zzping-database/src/config.rs`
  ```rust
  //! Configuration structures and loading.

  use serde::{Deserialize, Serialize};

  /// Database application configuration.
  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct DatabaseConfig {
      /// Network binding settings
      pub bind_host: String,
      pub bind_port: u16,

      /// TLS certificate paths (server certificates)
      pub tls: TlsConfig,

      /// Data storage settings
      pub persistence: PersistenceConfig,

      /// Component-specific settings
      pub components: ComponentConfig,

      /// Intent configuration file path
      pub intent_config_path: String,
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct TlsConfig {
      /// CA certificate for verifying client certificates
      pub ca_cert_path: String,
      /// Server certificate
      pub server_cert_path: String,
      /// Server private key
      pub server_key_path: String,
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct PersistenceConfig {
      /// Directory for storing ping data
      pub data_dir: String,
      /// Maximum size per data file (in bytes)
      pub max_file_size: usize,
      /// How many old files to keep
      pub max_files: usize,
  }

  #[derive(Debug, Clone, Serialize, Deserialize)]
  pub struct ComponentConfig {
      /// Stale timeout for collector state (seconds)
      pub stale_timeout_secs: u64,
      /// Maximum number of collectors to track
      pub max_collectors: Option<usize>,
  }

  impl DatabaseConfig {
      /// Load configuration from a RON file.
      ///
      /// # Errors
      /// Returns error if file cannot be read or parsed.
      pub fn load(path: &str) -> crate::error::Result<Self> {
          let content = std::fs::read_to_string(path)
              .map_err(|e| crate::error::DatabaseError::Config(
                  format!("Failed to read config file {}: {}", path, e)
              ))?;

          let config: Self = ron::from_str(&content)
              .map_err(|e| crate::error::DatabaseError::Config(
                  format!("Failed to parse config: {}", e)
              ))?;

          Ok(config)
      }

      /// Validate configuration values.
      ///
      /// # Errors
      /// Returns error if configuration has invalid values.
      pub fn validate(&self) -> crate::error::Result<()> {
          if self.bind_host.is_empty() {
              return Err(crate::error::DatabaseError::Config(
                  "bind_host cannot be empty".into()
              ));
          }

          if self.bind_port == 0 {
              return Err(crate::error::DatabaseError::Config(
                  "bind_port cannot be 0".into()
              ));
          }

          if self.components.stale_timeout_secs == 0 {
              return Err(crate::error::DatabaseError::Config(
                  "stale_timeout_secs cannot be 0".into()
              ));
          }

          if self.persistence.data_dir.is_empty() {
              return Err(crate::error::DatabaseError::Config(
                  "data_dir cannot be empty".into()
              ));
          }

          Ok(())
      }
  }
  ```

  **File:** `src/apps/zzping-database/src/service.rs`
  ```rust
  //! Service orchestration and component lifecycle management.

  use crate::config::DatabaseConfig;
  use crate::error::{DatabaseError, Result};

  /// Main database service that orchestrates all components.
  pub struct DatabaseService {
      config: DatabaseConfig,
  }

  impl DatabaseService {
      /// Create a new database service with the given configuration.
      pub fn new(config: DatabaseConfig) -> Result<Self> {
          config.validate()?;
          Ok(Self { config })
      }

      /// Start the database service.
      ///
      /// This will:
      /// 1. Initialize all components (Database roles)
      /// 2. Set up TLS server
      /// 3. Accept connections from collectors
      /// 4. Start the main event loop
      ///
      /// # Errors
      /// Returns error if service cannot start.
      pub async fn run(self) -> Result<()> {
          tracing::info!("Database service starting");
          tracing::info!("Binding to {}:{}", self.config.bind_host, self.config.bind_port);

          // TODO: Implement service logic in Day 2-5

          Ok(())
      }
  }
  ```

  **File:** `src/apps/zzping-database/src/tls.rs`
  ```rust
  //! TLS configuration for database server.
  //!
  //! The database acts as a TLS SERVER and requires client certificates
  //! from all connecting collectors.

  // Placeholder - will implement in Day 3
  ```

  **File:** `src/apps/zzping-database/src/persistence.rs`
  ```rust
  //! Data persistence layer for ping results.

  // Placeholder - will implement in Day 4
  ```

- [ ] **VERIFY:** Run `cargo check --bin zzping-database`
  - Expected: "Finished dev" with warnings about unused fields (that's OK)
  - **IF FAILS:** Check syntax errors, ensure all files created correctly

- [ ] **VERIFY:** Run `cargo test -p zzping-database`
  - Expected: "running 0 tests" and "test result: ok. 0 passed"
  - **IF FAILS:** Check compilation errors

- [ ] **COMMIT:** `git add -A && git commit -m "chore(database): Add module structure with stubs"`

**DONE CRITERIA:**
- [ ] Binary compiles: `cargo build --bin zzping-database` succeeds
- [ ] Binary runs: `./target/debug/zzping-database` prints startup message
- [ ] All modules compile: `cargo check -p zzping-database` succeeds
- [ ] Test harness works: `cargo test -p zzping-database` succeeds (0 tests OK)
- [ ] All changes committed with proper messages

---

### 🛑 CHECKPOINT 1: Crate Structure Complete

**VERIFY BEFORE PROCEEDING:**
- [ ] `cargo build --bin zzping-database` succeeds without errors
- [ ] Binary executable exists: `ls -la target/debug/zzping-database`
- [ ] Binary runs and prints message: `./target/debug/zzping-database`
- [ ] All module files created: cli.rs, config.rs, error.rs, service.rs, tls.rs, persistence.rs
- [ ] No compilation errors (warnings about unused code are OK)
- [ ] Git shows all files committed: `git status` shows clean or only new work

**IF ANY FAILS:** Stop. Fix the issue before moving to configuration implementation.

**SELF-CHECK QUESTIONS:**
- What TLS role does database use? (Answer: SERVER with server cert, requires client certs)
- How many collectors can connect? (Answer: Multiple - N clients)
- What's the key difference from collector app? (Answer: SERVER not client, ACCEPTS not CONNECTS)

---

### Afternoon: Configuration Loading and Validation - [90 min]

[Similar to Phase 4, but with server-specific configuration]

#### Step 1: Write Configuration Tests FIRST - [40 min]

**ACTIONS:**
- [ ] Create test file: `src/apps/zzping-database/tests/config_tests.rs`:
  ```rust
  //! Tests for configuration loading and validation.

  use zzping_database::config::*;
  use tempfile::NamedTempFile;
  use std::io::Write;

  /// Helper to create a valid test configuration.
  fn create_valid_config() -> DatabaseConfig {
      DatabaseConfig {
          bind_host: "0.0.0.0".into(),
          bind_port: 8443,
          tls: TlsConfig {
              ca_cert_path: "test_certs/ca.pem".into(),
              server_cert_path: "test_certs/database.pem".into(),
              server_key_path: "test_certs/database.key".into(),
          },
          persistence: PersistenceConfig {
              data_dir: "/var/lib/zzping".into(),
              max_file_size: 100 * 1024 * 1024, // 100MB
              max_files: 10,
          },
          components: ComponentConfig {
              stale_timeout_secs: 30,
              max_collectors: Some(100),
          },
          intent_config_path: "/etc/zzping/intent.ron".into(),
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
      assert!(result.unwrap_err().to_string().contains("stale"));
  }

  #[test]
  fn test_empty_data_dir_fails_validation() {
      let mut config = create_valid_config();
      config.persistence.data_dir = String::new();

      let result = config.validate();
      assert!(result.is_err());
      assert!(result.unwrap_err().to_string().contains("data_dir"));
  }

  #[test]
  fn test_load_valid_config_file() {
      let config_content = r#"
      DatabaseConfig(
          bind_host: "0.0.0.0",
          bind_port: 8443,
          tls: TlsConfig(
              ca_cert_path: "test_certs/ca.pem",
              server_cert_path: "test_certs/database.pem",
              server_key_path: "test_certs/database.key",
          ),
          persistence: PersistenceConfig(
              data_dir: "/var/lib/zzping",
              max_file_size: 104857600,
              max_files: 10,
          ),
          components: ComponentConfig(
              stale_timeout_secs: 30,
              max_collectors: Some(100),
          ),
          intent_config_path: "/etc/zzping/intent.ron",
      )
      "#;

      let mut temp_file = NamedTempFile::new().unwrap();
      temp_file.write_all(config_content.as_bytes()).unwrap();
      let path = temp_file.path().to_str().unwrap();

      let config = DatabaseConfig::load(path).expect("Failed to load config");
      assert_eq!(config.bind_host, "0.0.0.0");
      assert_eq!(config.bind_port, 8443);
      assert_eq!(config.components.stale_timeout_secs, 30);
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

- [ ] **VERIFY TESTS PASS:** Run `cargo test -p zzping-database config`
  - Expected: "test result: ok. 8 passed" (all config tests pass)
  - **IF FAILS:** The config.rs implementation from morning should make these pass. Check error messages.

- [ ] **COMMIT:** `git add -A && git commit -m "test(database): Add configuration loading and validation tests"`

[Continue with remaining Day 1 steps, Day 2-7 following the same pattern as Phase 4...]

---

## CRITICAL: Server vs Client Differences

Throughout this phase, remember these key differences:

### TLS Setup
```rust
// COLLECTOR (Client):
let connector = TlsConnector::new(client_config);
let stream = connector.connect(addr).await?;

// DATABASE (Server):
let acceptor = TlsAcceptor::new(server_config);
let stream = acceptor.accept(tcp_stream).await?;
```

### Certificate Files
```
Collector needs:
- ca.pem (to verify database)
- collector.pem (client certificate)
- collector.key (client private key)

Database needs:
- ca.pem (to verify collectors)
- database.pem (server certificate)
- database.key (server private key)
```

### SessionManager Mode
```rust
// Collector: Client mode, connects out
session_manager.connect(addr).await?;

// Database: Server mode, accepts in
let listener = TcpListener::bind(addr).await?;
loop {
    let (stream, peer_addr) = listener.accept().await?;
    // Handle connection
}
```

### Component Roles
```rust
// Collector: Sends data, receives config
IntentConfigRole::Collector
MemDBRole::Collector
CStateRole::Collector

// Database: Receives data, distributes config
IntentConfigRole::Database
MemDBRole::Database
CStateRole::Database
```

---

[TO BE CONTINUED WITH FULL DAYS 2-7 IMPLEMENTATION...]
**Note:** Continue here. The remainder of this checklist provides the full Day 1 evening and Days 2–7 detailed implementation plan. Follow the test-first, incremental pattern: write a failing test, run `cargo test`, implement minimal code to satisfy the test, run `cargo test` again, then commit.

---

### Day 1: Evening — Service Orchestration Tests and Skeleton

**GOAL:** Add test-first harness for `DatabaseService::run` and wire minimal startup orchestration so later pieces plug in cleanly.

Estimated time: 90 minutes

#### Step 1: Add service run tests FIRST — [30 min]

**ACTIONS:**
- [ ] Create tests: `src/apps/zzping-database/tests/service_tests.rs`
  - Write tests that assert `DatabaseService::new(...).await` constructs, and that `run()` returns Ok when components are provided via small test doubles.

Example test file (create exactly):
```rust
use zzping_database::{config::DatabaseConfig, service::DatabaseService};
use tempfile::tempdir;

#[tokio::test]
async fn service_new_and_run_with_stub_components() {
    // Minimal config
    let cfg = DatabaseConfig {
        bind_host: "127.0.0.1".into(),
        bind_port: 0, // ephemeral (test harness will not actually bind yet)
        tls: Default::default(),
        persistence: Default::default(),
        components: Default::default(),
        intent_config_path: "".into(),
    };

    let svc = DatabaseService::new(cfg).expect("Failed to create service");
    // run() should currently return Ok even if TODOs remain (implement minimal behavior)
    let result = svc.run().await;
    assert!(result.is_ok(), "Service run failed: {:?}", result.err());
}
```

**VERIFY:**
- [ ] Run: `cargo test -p zzping-database service_tests -- --nocapture`
  - Expected: The test may fail initially. Iterate: implement minimal behavior in `service::run` (already returns Ok) so the test passes.

**COMMIT:**
- [ ] `git add -A && git commit -m "test(database): Add DatabaseService run test"`

#### Common mistakes (Day 1 evening)
- ❌ Trying to implement full accept loop now — keep `run()` minimal. The goal is to produce a stable test harness.
- ❌ Importing heavy runtime types in unit tests instead of `#[tokio::test]` — use tokio test harness.

---

### Day 2: Server-side Component Builders (Database Roles)

**GOAL:** Implement the server-side variants of the component builders: IntentConfig, MemDB, and CState for Database role. Keep APIs symmetric with collector role but with database semantics.

Estimated time: 4 hours

#### Step 1: Test-first — Builder behavior tests — [60 min]

**ACTIONS:**
- [ ] Create tests: `src/apps/zzping-database/tests/builders_tests.rs`
  - Tests: each builder returns component instances for Database role and validates role-specific configuration.

Example tests to add (summary):
- `intent_config_builder_creates_database_role()` — verifies builder returns `IntentConfigComponent` with role Database
- `memdb_builder_creates_database_store()` — verifies memdb starts empty and accepts writes
- `cstate_builder_creates_database_cstate()` — verifies `CState` can record collector heartbeats

#### Step 2: Implement simple builders (stubs) — [90 min]

**ACTIONS:**
- [ ] Implement `components::intent_config::builder::for_database()` stub returning a struct that implements the minimal trait used by `DatabaseService`.
- [ ] Implement `components::zzmem_db::builder::for_database()` stub with an in-memory map and basic insert/get APIs used by tests.
- [ ] Implement `components::zzcollector_state::builder::for_database()` stub with heartbeat recording and stale-check API.

Keep implementations minimal and focused on the test signatures. Use `Arc<Mutex<...>>` to provide shared access where needed.

#### VERIFY:
- [ ] `cargo test -p zzping-database builders_tests`
  - Expected: All builder tests pass

#### CHECKPOINT 2: Server component builders pass tests
- [ ] All builder unit tests pass
- [ ] Hand off these concrete types to `service::run` as the components to initialize

#### Common mistakes (Day 2)
- ❌ Re-implement collector logic verbatim — database builders often invert behavior (accepting vs sending).
- ❌ Use `Rc` in code that crosses threads — use `Arc` for concurrency.

---

### Day 3: TLS Server Setup and Connection Acceptance

**GOAL:** Implement TLS server config loader and the accept loop skeleton. Write test-first unit tests to validate TLS config loading.

Estimated time: 6 hours (TLS is fiddly; allow buffer time)

#### Step 1: Test-first — TLS loader tests — [60 min]

**ACTIONS:**
- [ ] Create tests: `src/apps/zzping-database/tests/tls_tests.rs`
  - `test_load_server_tls_config_ok()` — ensure `tls::load_server_tls_config()` returns Ok for valid test certs
  - `test_load_server_tls_config_missing_key_fails()` — ensure missing key file results in a descriptive error

Example test snippet:
```rust
#[test]
fn test_load_server_tls_config_ok() {
    let cfg = load_server_tls_config("test_certs/ca.pem", "test_certs/database.pem", "test_certs/database.key");
    assert!(cfg.is_ok());
}
```

#### Step 2: Implement `tls::load_server_tls_config` — [120 min]

**ACTIONS:**
- [ ] Implement the code in `src/apps/zzping-database/src/tls.rs` to:
  - Read `server_cert_path`, `server_key_path`, `ca_cert_path`
  - Parse PEM with `rustls-pemfile` and convert to `rustls::Certificate` / `PrivateKey`
  - Create `RootCertStore`, add CA certs
  - Create `AllowAnyAuthenticatedClient` verifier
  - Build `ServerConfig` with `with_single_cert` and `with_client_cert_verifier`
  - Return `Arc<ServerConfig>` or error mapped to `DatabaseError::Tls`

Use the TLS_DEBUGGING_GUIDE patterns as a template.

#### Step 3: Accept loop skeleton in `service::run` — [90 min]

**ACTIONS:**
- [ ] In `service::run`, create an async accept loop skeleton that:
  - Binds `TcpListener` to `bind_host:bind_port`
  - Loads TLS config and constructs a `TlsAcceptor`
  - Accepts connections and spawns a tokio task to `handle_connection(stream)` which for now just logs and closes

**VERIFY:**
- [ ] `cargo test -p zzping-database tls_tests`
  - Expected: TLS tests pass
- [ ] `cargo build --bin zzping-database` then run: `RUST_LOG=info ./target/debug/zzping-database --config database.ron` and ensure it binds to configured port (if non-zero) and logs accept loop startup

#### CHECKPOINT 3: TLS server loads and accept loop skeleton
- [ ] TLS tests pass
- [ ] Service binds and accepts incoming TCP (no TLS handshake yet)

#### Common mistakes (Day 3)
- ❌ Using `ClientConfig` APIs for server — ensure `ServerConfig` and `TlsAcceptor` are used
- ❌ Not loading CA certs into root store — server must verify client certs
- ❌ Blocking file IO on the tokio runtime thread — prefer `tokio::fs` or do quick synchronous reads during startup only

---

### Day 4: Data Persistence (File-based + Rotation)

**GOAL:** Implement persistence layer for ping results with file rotation and recovery. Test-first approach with temporary directory.

Estimated time: 6 hours

#### Step 1: Tests for persistence API — [60 min]

**ACTIONS:**
- [ ] Create tests: `src/apps/zzping-database/tests/persistence_tests.rs`
  - Tests:
    - `write_and_read_record_roundtrip()` — writes a record and reads it back
    - `rotation_creates_new_file_when_size_exceeds()` — simulate many small writes until rotation triggers
    - `recovery_loads_existing_files_on_startup()` — write files into temp dir, init persistence, and verify records loaded

#### Step 2: Implement `persistence.rs` — [180 min]

**ACTIONS:**
- [ ] Implement a simple append-only file format with CRC or newline-delimited JSON records (choose newline-delimited JSON for simplicity)
- [ ] Implement functions:
  - `Persistence::new(path: &Path, max_file_size: usize, max_files: usize) -> Result<Self>`
  - `fn append(&self, record: &PingRecord) -> Result<()>`
  - `fn iter_records(&self) -> impl Iterator<Item=PingRecord>` or an async equivalent
  - `fn rotate_if_needed(&self) -> Result<()>`
  - `fn recover(&mut self) -> Result<()>` to load existing files at startup

Prefer using `tokio::fs::File` and `tokio::io::AsyncWriteExt` for background writing but synchronous file writes during tests are acceptable if wrapped via `spawn_blocking`.

#### VERIFY:
- [ ] `cargo test -p zzping-database persistence_tests`
  - Expected: All persistence tests pass

#### CHECKPOINT 4: Persistence implemented and tested
- [ ] Persistence API tests passing
- [ ] Add basic metrics for number of records written and last rotate time

#### Common mistakes (Day 4)
- ❌ Using `serde_json` with unbounded memory mapping—write records streaming to disk, not accumulating all in memory
- ❌ Forgetting to fsync on critical writes — at least provide an option to flush periodically for durability

---

### Day 5: Configuration Management and Distribution

**GOAL:** Implement loading of `intent` configs and ability to broadcast updated config to connected collectors. Test-first: verify distribution via `MockSessionManager`.

Estimated time: 4 hours

#### Step 1: Tests for intent loading and distribution — [60 min]

**ACTIONS:**
- [ ] Create tests: `src/apps/zzping-database/tests/intent_distribution_tests.rs`
  - `test_load_intent_config()` — loads a sample intent RON and verifies structure
  - `test_broadcast_intent_to_connected_collectors()` — uses `MockSessionManager` to capture outbound config messages

#### Step 2: Implement intent loader and broadcaster — [120 min]

**ACTIONS:**
- [ ] Implement `intent_loader::load(path) -> Result<IntentConfig>` using `ron::from_str`
- [ ] Implement `Broadcaster` that holds references to connected collector session handles and can `broadcast_intent(&self, intent: &IntentConfig)`
- [ ] Integrate broadcaster into `DatabaseService::run` to trigger a broadcast when the intent file is changed (watching can be postponed; explicit reload via CLI or signal is acceptable for MVP)

#### VERIFY:
- [ ] `cargo test -p zzping-database intent_distribution_tests`
  - Expected: Tests pass and messages are recorded by `MockSessionManager`

#### CHECKPOINT 5: Intent config distribution works with mock sessions
- [ ] Intent tests passing
- [ ] DatabaseService exposes an API to trigger reload and broadcast

#### Common mistakes (Day 5)
- ❌ Sending raw objects over network — serialize to the canonical wire format used by collectors
- ❌ Not verifying permission/ACL for which collectors receive which config — keep ACL simple for MVP and document limitations

---

### Day 6: Multi-collector Handling and Load Tests

**GOAL:** Ensure the server can manage N simultaneous collectors, handle heartbeats, and enforce `max_collectors`. Add integration-style tests with mocked or lightweight real connections.

Estimated time: 6 hours

#### Step 1: Tests for concurrency and limits — [90 min]

**ACTIONS:**
- [ ] Create tests: `src/apps/zzping-database/tests/multi_collector_tests.rs`
  - `test_enforce_max_collectors()` — spawn fake connections up to `max_collectors + 5`, verify extra connections are refused or dropped
  - `test_heartbeat_handling_under_load()` — simulate many collectors sending heartbeats and assert the CState tracks them properly

Use `tokio::spawn` with `MockSessionManager` or light-weight loopback TCP clients configured to speak the minimal handshake.

#### Step 2: Implement connection admission control — [120 min]

**ACTIONS:**
- [ ] Add admission control in `service::run` that keeps an atomic counter or registry of active collectors. When `max_collectors` is reached, refuse new connections with a brief TLS alert and log.
- [ ] Ensure collector disconnect removes registry entry (use a guard or Drop impl)

#### VERIFY:
- [ ] `cargo test -p zzping-database multi_collector_tests`
  - Expected: Tests pass under simulated load

#### CHECKPOINT 6: Multi-collector handling verified
- [ ] Admission control tests pass
- [ ] Heartbeat tests pass and do not leak resources

#### Common mistakes (Day 6)
- ❌ Using a global lock for every connection operation — prefer per-connection tasks and concurrent data structures (dashmap or Arc<Mutex<>> only where necessary)
- ❌ Not handling connection drops correctly — always remove from registry in drop handler

---

### Day 7: Documentation, Final Sanity Tests, and Polish

**GOAL:** Finalize docs, add CLI options for reload/init, run full integration sanity checks, and prepare for handoff.

Estimated time: 3–4 hours

#### Step 1: Documentation and README — [60 min]

**ACTIONS:**
- [ ] Update `src/apps/zzping-database/README.md` with:
  - How to run locally with test certificates
  - Config file example
  - How to trigger intent reload
  - Troubleshooting pointers (link to `TLS_DEBUGGING_GUIDE.md`)

#### Step 2: Sanity integration tests — [60–90 min]

**ACTIONS:**
- [ ] Add an end-to-end smoke test `tests/e2e_database_smoke.rs` that:
  - Starts database on an ephemeral port
  - Starts a minimal collector (or test client using openssl s_client) and performs a heartbeat exchange
  - Verifies persistence wrote at least one record

**VERIFY:**
- [ ] Run: `cargo test -p zzping-database --test e2e_database_smoke -- --nocapture`
  - Expected: Passes in CI-friendly timing (use small sleeps or time mocking where applicable)

#### Step 3: Final commits and branch cleanup — [30 min]

**ACTIONS:**
- [ ] Ensure all tests pass: `cargo test`
- [ ] Run `cargo clippy -p zzping-database -- -D warnings` and fix critical lints
- [ ] Commit final changes with clear messages
- [ ] Push branch and open PR against `main` with description referencing Phase 5 V2 checklist

#### DONE CRITERIA (Phase 5 completed):
- [ ] All unit and integration tests created in this checklist pass
- [ ] TLS server accepts connections and verifies client certificates
- [ ] Persistence layer writes/rotates and recovers data
- [ ] Intent config can be broadcast to connected collectors
- [ ] Server enforces `max_collectors` and handles connection lifecycle cleanly
- [ ] End-to-end smoke test passes
- [ ] Documentation updated and troubleshooting links present

#### Common mistakes (Day 7)
- ❌ Leaving heavy debug logging enabled in production builds — ensure log level toggles via CLI
- ❌ Not running final `cargo test` — always run full test suite before PR

---

## After Phase 5: Next Steps

- Create `PHASE6_CHECKLIST_V2.md` with the same pattern focusing on system integration, certificate infrastructure, long-running stability tests, performance testing, chaos scenarios, and full integration test matrix.
- Run a quick pass of `cargo test` across the workspace and fix any immediate regressions caused by new files.

---

### Quick Reference Commands (copyable)

```bash
# Build database binary
cargo build --bin zzping-database

# Run database with config
./target/debug/zzping-database --config database.ron

# Run one test file
cargo test -p zzping-database --test tls_tests -- --nocapture

# Run full package tests
cargo test -p zzping-database

# Run clippy for this package
cargo clippy -p zzping-database -- -D warnings
```

---

**END OF PHASE 5 CHECKLIST V2**
