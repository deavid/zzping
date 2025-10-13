# ZZPing Troubleshooting Guide

**Purpose:** Systematic debugging for common issues across all phases
**Last Updated:** October 13, 2025
**For:** AI agents and developers working on ZZPing

---

## How to Use This Guide

### When You See an Error

1. **Read the FULL error message** - Don't skim, every word matters
2. **Identify the error category** - Use the table of contents below
3. **Follow the systematic debug process** for that category
4. **If still stuck after 30 minutes** - Ask for help with:
   - Exact error message
   - What you were trying to do
   - What you've already tried

### Debug Process Template

For ANY error, follow this pattern:

```
1. OBSERVE: What is the exact error message?
2. LOCATE: Which file/line is causing it?
3. UNDERSTAND: What is the code trying to do?
4. HYPOTHESIZE: What might be wrong?
5. TEST: Try the fix
6. VERIFY: Does it work now?
7. DOCUMENT: What was the root cause?
```

---

## Table of Contents

### [A] Compilation Errors
- [A1] "cannot find type X"
- [A2] "trait bounds not satisfied"
- [A3] "method not found"
- [A4] "mismatched types"
- [A5] "cannot be sent between threads safely"
- [A6] "cannot be unpinned"
- [A7] "the trait bound X is not satisfied"

### [B] Runtime Errors
- [B1] Panics and unwrap failures
- [B2] "Connection refused"
- [B3] "Address already in use"
- [B4] Component not responding
- [B5] Deadlocks and hangs

### [C] TLS/mTLS Errors
- [C1] "TLS handshake failed"
- [C2] "Bad certificate"
- [C3] "Unknown CA"
- [C4] "Certificate expired"
- [C5] "Wrong side of connection"

### [D] Component Integration Errors
- [D1] "No rooms in common"
- [D2] Messages not flowing
- [D3] Components start but don't communicate
- [D4] SessionManager errors

### [E] Test Failures
- [E1] Tests hanging indefinitely
- [E2] Flaky tests (pass sometimes, fail others)
- [E3] Integration test failures
- [E4] Mock setup issues

### [F] Actor/Actix Errors
- [F1] "Mailbox closed"
- [F2] "Actor already stopped"
- [F3] Message handler not called
- [F4] Context errors

---

## [A] Compilation Errors

### [A1] "cannot find type X in this scope"

**Error Example:**
```
error[E0412]: cannot find type `IntentConfigBuilder` in this scope
  --> src/service.rs:42:14
   |
42 |     let builder: IntentConfigBuilder = ...
   |                  ^^^^^^^^^^^^^^^^^^^ not found in this scope
```

**Root Cause:** Missing import statement.

**Systematic Debug:**
```
1. OBSERVE: What type is missing? (IntentConfigBuilder)
2. LOCATE: Which module exports it? (zzintent_config)
3. CHECK: Is the dependency in Cargo.toml?
4. CHECK: Does the module have a public export?
5. FIX: Add the import
```

**Solution:**
```rust
// Add at top of file:
use zzintent_config::IntentConfigBuilder;
```

**Common Variations:**
- `cannot find struct X` → Same fix, add import
- `cannot find enum X` → Same fix, add import
- `cannot find trait X` → Same fix, add import

**Verification:**
```bash
cargo check
# Should now compile
```

---

### [A2] "trait bounds not satisfied"

**Error Example:**
```
error[E0277]: the trait bound `SessionManager<...>: Unpin` is not satisfied
  --> src/actor.rs:24:5
   |
24 |     session_manager: Arc<SessionManager<TMsg, TRole>>,
   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   |     `SessionManager<...>` cannot be unpinned
```

**Root Cause:** Generic type parameter missing required trait bound.

**Systematic Debug:**
```
1. OBSERVE: Which trait is not satisfied? (Unpin)
2. LOCATE: Which type needs it? (SessionManager or its generics)
3. UNDERSTAND: Why is Unpin needed? (Actix requires it for actor fields)
4. CHECK: Does the impl block have the bound?
5. FIX: Add the trait bound
```

