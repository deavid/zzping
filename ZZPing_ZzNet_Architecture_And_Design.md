# `zznet` Component: Architectural Design Discussion

## 1. Introduction: The Initial Problem

The primary goal is to create a networking library, `zznet`, within the ZZPing project. This library must provide a high-level, abstracted communication layer for use by other components. The initial concept was dubbed **RMCH** - Remote Multiplexed Channel.

## 2. Core Architectural Requirements

Through an iterative design discussion, the following set of core requirements for the `zznet` architecture was established:

1.  **Component Abstraction & Isolation:** Components must be completely decoupled from the network. They must operate on a message-oriented API (like Tokio channels) and be ignorant of sockets, serialization, framing, or TLS.

2.  **Clarity and Testability over Symmetry:** The primary goal is to make components easy to reason about and fully test in isolation. A perfectly symmetrical API for client and server components is a "nice-to-have," but secondary to an API that is explicit, clear, and highly testable.

3.  **Multi-Client to Server Model:** The architecture is designed for a "hub-and-spoke" topology. Multiple, distinct client processes (e.g., collectors, GUIs) connect to a single, central server process. Direct communication between clients (peer-to-peer) is not a goal.

4.  **Server Authority:** The server is the authoritative peer in the relationship. It has the ultimate power to accept or deny client requests to open new communication sub-channels.

5.  **Non-Blocking Component Lifecycle:** A component's initialization and primary logic must **not** be blocked waiting for a network connection or a channel to be established. Components must be able to start, run their own tasks, and handle network events asynchronously.

6.  **Concurrent Multi-Client Handling:** A single server-side component (e.g., the `HealthReport` component) must be able to concurrently handle channels from *multiple, distinct clients*. The component must be able to differentiate which client it is communicating with on a given channel.

7.  **Full Testability:** Components must be unit-testable in complete isolation, without requiring a real network. The `zznet` communication interface must be easily mockable.

## 3. The Evolution of the Design

The final design was reached through a series of refinements as we explored the conceptual challenges of creating a simple, testable API for an inherently asymmetrical networking world.

*(Refinements 1, 2 and 3 remain the same as they trace the origin of the core ideas)*

### Refinement 1: Acknowledging the Client/Server Roles
...
### Refinement 2: A Unified, Configuration-Driven API
...
### Refinement 3: The "Room Model" and the Central Conflict
...

### Refinement 4: Clarifying the Scope (Multi-Client, Not P2P)

The "Room Model" analogy was useful but potentially misleading. A key clarification was made: this is **not** a generic message broker or peer-to-peer system. It is a hub-and-spoke model.

This led to a more precise model: **The Main Connection and The Named Channels**. Each client establishes one main, long-lived TCP connection to the server. Within that single connection, multiple, independent "Named Channels" are multiplexed. A channel is always tied to the lifecycle of its parent TCP connection.

### Refinement 5: From Rendezvous to Client-Request/Server-ACK

The pure rendezvous model was deemed too loose. A more robust "client requests, server acknowledges" protocol for channel creation was adopted to enforce the Server Authority requirement. The challenge then became how to present this to components in a clear, testable way.

### The Final Architecture: A Non-Blocking, Event-Driven Model

The initial idea of a simple `async fn get_channel()` was flawed. The `await` on the server side was a form of **blocking**, violating a core requirement. A server component cannot wait for a client to appear before it finishes initializing.

This led to an event-driven model, but the initial version was still incomplete for a multi-client architecture. A server component receiving a new channel `(Sender, Receiver)` has no context of *which client* it belongs to.

This led to the final, agreed-upon architecture:

1.  **The Core Principle:** The design is **event-driven and non-blocking**. Server components **listen** for channel requests. Client components **request** channels. Communication occurs via streams of events, not blocking calls.

2.  **The Component API (Clarity over Symmetry):** To maximize clarity and testability, the API is slightly asymmetrical. The function names explicitly state the component's intent.

    *   **Server-Side API:**
        ```rust
        // A unique identifier for each connected client.
        type ClientId = u64;

        fn listen_for_channel(&self, name: &str) -> mpsc::Receiver<(ClientId, (Sender, Receiver))>
        ```
    *   **Client-Side API:**
        ```rust
        fn request_channel(&self, name: &str) -> mpsc::Receiver<(Sender, Receiver)>
        ```

3.  **The Asymmetrical Implementation:**
    -   **Server-Side:** When a server component calls `listen_for_channel`, the `zznet` runtime registers it as a handler for that channel name. The function returns immediately. The returned `Receiver` will yield a `(ClientId, (Sender, Receiver))` tuple for **every** client that connects and requests that channel. This correctly models the concurrent multi-client requirement in a non-blocking way.
    -   **Client-Side:** When a client component calls `request_channel`, the `zznet` runtime immediately sends a `REQUEST_CHANNEL` control message to the server over its main connection. The returned `Receiver` will yield at most **one** `(Sender, Receiver)` pair if and when the server acknowledges the request.

This final model successfully meets all identified requirements: it provides a clear, non-blocking, and highly testable API; it respects the authoritative multi-client/server roles; and it enables components to be developed and tested in complete isolation.

### Justification for the Asymmetrical API

The choice to use a slightly asymmetrical API (`listen_for_channel` / `request_channel`) instead of a single, symmetrical function was a deliberate design decision made to favor clarity and explicitly model the reality of the component roles.

