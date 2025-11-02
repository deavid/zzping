## Understanding of the Vision

We want a clear separation between a user's **identity** (who they are on the network) and their **capabilities** (what they can do inside a specific component).

Here's the flow as I understand it:

1.  **The Role is the Global "Passport".** When a peer connects to the system (e.g., a `zzping-collector` connecting to a `zzping-database`), it presents a `Role` (like "collector" or "client-admin"). This `Role` is its system-wide identity, its passport. It says who they are, but not what they're allowed to do in any specific area.

2.  **Each Component is a "Secure Room" with its Own Rules.** Each component, like `zzintent-config` or `zzmem-db`, is a secure room with its own specific set of locks and keys.
    *   The `zzintent-config` room doesn't care about "roles." It only cares about its own specific permissions: `can_read_config` and `can_write_config`.
    *   The `zzmem-db` room also doesn't care about roles. It only cares about its own permissions: `can_submit_batches` and `can_query_data`.
    These permissions are defined in a simple, component-local struct (e.g., `IntentConfigPermissions`).

3.  **The Application is the "Security Office".** The main application binary (e.g., `zzping-database/src/main.rs` or `service.rs`) acts as the central security office. This is where the **master policy** is defined. This is the only place that knows how to connect a global passport (`Role`) to the specific keys needed for each secure room (`ComponentPermissions`).

    The application's code will contain the logic:
    *   "A peer with the **'client-admin' Role** gets `IntentConfigPermissions` with `can_write = true`."
    *   "A peer with the **'collector' Role** gets `IntentConfigPermissions` with `can_write = false`."
    *   "A peer with the **'collector' Role** gets `MemDbPermissions` with `can_submit_batches = true`."