**Solution:**
```rust
// WRONG: Missing Unpin bound
impl<TMsg, TRole> Actor for MyActor<TMsg, TRole>
where
    TMsg: RoomMessageTrait,
    TRole: ApplicationRole,
{
    // ...
}

// CORRECT: Add Unpin to all generic types
impl<TMsg, TRole> Actor for MyActor<TMsg, TRole>
where
    TMsg: RoomMessageTrait + Unpin,  // Added Unpin
    TRole: ApplicationRole + Unpin,   // Added Unpin
{
    // ...
}
```

**Common Required Bounds:**
- `Unpin` - Required by Actix for actor types
- `Send` - Required for tokio::spawn
- `Sync` - Required for Arc<T> in multi-threaded contexts
- `'static` - Required for spawned tasks

**Verification:**
```bash
cargo check
# Should now compile
```

---

### [A3] "method not found"

**Error Example:**
```
error[E0599]: no method named `start` found for struct `IntentConfigBuilder` in the current scope
  --> src/service.rs:56:10
   |
56 |     builder.start()
   |             ^^^^^ method not found
```

**Root Cause:** Calling method before prerequisites met, or method doesn't exist.

**Systematic Debug:**
```
1. OBSERVE: Which method is missing? (start)
2. CHECK: Does the type have this method? (check docs/source)
3. CHECK: Are there prerequisite methods? (with_session_manager?)
4. CHECK: Is builder consumed by previous call?
5. FIX: Call prerequisite methods first
```

**Solution:**
```rust
// WRONG: Calling start() directly
let builder = IntentConfigBuilder::new(role);
let actor = builder.start()?;  // ERROR: method not found

// CORRECT: Call with_session_manager() first
let builder = IntentConfigBuilder::new(role);
let builder = builder.with_session_manager(session_manager)?;  // Prerequisite
let actor = builder.start()?;  // Now works
```

**Common Prerequisites:**
- Builder needs `with_session_manager()` before `start()`
- Builder needs `with_config()` before `build()`
- Must call `connect()` before `send()`

**Verification:**
```bash
cargo check
# Should now compile
```

---

### [A4] "mismatched types"

**Error Example:**
```
error[E0308]: mismatched types
  --> src/service.rs:78:30
   |
78 |     let session_manager: Arc<SessionManager<AppMessage, CollectorRole>> = ...
   |                              ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^
   |                              expected `Arc<SessionManager<CStateMessage, ...>>`, found `Arc<SessionManager<AppMessage, ...>>`
```

**Root Cause:** Generic type parameters don't match.

**Systematic Debug:**
```
1. OBSERVE: What types don't match? (AppMessage vs CStateMessage)
2. UNDERSTAND: Why are they different? (wrong generic parameter)
3. LOCATE: Where is each type defined?
4. DECIDE: Which is correct for this use case?
5. FIX: Use consistent types
```

**Solution:**
```rust
// Problem: Multiple incompatible SessionManagers
type SM1 = Arc<SessionManager<AppMessage, CollectorRole>>;
type SM2 = Arc<SessionManager<CStateMessage, DatabaseRole>>;
// These are DIFFERENT types!

// Solution: Use consistent message type throughout
type SM = Arc<SessionManager<AppMessage, CollectorRole>>;
// All components use same SM type
```

**Common Type Mismatches:**
- Generic parameters differ (TMsg, TRole)
- Reference vs owned value (&T vs T)
- Option wrapping (T vs Option<T>)
- Result wrapping (T vs Result<T>)

**Verification:**
```bash
cargo check
# Should now compile
```

---

### [A5] "cannot be sent between threads safely"

**Error Example:**
```
error[E0277]: `Rc<SessionManager<...>>` cannot be sent between threads safely
  --> src/service.rs:92:13
   |
92 |       tokio::spawn(async move {
   |       ^^^^^^^^^^^^ `Rc<SessionManager<...>>` cannot be sent between threads safely
   |
   = help: within `...`, the trait `Send` is not implemented for `Rc<SessionManager<...>>`
```

**Root Cause:** Using `Rc` (single-threaded) in async context that requires `Send`.

**Systematic Debug:**
```
1. OBSERVE: Which type is not Send? (Rc<...>)
2. UNDERSTAND: Why is Send needed? (tokio::spawn requires it)
3. LOCATE: Where is Rc created?
4. FIX: Replace Rc with Arc
```

