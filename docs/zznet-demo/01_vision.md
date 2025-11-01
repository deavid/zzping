I want you to create a plan to add a big integration test in zznet-builder that exercises the whole zznet-* suite as if it was 2 apps communicating.

We first need to drive the requirements. The vision for this is:

Instead of running/spawning 2 processes, we create an integration test with example components that runs two stacks in two threads and two tokio stacks, communicates via the mock transport that we already have in the code, and demonstrates 2 components that exist on 2 stacks interchanging messages, both Component A -> Component B, and Component A <-> Component A in the two emulated processes.

Let's discuss. Take a look at the codebase to understand better what I want. Your task now is to understand what I do really want.

----

Excellent. This is the perfect next step. A comprehensive integration test will serve as the "capstone" validation for the entire `zznet-*` suite, proving that the architectural vision works in practice.

I have investigated the codebase and your request. Your vision is clear, and it's an excellent goal. Before I create a detailed plan, I need to clarify one critical architectural point and one practical point to ensure the plan I create perfectly matches your intent.

### Understanding Your Vision

First, let me confirm my understanding of the test you want to build.

1.  **Two Simulated Applications:** We will instantiate two complete, independent `zznet` stacks within a single test function. Each stack will have its own `PeerManagerActor`, `RouterActor`, `ConnectionManager`, and its own set of component instances. This perfectly simulates two separate processes.

2.  **Mock Transport:** The two stacks will be connected using an in-memory `MockConnection` pair from `zznet-api/src/mock.rs`. This bypasses all TCP/TLS complexity and makes the test fast and deterministic, while still exercising the entire protocol stack from `zznet-hello` upwards.

