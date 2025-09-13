# Design and Process Log

This document captures the key architectural decisions and workflow processes established during our interactive sessions.

## Our Workflow

1.  **Roles:** My role is to act as a high-level architect. I will analyze requirements, propose designs, and break down the work into small, self-contained tasks. The user (David) will use these tasks to guide a separate implementation agent.

2.  **Task Format:** All tasks will be delivered as Markdown files. These tasks will focus on the "what" and "why," not the specific "how." They will define goals, file structures, and the conceptual contracts of components, but will not contain prescriptive code snippets.

3.  **Coding Standards:** All work must adhere to the principles laid out in `AGENT_CODING_STANDARDS.md`. This document is the source of truth for file structure, code style, documentation standards, and architectural patterns.

4.  **File Modification Process:** When creating or modifying design documents like this one, I will state my intent and then write the file directly. The user will then review the file itself, rather than approving the content in the chat. This streamlines the review process.

5.  **Task Self-Containment:** The implementation agent's context is cleared between tasks. Therefore, every task created must be self-describing. If a task builds upon the work of a previous one, it must provide the necessary context and explain the dependencies. The agent will not remember previous conversations or file states.

## Architectural Decisions

### 1. `zznet` Architecture: A Symmetrical, Layered Model

We have defined a layered architecture for `zznet` that prioritizes symmetry and a reactive design.

*   **Layer 1: `connection_manager` (`Connection`)**
    *   **Responsibility:** To manage the data framing for a **single, active connection**.
    *   It wraps a raw I/O stream and provides a message-oriented API by handling data frames.

*   **Layer 2: `runtime` (`ClientRuntime`, `ServerRuntime`)**
    *   **Responsibility:** To act as a factory that produces a `Stream` of `Connection` objects.
    *   **`ServerRuntime`:** Produces a `Stream` of `Connection`s from incoming TCP listeners.
    *   **`ClientRuntime`:** Produces a `Stream` of `Connection`s for an outgoing connection, implicitly handling all reconnect logic.

*   **Layer 3: `zznet-lib` Facade (`ZzNet`)**
    *   **Responsibility:** To provide a simple, stable API for application components.
    *   This will be a **new, separate crate** located at `src/common/zznet-lib`.
    *   The `ZzNet` struct within this crate will be the **only** API that high-level components (e.g., `MemDB`) interact with. They will not depend on the `zznet` crate directly.
    *   This facade will own and manage the underlying `runtime` and `connection_manager` components.

### 2. The Client-Side Design: A Deliberate Choice for Symmetry

We have **explicitly chosen a Reactive Stream model** for the client, where the `ClientRuntime` provides a `Stream` of connections. The reasoning is as follows:

*   **Enforces Component Resilience:** Application components **must not have control over the connection state**. The stream model enforces this by pushing connections to them.
*   **Matches Component Needs:** Components only care about the binary state of being "connected" or "not connected." A stream that yields a `Connection` when one is available perfectly models this.
*   **Architectural Symmetry:** It provides an elegant symmetry with the server-side API.

### 3. Connection & Channel Lifecycle Guarantees

We explicitly considered the risk of a component attempting to use a "stale" channel handle after a network reconnection. The design mitigates this risk via Rust's ownership model.

*   **The Risk:** A component holds a `mpsc::Sender` for a channel from an old, now-dead `Connection`.
*   **The Mitigation:** The `Connection` object owns the `Receiver` end of all its channels. When a `Connection` is dropped (due to a disconnect), its `Receiver`s are also dropped.
*   **The Contract:** Any subsequent attempt by a component to use its stale `Sender` will immediately fail with a `SendError`. This error is the application component's designated, unambiguous signal that it is disconnected.
*   **The Recovery Path:** Upon receiving a `SendError`, the component must discard the stale sender and request a new one from the `ZzNet` facade. This makes the design "easy to get right and hard to get wrong" without needing extra "connection IDs" on every data packet.

### 4. Future Consideration: Instrumentation

Instrumentation is an orthogonal concern. The runtimes should be instrumented to emit status changes (e.g., `Connecting`, `Connected`, number of clients) for consumption by GUIs or monitoring systems, but this data should not be part of the core component communication.

### 5. Other Decisions

*   **Channel ID Protocol:** For performance, data messages will not identify their channel by a `String` name. Instead, a control message handshake will be used to map a human-readable channel name to a numeric `ChannelId` (`u16`) for the lifetime of a connection. All subsequent data packets will use this efficient numeric ID.
*   **TLS/mTLS Policy:** If TLS is enabled, it is **always mutual TLS (mTLS)**.
*   **Testability Strategy:** We will improve testability by refactoring logic into self-contained units. It is acceptable for tests to rely on dedicated test files (e.g., certificates) stored within the project repository.
*   **Configurable Timing:** All time-based behaviors (e.g., reconnect delays) must be configurable, not hardcoded, to ensure testability.