**Solution:**
```rust
// WRONG: Rc is not Send
use std::rc::Rc;
let session_manager = Rc::new(SessionManager::new());

tokio::spawn(async move {
    session_manager.connect().await;  // ERROR: Rc is not Send
});

// CORRECT: Arc is Send + Sync
use std::sync::Arc;
let session_manager = Arc::new(SessionManager::new());

tokio::spawn(async move {
    session_manager.connect().await;  // Works!
});
```

**Rule of Thumb:**
- **Rc:** Single-threaded reference counting (NOT for async)
- **Arc:** Multi-threaded reference counting (USE for async/await)

**Verification:**
```bash
cargo check
# Should now compile
```

---

### [A6] "cannot be unpinned"

**Error Example:**
```
error[E0277]: `impl Future<Output = ...>` cannot be unpinned
  --> src/actor.rs:134:9
   |
134 |         fut.await
   |         ^^^ the trait `Unpin` is not implemented for `impl Future<Output = ...>`
```

**Root Cause:** Future is not Unpin and needs to be pinned or boxed.

**Systematic Debug:**
```
1. OBSERVE: What future is not Unpin?
2. UNDERSTAND: Why is Unpin needed? (await requires Unpin or Pin)
3. DECIDE: Box the future or use Pin::new
4. FIX: Box::pin() the future
```

**Solution:**
```rust
// WRONG: Complex future might not be Unpin
async fn complex_operation() -> Result<()> {
    // ... complex async code
}

let fut = complex_operation();
fut.await;  // Might fail with "cannot be unpinned"

// CORRECT: Box::pin the future
use std::pin::Pin;

let fut = Box::pin(complex_operation());
fut.await;  // Works!
```

**When to Box::pin:**
- Complex async functions
- Futures returned from trait methods
- When building recursive async functions
- When storing futures in structs

**Verification:**
```bash
cargo check
# Should now compile
```

---

## [B] Runtime Errors

### [B1] Panics and unwrap failures

**Error Example:**
```
thread 'main' panicked at 'called `Result::unwrap()` on an `Err` value: SystemTimeError(...)', src/actor.rs:145:10
```

**Root Cause:** Using `.unwrap()` on a fallible operation without handling errors.

**Systematic Debug:**
```
1. OBSERVE: Where is the panic? (actor.rs line 145)
2. LOCATE: Find the .unwrap() call
3. UNDERSTAND: What operation is failing?
4. CHECK: Can this operation legitimately fail?
5. FIX: Replace unwrap with proper error handling
```

**Solution:**
```rust
// WRONG: Panic on error
let timestamp = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap()  // PANICS if system clock before 1970!
    .as_millis() as u64;

// CORRECT: Handle error gracefully
let timestamp = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .unwrap_or_default()  // Safe fallback to 0
    .as_millis() as u64;

// OR: Propagate error
let timestamp = SystemTime::now()
    .duration_since(UNIX_EPOCH)
    .map_err(|e| MyError::TimeError(e.to_string()))?
    .as_millis() as u64;
```

**Common unwrap Failures:**
- Time operations (system clock issues)
- File operations (permissions, missing files)
- Network operations (connection refused)
- Parsing operations (invalid input)

**Prevention:**
- Use `.unwrap_or_default()` for safe fallbacks
- Use `.unwrap_or_else(|| ...)` for computed fallbacks
- Use `?` operator to propagate errors
- Use `.expect("reason")` only for truly impossible cases

**Verification:**
```bash
# Run the code again
cargo run
# Should not panic
```

---

### [B2] "Connection refused"

**Error Example:**
```
Error: Connection refused (os error 111)
```

**Root Cause:** Trying to connect to a service that's not running or not listening on that port.

**Systematic Debug:**
```
1. OBSERVE: What address are you connecting to?
2. CHECK: Is the server actually running?
3. CHECK: Is the server listening on the correct port?
4. CHECK: Is there a firewall blocking?
5. CHECK: Are you using the correct IP address?
6. FIX: Start server or fix configuration
```

**Solution:**

**Step 1: Verify server is running**
```bash
# Check if database process running
ps aux | grep zzping-database
# Should see process if running

# Check if listening on port
netstat -tlnp | grep 8443
# Should see: tcp  0  0.0.0.0:8443  0.0.0.0:*  LISTEN  <pid>/zzping-database
```