3.  **Example Components:** We will create two minimal, example components for this test (let's call them `ComponentA` and `ComponentB`). They will follow the established "Three-Actor Pattern" (`MainActor`, `NetworkManager`, `NetworkActor`) and implement the `RoomManager` trait to register themselves with their respective `RouterActor`.

4.  **Communication Scenarios:**
    *   **`A <-> A`:** `ComponentA` on Stack 1 will send a message that is successfully received by `ComponentA` on Stack 2. We'll also test the reverse.
    *   **`A -> B`:** `ComponentA` on Stack 1 will send a message that ultimately results in a state change or action in `ComponentB` on Stack 2.

This setup will stress-test almost every part of the framework: builder wiring, peer lifecycle, room negotiation, serialization, routing, and component integration.

### Critical Architectural Question: `Component A -> Component B` Routing

Your request to test `ComponentA` sending a message to `ComponentB` raises an important architectural question that we must resolve.

Our "Ground Truth" architecture, established in the SOLID refactor, enforces a strict **1-to-1 mapping between a Room and a Component**.
- `ComponentA` uses the `"room-a"` room.
- `ComponentB` uses the `"room-b"` room.

When `ComponentA` sends a message into `"room-a"`, the `Router` on the receiving side will **always** deliver it to the registered handler for `"room-a"`, which is the `NetworkManager` for `ComponentA`. There is currently no mechanism for `ComponentA` to directly address `ComponentB` on another peer.

How do you envision a message from `ComponentA` on Stack 1 being routed to `ComponentB` on Stack 2? Here are a few possibilities:

**Option 1: The "Message Bus" Anti-Pattern (Deviation from Vision)**
We could modify the `Router` to allow senders to specify a destination component. This would turn the `Router` into a more complex message bus, which contradicts our vision of the `Router` as a simple, dumb forwarder. I do not recommend this.

**Option 2: The "Local Forwarding" Pattern (Aligned with Vision)**
This seems to be the most architecturally sound approach:
1.  `ComponentA` on Stack 1 sends a message to `"room-a"`.
2.  `ComponentA` on Stack 2 receives this message via its `NetworkManager`.
3.  The `MainActor` of `ComponentA` on Stack 2 then makes a **local, in-process** call (e.g., via an `Addr`) to the `MainActor` of `ComponentB` on Stack 2 to forward the data or trigger an action.

In this model, the network layer remains pure (A talks to A), and the cross-component logic is handled explicitly within the application's business logic.

**Option 3: The "Shared Room" Pattern (Violates 1:1 Rule)**
We could allow both `ComponentA` and `ComponentB` to register a handler for the same room (e.g., a `"shared-bus"` room). When a message arrives, the `Router` would deliver it to both. This would violate our strict 1:1 mapping and could lead to ambiguity.

**My recommendation is strongly for Option 2.** It preserves our architectural principles. Does this "Local Forwarding" pattern match your vision for how `A -> B` communication should work?

### Practical Question: Test Location

You requested the test be added to `zznet-builder`. This is a good instinct, as the builder is the top-level integration point.

However, placing this test within `zznet-builder` would require `zznet-builder` to have `dev-dependencies` on every component it needs to test (`zzintent-config`, `zzmem-db`, etc.). This can create circular dependencies or make the builder crate very heavy.

A common pattern is to place such "capstone" integration tests in one of the final application crates, since they already depend on all the necessary components. For example, this test could live in `src/apps/zzping-database/tests/`.

This is just a practical suggestion to keep the crate dependencies clean. Are you firm on the test living in `zznet-builder`, or would you be open to placing it in an application's `tests/` directory?

---

Once we clarify these two points (the `A->B` routing mechanism and the test's location), I will be ready to create a detailed, phased implementation plan to build this crucial integration test.

-------------


> This setup will stress-test almost every part of the framework: builder wiring, peer lifecycle, room negotiation, serialization, routing, and component integration.

No room here for almost. It has to test every part of the framework, leaving no gap.

Also, we're not testing here performance at all. I don't care.

The objective: I can later run this integration test alone and gather coverage data from just running it alone without running any other test - anything that does not run is either code that's not needed or something that's missing wiring properly. We will aim for 100% code coverage in zznet in this approach.

> How do you envision a message from ComponentA on Stack 1 being routed to ComponentB on Stack 2? Here are a few possibilities:

ComponentA must not be able to send to ComponentB across stacks.

The way this works is: ComponentA Stack1 sends to ComponentA Stack2. ComponentA Stack2 publishes the new state. Component B in Stack 2 is subscribed to ComponentA and receives the change.

> This seems to be the most architecturally sound approach:
> 1.  `ComponentA` on Stack 1 sends a message to `"room-a"`.
> 2.  `ComponentA` on Stack 2 receives this message via its `NetworkManager`.
> 3.  The `MainActor` of `ComponentA` on Stack 2 then makes a **local, in-process** call (e.g., via an `Addr`) to the `MainActor` of `ComponentB` on Stack 2 to forward the data or trigger an action.

This seems to be the same as I described. Let me know if it's not.

> A common pattern is to place such "capstone" integration tests in one of the final application crates, since they already depend on all the necessary components. For example, this test could live in `src/apps/zzping-database/tests/`.

We could build this in a new crate src/apps/zznet-demo - entirely for this.

But I choose zznet-builder because it must be on top of all the stack, it's meant to build all other things.

but a zznet-demo is okay too.

Let me know if everything is 100% crystal clear.

-----------

Everything is 100% crystal clear.

Your clarifications are perfect. We are completely aligned on the vision.

-   **Exhaustive Testing:** The goal is not a simple smoke test, but a comprehensive validation suite that exercises every feature and error path of the `zznet-*` framework. The target of 100% coverage is ambitious but sets the right standard.
-   **A->B Communication:** Your description of local, in-process pub/sub (`ComponentA` on Stack 2 notifies `ComponentB` on Stack 2) is exactly what I meant by the "Local Forwarding" pattern. This is the architecturally correct approach, and we will implement it.
-   **Test Location:** Your reasoning is sound. Creating a dedicated `src/apps/zznet-demo` crate is the cleanest solution. It will be a top-level consumer of the entire `zznet` stack, just like `zzping-collector`, but will exist purely for this comprehensive integration test and as a canonical example.

With these points clarified, I can now provide the definitive, multi-phase plan to create this "capstone" integration test.

---
