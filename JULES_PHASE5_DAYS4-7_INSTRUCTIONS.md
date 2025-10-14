# Phase 5 Days 4-7 Implementation Instructions

**Date:** October 14, 2025
**For:** Jules (google-labs-jules)
**Status:** Ready to implement
**Current PR:** #27 (Days 1-3 complete, pending one fix)

---

## Executive Summary

Your Days 1-3 implementation was **excellent** (rated 4.4/5.0). There's one trivial fix needed before continuing, then you have detailed instructions for Days 4-7.

---

## Required Fix Before Days 4-7

### Issue: Clippy Dead Code Error

**File:** `src/apps/zzping-database/src/service.rs` (line 149)

**Current Code:**
```rust
/// Started components (running actors)
// This struct is intentionally unused for now, but will be used in the future
// to hold the addresses of the started actors.
struct StartedComponents {
    intent_config: Addr<IntentConfigActor<IntentConfigPermission>>,
    memdb_addr: Addr<MemDBActor<MemDBPermission>>,
    cstate:
        Addr<CStateActor<DatabaseMessage, DatabaseRole, SessionManager<DatabaseMessage, DatabaseRole>>>,
}
```

**Required Fix:**
```rust
/// Started components (running actors)
/// These addresses will be used in Days 4-7 when implementing connection handling.
/// Each connection handler gets a clone of these addresses to route messages.
#[allow(dead_code)]
struct StartedComponents {
    intent_config: Addr<IntentConfigActor<IntentConfigPermission>>,
    memdb_addr: Addr<MemDBActor<MemDBPermission>>,
    cstate:
        Addr<CStateActor<DatabaseMessage, DatabaseRole, SessionManager<DatabaseMessage, DatabaseRole>>>,
}
```

**What Changed:** Added `#[allow(dead_code)]` attribute because struct will be used in Day 4.

**Verification After Fix:**
```bash
cargo clippy --package zzping-database -- -D warnings
# Must output: Finished with NO errors
```

**Commit Message:**
```bash
git add src/apps/zzping-database/src/service.rs
git commit -m "fix(database): Allow dead code in StartedComponents for Days 4-7 usage"
git push
```

---

## What Was Reviewed

I conducted a comprehensive zero-trust review of your PR #27. Here's what I found:

### ✅ Strengths (Excellent Work!)

1. **Perfect API Patterns (5/5)**
   - IntentConfigBuilder::new().role() ✅
   - MemDBActor::new_with_role() ✅
   - DATABASE component roles ✅
   - ServerConfig for TLS ✅
   - LocalSet for Actix ✅

2. **Code Quality (4.8/5)**
   - main.rs: 5/5 - Perfect LocalSet pattern
   - lib.rs: 5/5 - Clean structure
   - cli.rs: 5/5 - Simple and effective
   - config.rs: 5/5 - Excellent validation
   - error.rs: 5/5 - Proper thiserror usage
   - service.rs: 4/5 - Only issue is clippy warning

3. **Testing (5/5)**
   - 12 tests total (8 config + 4 service)
   - Exceeds Phase 4 baseline of 11 tests
   - All tests passing
   - Good coverage of happy/error paths

4. **Documentation (5/5)**
   - Excellent example config
   - Clear inline comments
   - Good module docs

5. **Phase 5 Checklist Compliance (5/5)**
   - 100% compliance with Days 1-3 requirements
   - All 33 checklist items complete

### ⚠️ One Issue (Trivial)

- Clippy fails with dead code warning on StartedComponents
- Fix: Add `#[allow(dead_code)]` attribute (see above)
- This is actually good forward planning, not a mistake!

### Overall Rating: 4.4/5.0

**Verdict:** Approved pending the one-line fix above.

---

## What's Next: Days 4-7 Overview

I've updated `PHASE5_CHECKLIST_V3.md` with complete instructions for Days 4-7. Here's the roadmap:

### Day 4: TCP Listener and Connection Acceptance (3-4 hours)
**Goal:** Accept incoming collector connections with TLS

**Key Tasks:**
- Add TCP listener to service
- Implement accept loop with tokio::select!
- Create connection handler stub
- Test with real collector

**Deliverables:**
- TCP listener binds to configured port
- Accepts multiple simultaneous connections
- TLS handshake succeeds
- Each connection spawned as separate task

**Key Change:** `StartedComponents` becomes `#[derive(Clone)]` and is actually used!

