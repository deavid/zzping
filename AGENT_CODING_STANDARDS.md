# Agent Coding Standards and Project Conventions

This document contains the global rules and conventions for all coding tasks in this project. These standards must be followed for any implementation task to ensure the codebase remains clean, consistent, and maintainable.

## 1. Introduction

This document contains the global rules and conventions for all coding tasks in this project. These standards must be followed for any implementation task to ensure the codebase remains clean, consistent, and maintainable.

## 2. File Structure and Organization

*   **Keep Module Entry Points Clean:** `lib.rs` and `mod.rs` files should be kept as empty as possible. Their only role is to declare the modules within the directory using `pub mod <module_name>;`.
*   **Group by Feature:** Group related functionality into dedicated modules inside folders. For example, the client and server runtime managers belong in a `runtime/` directory.
*   **One Component, One File:** Each major struct or component should reside in its own file. For example, `ClientRuntime` lives in `runtime/client.rs`.
*   **Shared Utilities:** Truly generic, project-wide utilities (like shared traits) should be placed in a dedicated file at the `src` level, such as `src/traits.rs`.

## 3. Code Style

*   **Prefer Structs with Methods:** Business logic should be encapsulated within methods on a struct, rather than as free-standing functions, where appropriate.
*   **Keep It Simple (KISS):** Write small, simple functions that have a single, clear responsibility. A function that configures TLS, makes a connection, *and* handles messages is doing too much. Break it down.

## 4. Documentation and Comments (Crucial)

This is the most important section. The goal of documentation is to explain the **concept, contract, and purpose (the "why")**, not to describe what the code is literally doing (the "what").

### Docstrings (`///`)

*   All public items (`structs`, `enums`, `functions`, `traits`, and public struct fields) **MUST** have a docstring.
*   **DO NOT** describe the parameters or return values in a list format. The function signature already contains this information. Instead, explain what the function *achieves* and what its *contract* is with the caller.

### Inline Comments (`//`)

*   Use inline comments sparingly. The code should be as self-documenting as possible.
*   Add comments only to explain non-obvious logic, performance-critical sections, workarounds for bugs, or the reasoning behind a complex architectural choice.

## 5. Code Design and Architecture

### Single Responsibility Principle (SRP)

*   **Rule:** A function or a struct should have one, and only one, reason to change.
*   **Guidance:** If a function loads configuration, establishes a connection, *and* handles messages, it's doing too much and must be broken down.

### Fail-Fast with Guard Clauses

*   **Rule:** Handle error conditions and simple cases at the very beginning of a function. This avoids nesting the main logic.
*   **Guidance:** This pattern makes the "happy path" clearer by reducing indentation.

### Avoid Deep Nesting ("Arrow Code")

*   **Rule:** Aim for a maximum of 2-3 levels of indentation within a single function.
*   **Guidance:** Deeply nested code is a clear sign that a function is doing too much. Immediately look for opportunities to extract logic into a private helper function.

### Manage Task and Object Lifecycles

*   **Rule:** The lifetime of spawned tasks must be explicitly managed and tied to the lifetime of the object or component that creates them.
*   **Guidance:** Avoid "fire-and-forget" `tokio::spawn` calls for core components. Use ownership (`run(self)`) to make lifecycles explicit and prevent "zombie" tasks.

### Ensure Configurability for Testability

*   **Rule:** Avoid hardcoding values that directly affect behavior, especially time, counts, or buffer sizes.
*   **Guidance:** These values should be passed in via a configuration struct. This allows tests to use small, fast values (e.g., `Duration::from_micros(1)`) while production can use larger, more sensible values. A component should not dictate its own timing; it should be configured.

## 6. Testing Standards

### Test Location and Organization

*   **Rule:** ALL tests must be in `src/` files using `#[cfg(test)] mod tests { ... }`. Integration tests in `tests/` are NOT allowed.
*   **Rationale:** Keeps tests close to implementation, easier to maintain, ensures private APIs are tested.

### Test Coverage Requirements