**Step 2: If not running, start it**
```bash
./target/debug/zzping-database --config database.ron &
# Wait a few seconds for startup
```

**Step 3: Verify bind address**
```ron
// In database.ron:
DatabaseConfig(
    bind_host: "0.0.0.0",  // ✅ Accepts from anywhere
    // NOT:
    // bind_host: "localhost",  // ❌ Might only bind IPv6
    bind_port: 8443,
)
```

**Step 4: Verify collector connects to correct address**
```ron
// In collector.ron:
CollectorConfig(
    database: DatabaseConfig(
        host: "127.0.0.1",  // ✅ For local testing
        // NOT:
        // host: "localhost",  // ⚠️ Might resolve to IPv6
        port: 8443,  // ✅ Must match database bind_port
    ),
)
```

**Step 5: Test raw connection**
```bash
# Try telnet to port
telnet 127.0.0.1 8443
# Should connect (then Ctrl+] and type 'quit')

# If telnet fails, server isn't listening
# If telnet succeeds, problem is in TLS or application layer
```

**Common Causes:**
- Database not started
- Wrong port number
- Firewall blocking
- Using "localhost" which resolves to IPv6 but server only on IPv4
- Server crashed during startup

**Verification:**
```bash
# Start database
./target/debug/zzping-database --config database.ron &

# Wait for startup (check logs)
# Should see: "Database service listening on 0.0.0.0:8443"

# Try collector
./target/debug/zzping-collector --config collector.ron
# Should connect successfully
```

---

### [B3] "Address already in use"

**Error Example:**
```
Error: Address already in use (os error 98)
```

**Root Cause:** Another process is already using the port.

**Systematic Debug:**
```
1. OBSERVE: Which port is in use?
2. CHECK: Is another instance of your server running?
3. CHECK: Is another service using that port?
4. DECIDE: Kill the process or use different port
5. FIX: Kill process or change configuration
```

**Solution:**

**Step 1: Find what's using the port**
```bash
# Find process using port 8443
sudo lsof -i :8443
# OR
sudo netstat -tlnp | grep 8443

# Output shows:
# COMMAND   PID   USER   FD   TYPE  DEVICE  SIZE/OFF  NODE  NAME
# zzping-da 12345 user   3u   IPv4  123456  0t0       TCP   *:8443 (LISTEN)
```

**Step 2: Kill the process**
```bash
# If it's an old instance of your server:
kill 12345

# If it won't die:
kill -9 12345

# Verify it's gone:
sudo lsof -i :8443
# Should show nothing
```

**Step 3: Or use a different port**
```ron
// In database.ron:
DatabaseConfig(
    bind_host: "0.0.0.0",
    bind_port: 8444,  // Changed from 8443
)

// Also update collector.ron:
CollectorConfig(
    database: DatabaseConfig(
        host: "127.0.0.1",
        port: 8444,  // Must match!
    ),
)
```

**Prevention:**
- Implement graceful shutdown to properly close ports
- Use unique ports for testing (8443, 8444, 8445...)
- Clean up processes before restarting

**Verification:**
```bash
# Port should be available
sudo lsof -i :8443
# Should show nothing

# Start server
./target/debug/zzping-database --config database.ron
# Should start successfully
```

---

### [B4] Component not responding

**Symptom:** Component starts but doesn't process messages.

**Systematic Debug:**
```
1. OBSERVE: Which component is not responding?
2. CHECK: Is the actor actually started?
3. CHECK: Are message handlers registered?
4. CHECK: Is the message being sent correctly?
5. ADD: Instrumentation to trace message flow
6. FIX: Based on findings
```

**Solution:**

**Step 1: Verify actor started**
```rust
// Add logging in started() hook:
impl Actor for MyActor {
    fn started(&mut self, ctx: &mut Self::Context) {
        tracing::info!("MyActor STARTED - address: {:?}", ctx.address());
        // This MUST appear in logs if actor is actually running
    }
}
```

