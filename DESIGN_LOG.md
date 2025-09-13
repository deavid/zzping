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

### 5. The Three-Crate Architecture for True Component Isolation

To achieve the highest degree of component isolation and adhere strictly to the Dependency Inversion Principle, we have evolved the design into a three-crate system. This ensures that application components are completely decoupled from the networking implementation.

*   **`zznet-api` (The Contract):** A new, minimal, foundational crate.
    *   **Responsibility:** To define the abstract interface for communication.
    *   **Contents:** It defines a public `ZzChannel` trait. This trait provides a generic, asynchronous interface for sending and receiving byte payloads (`async fn send`, `async fn recv`).
    *   **Dependencies:** Has zero dependencies on other `zznet` crates. It is the root of the dependency graph.

*   **`zznet` (The Engine):** The low-level implementation crate.
    *   **Responsibility:** To handle all the complexities of the network protocol, including TLS, configuration, runtimes, and the connection actor model.
    *   **Contents:** All networking details (`ClientConfig`, `ServerConfig`, `ConnectionActor`, etc.) remain here. Its internal, concrete `Channel` struct is modified to implement the `ZzChannel` trait from `zznet-api`.
    *   **Dependencies:** Depends on `zznet-api`.

*   **`zznet-lib` (The Factory/Facade):** The high-level crate that connects the application to the engine.
    *   **Responsibility:** To act as a factory that constructs the networking engine and produces abstract channels for the application.
    *   **Contents:** Contains the `ZzNet` struct and `ZzNetConfig` enum. Its public API (`listen_for_channel`, `request_channel`) returns trait objects (`Box<dyn ZzChannel>`), completely hiding the concrete `zznet::Channel` type from the consumer.
    *   **Dependencies:** Depends on both `zznet` (to build the engine) and `zznet-api` (to return the trait objects).

#### The Resulting Dependency Graph

This structure ensures that application components have the cleanest possible dependency graph:

*   `Application Component` -> `zznet-api`
*   `zznet` (Engine) -> `zznet-api`
*   `zznet-lib` (Factory) -> `zznet` -> `zznet-api`

The "Composer" (the final application binary) is the only entity that depends on all three crates, using `zznet-lib` and `zznet` to configure and create the network, and then passing the abstract `Box<dyn ZzChannel>` objects to the application components, which only know about the `zznet-api` contract. This achieves perfect isolation.

### 6. The First Component: `IntentConfig` Proof of Concept

To validate the entire three-crate architecture and establish a pattern for future components, we will build a Proof of Concept for the `IntentConfig` component.

*   **Goal:** The PoC's primary goal is to serve as a full, end-to-end integration test for the entire `zznet` stack. It will demonstrate that a single, logical component can communicate with itself across the network, with its behavior determined by its assigned `Role`.
*   **Single Component, Multiple Roles:** We will create a single `intent-config` crate. This component will be instantiable in one of three roles: `Server`, `ClientAdmin`, or `ClientRo`. This `Role` enum will be a core concept shared across the system.
*   **Moving `Role` to the API:** The `Role` enum, previously in `zznet`, is a cross-cutting concern used for both network authentication and application-level authorization. It will be moved to the `zznet-api` crate to become part of the system's fundamental contract.
*   **PoC Scope (In-Memory):** The PoC will focus on the communication pattern, not persistence. The `Server` will hold the configuration state in memory. The `ClientAdmin` will have a method to update this state. The `ClientRo` will have a method to subscribe to a stream of updates. Disk I/O (reading/writing `.ron` files) is out of scope for the PoC.
*   **Internal Protocol:** The component will communicate with itself over a `ZzChannel` using a private, internal protocol (e.g., a `serde`-serializable enum) to differentiate between update, broadcast, and request messages. The component's public API will be action-oriented (e.g., `update_config`, `subscribe_to_changes`), completely hiding this internal protocol and its data structures.
*   **The End-to-End Test:** The deliverable will be a single `#[tokio::test]` within the `intent-config` crate. This test will act as the "Composer":
    1.  It will instantiate the *real* `zznet-lib` and `zznet` components to create an in-memory network.
    2.  It will create one `Server`, one `ClientAdmin`, and one `ClientRo` instance of the `IntentConfig` component.
    3.  It will wire them together using the `zznet-lib` facade.
    4.  It will call the `update` method on the `ClientAdmin`.
    5.  It will assert that the `ClientRo` receives the updated state, proving that the entire stack—from application logic, through the abstract API, down to the network engine, and back up—is functioning correctly.