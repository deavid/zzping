
# Architectural Review Notes: `src/components/zzchorale-examples/pingpong/src/ping.rs`

This document records the findings from the architectural review of the `ping` example component. This component serves as a valuable example of the anti-patterns the framework should prevent.

### Summary of Findings

This component's implementation violates the newly-established core principles of the framework, particularly the "pure Actor model" (no blocking request-response) and the need for testable logic. It is a perfect example of what the refactored `zzchorale` framework should make difficult or impossible to write.

### List of Deviations and Notes

1.  **Primary Deviation (Principle): Use of Request-Response Pattern**
    *   **Finding:** The `PingActor` sends a command with a `oneshot::Sender` and then blocks its own logic waiting for the `oneshot::Receiver` to return a value.
    *   **Impact:** This is a blocking RPC-style call that violates the "pure Actor model" (fire-and-forget) philosophy. It can lead to deadlocks and makes system-wide reasoning difficult.
    *   **Principle Violated:** Inter-component communication must be purely asynchronous and non-blocking.
    *   **Recommendation:** This pattern must be disallowed. The framework should not provide affordances for it. If a "response" is needed, it must be sent as a separate, asynchronous message back to the original component.

2.  **Primary Deviation (Testability): Monolithic `run` Method**
    *   **Finding:** The component's entire logic is contained within a single `run` method with an infinite `loop` and a `tokio::select!`.
    *   **Impact:** It is extremely difficult to unit test the business logic for handling a `PingCommand` without mocking the entire `ActorContext` and `tokio` runtime.
    *   **Principle Violated:** The framework should promote testable code by default.
    *   **Recommendation:** The `ComponentActor` trait should be refactored to separate one-time setup from message handling (e.g., `setup()` and `handle_message(msg)` methods). The framework should own the loop.

3.  **Framework Gap: Boilerplate Logic**
    *   **Finding:** The component's `run` method contains boilerplate for handling the shutdown signal. The `start` method contains boilerplate for creating the component's own command channel.
    *   **Impact:** This is repeated, error-prone code that every component author must write.
    *   **Recommendation:** The framework should manage this automatically. The `select!` loop and shutdown handling should be internal to the framework. The framework should also create the command channel for the component.

4.  **Naming Convention: Confusing `Actor` type**
    *   **Finding:** A developer trying to create a "Ping Component" must implement a trait named `Actor`.
    *   **Recommendation:** Rename the `Actor` trait to `ComponentActor` to make the relationship between the user's conceptual "Component" and the framework's "Actor" primitive clear.

5.  **API Smell: Encapsulation-Breaking `PingApi` Trait**
    *   **Finding:** The `PingApi` trait is implemented on the generic `ComponentHandle`, exposing `command_tx` and allowing external code to inject commands at any time.
    *   **Impact:** This is a symptom of the disallowed request-response pattern. It creates a "booby trap" where a synchronous-looking API call (`handle.ping()`) interferes with the actor's asynchronous state machine.
    *   **Recommendation:** With the removal of the request-response pattern, this trait and its methods should be removed entirely.

6.  **Documentation Note: Missing Docstrings**
    *   **Finding:** The `PingApi` trait and its methods lack docstrings.
    *   **Recommendation:** All public traits and methods must be documented.
