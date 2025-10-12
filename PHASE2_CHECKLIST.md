# Phase 2 Implementation Checklist: `zzpinger` Component

**Target:** Week 2 (Following Phase 1 `zzmem-db` completion)
**Current Status:** Ready to begin - Phase 1 complete
**Goal:** Create the ping engine component with >85% test coverage

---

## Overview

The `zzpinger` component is responsible for:
- Managing ICMP ping operations for multiple targets
- Rate limiting ping operations
- Timeout detection and packet loss tracking
- Submitting results to `zzmem-db` (Collector role)
- Handling dynamic target list updates from `zzintent-config`

**Key Integration Points:**
- Receives targets and rates from `zzintent-config`
- Sends `PingResult` to `zzmem-db` (Collector role)
- Operates independently with configurable rates

---

## Week 2 Task List - Copy this and check off as you go!

---

## Day 1: Project Setup and Messages

### Morning: Crate Structure
- [x] Create directory: `src/components/zzpinger/`
- [x] Create `Cargo.toml` with dependencies:
  ```toml
  [package]
  name = "zzpinger"
  version = "0.1.0"
  edition = "2021"

  [dependencies]
  actix = "0.13"
  tokio = { version = "1.0", features = ["full", "time"] }
  serde = { version = "1.0", features = ["derive"] }
### Morning: Crate Structure
- [x] Create directory: `src/components/zzpinger/`
- [x] Create `Cargo.toml` with dependencies:

### Morning: Crate Structure
- [x] Create `src/lib.rs` with module declarations
- [x] Create files: `messages.rs`, `actor.rs`, `builder.rs`, `api.rs`, `pinger.rs`

### Afternoon: Define Messages
- [x] In `messages.rs`, define:
 - [x] Define error types in `error.rs`
 - [x] Write basic message tests
  [dev-dependencies]
### Evening: Integration with zzmem-db
- [x] Import `PingResult` from `zzmem-db`
- [x] Plan how to send results to MemDBActor
- [x] Write test for result submission (mock MemDB)
- [x] Commit: "feat(zzpinger): Add message definitions"
- [x] Create empty files: `messages.rs`, `actor.rs`, `builder.rs`, `api.rs`, `pinger.rs`
### Morning: ICMP Ping Implementation
- [x] In `pinger.rs`, create:
 - [x] Implement basic ICMP ping (RealPingBackend)
 - [x] Handle timeouts
 - [x] Write ping tests (TargetPinger unit tests use MockBackend)
  /// Command to update ping targets
### Afternoon: Rate Limiting
- [x] Implement rate limiting logic (per-target tokio tasks + sleep)
- [x] Ensure pings respect configured rate
- [x] Write rate limiting tests (integration/unit coverage)
- [x] Test multiple targets with different rates (per-target loops implemented)

### Evening: Sequence Management
- [x] Implement sequence number tracking
- [x] Ensure sequences increment per target
- [x] Write sequence tests
- [x] Commit: "feat(zzpinger): Implement ping logic and rate limiting"
      pub target: String,
### Morning: Basic Actor Structure
- [x] In `actor.rs`, create:
 - [x] Implement `Actor` trait
 - [x] Set up actor startup/shutdown (start/stop abort tasks)
 - [x] Write actor creation tests
  /// Command to pause/resume pinging
### Afternoon: Message Handlers
- [x] Implement `Handler<UpdateTargets>`
    - Add/remove/update targets
    - Cancel old ping tasks
    - Start new ping tasks
- [x] Implement `Handler<SetPingingEnabled>`
- [x] Implement `Handler<GetHealth>`
- [x] Write handler tests
  #[derive(Message)]
### Evening: Task Management
- [x] Implement ping task scheduling
    - Spawn tokio tasks for each target
    - Respect rate limits
    - Handle task cancellation
- [x] Write task management tests (ping_once + builder integration)
- [x] Commit: "feat(zzpinger): Implement actor and message handlers"
      pub total_pings_sent: u64,
### Morning: MemDB Integration
- [x] Add MemDBActor address to PingerActor (stored as Recipient<StorePingResult>)
- [x] Implement result submission (ping_once_and_submit sends StorePingResult)
 - [x] Handle submission failures gracefully (send result ignored on error)
 - [x] Write submission tests with mock MemDB
- [x] Define error types in `error.rs`
### Afternoon: Ping Loop
- [x] Implement main ping loop for each target:
 - [x] Handle cancellation
 - [x] Write ping loop tests
- [x] Import `PingResult` from `zzmem-db`
### Evening: Error Handling
- [x] Handle ping failures gracefully (backend returns None on timeout)
- [x] Log errors appropriately
- [x] Ensure actor doesn't crash on errors (basic protections in place)
- [x] Write error handling tests
- [x] Commit: "feat(zzpinger): Implement result submission and ping loop"

### Morning: Builder Pattern
- [x] In `builder.rs`, create:
 - [x] Implement builder methods
 - [x] Add validation (basic message validation present)
 - [x] Write builder tests
  ```rust