**Step 2: Verify message handler exists**
```rust
// Check that Handler trait is implemented:
impl Handler<MyMessage> for MyActor {
    type Result = ();

    fn handle(&mut self, msg: MyMessage, _ctx: &mut Context<Self>) -> Self::Result {
        tracing::info!("RECEIVED MyMessage: {:?}", msg);  // Add this!
        // ... handle message
    }
}
```

**Step 3: Verify message is being sent**
```rust
// Add logging around send:
tracing::info!("SENDING MyMessage to actor");
let result = actor.send(MyMessage { ... }).await;
tracing::info!("Send result: {:?}", result);

match result {
    Ok(_) => tracing::info!("Message sent successfully"),
    Err(e) => tracing::error!("Failed to send message: {:?}", e),
}
```

**Step 4: Check for mailbox issues**
```rust
// Mailbox might be full (default 16 messages)
// Increase if needed:
impl Actor for MyActor {
    fn started(&mut self, ctx: &mut Self::Context) {
        ctx.set_mailbox_capacity(1000);  // Increase from default 16
    }
}
```

**Common Causes:**
- Actor not started (forgot to call `.start()`)
- Handler not implemented for message type
- Message type mismatch (sending WrongMessage to actor expecting RightMessage)
- Mailbox full (too many messages queued)
- Actor panicked during handler (stops processing)

**Verification:**
```bash
# Run with debug logging
RUST_LOG=debug cargo run

# Look for:
# - "MyActor STARTED" - confirms actor started
# - "SENDING MyMessage" - confirms send attempted
# - "RECEIVED MyMessage" - confirms handler called
#
# If you see START and SENDING but NOT RECEIVED:
#   -> Handler not registered or wrong message type
# If you see none of them:
#   -> Actor not started
```

---

## [C] TLS/mTLS Errors

**Note:** For detailed TLS troubleshooting, see `TLS_DEBUGGING_GUIDE.md`

### [C1] "TLS handshake failed"

**Error Example:**
```
Error: TLS handshake failed: received fatal alert: BadCertificate
```

**Quick Triage:**
```bash
# 1. Verify certificates exist
ls -la test_certs/
# Should see: ca.pem, database.pem, database.key, collector.pem, collector.key

# 2. Verify certificates are valid
openssl x509 -in test_certs/database.pem -noout -dates
# Check notBefore and notAfter

# 3. Verify certificate chain
openssl verify -CAfile test_certs/ca.pem test_certs/database.pem
# Expected: test_certs/database.pem: OK
```

**See TLS_DEBUGGING_GUIDE.md** for complete troubleshooting.

---

### [C2] "Bad certificate"

**Root Cause:** Certificate verification failed.

**Quick Fixes:**
```bash
# Regenerate certificates
./generate_certs.sh

# Verify new certificates
openssl verify -CAfile test_certs/ca.pem test_certs/database.pem
openssl verify -CAfile test_certs/ca.pem test_certs/collector.pem
# Both should say: OK
```

---

### [C3] "Unknown CA"

**Root Cause:** CA certificate mismatch between client and server.

**Solution:**
```bash
# Both database and collector MUST use same ca.pem
# Check configuration:

# database.ron:
# tls: TlsConfig(
#     ca_cert_path: "test_certs/ca.pem",  # ← Must match
# )

# collector.ron:
# tls: TlsConfig(
#     ca_cert_path: "test_certs/ca.pem",  # ← Must match
# )
```

---

## [D] Component Integration Errors

### [D1] "No rooms in common"

**Error Example:**
```
Connection established but no data flows
Logs show: "No rooms in common with peer"
```

**Root Cause:** Room names don't match between client and server.

**Systematic Debug:**
```
1. CHECK: What rooms does database register?
2. CHECK: What rooms does collector register?
3. COMPARE: Do they match EXACTLY?
4. FIX: Make room names consistent
```

**Solution:**

**Step 1: Check room registration**
```rust
// In database service:
session_manager.register_room_handler(
    RoomId::from("intent-config"),  // ← Note exact string
    intent_handler
);

// In collector service:
session_manager.register_room_handler(
    RoomId::from("intent-config"),  // ← Must match EXACTLY
    intent_handler
);
```

**Step 2: Common room name mistakes**
```rust
// WRONG: Different strings
RoomId::from("intentconfig")   // No hyphen
RoomId::from("intent_config")  // Underscore instead of hyphen
RoomId::from("IntentConfig")   // Wrong case

// CORRECT: Exact match
RoomId::from("intent-config")  // ✅
```

