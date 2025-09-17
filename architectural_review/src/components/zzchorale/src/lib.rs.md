
# Architectural Review Notes: `src/components/zzchorale/src/lib.rs`

This document records the findings from the architectural review of the `zzchorale` framework core.

### Summary of Findings

The fundamental issue with this file is that it provides the *pieces* of a safe framework but fails to provide an API that *enforces* a safe workflow. It prioritizes flexibility over correctness, which violates the core design principle of "safety and correctness first."

### List of Deviations and Notes

1.  **Primary Deviation (Safety): API Enables Incorrect Usage**
    *   **Finding:** The API is a set of free-floating functions (`spawn_actor`, `create_channel`). This design fails to guide the developer through the correct `Wire -> Start` sequence.
    *   **Impact:** It allows developers to start unwired or "halfway" components, leading to preventable runtime errors.
    *   **Principle Violated:** "Make invalid states impossible."
    *   **Recommendation:** Replace the free-function API with a guided structure, such as the "Typestate Builder" pattern, to make starting an unwired component a compile-time error.

2.  **Primary Deviation (Safety): Shutdown Lacks Guarantees**
    *   **Finding:** The `shutdown()` method on the `ComponentHandle` waits forever on the actor's task handle.
    *   **Impact:** A misbehaving component that never terminates its `run` loop will hang the entire service shutdown process.
    *   **Principle Violated:** "Safety and correctness first."
    *   **Recommendation:** The `shutdown()` method must incorporate a timeout. If the actor fails to terminate within the timeout, the framework should log a critical error and forcefully abort the task.

3.  **Design Smell (Clarity/Safety): Ambiguous Lifecycle Ownership**
    *   **Finding:** The `ComponentHandle` uses `Arc<Mutex<...>>` to make `shutdown(&self)` cloneable.
    *   **Impact:** This creates ambiguity about which part of the system is responsible for a component's lifecycle. Any handle holder can initiate a shutdown.
    *   **Principle Violated:** The goal of having a system that is easy to reason about.
    *   **Recommendation:** Redesign to a single-owner lifecycle model. `shutdown()` should take `self` to enforce that only one "lifecycle handle" can terminate the component. Cloneable "command handles" could be provided for sending messages.

4.  **Convention Violation: `lib.rs` Contains Code**
    *   **Finding:** The `lib.rs` file contains the full implementation.
    *   **Impact:** Violates standard Rust project structure, making the crate harder to navigate.
    *   **Recommendation:** Move all implementation code to a sub-module (e.g., `framework.rs`) and use `lib.rs` only for module declaration and public exports.

5.  **Improvement Note: Underdeveloped `readiness_future`**
    *   **Finding:** The readiness signal is trivial and only indicates that the actor's `run` loop has begun.
    *   **Impact:** It doesn't allow for components that have a fallible or lengthy initialization phase.
    *   **Recommendation:** Enhance the readiness mechanism to allow the actor to perform its own setup and then signal either success or failure back to the composer.

6.  **Design Note: Needless `create_channel` Abstraction**
    *   **Finding:** The `create_channel` function is a simple wrapper around `mpsc::channel(32)` that provides no additional value.
    *   **Recommendation:** Remove this function. Developers can use the `tokio` primitive directly, or the function can be enhanced to provide a meaningful, value-add abstraction.

7.  **Documentation Note: Underspecified `Actor` Trait**
    *   **Finding:** The docstring for the `Actor` trait does not explain its conceptual role within the framework.
    *   **Recommendation:** The documentation must explain the relationship between a "Component" (the user-facing concept) and an "Actor" (the framework's internal implementation primitive).