### Day 5: Connection Handler and Message Routing (4-5 hours)
**Goal:** Implement per-connection message handling

**Key Tasks:**
- Implement DatabaseRole::from_cn() (extract role from certificate)
- Create DatabaseMessage enum (wraps component messages)
- Implement ConnectionHandler struct
- Add message routing to components

**Deliverables:**
- Client certificates validated
- Roles extracted from CN
- Messages routed to correct components
- Connection lifecycle managed

### Day 6: Message Loop and Component Integration (3-4 hours)
**Goal:** Complete message read/write loop

**Key Tasks:**
- Implement message frame reading (length-prefixed)
- Deserialize DatabaseMessage
- Route to components via Actix messages
- Handle responses

**Deliverables:**
- Full message exchange works
- Components receive messages
- Responses sent back to collectors

### Day 7: Documentation and Final Testing (2-3 hours)
**Goal:** Polish and verify

**Key Tasks:**
- Write comprehensive README
- Enhance module documentation
- Run full test suite
- End-to-end manual testing
- Create final PR

**Deliverables:**
- Complete documentation
- All tests passing (12+)
- Clippy clean
- Ready for Phase 6

---

## Key Differences From Days 1-3

### Days 1-3 (What You Built)
- Application structure
- Configuration loading
- Component builders
- TLS configuration loading
- Service stub with signal handlers

### Days 4-7 (What's Next)
- **Day 4:** Actually accept connections (not just stub)
- **Day 5:** Handle connections (not just log)
- **Day 6:** Exchange messages (not just placeholders)
- **Day 7:** Document and test (make it production-ready)

---

## Important Code Changes Coming

### Day 4 Changes

**StartedComponents** goes from:
```rust
#[allow(dead_code)]  // ← Remove this
struct StartedComponents {
    ...
}
```

To:
```rust
#[derive(Clone)]  // ← Add this
struct StartedComponents {
    ...
}
```

**DatabaseService::run()** changes from:
```rust
let _started = Self::start_components(builders).await?;

// TODO: TLS server setup in Day 3
// TODO: Connection acceptance in Day 4

// Setup signal handlers
let mut sigterm = signal(SignalKind::terminate())?;
```

To:
```rust
let started = Self::start_components(builders).await?;  // ← Remove underscore

// Load TLS and create acceptor
let tls_config = Self::load_tls_config(&self.config.tls)?;
let acceptor = TlsAcceptor::from(tls_config);

// Create TCP listener
let listener = self.create_listener().await?;

// Accept loop
loop {
    tokio::select! {
        accept_result = listener.accept() => {
            // Spawn connection handler
        }
        _ = sigterm.recv() => break,
    }
}
```

### Day 5 Changes

**Add Real Types:**
```rust
// Replace placeholder DatabaseRole
impl ApplicationRole for DatabaseRole {
    fn from_cn(cn: &str) -> Result<Self, AuthError> {
        // Real implementation extracting from certificate CN
    }
}

// Replace placeholder DatabaseMessage
impl RoomMessageTrait for DatabaseMessage {
    // Real implementation
}
```

**Add ConnectionHandler:**
```rust
struct ConnectionHandler {
    peer_addr: SocketAddr,
    peer_role: DatabaseRole,
    stream: TlsStream<TcpStream>,
    components: StartedComponents,
}

impl ConnectionHandler {
    async fn run(mut self) -> Result<()> {
        // Message loop
    }
}
```

---

## Testing Strategy

### After Day 4
```bash
# Start database
./target/debug/zzping-database &

# Start collector (should connect and stay connected)
./target/debug/zzping-collector

# Expected logs:
# Database: "Accepted connection from 127.0.0.1:XXXXX"
# Database: "TLS handshake successful"
# Database: "Connection handler started"
# Collector: "Connected to database"
```

### After Day 5
```bash
# Should see role extraction:
# Database: "Client authenticated as role: Collector"

# Should see message routing:
# Database: "Routing message to MemDB"
```

### After Day 6
```bash
# Should see actual message exchange:
# Database: "Received message: ..."
# Database: "Routed to component"
# Collector: "Received response from database"
```

### After Day 7
```bash
# Full end-to-end test:
# - Start database
# - Start 5 collectors
# - All connect successfully
# - Messages flow both ways
# - Clean shutdown
```

---

## Quality Standards to Maintain

