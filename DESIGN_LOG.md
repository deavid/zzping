# Design and Process Log

This document captures the key architectural decisions and workflow processes established during our interactive sessions.

## Our Workflow

1.  **Roles:** My role is to act as a high-level architect. I will analyze requirements, propose designs, and break down the work into small, self-contained tasks. The user (David) will use these tasks to guide a separate implementation agent.

2.  **Task Format:** All tasks will be delivered as Markdown files. These tasks will focus on the "what" and "why," not the specific "how." They will define goals, file structures, and the conceptual contracts of components, but will not contain prescriptive code snippets.

3.  **Coding Standards:** All work must adhere to the principles laid out in `AGENT_CODING_STANDARDS.md`. This document is the source of truth for file structure, code style, documentation standards, and architectural patterns.

4.  **File Modification Process:** When creating or modifying design documents like this one, I will state my intent and then write the file directly. The user will then review the file itself, rather than approving the content in the chat. This streamlines the review process.

5.  **Task Self-Containment:** The implementation agent's context is cleared between tasks. Therefore, every task created must be self-describing. If a task builds upon the work of a previous one, it must provide the necessary context and explain the dependencies. The agent will not remember previous conversations or file states.

## Architectural Decisions

### 1. `zznet` Architecture: A Symmetrical, Layered, Actor-Based Model

We have defined a layered architecture for `zznet` that prioritizes symmetry, a reactive design, and a robust, lock-free implementation via the Actor Model.

*   **Layer 1: `runtime` (`ClientRuntime`, `ServerRuntime`)**
    *   **Responsibility:** To act as a factory that produces a `Stream` of raw I/O streams (`Box<dyn AsyncReadWrite>`). This layer manages the lifecycle of network resources (sockets, listeners) and all reconnect logic.

*   **Layer 2: `connection_manager` (The Connection Actor)**
    *   **Responsibility:** To manage the entire lifecycle and protocol for a **single, active connection**.
    *   **`Connection` (Public Handle):** A lightweight public struct that provides the API for interacting with a connection (e.g., `request_channel`). It sends commands to the actor.
    *   **`ConnectionActor` (Private State Machine):** A private struct that runs in its own spawned task. It **owns all the state** for a single connection (channel maps, stream reader/writer), eliminating the need for `Arc<Mutex<...>>`. It processes commands from the handle and frames from the network in a `tokio::select!` loop.

*   **Layer 3: `zznet-lib` Facade (`ZzNet`)**
    *   **Responsibility:** To provide a simple, stable API for application components.
    *   This will be a **new, separate crate** located at `src/common/zznet-lib`.
    *   The `ZzNet` struct will be the **only** API that high-level components interact with.

### 2. The Connection Actor: Design Details

To ensure safe and clear communication, the `Connection` handle and `ConnectionActor` will use a command-based pattern.

*   **Request/Response with `oneshot`:** API calls that require a response (like `request_channel`) will use a `tokio::sync::oneshot` channel. The caller sends the `oneshot::Sender` as part of the command, and the actor uses it to send the response directly back to the waiting caller.
*   **`Channel` Struct:** Instead of returning raw MPSC senders/receivers, the API will return a dedicated `Channel` struct that encapsulates the channel's ID and its communication endpoints. This provides a cleaner abstraction to the application.

### 3. Connection & Channel Lifecycle Guarantees

We explicitly considered the risk of using a "stale" channel handle after a reconnection. The design mitigates this via Rust's ownership model.

*   **The Contract:** When a `Connection` (and its internal actor) is dropped due to a disconnect, all of its internal channel `Receiver`s are also dropped. Any subsequent attempt by a component to use a stale `Sender` will immediately fail with a `SendError`. This error is the application component's designated, unambiguous signal that it is disconnected and must request a new channel from the `ZzNet` facade.

### 4. Other Decisions

*   **Channel ID Protocol:** Data messages will use a numeric `ChannelId` (`u16`) for performance. A control message handshake will map a `String` name to a `ChannelId` for the lifetime of a connection.
*   **TLS/mTLS Policy:** If TLS is enabled, it is **always mutual TLS (mTLS)**.
*   **Configurable Timing:** All time-based behaviors (e.g., reconnect delays) must be configurable.
*   **Testability Strategy:** Logic will be kept pure where possible. It is acceptable for tests to rely on dedicated test files (e.g., certificates) in the repo.