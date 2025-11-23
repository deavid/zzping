# ZZPing Commenting Policy & Style Guide

**Philosophy:** Code is the primary source of truth. Comments are for _intent_, _constraints_, and _context_ that cannot
be expressed in the type system. If the code is clear, no comment is better than a redundant one.

## 1. The "One-Liner" Standard

**Rule:** 90% of functions and structs should be documented with a **single, concise sentence**.

- **Goal:** Explain the _responsibility_ of the item, not its mechanics.
- **Voice:** Imperative, active voice. "Handles..." not "This function handles...".

**❌ Bad (Verbose/Redundant):**

```rust
/// This is the actor that handles the intent configuration.
/// It receives messages from the network and updates the local state.
/// It also allows local components to subscribe to changes.
pub struct IntentConfigActor { ... }
```

**✅ Good (Concise):**

```rust
/// Manages configuration state and broadcasts updates to local subscribers.
pub struct IntentConfigActor { ... }
```

## 2. No Type Signature Echoing

**Rule:** Do **NOT** describe arguments, return types, or error variants in the docstring unless there is a hidden
invariant (e.g., "must be sorted"). The function signature tells us the types; the comment tells us the _why_.

**❌ Bad:**

```rust
/// Connects to the server.
///
/// Arguments:
/// * `addr` - The address to connect to.
/// * `retries` - How many times to retry.
///
/// Returns:
/// * `Ok(())` on success
/// * `Err(NetworkError)` on failure
pub async fn connect(addr: &str, retries: u32) -> Result<(), NetworkError>
```

**✅ Good:**

```rust
/// Establishes a connection, retrying with exponential backoff on failure.
pub async fn connect(addr: &str, retries: u32) -> Result<(), NetworkError>
```

## 3. "Why", Not "What"

**Rule:** Inline comments (`//`) must explain **non-obvious intent** or **business constraints**. Never explain what the
Rust syntax is doing.

**❌ Bad (The Narrator):**

```rust
// Create a new hash map
let mut map = HashMap::new();
// Insert the item
map.insert(key, value);
// Check if the map is full
if map.len() > 100 { ... }
```

**✅ Good (The Architect):**

```rust
// Enforce hard cap to prevent memory exhaustion during outages.
if map.len() > 100 { ... }
```

## 4. Historical & Process Artifacts

**Rule:** The code lives in the present. Remove all references to:

- Development phases ("Phase 1", "Step 3").
- Refactoring history ("Moved from SessionManager...").
- Author names or AI attribution ("Fixed by Agent X").
- Dead code commented out.

**Exception:** `TODO` or `FIXME` comments are allowed but must describe a specific technical debt, not a process step.

**❌ Bad:**

```rust
// Phase 9: Buffer if room_actor not yet set
if self.room_actor.is_none() { ... }
```

**✅ Good:**

```rust
// Buffer messages until the RoomActor wiring completes.
if self.room_actor.is_none() { ... }
```

## 5. Implementation Details in Public Docs

**Rule:** Public docstrings (`///`) on structs/enums should describe the **abstraction**, not the internal
implementation fields, unless those fields are public.

**❌ Bad:**

```rust
/// A component that uses a DashMap to store ping results and flushes them every 60 seconds.
pub struct MemDB { ... }
```

**✅ Good:**

```rust
/// In-memory storage for ping results with periodic flushing.
pub struct MemDB { ... }
```

---

### Compression Strategy for Agents

When asking an Agent to refactor comments, give them this directive:

```text
Refactor the comments in this file to adhere to `AGENT_COMMENTING_POLICY.md`. Compress verbose explanations into
single summary lines. Remove parameter lists. Remove 'Phase' references. Delete comments that merely narrate the Rust
syntax.
```