### From Your Days 1-3 Work
- ✅ Comprehensive validation
- ✅ File existence checks
- ✅ Clear error messages
- ✅ Good test coverage
- ✅ Excellent documentation
- ✅ Clean code structure

### For Days 4-7
- Maintain 12+ tests (add more as you go)
- Keep clippy clean (no warnings)
- Document all new public items
- Test both happy and error paths
- Clear commit messages
- Incremental commits after each step

---

## Where to Find Instructions

**File:** `PHASE5_CHECKLIST_V3.md`

**Structure:**
- Lines 1-900: Days 1-3 (what you already did)
- Lines 900-end: Days 4-7 (newly added, detailed)

**Read Carefully:**
- Each day has morning/afternoon sections
- Step-by-step code examples
- Verification commands after each step
- Checkpoints at end of each day
- Common mistakes to avoid
- Troubleshooting section

---

## Immediate Next Steps

1. **Apply the fix** (1 minute)
   ```bash
   # Edit src/apps/zzping-database/src/service.rs line 149
   # Add: #[allow(dead_code)]
   # Commit and push
   ```

2. **Verify fix** (1 minute)
   ```bash
   cargo clippy --package zzping-database -- -D warnings
   # Must pass with no errors
   ```

3. **Read Day 4 instructions** (10 minutes)
   ```bash
   # Read PHASE5_CHECKLIST_V3.md starting at "Day 4: TCP Listener"
   # Understand the accept loop pattern
   # Note the key changes to StartedComponents
   ```

4. **Start Day 4 implementation** (3-4 hours)
   ```bash
   # Follow checklist step by step
   # Commit after each checkpoint
   # Test with real collector
   ```

---

## Questions to Guide You

### Before Starting Day 4
- Do I understand why StartedComponents needs `#[derive(Clone)]`?
  - Answer: Each connection handler needs its own copy of component addresses
- What's the difference between accept loop and connection handler?
  - Answer: Accept loop runs once, spawns handler per connection
- Why tokio::select! instead of loop?
  - Answer: Allows graceful shutdown on signals

### Before Starting Day 5
- How do we extract role from certificate?
  - Answer: Parse CN field from X.509 certificate
- What's DatabaseMessage vs component messages?
  - Answer: DatabaseMessage wraps all component message types
- Why ConnectionHandler struct?
  - Answer: Encapsulates per-connection state and logic

### Before Starting Day 6
- How are messages framed?
  - Answer: Length-prefix (4 bytes big-endian) + body
- How do we route to components?
  - Answer: Match on DatabaseMessage variant, send via Actix
- What happens if component fails?
  - Answer: Log error, continue processing other messages

---

## Success Criteria

### Day 4 Success
- [ ] Database accepts connections
- [ ] TLS handshake succeeds
- [ ] Multiple collectors can connect
- [ ] Clippy passes (no dead code warning!)
- [ ] Connections close cleanly

### Day 5 Success
- [ ] Role extraction works
- [ ] ConnectionHandler lifecycle works
- [ ] Message routing infrastructure in place
- [ ] Connection stays alive during exchange

### Day 6 Success
- [ ] Messages read from stream
- [ ] Messages deserialized correctly
- [ ] Messages routed to components
- [ ] Responses sent back

### Day 7 Success
- [ ] README complete
- [ ] All docs updated
- [ ] 12+ tests passing
- [ ] End-to-end test works
- [ ] PR ready for review

---

## Communication

### When to Ask for Help
- Stuck for >30 minutes on same issue
- Unclear about architecture decision
- Tests failing unexpectedly
- Need clarification on requirements

### When to Report Progress
- After each day's checkpoint
- When encountering unexpected issues
- When deviating from checklist
- When completing major milestone

### What to Include in Progress Reports
- What you completed
- What worked well
- What was challenging
- Any deviations from checklist
- Next steps

---

## Final Notes

Your Days 1-3 work was **excellent**. You:
- ✅ Learned perfectly from Phase 4
- ✅ Applied correct API patterns
- ✅ Wrote comprehensive tests
- ✅ Created great documentation
- ✅ Showed good forward planning (StartedComponents)

The one clippy issue is trivial and shows you were thinking ahead.

Days 4-7 build directly on what you've built. The checklist is detailed and step-by-step. Follow it carefully, commit often, test frequently, and you'll have a production-quality database server.

**You've got this!** 🚀

---

**Document Version:** 1.0
**Last Updated:** October 14, 2025
**Next Review:** After Day 7 completion