*   **Rule:** EVERY public function, method, and struct must have at least one test. EVERY error path must be tested.
*   **Guidance:** If you implement a method, you MUST write tests for it. No exceptions.
*   **Coverage target:** Aim for 100% line coverage. If coverage is below 90%, add more tests.

### What to Test

*   **Rule:** Test the contract, not the implementation.
*   **Examples:**
    *   ✅ Test that calling `connect()` changes state to `Connected`
    *   ✅ Test that sending while disconnected returns an error
    *   ✅ Test that `Drop` properly cleans up resources
    *   ❌ Don't test internal private helper functions unless they have complex logic

### Test Organization

*   **Rule:** Group related tests together. Use descriptive test names that explain what behavior is being tested.
*   **Pattern:** `test_<function_name>_<scenario>_<expected_result>`
*   **Examples:**
    *   `test_send_to_room_when_connected_succeeds`
    *   `test_send_to_room_when_disconnected_returns_error`
    *   `test_add_peer_already_exists_returns_error`

## 7. Module Structure and Exports

### lib.rs Must Be Minimal

*   **Rule:** `lib.rs` should ONLY contain module declarations (`pub mod <name>;`) and crate-level documentation. NO re-exports with `pub use`.
*   **Rationale:** Re-exports hide the actual module structure and make it harder to understand where types are defined. Users should import from the actual module: `use crate::types::PeerId` not `use crate::PeerId`.
*   **Exception:** If a type is genuinely moved and you want to provide a deprecation path, re-exports are acceptable with a deprecation warning.

### Import Paths

*   **Rule:** Public APIs should be imported from their defining module, not from `lib.rs` re-exports.
*   **Example:**
    *   ✅ `use zznet_session::types::{PeerId, RoomId};`
    *   ✅ `use zznet_session::session_manager::SessionManager;`
    *   ❌ `use zznet_session::{PeerId, RoomId, SessionManager};`

## 8. Common Anti-Patterns to Avoid

### Dead Code

*   **Rule:** Remove or use all declared fields and methods. If a field is declared but never read, either use it or remove it.
*   **Guidance:** Compiler warnings about dead code are RED FLAGS. Fix them immediately.

### Over-Documentation

*   **Rule:** Don't document what the code obviously does. Document WHY and the CONTRACT.
*   **Examples:**
    *   ❌ `/// Returns true if connected` (obvious from signature)
    *   ✅ `/// Checks connection state. Returns true only when channels are active and messages can be sent.`

### Integration Tests in tests/

*   **Rule:** Do NOT create integration tests in `tests/` directory.
*   **Rationale:** Integration tests should also be in `src/` using `#[cfg(test)]`. This keeps all test code together with the implementation.

### Long-Lived Loop Bodies Not Extracted

*   **Rule:** Long-lived loops (event loops, message routers, etc.) MUST have their per-iteration logic extracted into a separate method or function.
*   **Rationale:**
    *   Makes the loop body independently testable
    *   Separates "loop infrastructure" from "iteration logic"
    *   Enables unit testing without spawning actual tasks
    *   Improves code clarity and maintainability
*   **Pattern:**
    ```rust
    // ❌ BAD: Loop body inline, not testable
    tokio::spawn(async move {
        while let Some((room_id, msg)) = rx.recv().await {
            if let Some(tx) = room_txs.get(&room_id) {
                if tx.send(msg).await.is_err() {
                    tracing::warn!("Failed to route");
                }
            } else {
                tracing::warn!("Unknown room");
            }
        }
    });

    // ✅ GOOD: Loop body extracted, testable
    tokio::spawn(async move {
        while let Some((room_id, msg)) = rx.recv().await {
            Self::route_inbound_message(&room_txs, &peer_id, room_id, msg).await;
        }
    });

    // With corresponding method:
    async fn route_inbound_message(
        room_txs: &HashMap<RoomId, mpsc::Sender<Msg>>,
        peer_id: &PeerId,
        room_id: RoomId,
        msg: Msg,
    ) {
        // Logic here, now testable!
    }
    ```
*   **Testing Requirement:** The extracted method MUST have comprehensive tests covering:
    *   Success case (message routed correctly)
    *   Error cases (unknown room, closed channel, etc.)
    *   Edge cases specific to the logic
