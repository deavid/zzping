
# Architectural Review Notes: `src/components/zzchorale-examples/pingpong/src/pong.rs`

This document records the findings from the architectural review of the `pong` example component.

### Summary of Findings

This file is a useful case study. It deceptively appears simpler than `ping.rs`, but this simplicity is an artifact of it being the passive "responder" in a request-response pattern. This confirms that the request-response pattern itself creates an undesirable asymmetry in component complexity and should be forbidden by the framework.

### List of Deviations and Notes

1.  **Analysis: Deceptive Simplicity**
    *   **Finding:** The `run` loop in `PongActor` is simple because it does not need to `await` a response. It receives a command that includes a `oneshot::Sender` and simply uses it to send a reply.
    *   **Impact:** This demonstrates how the request-response pattern pushes all the complex, stateful, blocking logic onto the "initiator" (`ping`), while the "responder" (`pong`) looks simple. This is not a desirable property for a uniform framework.
    *   **Principle Violated:** The goal of a uniform and predictable component architecture. The complexity of a component's logic should depend on its business requirements, not its incidental role in a communication pattern.

2.  **Primary Deviation (Safety/Clarity): Brittle Builder Implementation**
    *   **Finding:** The `PongBuilder::start` method takes `mut self` but uses `self.command_rx.take().expect(...)` to simulate being consumable. 
    *   **Impact:** This is a runtime check that will panic, rather than a compile-time guarantee. It's an implementation smell to work around an incorrect method signature.
    *   **Principle Violated:** "Make invalid states impossible."
    *   **Recommendation:** The `start` method must take `self` to consume the builder, making it impossible to call more than once at compile time. This removes the need for the `Option` and the `.expect()`.

3.  **Framework Gap: Non-Standard API**
    *   **Finding:** The builder exposes a custom `get_command_sender` method for wiring.
    *   **Impact:** This is a component-specific method, which violates the goal of having a uniform wiring mechanism.
    *   **Principle Violated:** Uniformity.
    *   **Recommendation:** This method should be removed. The builder should instead implement the standard framework trait `InputProvider<PongCommand>`.

4.  **Design Smell: Type Alias as Command**
    *   **Finding:** `PongCommand` is a raw type alias for a tuple: `(String, oneshot::Sender<String>)`.
    *   **Impact:** This is unclear and inextensible. It hides the meaning of the fields.
    *   **Recommendation:** All component commands should be proper structs or enums with named fields (e.g., `struct PongCommand { message: String, response_channel: oneshot::Sender<String> }`).
