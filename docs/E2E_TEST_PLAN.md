# End-to-End Integration Test Plan

## Objective

Create a true end-to-end test that validates the **complete protocol flow** between collector and database without requiring:
- Real TCP sockets
- Real TLS certificates
- Real ICMP ping operations
- Separate processes

## Current Gaps

### Why Existing Tests Fail
1. **Unit tests** - Only test isolated components, not protocol interactions
2. **TCP integration tests** - Test real processes but hide serialization/deserialization bugs
3. **Mock transport exists** - But not used in any integration tests

### What We Need to Catch
- ❌ HELLO protocol serialization/deserialization with real messages
- ❌ Room negotiation logic under different scenarios
- ❌ Role extraction from HELLO messages (currently broken - shows as None)
- ❌ Permission checks during message routing
- ❌ State synchronization (collector receives config, sends ping data)
- ❌ Session bridge message forwarding
- ❌ Error handling in protocol flow

## Architecture

### Test Structure
```
tests/integration/e2e_mock_transport_test.rs
├── Setup Phase
│   ├── Create mock transport pair (database_transport, collector_transport)
│   ├── Spawn database service components in same process
│   ├── Spawn collector service components in same process
│   └── Wire them via mock transport
├── Execution Phase
│   ├── Let HELLO handshake complete
│   ├── Verify room negotiation
│   ├── Send config from database → collector
│   ├── Send ping results from collector → database
│   └── Verify state updates
└── Verification Phase
    ├── Assert collector received config
    ├── Assert database received ping data
    ├── Assert no serialization errors
    └── Assert roles and rooms correct
```

### Key Components to Extract

**From `src/apps/zzping-database/src/service.rs`:**
```rust
// Extract into reusable function
fn create_database_components() -> Result<StartedComponents> {
    // Create IntentConfigActor
    // Create MemDBActor
    // Create CStateActor
    // Return all actors ready to run
}

// Extract into reusable function
fn create_database_connection_manager() -> ConnectionManager<DatabaseMessage, DatabaseRole> {
    // Create with correct offered rooms: [memdb, query]
    // Return configured ConnectionManager
}
```

**From `src/apps/zzping-collector/src/service.rs`:**
```rust
// Similar extraction for collector
fn create_collector_components() -> Result<StartedComponents> {
    // Create IntentConfigActor
    // Create MemDBActor
    // Create PingerActor (needs mock ping backend)
    // Return all actors ready to run
}

fn create_collector_connection_manager() -> ConnectionManager<CollectorMessage, CollectorRole> {
    // Create with correct offered rooms: [intent-config]
    // Return configured ConnectionManager
}
```

### Mock Transport Integration

Use `zznet_api::mock::create_mock_pair()`:
```rust
let (db_transport, collector_transport) = create_mock_pair("e2e_test");

// Send db_transport to ConnectionManager.accept()
// Send collector_transport to TcpTransportClient.connect()
```

Wrap in `MockServer` / `MockClient`:
```rust
let db_server = MockServer::new(vec![Box::new(db_transport)]);
let collector_client = MockClient::with_connection(Box::new(collector_transport));
```

### Logging

Use `tracing_subscriber` to capture all logs:
```rust
let subscriber = tracing_subscriber::fmt()
    .with_test_writer()
    .with_max_level(Level::DEBUG)
    .finish();
let guard = tracing::subscriber::set_default(subscriber);

// Test code here - all logs captured
```

Output example:
```
[DATABASE] Created IntentConfigActor
[DATABASE] Created MemDBActor
[DATABASE] ConnectionManager configured with rooms: [memdb, query]
[DATABASE] Waiting for transport...
[COLLECTOR] Created IntentConfigActor
[COLLECTOR] Created MemDBActor
[COLLECTOR] Created PingerActor
[COLLECTOR] ConnectionManager configured with rooms: [intent-config]
[TRANSPORT] Mock connection established
[HELLO] Starting HELLO handshake, peer_role=collector
[HELLO] Handshake complete, active_rooms=[intent-config]
[ROOM] Negotiated intent-config room
[MESSAGE] Collector received config: IntentConfig { targets: [8.8.8.8], ... }
[PING] Database received ping result: 1.2ms
[ASSERTION] Config sync successful ✓
[ASSERTION] Ping data sync successful ✓
```

## Test Cases

### Test 1: Basic HELLO + Room Negotiation
- Collector connects via mock transport
- HELLO handshake completes
- Room negotiation succeeds
- Verify active_rooms matches expected

### Test 2: Config Distribution
- Database holds IntentConfig
- Collector connects and subscribes to intent-config room
- Config is sent to collector
- Verify collector receives config correctly

### Test 3: Ping Data Upload
- Collector has PingerActor with mock ping backend
- Inject fake ping results
- Verify data flows through MemDB room to database
- Verify database MemDBActor receives it

### Test 4: Role Extraction (Currently Broken!)
- Verify peer role is extracted from HELLO message
- NOT hardcoded as "default-hostname"
- NOT set to None
- Should be "Collector" from peer_role in HELLO frame

### Test 5: Permission Checks
- Collector tries to access "memdb" room (should fail)
- Collector accesses "intent-config" room (should succeed)
- Verify session bridge enforces permissions

### Test 6: Serialization Round-trip
- Messages serialize correctly to bytes
- Messages deserialize correctly from bytes
- No data corruption
- All room types supported

## Implementation Phases

**Phase 1: Extract Reusable Code**
- Move component creation from service.rs to standalone functions
- Move ConnectionManager creation to standalone functions
- Create mock ping backend for PingerActor

**Phase 2: Build Test Infrastructure**
- Create mock transport pair
- Wrap in MockServer/MockClient
- Set up tracing subscriber

**Phase 3: Basic E2E Test**
- Spawn database components
- Spawn collector components
- Let HELLO handshake complete
- Assert basic connectivity

**Phase 4: Full Protocol Testing**
- Add config distribution test
- Add ping data upload test
- Add permission check test

**Phase 5: Verification & Documentation**
- Run test, capture logs
- Document all assertions
- Add performance benchmarks

## Expected Failure Points (What This Will Expose)

Currently broken (based on logs showing `peer_role: None`):
```
❌ Peer role not extracted from HELLO message
❌ peer_id shows "default-hostname" instead of certificate CN
❌ No ACL applied ("connected without ACL")
```

This test will:
1. Show exactly where role extraction fails
2. Show the HELLO message content
3. Show when/where role becomes None
4. Enable quick iteration on fixes

## Benefits

- ✅ Tests **real code**, not mocks of code
- ✅ **No TCP/TLS overhead** - runs in microseconds
- ✅ **Deterministic** - no timing issues
- ✅ **Comprehensive logging** - shows exact flow
- ✅ **Catches protocol bugs** - serialization, deserialization, routing
- ✅ **Can inject failures** - error handling tests
- ✅ **Reusable** - extract logic into libraries for broader use

