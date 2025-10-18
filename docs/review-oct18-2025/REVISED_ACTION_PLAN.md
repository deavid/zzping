# Revised Action Plan - Based on Code Validation

**Previous Plan Source:** REPORT.md (High-Level Review)
**Current Status:** Validated against actual codebase and corrected
**Date:** October 18, 2025

---

## Key Correction to Previous Plan

### ❌ Old Priority #1 (From REPORT)
> "Fix Certificate Generation: I will provide a corrected `generate_certs.sh` script that includes the necessary SAN extensions."

### ✅ Corrected Reality
The script **ALREADY HAS** SAN extensions. No changes needed.

**What To Do Instead:**
1. Run existing script: `./generate_certs.sh --all`
2. Verify certs have SAN: `openssl x509 -text -noout -in test_certs/database.pem | grep -A2 "Subject Alternative Name"`
3. Move forward - no fixes needed

---

## Revised Action Plan (3 Priorities)

### Priority #1: Verify & Fix Test Harness (1-2 days)

**Current State:** Integration tests likely not registered in Cargo.toml

**What Needs to Happen:**
1. Verify integration test structure
   - Location: Likely in `src/integration-tests/` or similar
   - Check if they're registered in `Cargo.toml` as `[[test]]` sections

2. Ensure tests can run:
   ```bash
   cargo test --lib              # Unit tests
   cargo test --test e2e_smoke   # Integration tests
   ```

3. Fix test certs auto-generation:
   - Make sure certs are generated before tests run
   - Or ensure tests skip if certs missing (graceful failure)

4. Register all tests in CI pipeline

**Why This First:**
- Can't verify implementation work without working tests
- Takes relatively little time
- Unblocks validation of all future work

**Success Criteria:**
- `cargo test --all` runs without errors
- Both unit and integration tests execute
- Test certs are generated automatically if missing


### Priority #2: Implement Core Data Pipeline (3-5 days)

**This is the REAL MVP work.**

This phase connects all the pieces so the system actually does something:

#### Phase 2a: Wire Components in Collector App

**File:** `src/apps/zzping-collector/src/service.rs` - `run()` method

**Current (Lines 60-100):**
```rust
// Creates components but doesn't wire them or start any data flow
let builders = self.create_builders()?;
let _started = Self::start_components(builders).await?;
// ... then just waits for signal to exit
```

**Needed Changes:**
1. After creating IntentConfig builder, set initial targets:
   - Load targets from config file
   - Send to zzintent-config to broadcast

2. Subscribe zzpinger to zzintent-config updates:
   - IntentConfig broadcasts target changes
   - Pinger receives and updates its targets

3. Create SessionManager for network communication:
   - Initialize with configured database host/port
   - Set up TLS connection pool

4. Implement main message loop:
   - Periodic: flush MemDB batches to database
   - Handle: incoming ACKs from database
   - Monitor: health metrics

**Files to Modify:**
- `src/apps/zzping-collector/src/service.rs` - Add main loop
- `src/apps/zzping-collector/src/config.rs` - Add targets config
- Potentially `src/apps/zzping-collector/src/lib.rs` - New exports

**Estimated Complexity:** Medium - mostly wiring existing pieces

---

#### Phase 2b: Implement Batch Sending Protocol

**File:** `src/components/zzmem-db/src/actor.rs` - Enhance `send_batch()`

**Current State:**
- `send_batch()` exists but just sends and forgets
- No tracking of whether batch was received

**Needed Changes:**
1. Simple ACK protocol:
   - Collector sends: `MemDBMessage::SubmitBatch { results, timestamp }`
   - Database receives and stores
   - Database sends back: `MemDBMessage::BatchAck { timestamp }`
   - Collector clears buffer on ACK

2. Handle failures:
   - If no ACK within timeout (30 seconds?), retry send
   - Keep batch in buffer until ACK received
   - Log warnings if batch resent > 2 times

3. Track metrics:
   - Count successful sends
   - Count retries
   - Count dropped batches (if buffer overflows)

**Files to Modify:**
- `src/components/zzmem-db/src/network_messages.rs` - Add BatchAck message type
- `src/components/zzmem-db/src/actor.rs` - Implement ACK handling
- `src/components/zzmem-db/src/messages.rs` - Add metrics messages

**Estimated Complexity:** Medium - protocol logic

---

#### Phase 2c: Receive & Store on Database Side

**File:** `src/apps/zzping-database/src/service.rs` - Add message receive loop

**Current State:**
- TLS server setup exists (code is there but might be incomplete)
- MemDB actor running but not receiving batches

**Needed Changes:**
1. Create SessionManager that:
   - Listens on configured port
   - Accepts collector connections
   - Routes messages to MemDB component

2. Implement receiver handler:
   - When `MemDBMessage::SubmitBatch` arrives, forward to MemDB actor
   - MemDB stores in StorageBackend
   - Collector receives ACK automatically

3. Main loop:
   - Accept connections
   - Route messages
   - Handle disconnections (cleanup)

**Files to Modify:**
- `src/apps/zzping-database/src/service.rs` - Expand run() method
- Potentially new module for message routing

**Estimated Complexity:** Medium-High - most complex part

---

### Priority #3: Add Disk Persistence (2-3 days)

**Current State:** MemDB only stores in RAM; data lost on restart

**What Needs to Happen:**

#### Phase 3a: Create Storage Crate

**New File:** Create `src/storage/zzping-storage-v1/`

```
src/
├── storage/
│   └── zzping-storage-v1/
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs
│           └── chunked_v1.rs  (copied from src/old/common/zzping-lib)
```

