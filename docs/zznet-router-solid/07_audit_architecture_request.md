### **The Audit Action Plan: Verifying the Architectural Vision in Code**

**Objective:** To systematically compare the current codebase against the agreed-upon "Ground Truth 3.0" and produce a definitive list of concrete deviations.

**Methodology:** For each architectural principle, we will ask a key question and then define a specific code-level investigation to answer it.

#### **Audit Point 1: The Role of the `Router`**

*   **Principle to Verify:** `Router` is a *Session Factory and Lifecycle Manager*, not a runtime message router. Its job is limited to connect/disconnect events.
*   **Key Question:** Does the `RouterActor` contain logic for routing in-flight data messages, or is its API limited to session lifecycle management?
*   **Investigation Plan:**
    1.  **Examine the `RouterActor`'s message handlers** in `src/net/zznet-router/src/actor.rs`.
    2.  **Identify all public messages** it handles (e.g., `OnPeerConnected`, `SendToPeer`, `BroadcastToPeers`).
    3.  **Categorize each message:** Is its purpose related to **session setup/teardown** (e.g., `OnPeerConnected`, `RegisterManager`) or **ongoing data routing** (e.g., `SendToPeer`)?
    *   **Expected Finding (if aligned):** The `RouterActor` should primarily handle messages related to creating and destroying peer sessions.
    *   **Potential Deviation:** The presence of `SendToPeer` and `BroadcastToPeers` handlers on the `RouterActor` suggests it has runtime routing responsibilities, which contradicts the vision of `PeerChannels` owning the data path.

#### **Audit Point 2: The Role of `PeerChannels`**

*   **Principle to Verify:** `PeerChannels` is the *1:1 Session Data Handler*, owning the data path for a single peer.
*   **Key Question:** Does the `PeerChannels` struct contain the core machinery for a single peer's data flow?
*   **Investigation Plan:**
    1.  **Inspect the struct definition** of `PeerChannels` in `src/net/zznet-router/src/peer_channels.rs`.
    2.  **Confirm the presence of:**
        *   An outbound channel (`outbound_tx: mpsc::Sender<...>`).
        *   An inbound broadcast channel (`inbound_broadcast: broadcast::Sender<...>`).
        *   An inbound processing task (`inbound_task: JoinHandle<()`).
    *   **Expected Finding (if aligned):** The struct should clearly contain the state and tasks necessary to manage the bidirectional flow of data for one peer.
    *   **Potential Deviation:** If this logic is absent or minimal, it implies that another actor (likely the `Router`) is incorrectly handling the per-peer data flow.

#### **Audit Point 3: `zznet-hello` as the Protocol Boundary**

*   **Principle to Verify:** `zznet-hello` is responsible for handling both Protocol A (`HELLO`) and the outer envelope of Protocol B (`RoomMessage`).
*   **Key Question:** Does the `HelloActor`'s implementation show that it processes both handshake frames and room data frames?
*   **Investigation Plan:**
    1.  **Review the `Frame` enum** in `src/net/zznet-hello/src/protocol.rs`. It should have variants for both `Handshake` and `Room`.
    2.  **Examine the main message handling logic** of `HelloActor` in `src/net/zznet-hello/src/actor.rs` (likely a method like `handle_received_frame`).
    3.  **Verify that the logic explicitly checks the `Frame` type** and dispatches to different handlers for `Frame::Handshake(...)` vs. `Frame::Room(...)`.
    *   **Expected Finding (if aligned):** The actor should have a state machine (`Handshaking`, `Ready`) and handle `Room` frames only when in the `Ready` state.
    *   **Potential Deviation:** If `HelloActor` only handles handshake frames and immediately passes the raw transport to another actor, it is not fulfilling its role as the Protocol B handler.

#### **Audit Point 4: The `PeerChannels` to `Room<T>` Connection**

*   **Principle to Verify:** The connection between `PeerChannels` and the various `Room<T>` actors is achieved via actor messaging (`Recipient`), not trait objects (`Box<dyn RoomHandle>`).
*   **Key Question:** How does `PeerChannels` store references to the rooms it needs to dispatch messages to?
*   **Investigation Plan:**
    1.  **Search the `zznet-router` crate** for the string `Box<dyn RoomHandle>`.
    2.  **Inspect the `PeerChannels` struct definition.** Does it contain a map or collection of `Box<dyn RoomHandle>`?
    *   **Expected Finding (if aligned):** The code should be free of `Box<dyn RoomHandle>`. `PeerChannels` should instead hold a map of `RoomId -> Recipient<SomeMessageType>`.
    *   **Potential Deviation:** The presence and use of `Box<dyn RoomHandle>` indicates the old, trait-object-based wiring is still in place and the vision has not been implemented.

#### **Audit Point 5: The Unidirectional "Club Sandwich" Flow**

*   **Principle to Verify:** Information flows one-way: `ConnectionManager` -> `PeerManagerActor` -> `RouterActor`.
*   **Key Question:** What is the actual code path for handling a new, authenticated connection?
*   **Investigation Plan:**
    1.  **Start at `zznet-hello/src/connection_manager.rs`**. Find the `Handler<HandshakeComplete>` implementation.
    2.  **Trace the logic:** What does it do after a handshake succeeds?
        *   Does it send a message to the `PeerManagerActor`?
        *   Does it also send a message directly to the `RouterActor`?
        *   Does it await a response from either actor?
    *   **Expected Finding (if aligned):** The `ConnectionManager` should send a single, fire-and-forget message to `PeerManagerActor`. The `PeerManagerActor` should then publish an event. The `RouterActor` should be an independent subscriber to that event. There should be no direct communication from `ConnectionManager` to `RouterActor`.
    *   **Potential Deviation:** Any code where `ConnectionManager` directly calls or sends messages to `RouterActor`, or where it awaits a response from `PeerManagerActor` before proceeding, violates the unidirectional, event-driven model.

