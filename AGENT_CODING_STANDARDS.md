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