**Steps:**
1. Copy `chunked_v1.rs` logic from `src/old/common/zzping-lib/`
2. Create clean wrapper crate
3. Implement write API: `write_batch(target, results)` → writes to disk
4. Implement read API: `read_results(target, from_time, to_time)` → reads from disk

**Files:**
- `Cargo.toml` - New workspace member
- `src/lib.rs` - API surface
- `src/chunked_v1.rs` - Copied from old codebase

**Estimated Complexity:** Low - mostly copy/paste with cleanup

---

#### Phase 3b: Integrate into Database-side MemDB

**File:** `src/components/zzmem-db/src/actor.rs` - Database role

**Current State:**
- Receives batches from collectors
- Stores only in-memory HashMap

**Needed Changes:**
1. When receiving `SubmitBatch` message in Database role:
   - Store in memory immediately (for fast ACK)
   - Spawn background task to write to disk

2. On startup:
   - Load existing data from disk into memory
   - Continue from where we left off

3. Add periodic flush:
   - Every N seconds, ensure all received data is persisted

**Files to Modify:**
- `src/components/zzmem-db/src/actor.rs` - Add disk I/O calls
- `src/components/zzmem-db/src/storage.rs` - Add persistence methods
- `src/components/zzmem-db/Cargo.toml` - Depend on new storage crate

**Estimated Complexity:** Medium - async disk I/O

---

## Implementation Timeline

```
Week 1:
├─ Priority #1 (Test Harness)
│  ├─ Monday: Verify test structure & registration (2 hours)
│  ├─ Tuesday: Fix any registration issues (4 hours)
│  ├─ Wednesday: Ensure tests run in CI (2 hours)
│  └─ Done: Can run full test suite
│
├─ Priority #2 (Data Pipeline) - START
│  ├─ Thursday-Friday: Phase 2a (Component wiring)
│  └─ Start: Monday of Week 2

Week 2:
├─ Priority #2 (Data Pipeline) - CONTINUE
│  ├─ Phase 2b (Batch sending protocol)
│  ├─ Phase 2c (Database receiving)
│  ├─ Integration testing
│  └─ Done: Can ping and store to database (in-memory)
│
├─ Priority #3 (Disk Persistence) - START
│  └─ Create storage crate & integrate

Week 3:
├─ Priority #3 (Disk Persistence) - FINISH
├─ End-to-end testing
├─ Performance tuning
└─ MVP Complete: "Collector pings, database stores data reliably"
```

---

## Success Criteria for Each Priority

### Priority #1 Success
- [ ] `cargo test --lib` passes (all unit tests)
- [ ] `cargo test --test '*'` passes (all integration tests)
- [ ] New test coverage includes end-to-end flow
- [ ] CI pipeline runs tests successfully

### Priority #2 Success
- [ ] Collector can connect to database
- [ ] Collector sends ping results to database
- [ ] Database receives and stores results in memory
- [ ] Database sends ACK back
- [ ] Collector handles retries on failure
- [ ] 100+ pings successfully stored
- [ ] No data loss during normal operation

### Priority #3 Success
- [ ] Database persists data to disk on startup
- [ ] Data survives process restart
- [ ] Can query historical data
- [ ] No data loss on restart
- [ ] Performance acceptable (< 100ms batch write)

---

## Risk Assessment

| Risk | Impact | Mitigation |
|------|--------|-----------|
| SessionManager not working | 🔴 HIGH | Already tested in components, just needs wiring |
| Network protocol version mismatch | 🟡 MEDIUM | Carefully test phase 2b; add version headers |
| Disk I/O performance | 🟡 MEDIUM | Use async writes; batch at application level |
| Data consistency on failure | 🟡 MEDIUM | OK/DESYNC protocol in Phase 2b; proper error handling |

---

## Resources Needed

### Documentation
- ✅ `ZZPing_Collector_Database_Migration_Zznet.md` - Already excellent
- ✅ Vision docs - Already comprehensive
- ⚠️ May need new docs on actual implementation pattern used

### Code References
- ✅ `src/old/common/zzping-lib/` - For chunked_v1 format
- ✅ Existing components - Models to follow
- ⚠️ May need SessionManager examples in zznet crates

### Testing Infrastructure
- ⚠️ Test harness (to be fixed in Priority #1)
- ⚠️ Mock database server for offline testing

---

## What Changed from Original REPORT Plan

| Aspect | Original Plan | Revised Plan | Reason |
|--------|---------------|--------------|--------|
| Priority #1 | Fix cert generation | Verify test harness | Certs already working; tests are actual blocker |
| Priority #2 | Implement data pipeline | Same | Correct priority |
| Priority #3 | Modernize chunked_v1 | Same | Correct priority |
| Priority #4 | Polish | Removed | Not needed before MVP |
| Certificate work | Major task | Minor task | Already done in script |
| Estimate | 2-3 weeks? | 3 weeks (Priority #2 expanded) | More realistic assessment |

---

## How to Use This Plan

1. **First:** Run `VERIFICATION_CHECKLIST.md` to see actual current state
2. **Then:** Come back to this plan and see which Priorities apply
3. **During implementation:** Break each Priority into daily tasks
4. **After each Priority:** Run tests to verify success
5. **Adjust:** Based on actual blockers found during implementation

---

## Next Steps for You

1. ✅ Read `ANALYSIS_COMPLETE.md` and `VISUAL_SUMMARY.md`
2. ✅ Run `VERIFICATION_CHECKLIST.md`
3. 📋 Come back with results
4. 🚀 We'll start with Priority #1 with specific implementation details

The foundation is solid. This is doable.