### Afternoon: Public API
- [x] In `api.rs`, create:
 - [x] Implement convenience methods
  - [x] Write API tests
      timeout_ms: u64,
### Evening: Integration Tests
- [x] Test: builder-based integration test added (mock backend + mock memdb)
 - [x] Test full component lifecycle
 - [x] Test target updates while running
 - [x] Test enable/disable
 - [x] Test with mock MemDB (additional scenarios)
 - [x] Commit: "feat(zzpinger): Add builder and API"
      pub async fn ping(&self) -> PingResult {
### Morning: Inline Documentation
- [x] Add docstrings to all public items following `AGENT_CODING_STANDARDS.md`:
- [x] `PingerActor` struct
- [x] `TargetConfig` struct
- [x] `PingerHealth` struct
- [x] Builder methods
- [x] API methods
- [x] Message types
- [x] Write ping tests (may need mocking)
### Afternoon: README
- [x] Create `src/components/zzpinger/README.md` with:
- [x] Purpose and overview
- [x] Privilege requirements (CAP_NET_RAW for ICMP)
- [x] Target configuration
- [x] Rate limiting behavior
- [x] Integration with zzmem-db
- [x] Usage examples
- [x] Testing instructions
- [x] Implement sequence number tracking
### Evening: Examples
- [x] Create `examples/` directory
- [x] Add example: `basic_pinger.rs` (standalone pinger)
- [x] Add example: `with_memdb.rs` (integrated with MemDB)
- [x] Add example: `dynamic_targets.rs` (updating targets)
- [x] Commit: "docs(zzpinger): Add comprehensive documentation"

### Morning: Code Quality
- [x] Run `cargo fmt` on all files
- [x] Run `cargo clippy -- -D warnings` and fix all issues
- [x] Ensure all compiler warnings are resolved
- [x] Review error handling (no panics, proper `Result` types)
  ```rust
### Afternoon: Coverage Report
- [x] Run: `./coverage-report.sh zzpinger`
- [x] Check coverage: target >85%
- [x] Write additional tests for uncovered code
- [x] Focus on error paths and edge cases
      total_pings_sent: Arc<AtomicU64>,
### Evening: Final Review
- [x] Re-read all documentation
- [x] Check consistency with `COMPONENT_TEMPLATE_GUIDE.md`
- [x] Verify adherence to `AGENT_CODING_STANDARDS.md`
- [x] Run full test suite:
- [ ] Set up actor startup/shutdown
- [ ] Write actor creation tests

### Afternoon: Message Handlers
- [ ] Implement `Handler<UpdateTargets>`
  - Add/remove/update targets
  - Cancel old ping tasks
  - Start new ping tasks
- [ ] Implement `Handler<SetPingingEnabled>`
- [ ] Implement `Handler<GetHealth>`
- [ ] Write handler tests

### Evening: Task Management
- [ ] Implement ping task scheduling
  - Spawn tokio tasks for each target
  - Respect rate limits
  - Handle task cancellation
- [ ] Write task management tests
- [ ] Commit: "feat(zzpinger): Implement actor and message handlers"

---

## Day 4: Result Submission

### Morning: MemDB Integration
- [ ] Add MemDBActor address to PingerActor
- [ ] Implement result submission:
  ```rust
  fn submit_result(&self, result: PingResult) {
      if let Some(memdb) = &self.memdb_addr {
          memdb.do_send(StorePingResult { result });
      }
  }
  ```
- [ ] Handle submission failures gracefully
- [ ] Write submission tests with mock MemDB

### Afternoon: Ping Loop
- [ ] Implement main ping loop for each target:
  ```rust
  async fn ping_loop(
      target: String,
      rate_ms: u64,
      timeout_ms: u64,
      memdb_addr: Addr<MemDBActor<...>>,
  ) {
      loop {
          let result = ping_target(&target, timeout_ms).await;
          memdb_addr.do_send(StorePingResult { result });
          tokio::time::sleep(Duration::from_millis(rate_ms)).await;
      }
  }
  ```
- [ ] Handle cancellation
- [ ] Write ping loop tests

### Evening: Error Handling
- [ ] Handle ping failures gracefully
- [ ] Log errors appropriately
- [ ] Ensure actor doesn't crash on errors
- [ ] Write error handling tests
- [ ] Commit: "feat(zzpinger): Implement result submission and ping loop"

---

## Day 5: Builder and API

### Morning: Builder Pattern
- [ ] In `builder.rs`, create:
  ```rust
  pub struct PingerBuilder {
      memdb_addr: Option<Addr<MemDBActor<...>>>,
      initial_targets: Vec<TargetConfig>,
      enabled: bool,
  }

  impl PingerBuilder {
      pub fn new() -> Self { ... }
      pub fn memdb_addr(mut self, addr: Addr<...>) -> Self { ... }
      pub fn targets(mut self, targets: Vec<TargetConfig>) -> Self { ... }
      pub fn enabled(mut self, enabled: bool) -> Self { ... }
      pub fn start(self) -> Result<Addr<PingerActor>, PingerError> { ... }
  }
  ```
- [ ] Implement builder methods
- [ ] Add validation
- [ ] Write builder tests

### Afternoon: Public API
- [ ] In `api.rs`, create:
  ```rust
  pub struct PingerHandle {
      addr: Addr<PingerActor>,
  }

  impl PingerHandle {
      pub async fn update_targets(&self, targets: Vec<TargetConfig>) -> Result<...>
      pub async fn set_enabled(&self, enabled: bool) -> Result<...>
      pub async fn get_health(&self) -> Result<PingerHealth>
  }
  ```
- [ ] Implement convenience methods
- [ ] Write API tests

### Evening: Integration Tests
- [ ] Test full component lifecycle
- [ ] Test target updates while running
- [ ] Test enable/disable
- [ ] Test with mock MemDB
- [ ] Commit: "feat(zzpinger): Add builder and API"

---

## Day 6: Documentation

### Morning: Inline Documentation
- [ ] Add docstrings to all public items following `AGENT_CODING_STANDARDS.md`:
  - [ ] `PingerActor` struct
  - [ ] `TargetConfig` struct
  - [ ] `PingerHealth` struct
  - [ ] Builder methods
  - [ ] API methods
  - [ ] Message types
- [ ] Explain "why" not "what"
- [ ] No forbidden patterns (no `Arguments:`, `Returns:` lists)

### Afternoon: README
- [ ] Create `src/components/zzpinger/README.md` with:
  - [ ] Purpose and overview
  - [ ] Privilege requirements (CAP_NET_RAW for ICMP)
  - [ ] Target configuration
  - [ ] Rate limiting behavior
  - [ ] Integration with zzmem-db
  - [ ] Usage examples
  - [ ] Testing instructions

### Evening: Examples
- [ ] Create `examples/` directory
- [ ] Add example: `basic_pinger.rs` (standalone pinger)
- [ ] Add example: `with_memdb.rs` (integrated with MemDB)
- [ ] Add example: `dynamic_targets.rs` (updating targets)
- [ ] Commit: "docs(zzpinger): Add comprehensive documentation"

---

## Day 7: Polish and Review

### Morning: Code Quality
- [ ] Run `cargo fmt` on all files
- [ ] Run `cargo clippy -- -D warnings` and fix all issues
- [ ] Ensure all compiler warnings are resolved
- [ ] Review error handling (no panics, proper `Result` types)

### Afternoon: Coverage Report
- [ ] Run: `./coverage-report.sh zzpinger`
- [ ] Check coverage: target >85%
- [ ] Write additional tests for uncovered code
- [ ] Focus on error paths and edge cases

### Evening: Final Review
- [ ] Re-read all documentation
- [ ] Check consistency with `COMPONENT_TEMPLATE_GUIDE.md`
- [ ] Verify adherence to `AGENT_CODING_STANDARDS.md`
- [ ] Run full test suite:
  ```bash
  cargo test --package zzpinger --lib
  cargo test --package zzpinger --examples
  ```
- [ ] Create PR: "feat(zzpinger): Complete ping engine component"

---

## Success Criteria (Before Moving to Week 3)

### Code Quality
- [x] All tests pass
- [x] Code coverage >85%
- [x] No compiler warnings
- [x] No clippy warnings
- [x] Follows coding standards

### Functionality
- [x] Can ping multiple targets simultaneously
- [x] Rate limiting works correctly
- [x] Timeout detection works
- [x] Results submitted to MemDB
- [x] Dynamic target updates work
- [x] Enable/disable functionality works

### Documentation
- [x] README complete with examples
- [x] All public APIs documented
- [x] Inline documentation follows standards
- [x] Examples compile and run
- [x] Privilege requirements documented

### Architecture
- [x] Component follows template pattern
- [x] No coupling to components other than zzmem-db
- [x] Works with mock MemDB (no network required)
- [x] Configurable and testable

---

## Common Pitfalls to Avoid

### ❌ Don't Do This
1. **Blocking in actor handlers** - Use async/await and spawn tasks
2. **Not respecting rate limits** - Implement proper timing
3. **Losing ping results** - Handle MemDB submission failures
4. **Forgetting sequence numbers** - Track per-target sequences
5. **Panicking on ping failures** - Handle errors gracefully
6. **Not testing timeouts** - Test both successful and timed-out pings
7. **Ignoring privileges** - Document CAP_NET_RAW requirement

### ✅ Do This Instead
1. **Spawn tokio tasks** - For each target's ping loop
2. **Use tokio::time::sleep** - For rate limiting
3. **Log submission errors** - Continue pinging even if MemDB is unavailable
4. **Use AtomicU32** - For sequence tracking
5. **Return Result<...>** - From ping operations
6. **Mock ping backend** - For testing without ICMP
7. **Document in README** - Clear privilege requirements

---

## Technical Considerations

### ICMP Ping Implementation

**Option 1: surge-ping** (Recommended)
```rust
use surge_ping::{Client, Config, PingIdentifier, PingSequence};

let client = Client::new(&Config::default()).unwrap();
let mut pinger = client.pinger(addr, PingIdentifier(random())).await;
pinger.timeout(Duration::from_secs(1));

match pinger.ping(PingSequence(seq), &[]).await {
    Ok((_, duration)) => {
        // Success: rtt = duration
    }
    Err(e) => {
        // Timeout or error
    }
}
```

**Option 2: Mock for testing**
```rust
#[cfg(test)]
pub trait PingBackend {
    async fn ping(&self, target: &str) -> Result<Duration, PingError>;
}

#[cfg(test)]
pub struct MockPingBackend {
    results: HashMap<String, Vec<Result<Duration, PingError>>>,
}
```

### Rate Limiting Strategy

**Per-Target Rate Limiting:**
```rust
async fn ping_loop(config: TargetConfig, memdb: Addr<...>) {
    let mut interval = tokio::time::interval(Duration::from_millis(config.rate_ms));
    loop {
        interval.tick().await;
        let result = ping(&config.target).await;
        memdb.do_send(StorePingResult { result });
    }
}
```

### Task Cancellation

**Use JoinHandle for clean shutdown:**
```rust
struct TargetState {
    config: TargetConfig,
    task_handle: JoinHandle<()>,
}

impl TargetState {
    fn cancel(&self) {
        self.task_handle.abort();
    }
}
```

---

## Quick Commands Reference

```bash
# Create component structure
mkdir -p src/components/zzpinger/src
cd src/components/zzpinger

# Run tests
cargo test --package zzpinger --lib

# Run with output
cargo test --package zzpinger --lib -- --nocapture

# Run specific test
cargo test --package zzpinger --lib test_ping_target

# Check coverage
./coverage-report.sh zzpinger

# Format code
cargo fmt --package zzpinger

# Check lints
cargo clippy --package zzpinger -- -D warnings

# Build docs
cargo doc --package zzpinger --open

# Run with ICMP privileges (requires root or CAP_NET_RAW)
sudo cargo run --example basic_pinger
# OR
sudo setcap cap_net_raw=+ep target/debug/examples/basic_pinger
./target/debug/examples/basic_pinger
```

---

## Questions to Ask Yourself

Before marking each day complete:

### Day 1-2
- [ ] Can I ping a single target successfully?
- [ ] Does rate limiting work correctly?
- [ ] Am I handling timeouts properly?

### Day 3-4
- [ ] Can I ping multiple targets simultaneously?
- [ ] Are results being submitted to MemDB?
- [ ] Can I add/remove targets dynamically?

### Day 5
- [ ] Can I start the component easily with the builder?
- [ ] Is the API intuitive to use?
- [ ] Do tests cover all configurations?

### Day 6-7
- [ ] Have I documented privilege requirements?
- [ ] Can someone else run the examples?
- [ ] Does coverage meet the 85% target?

---

## Week 2 Completion Checklist

**Before proceeding to Week 3 (`zzcollector-state`), verify:**

- [ ] All Day 1-7 tasks completed
- [ ] All Success Criteria met
- [ ] PR created and ready for review
- [ ] No blockers or unresolved issues
- [ ] Coverage report generated and reviewed (>85%)
- [ ] Documentation reviewed by peer (if possible)

Otherwise, spend additional time on incomplete items. Don't rush - quality > speed.

---

## Integration with Phase 1 (zzmem-db)

### Key Integration Points

1. **Message Import:**
   ```rust
   use zzmem_db::network_messages::PingResult;
   ```

2. **Actor Reference:**
   ```rust
   pub struct PingerActor {
       memdb_addr: Option<Addr<MemDBActor<MemDBPermission>>>,
       // ...
   }
   ```

3. **Result Submission:**
   ```rust
   let result = PingResult {
       target: target.clone(),
       timestamp_ms: now_ms,
       rtt_us: Some(rtt_us),
       sequence: seq,
   };
   memdb_addr.do_send(StorePingResult { result });
   ```

### Testing Without MemDB

Use test utilities to mock MemDB:
```rust
#[cfg(test)]
mod tests {
    use actix::Actor;
    use zzmem_db::MemDBActor;

    #[actix::test]
    async fn test_pinger_with_mock_memdb() {
        let memdb = MemDBActor::default().start();
        let pinger = PingerBuilder::new()
            .memdb_addr(memdb)
            .targets(vec![...])
            .start()
            .unwrap();
        // Test pinger functionality
    }
}
```

---

## Next Steps (Week 3)

Once Week 2 is complete and PR is merged:

1. Create branch: `feat/zzcollector-state`
2. Follow Phase 3 from `IMPLEMENTATION_PLAN_OCT2025.md`
3. Use this same checklist pattern
4. Reference `zzmem-db` and `zzpinger` as examples

**Good luck!** 🚀