1.  **Clarity of Intent:** The function names are self-documenting. A developer working on a server-side component, like the `CState` orchestrator, is explicitly `listen`ing for incoming clients that need orders. Conversely, a developer on the collector side is `request`ing a channel to receive those orders. This removes ambiguity and reduces cognitive load compared to a single function name whose behavior would change dramatically based on context.

2.  **Embracing Inherent Asymmetry:** The application components themselves have fundamentally asymmetrical roles. The server `MemDB` aggregates data from many collectors, while a collector `MemDB` is only a single producer. The server `IntentConfig` is the authoritative source of truth, while the collector's is a read-only cache. Forcing these different roles through a symmetrical API creates a "leaky abstraction." The asymmetrical API acknowledges the reality of the system, leading to code that is easier to reason about.

3.  **Preserving Testability:** The primary goal of isolated testing is not only preserved but enhanced. To test any component, one simply provides it with a standard `mpsc::channel` receiver. The name of the `zznet` function used to acquire that receiver is irrelevant to the test's implementation. The explicit naming, however, makes the *intent* of the test setup clearer.

In conclusion, while a symmetrical API was an initial goal, the slightly asymmetrical design was adopted as it leads to clearer, more robust, and more debuggable code without compromising the critical requirement of component testability.

## 4. Application Layer: Component Architecture

With the `zznet` layer providing a robust foundation for communication, this section describes the high-level application components that are built on top of it.

### Component Overview

The application logic is broken down into several distinct, isolated components:

*   **Pinger:** The workhorse on the collector. This is an internal component with a sub-component running for each target host. It takes its instructions from `IntentConfig` and `CState` and is responsible for executing pings and passing the resulting data to the `MemDB`.

*   **MemDB (Memory Database):** This component represents the data plane.
    *   On the **Collector**, it acts as an in-memory buffer for recent ping data. It is responsible for reliably replicating this data to the `MemDB` instance on the server.
    *   On the **Server**, it is the central, authoritative in-memory database. It receives data streams from all collectors, handles out-of-order data, and manages persistence to long-term disk storage.

*   **IntentConfig (Desired State):** This component represents the user-facing control plane.
    *   The **Server** instance is the single source of truth, storing the user's desired configuration (e.g., hosts to ping, rates) to disk.
    *   The **GUI/CLI** acts as the writer, allowing users to modify the configuration on the server.
    *   The **Collector** instance is a reader, consuming the configuration from the server. It maintains a local cache of the last known configuration to ensure resilient operation during server disconnects.

*   **CState (Collector State):** This component represents the operational control plane.
    *   It is a **server-managed, in-memory** component that acts as the system's orchestrator.
    *   It translates the high-level *intent* from `IntentConfig` into concrete, real-time *orders* for each collector (e.g., "Collector 'A', you are now the master pinger for 8.8.8.8 at 50 pps").

*   **Health:** This is the system's observability component.
    *   On the **Collector**, it gathers health metrics from all other local components.
    *   On the **Server**, it receives these metrics from all collectors and aggregates them.
    *   The **GUI/CLI** reads the aggregated data from the server to provide a system-wide health overview.

### Mapping Components to Network Channels

The interactions between these distributed components are realized as named channels provided by `zznet`. This mapping provides a clear overview of the entire system's communication topology:

| Channel Name | Client Component | Server Component | Purpose |
| :--- | :--- | :--- | :--- |
| `"memdb-replication"` | `MemDB` (Collector) | `MemDB` (Server) | Collector streams ping data. Protocol includes handshake for re-sync. |
| `"intent-config"` | `IntentConfig` (Collector) | `IntentConfig` (Server) | Server pushes config updates. Protocol includes full sync on connect. |
| `"cstate-orders"` | `CState` logic (Collector) | `CState` (Server) | Server sends operational commands to collectors. |
| `"health-metrics"` | `Health` (Collector) | `Health` (Server) | Collector streams internal component health metrics to the server. |
| `"gui-intent-config"` | `IntentConfig` (GUI/CLI) | `IntentConfig` (Server) | GUI reads and writes the desired configuration. |
| `"gui-cstate-view"` | `CState` logic (GUI/CLI) | `CState` (Server) | GUI subscribes to real-time operational state. |
| `"gui-query"` | `MemDB` logic (GUI/CLI) | `MemDB` (Server) | GUI requests historical/real-time ping data. |
| `"gui-health-view"` | `Health` logic (GUI/CLI) | `Health` (Server) | GUI subscribes to aggregated system health status. |

### Handling Stateful Protocols and Lifecycle Events

A key requirement is that components must be resilient to network disconnects and reconnections. The `zznet` architecture facilitates this by separating the transport layer from the component-level protocol.

*   **Component Protocols:** While `zznet` provides the `(Sender, Receiver)` pair for a channel, the components themselves are responsible for the protocol spoken over it. For example, the `MemDB` components will first perform a handshake protocol over their channel to negotiate a synchronization state before streaming live data. This ensures no data is lost during a reconnection.

*   **Lifecycle Events:** To enable components like `CState` to react to collector availability, the central `zznet` manager on the server will provide a stream of system lifecycle events. `CState` can consume this stream to be immediately notified of client connections and disconnections, allowing it to dynamically re-assign pinging responsibilities to maintain the user's desired configuration.