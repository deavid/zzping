# Design and Process Log

This document captures the key architectural decisions and workflow processes established during our interactive sessions.

## Our Workflow

1.  **Roles:** My role is to act as a high-level architect. I will analyze requirements, propose designs, and break down the work into small, self-contained tasks. The user (David) will use these tasks to guide a separate implementation agent.

2.  **Task Format:** All tasks will be delivered as Markdown files. These tasks will focus on the "what" and "why," not the specific "how." They will define goals, file structures, and the conceptual contracts of components, but will not contain prescriptive code snippets.

3.  **Coding Standards:** All work must adhere to the principles laid out in `AGENT_CODING_STANDARDS.md`. This document is the source of truth for file structure, code style, documentation standards, and architectural patterns.

4.  **File Modification Process:** When creating or modifying design documents like this one, I will state my intent and then write the file directly. The user will then review the file itself, rather than approving the content in the chat. This streamlines the review process.

## Architectural Decisions

### 1. `zznet` Architecture: A Symmetrical, Stream-Based Model

We have defined a layered architecture for `zznet` that prioritizes symmetry and a reactive design, where components consume streams of events rather than managing state directly.

*   **Layer 1: `connection_manager` (`Connection`)**
    *   **Responsibility:** To manage the data framing for a **single, active connection**.
    *   It wraps a raw I/O stream (`Box<dyn AsyncReadWrite>`) and will provide a message-oriented API by handling the length-prefixing of data frames. It represents a single, established connection.

*   **Layer 2: `runtime` (`ClientRuntime`, `ServerRuntime`)**
    *   **Responsibility:** To act as a factory that produces a `Stream` of `Connection` objects. This layer manages the lifecycle of network resources (sockets, listeners).
    *   **`ServerRuntime`:** Listens on a network port and produces a `Stream` of `Connection` objects, one for each successfully accepted client.
    *   **`ClientRuntime`:** Proactively connects to a server and produces a `Stream` of `Connection` objects. The stream will yield a new `Connection` whenever a connection is successfully established. This layer **implicitly handles all reconnect logic** (retries, backoff), presenting a simple, continuous stream of active connections to the consumer.

### 2. The Client-Side Design: A Deliberate Choice for Symmetry

During our discussion, we considered two primary models for the client-side architecture.

1.  **Discarded Model: Explicit State Machine.** This approach involved a `ClientSession` object that would act as an explicit state machine (`Connecting`, `Connected`, `Retrying`). The application would own this object and could directly query its state and control its behavior.
2.  **Chosen Model: Reactive Stream.** This is the model described above, where the `ClientRuntime` provides a `Stream` of connections.

We have **explicitly chosen the Reactive Stream model**. The reasoning for this critical decision is as follows:

*   **Enforces Component Resilience:** The primary design principle is that application components **must not have control over the connection state**. They must be designed to be resilient to disconnections and reconnections at any time. The stream model enforces this by pushing connections to the components, preventing them from managing the connection's lifecycle.
*   **Matches Component Needs:** Application components do not need to know *how* a connection is being maintained (e.g., "when is the next retry?"). They only care about the binary state of being "connected" or "not connected." A stream that yields a `Connection` when one is available perfectly models this reality.
*   **Architectural Symmetry:** This model provides an elegant symmetry with the server-side API, which already provides a stream of incoming client connections.

### 3. Future Consideration: Instrumentation

While the core architecture focuses on the logic of establishing and managing connections, a parallel requirement will be to add robust instrumentation. This is an orthogonal concern to the application components that will use the data channels.

*   **Client-Side:** The `ClientRuntime`'s stream should be instrumented to emit status changes (e.g., `Connecting`, `Connected`, `RetryingWithBackoff`). This information is not for the application components but for higher-level systems like a GUI or a health-metrics exporter to observe the state of the session.
*   **Server-Side:** The `ServerRuntime` should be instrumented to provide metrics such as the number of active clients, connection rates, etc.

This ensures that the core logic remains clean, while still providing the necessary visibility for monitoring and user feedback.

### 4. Other Decisions

*   **TLS/mTLS Policy:** If TLS is enabled for a connection, it is **always mutual TLS (mTLS)**. There is no server-only TLS mode.
*   **Testability Strategy:** We will improve testability by refactoring logic into self-contained units (e.g., moving TLS build logic into `TlsCfg`). However, for testing purposes, it is acceptable to rely on dedicated test files (e.g., certificates) stored within the project repository. Full network mocking is deferred.