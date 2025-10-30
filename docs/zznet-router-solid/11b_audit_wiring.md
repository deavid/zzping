### **Crate: `zznet-api`**

This crate contains only traits and data structures. No actors are created here. Its types are created by various layers.

*   **Type:** `PeerId`, `RoomId`, `Role`, `Permission`, `PeerIdentity`
    *   **Created By:** Any part of the system that needs to reference a peer, room, or identity. They are simple data containers.
    *   **Sends To:** N/A (Data structure)
    *   **Receives From:** N/A (Data structure)

---

### **Crate: `zznet-transport-tcp`** (and other transport crates)

*   **Type:** `TcpTransportServer` / `TcpTransportClient`
    *   **Created By:** The application's `main.rs` (or `service.rs`). This is the entry point to the network.
    *   **Sends To:** The underlying OS network stack.
    *   **Receives From:** The underlying OS network stack.

*   **Type:** `TcpTransport` (implements `TransportConnection`)
    *   **Created By:** The `TcpTransportServer` (on `accept()`) or `TcpTransportClient` (on `connect()`).
    *   **Sends To:** The OS network stack.
    *   **Receives From:** The OS network stack.

---

### **Crate: `zznet-hello`**

*   **Type:** `ConnectionManager` (Actor)
    *   **Created By:** The application's `main.rs` / `service.rs`. It's the top-level network orchestrator for one side (client or server).
    *   **Sends To:**
        *   `HelloActor` (spawns it and sends `HandleTransport`).
        *   `PeerManagerActor` (sends `ConnectPeerWithChannels` and `AddPeer` after a successful handshake).
    *   **Receives From:**
        *   The application (receives `HandleTransport` messages containing new `TransportConnection`s).
        *   `HelloActor` (receives `HandshakeComplete` notifications).

*   **Type:** `HelloActor` (Actor)
    *   **Created By:** The `ConnectionManager`. One `HelloActor` is created for each new `TransportConnection`.
    *   **Sends To:**
        *   `TransportConnection` (sends raw `Vec<u8>` frames for Protocol A & B).
        *   `ConnectionManager` (sends `HandshakeComplete` notification).
        *   `PeerChannels` (sends `InboundRoomMessage { room_id, payload }` tuples).
    *   **Receives From:**
        *   `TransportConnection` (receives raw `Vec<u8>` frames).
        *   `PeerChannels` (receives `OutboundRoomMessage { room_id, payload }` tuples to be framed and sent).

---

### **Crate: `zznet-peer-manager`**

*   **Type:** `PeerManagerActor` (Actor)
    *   **Created By:** The application's `main.rs` / `service.rs`. There is **one** singleton instance for the entire application.
    *   **Sends To:**
        *   Itself (internally manages state).
        *   `RouterActor` (sends `OnPeerConnected` event after processing a connection from `ConnectionManager`).
        *   *Subscribers* (broadcasts `PeerLifecycleEvent`s to any actor that has subscribed, including component `NetworkManager`s).
    *   **Receives From:**
        *   `ConnectionManager` (receives `AddPeer` and `ConnectPeerWithChannels` messages).
        *   *Any component* that needs to query peer state (e.g., receives `GetPeerRole` from a component's `NetworkManager`).

---

### **Crate: `zznet-router`**

*   **Type:** `RouterActor` (Actor)
    *   **Created By:** The application's `main.rs` / `service.rs`. There is **one** singleton instance for the entire application.
    *   **Sends To:**
        *   Component `RoomManager`s (sends `CreateRoomForPeer` messages to their `Recipient`s to request the creation of a room actor).
    *   **Receives From:**
        *   The application (receives `RegisterManager` messages at startup).
        *   `PeerManagerActor` (receives `OnPeerConnected` and `OnPeerDisconnected` events).

*   **Type:** `PeerChannels` (Actor)
    *   **Created By:** The `RouterActor`. One `PeerChannels` actor is created for each connected peer.
    *   **Sends To:**
        *   Component `Room<T>` Actors (sends `InboundRoomPayload { payload: Vec<u8> }` messages to their `Recipient`s).
        *   `HelloActor` (sends outbound `(RoomId, Vec<u8>)` tuples to be framed and put on the wire).
    *   **Receives From:**
        *   `HelloActor` (receives inbound `(RoomId, Vec<u8>)` tuples from the network).
        *   Component `Room<T>` Actors (receives outbound `(RoomId, Vec<u8>)` tuples to be sent to the network).

---

### **Crate: `zznet-room`**

*   **Type:** `RoomManager` (This is a trait implemented by components)
    *   **Created By:** The application's `main.rs` / `service.rs`, as part of setting up a component's network stack.
    *   **Sends To:** N/A (It's a factory; it returns a `Recipient`).
    *   **Receives From:** `RouterActor` (receives `CreateRoomForPeer` messages).

*   **Type:** `Room<T>` (This is the logical concept, implemented by the component's `NetworkActor`)
    *   **Created By:** The component's `RoomManager`. One is created per-peer, per-room.
    *   **Sends To:**
        *   The component's `MainActor` (sends the final, deserialized, typed message `T`).
        *   `PeerChannels` (sends the outbound, serialized `(RoomId, Vec<u8>)` tuple).
    *   **Receives From:**
        *   The component's `MainActor` (receives a typed message `T` to be sent).
        *   `PeerChannels` (receives the inbound `InboundRoomPayload { payload: Vec<u8> }`).

---

### Summary Diagram of Creation and Wiring

```mermaid
graph TD
    subgraph App Startup (main.rs / service.rs)
        A[Create TransportServer/Client]
        B[Create ConnectionManager]
        C[Create PeerManagerActor]
        D[Create RouterActor]
        E[Create Component RoomManagers]

        A -- transport --> B;
        C -- Addr --> B;
        D -- Addr --> B;
        C -- Subscribe --> D;
        E -- Register --> D;
    end

    subgraph Per-Connection Lifecycle
        F(ConnectionManager) -- spawns --> G(HelloActor);
        G -- HandshakeComplete --> F;
        F -- AddPeer/Connect --> H(PeerManagerActor);
        H -- OnPeerConnected --> I(RouterActor);
        I -- CreateRoomForPeer --> J(Component RoomManager);
        J -- creates --> K(Component NetworkActor / Room&lt;T&gt;);
        I -- creates --> L(PeerChannels);
        K -- Recipient --> I;
        L -- holds --> M[Recipient Map];
    end

    style K fill:#f9f,stroke:#333,stroke-width:2px;
    style J fill:#ccf,stroke:#333,stroke-width:2px;
```

This covers the complete chain of creation, from the application's entry point down to the per-peer, per-room actors, and defines the communication pathways between them.