**Step 3: Add debug logging**
```rust
// In connection setup:
tracing::info!("Database registered rooms: {:?}", session_manager.rooms());
tracing::info!("Collector hello rooms: {:?}", hello_msg.supported_rooms);
tracing::info!("Room intersection: {:?}", intersection);
```

**Verification:**
```bash
# Run database with debug logging
RUST_LOG=debug ./target/debug/zzping-database --config database.ron &

# Run collector with debug logging
RUST_LOG=debug ./target/debug/zzping-collector --config collector.ron

# Look for:
# "Database registered rooms: [intent-config, mem-db, c-state]"
# "Collector hello rooms: [intent-config, mem-db, c-state]"
# "Room intersection: [intent-config, mem-db, c-state]"  ← Should match!
```

---

## [E] Test Failures

### [E1] Tests hanging indefinitely

**Symptom:** Test runs forever, never completes.

**Common Causes:**
1. Waiting on channel that never receives
2. Actor waiting for message that never comes
3. Deadlock between components
4. Missing timeout in async operation

**Solution:**

**Add timeouts to all async operations:**
```rust
use tokio::time::{timeout, Duration};

// WRONG: No timeout - might hang forever
let result = actor.send(message).await?;

// CORRECT: With timeout
let result = timeout(
    Duration::from_secs(5),
    actor.send(message)
).await
.map_err(|_| TestError::Timeout("Actor didn't respond"))?
.map_err(|e| TestError::ActorError(e))?;
```

**Use tokio::time::pause() in tests:**
```rust
#[tokio::test]
async fn test_with_mock_time() {
    use tokio::time::{pause, advance, Duration};

    pause();  // Mock time - tests run instantly

    // Setup...

    // Advance mock time
    advance(Duration::from_secs(60)).await;

    // Check results
}
```

---

### [E2] Flaky tests (pass sometimes, fail others)

**Common Causes:**
1. Race conditions
2. Depending on real time
3. Depending on execution order
4. Shared mutable state

**Solution:**

**Use mock time instead of real time:**
```rust
// WRONG: Depends on real time
tokio::time::sleep(Duration::from_millis(100)).await;
assert!(something_happened);  // Flaky!

// CORRECT: Use mock time
use tokio::time::{pause, advance, Duration};

pause();  // Enable mock time
advance(Duration::from_millis(100)).await;
assert!(something_happened);  // Deterministic!
```

**Use barriers for synchronization:**
```rust
use tokio::sync::Barrier;
use std::sync::Arc;

let barrier = Arc::new(Barrier::new(2));
let b1 = barrier.clone();
let b2 = barrier.clone();

// Task 1
tokio::spawn(async move {
    // Do work
    b1.wait().await;  // Wait for both tasks
});

// Task 2
tokio::spawn(async move {
    // Do work
    b2.wait().await;  // Wait for both tasks
});
```

---

## [F] Actor/Actix Errors

### [F1] "Mailbox closed"

**Error Example:**
```
Error: MailboxError(Closed)
```

**Root Cause:** Trying to send message to actor that has stopped.

**Solution:**
```rust
// Check if actor is still running before sending:
match actor.send(message).await {
    Ok(response) => { /* handle response */ },
    Err(MailboxError::Closed) => {
        tracing::warn!("Actor has stopped, cannot send message");
        // Handle gracefully - don't panic
    },
    Err(e) => return Err(e.into()),
}
```

---

## Quick Reference: First Steps for Any Error

```
1. Read the FULL error message - every word matters
2. Note the file and line number
3. Look at the code at that location
4. Check if error is in this guide
5. Follow the systematic debug process
6. If stuck >30 min, ask for help
```

## When to Ask for Help

Ask immediately if:
- Stuck on same error >30 minutes
- Error message is completely unclear
- Solution attempts make it worse
- Multiple errors cascading

**What to include when asking:**
- Exact error message (copy/paste, don't paraphrase)
- File and line number
- What you were trying to do
- What you've already tried
- Minimal code example that reproduces issue

---

**Remember:** Every error is a learning opportunity. Document what you find!