4.  **The `NetworkManager` is the "Security Guard at the Door".** When a new peer connects, the `NetworkManager` for each component acts as the security guard at that component's door.
    *   The guard sees the peer's passport (their `Role`, e.g., "client-admin").
    *   The guard calls the security office (the application's mapping logic) to get the correct set of keys (`ComponentPermissions`) for that passport.
    *   The guard then creates a dedicated, per-peer handler (the `NetworkActor`).

5.  **The `NetworkActor` Holds the "Keys," Not the "Passport".** The `NetworkActor` that the guard creates is given the specific `ComponentPermissions` struct. It **does not store the `Role`**. For the entire lifetime of that connection, this `NetworkActor` holds an immutable set of permissions.

6.  **The Final Check is Simple and Local.** When a request arrives at that `NetworkActor` (e.g., "please update the config"), the actor performs a simple, instant, local check:
    ```rust
    // Inside the NetworkActor for zzintent-config
    if self.permissions.can_write_config {
        // Allow the operation
    } else {
        // Deny the operation
    }
    ```
    The actor never has to ask anyone else about roles. It doesn't know what a "client-admin" is. It only knows whether the `can_write_config` flag is `true`.

---

**In short:** The `Role` is a global concern that dies at the component boundary. It is translated *once* into a local `Permissions` struct that lives for the duration of the session. The component's internal business logic is completely decoupled from the concept of roles.

Does this accurately capture your intent? Specifically, the idea that the per-peer `NetworkActor` holds an immutable `Permissions` struct and never sees the `Role` string after it's been created?

---

## **A Plan for Simple, SOLID, and Component-Specific Authorization**

### **Executive Summary**

This plan outlines an authorization model that cleanly separates a peer's global identity from its specific capabilities within any given component. The core principle is to empower each component to define its own set of required permissions, while centralizing the policy decisions in the main application.

This approach eliminates the tight coupling between components and the global authentication system. Instead of checking roles (`if role == "admin"`), components will perform simple, local checks against a permissions struct (`if self.permissions.can_write`). This makes components more modular, easier to test, and architecturally pure.

The design is intentionally simple, avoiding complex "authorization service" actors or dynamic, string-based permission lookups. It prioritizes compile-time safety and architectural clarity over unnecessary complexity.

---

### **Part 1: The Vision - Passports, Keys, and Secure Rooms**

To understand the intention, we will use a simple metaphor: a secure facility with multiple rooms, each requiring different keys.

1.  **The `Role` is a "Passport": A System-Wide Identity.**
    When a peer connects, it presents its `Role` (e.g., "collector", "client-admin"). This is its passport. It proves *who they are* at a high level across the entire system. This identity is established once at the network boundary and does not change for the duration of the connection.

2.  **Each Component is a "Secure Room" with its Own Locks.**
    Each component, like `zzintent-config` or `zzmem-db`, is a secure room with a unique set of locks. These locks are not labeled "for admins" or "for collectors"; they are labeled by the specific action they protect.
    *   The `zzintent-config` room has locks for `can_read_config` and `can_write_config`.
    *   The `zzmem-db` room has locks for `can_submit_pings` and `can_query_history`.
    Each component defines its own set of permissions (its "locks") in a simple, local struct, without any knowledge of the permission systems in other components.

3.  **The Application is the "Security Office" that Issues Keys.**
    The main application binary (e.g., `zzping-database`) is the only part of the system that understands the master policy. It acts as the security office, holding the "key-cutting machine" that knows how to create the right set of keys for each passport.

    This is where the master policy lives, stating rules like:
    *   "Anyone with a 'client-admin' passport gets a key that unlocks `can_write_config` in the `zzintent-config` room."
    *   "Anyone with a 'collector' passport gets a key that unlocks `can_submit_pings` in the `zzmem-db` room, but *not* `can_query_history`."

4.  **The `NetworkManager` is the "Guard" who Hands Out the Keys.**
    When a new peer arrives at a component's "door," the component's `NetworkManager` acts as the security guard. The guard's job is simple:
    *   It looks at the peer's passport (`Role`).
    *   It calls the security office (the application's mapping logic) to get the correct, pre-made set of keys (`Permissions` struct) for that passport.
    *   It then gives this set of keys to a dedicated attendant (the per-peer `NetworkActor`) who will escort the peer for the rest of their visit.

5.  **The `NetworkActor` Holds the Keys, Not the Passport.**
    The per-peer `NetworkActor` holds onto this immutable set of keys (`Permissions` struct) for the entire duration of the connection. It never needs to see the passport again. When the peer asks to perform an action (e.g., "update the config"), the `NetworkActor` simply checks if it holds the right key (`if self.permissions.can_write_config`). The check is instant, local, and requires no further lookups.

---

### **Part 2: The Architectural Requirements**

To implement this vision, the system **MUST** adhere to the following architectural requirements:

*   **Requirement 1: The Application MUST Define the Master Policy.**
    The main application (`zzping-database`, `zzping-collector`) is the **sole owner** of the `Role` -> `Permissions` mapping. This logic **MUST NOT** exist inside the generic `zznet-*` framework or within the components themselves. This policy will be defined in the application's "composition root" (e.g., in `service.rs`).

*   **Requirement 2: Components MUST Define Their Own Permissions.**
    Each networked component (e.g., `zzintent-config`) **MUST** define its own, local `Permissions` struct. This struct will consist of simple boolean flags (e.g., `can_read: bool`, `can_write: bool`). It **MUST NOT** reference the global `AuthRole` enum or any types from other components.

*   **Requirement 3: The `NetworkManager` MUST Perform the Translation.**
    A component's `NetworkManager` **MUST** receive the peer's global `Role` string exactly once when a peer connection for its room is established. It **MUST** immediately use the application-provided mapping logic to translate this `Role` into the component-specific `Permissions` struct. It **MUST NOT** store the `Role` string.

*   **Requirement 4: The Per-Peer `NetworkActor` MUST Be Role-Agnostic.**
    The `NetworkActor` for a given peer **MUST** be instantiated with the immutable `Permissions` struct. It **MUST** perform all authorization checks by reading the boolean flags in this struct. It **MUST NOT** have any knowledge of the original `Role` and **MUST NOT** have any mechanism to query for roles or permissions at runtime.

*   **Requirement 5: The `MainActor` (Business Logic) MUST Be Security-Agnostic.**
    The component's `MainActor` **MUST** remain completely pure. It should have no knowledge of `Roles`, `Permissions`, or authorization checks. It receives commands that are *already authorized* by the `NetworkActor` and simply executes the business logic.

---

### **Part 3: The KISS Principle - What We Are NOT Building**

To keep the system simple, maintainable, and free of overengineering, we are explicitly **prohibiting** the following complex patterns:

*   **NO Central "Authorization Service" Actor.**
    We will not create a single, monolithic "AuthService" that all components must query at runtime. The permission mapping is a simple, stateless function provided by the application at startup, not a complex, stateful service. This avoids creating a new bottleneck and "God Object."

*   **NO Dynamic, Mid-Session Permission Changes.**
    The `Permissions` struct given to a `NetworkActor` is **immutable** and lasts for the entire session. If a peer's permissions need to change, they must disconnect and reconnect. This avoids the immense complexity of managing dynamic permission updates, caching, and state synchronization.

*   **NO String-Based Permissions.**
    Authorization checks will be `if self.permissions.can_write`, not `if self.permissions.contains("can_write")`. Using a `struct` with boolean flags provides **compile-time safety**. It makes it impossible to make a typo in a permission string and accidentally create a security hole.

*   **NO Complex Permission Objects.**
    The `Permissions` struct should be a simple container of booleans. We are not building a complex object-graph or a rule engine. It's a straightforward checklist of capabilities.

*   **NO Global `Permission` Type.**
    There will be no `zznet_api::Permission` struct. Each component defines its own. `zzintent-config`'s permissions are its own business and completely unknown to `zzmem-db`. This enforces true decoupling.

---

### **Conclusion**

This plan provides a clear path to a robust and simple authorization model. By strictly separating the global concept of a `Role` from the local, component-specific concept of `Permissions`, we achieve an architecture that is:

*   **SOLID:** Components are decoupled and have a single responsibility.
*   **Simple:** Authorization checks are simple boolean comparisons.
*   **Testable:** The `MainActor` can be tested without any network or auth logic, and the `NetworkActor` can be tested by simply giving it a `Permissions` struct.
*   **Secure:** The policy is centralized in the application, and the checks are enforced at the network boundary of each component, with no room for ambiguity.