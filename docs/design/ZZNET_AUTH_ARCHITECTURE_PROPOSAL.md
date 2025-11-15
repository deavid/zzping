# ZZNet Auth & ACL: Architectural Vision and Analysis

**Status**: Superseded by `ZZNET_AUTH_ARCHITECTURE_REVISED.md`
**Date**: Original analysis - October 2025
**Preserved for**: Historical context and problem space analysis

## 1. Overview

This document outlines the original proposed architecture for authentication and authorization (Auth/ACL) in the ZZNet network layer. **This document has been superseded by `ZZNET_AUTH_ARCHITECTURE_REVISED.md`**, which incorporates the results of our analysis and discussions.

**Why this document is preserved:**
- It contains valuable analysis of the current codebase (Section 3)
- It documents the original thought process and concerns
- It serves as a reference for the evolution of the architecture

**Key changes in the revised architecture:**
- Certificate identity model changed to **CN=role, SAN=username** (hybrid model)
- ZZNet made auth-agnostic (auth logic moved to application layer)
- Simplified from three-tier (username→role→permissions) to two-tier (identity→role)
- Configuration centralized primarily in database server (not distributed everywhere)

The purpose of this original document was to:

1.  **Define the Architectural Vision:** Formally document the proposed architecture, including its core principles and data flows.
2.  **Analyze the Current Codebase:** Assess the existing components (`zznet-transport-tcp`, `zznet-hello`, etc.) against this vision, identifying both aligned elements and significant gaps.
3.  **Identify Key Challenges:** Discuss the architectural and implementation challenges that must be addressed to realize this vision.

This document is an analysis of the problem space and is not a prescriptive implementation plan.

---

## 2. Proposed Architecture: The Vision

The proposed architecture is founded on the core security principle of separating authentication ("who you are") from authorization ("what you can do").

### 2.1. Core Principles

> **NOTE**: The revised architecture modifies these principles. See `ZZNET_AUTH_ARCHITECTURE_REVISED.md` for the current approach.

- **Authenticate Identity, Not Role:** The system's first step is to cryptographically verify a peer's unique identity. This identity is a `username` or `service-name` (e.g., "collector-us-east-1"), not a coarse-grained role like "Collector". For mTLS, this identity is the **Common Name (CN)** from the peer's client certificate.
  - **Revised**: Identity comes from **CN (role) + SAN (username)** in the certificate. Services use `SAN=DNS:root`.

- **Authorize Based on Policy:** Once a peer's identity is authenticated, its permissions are determined by looking up that identity in a defined policy. The peer does not get to declare its own permissions.
  - **Revised**: Application layer performs authorization using allow-lists. Role-based permissions, not fine-grained permission strings.

- **Distributed Policy Management:** The "policy" that maps identities to permissions is managed in local configuration files on each node. This avoids a dependency on a central policy server, at the cost of requiring careful configuration management.
  - **Revised**: Centralized primarily in database server. Other services only need minimal config (just the database address).

### 2.2. The Three Key Mappings

> **NOTE**: The revised architecture simplifies this to a two-tier model. The three-tier approach was deemed over-engineered for zzping's needs.

This original model was built on three distinct but related mappings:

1.  **Identity -> Role (`username -> [Role]`):** This is the primary policy lookup. Each service maintains a local configuration file that maps the expected `username`s of its peers to a logical `Role` (e.g., `ClientAdmin`, `Collector`). This acts as an allow-list.
    -   A `Database` server's config would list all authorized client and collector `username`s.
    -   A `Collector` client's config would list the `username` of the `Database` it connects to.
    - **Revised**: Database maintains allow-list of full identities (`username@role` or `role` for services). Other services trust the CA and don't need extensive config.

2.  **Role -> Permissions (`Role -> [Permission]`):** A `Role` is simply a named collection of granular permission strings. This mapping defines what a given role is allowed to do.
    -   *Example:* The `ClientAdmin` role might map to permissions like `["intentconfig:read", "intentconfig:write", "database:shutdown"]`.
    - **Revised**: Roles directly have methods like `can_connect_to()` and `can_access_room()`. No intermediate permission strings needed for zzping's use case.

3.  **Component-Defined Permissions:** Application components (like `zzintent-config`) are decoupled from the auth system. They don't know about roles; they only care about specific, fine-grained permission strings. When an action is requested, the component requires the caller to have a specific permission (e.g., `intentconfig:write`).
    - **Revised**: Components check roles directly. Fine-grained permission strings were over-engineering. Components can be made reusable by accepting generic role types via traits if needed in the future.

### 2.3. The Authentication & Authorization Flow

> **NOTE**: The revised architecture significantly simplifies this flow and moves auth decisions to the application layer.

This is how the pieces were originally envisioned to fit together during a connection:

1.  **mTLS Handshake:** A client connects to a server. The `zznet-transport-tcp` layer performs a mutual TLS handshake.
2.  **Identity Extraction:** The transport layer on the server side successfully verifies the client's certificate and extracts its `username` from the certificate's Common Name.
    - **Revised**: Extracts both CN (role) and SAN (username) from the certificate. Format: `PeerIdentity { cn, san_username, peer_addr }`.
3.  **Identity Plumbing:** The transport passes this verified `username` up to the `zznet-hello` layer's `HelloActor`.
    - **Revised**: Transport provides `peer_identity()` method. `HelloActor` passes `PeerIdentity` to application via `HandshakeComplete` message. **No auth decisions in zznet layers**.
4.  **Policy Lookup:** The `HelloActor` consults a local **`PolicyManager`** component. It asks, "What are the permissions for the authenticated user `'<username>'`?"
    - **Revised**: Application layer (e.g., `main.rs` or `SessionManager`) receives `HandshakeComplete` with `PeerIdentity`. Application's `AclManager` checks allow-list and resolves to a canonical `Role` (string/newtype); mapping to app-specific enums is performed explicitly by applications.
5.  **Permission Resolution:** The `PolicyManager` uses its loaded configuration to perform the two-step mapping:
    a. It finds the `username` in its `username -> Role` map.
    b. It finds that `Role` in its `Role -> [Permission]` map.
    c. It returns a `HashSet<String>` of all resolved permissions to the `HelloActor`.
    - **Revised**: Single-step: `PeerIdentity` → check allow-list → resolve CN to `Role` (string/newtype). No permission strings required at the core.
6.  **Session Creation:** The `HelloActor` informs the `SessionManager` that the handshake is complete. The `PeerSession` is created and now stores the `HashSet` of permissions for this authenticated peer for the duration of the session.
    - **Revised**: Application creates `PeerSession` with resolved `Role` and optional application-side permission mapping. Role-based checks are enforced by application-specific logic.
7.  **Permission Enforcement:** Later, when the peer attempts an action (e.g., sending a message to the `intentconfig` room to change configuration), the `PeerSession` checks if the peer's stored permission set contains the required permission (e.g., `"intentconfig:write"`). If it does, the action proceeds. If not, it is denied.
    - **Revised**: Components check `role.can_access_room(room_name)` or similar methods. Direct role-based checks, not string permissions.

### 2.4. Insecure/Fallback Mode (Raw TCP)

For trusted environments (e.g., local development), the system must support a non-mTLS mode.

- In this mode, the transport layer cannot provide a verified identity.
- The `HelloActor` must be **explicitly configured** to operate in an "insecure" mode.
- In this mode only, it will trust the role or username claimed by the peer in the `HELLO` protocol message and resolve permissions based on that claimed identity.
- If not explicitly configured for insecure mode, the `HelloActor` must reject any connection that does not provide a verifiable identity.

---

## 3. Analysis of the Current Codebase

### 3.1. What is Aligned

The current codebase has several excellent foundational elements that align well with this vision.

- **mTLS Foundation (`zznet-transport-tcp`):** The transport layer is already built with `rustls` and has full support for loading role-based certificates and performing mutual authentication.
- **Role Enum (`zznet-hello`):** The `AuthRole` enum (`Collector`, `Database`, `ClientAdmin`, `ClientRo`) provides a perfect starting point for the logical roles in the policy mapping.
- **ACL Stubs (`zznet-hello`):** The `can_connect_to` and `can_access_room` functions, while currently hardcoded, demonstrate that the architecture anticipates the need for ACL enforcement at the connection and room level.

### 3.2. Deficiencies and Gaps

Despite the strong foundation, the current implementation has critical deficiencies that prevent the vision from being realized.

- **Primary Deficiency: No Identity Plumbing:** ✅ **CONFIRMED - ACCURATE** The verified identity (`username`) from the mTLS certificate is not passed up from the transport layer. The `TransportConnection` trait is missing a method to expose this crucial information, effectively decoupling the secure transport from the application logic.
  - **Solution**: Add `peer_identity()` method to `TransportConnection` trait that returns `PeerIdentity { cn, san_username, peer_addr }`.

- **Secondary Deficiency: No Policy Management:** ✅ **CONFIRMED - ACCURATE** The system completely lacks the concept of a `PolicyManager`. There is no mechanism to load, map, or query `username -> role -> permission` policies. Authorization was historically hardcoded into typed app enums (e.g., `AuthRole`) and needs a cleaner, centralized `AclManager` in application code.
  - **Solution**: Create `AclManager` in application layer (not zznet) that loads allow-lists from TOML config. Simpler than originally proposed - just allow-list checking, no permission strings.

- **Tertiary Deficiency: Insecure Trust Model:** ✅ **CONFIRMED - ACCURATE** The `HelloActor` currently trusts the `AuthRole` that a peer declares in its `HELLO` message. This is fundamentally insecure for the mTLS workflow, as it allows a peer to lie about its role, but it coincidentally matches the requirements for the "insecure" fallback mode.
  - **Solution**: In TLS mode, ignore HELLO role field (keep for backward compat / debug info). Use certificate identity only. In raw TCP mode, require explicit `insecure_trust_hello = true` config flag and trust HELLO claim.

---

## 4. Key Challenges

> **NOTE**: These challenges were addressed in the revised architecture.

Implementing this architecture will involve solving several key challenges:

1.  **Transport Trait Design:** Extending the `TransportConnection` trait to expose peer identity must be done carefully to maintain transport-agnosticism. The identity type should be flexible enough to support mTLS (`username`), insecure modes, and potentially future transport mechanisms (e.g., OAuth tokens).
    - **Resolved**: `PeerIdentity` struct with `cn` + `san_username` + `peer_addr`. Returns `Option<PeerIdentity>` (None for raw TCP). Simple and transport-agnostic.

2.  **Policy Configuration:** A clear, human-readable, and robust format for the local policy files needs to be designed. This includes defining the `username -> role` and `role -> permission` sections.
    - **Resolved**: TOML format with `[acl]` section. Database has `allowed_peers = ["username@role", "role"]` list. No role→permission mapping needed (simplified to role methods).

3.  **Operational Overhead:** The distributed policy model introduces an operational challenge: **configuration drift**. Managing identity and permission mappings across many separate files is error-prone and requires robust deployment and configuration management tooling (e.g., Ansible, Salt, etc.) to prevent inconsistencies.
    - **Resolved**: **Not actually distributed.** Only the database maintains the allow-list. Collectors/clients just need database address and their own cert. Much simpler operational model.

4.  **Secure-by-Default Logic:** The implementation must be secure by default. The "insecure" mode must be an explicit, non-default configuration choice. The system should fail-closed, rejecting connections it cannot securely authenticate according to its configuration.
    - **Resolved**: `insecure_trust_hello = false` by default. Service refuses to start in insecure mode without explicit config flag. Logs prominent WARNING when insecure mode is enabled.

## 5. Conclusion

> **This analysis led to the revised architecture in `ZZNET_AUTH_ARCHITECTURE_REVISED.md`.**

The proposed identity-based architecture is a significant step forward, enabling a far more secure, scalable, and flexible system than the current hardcoded role model. The existing codebase provides a strong foundation for the transport and a good structural outline for the `hello` and `session` layers.

**Key Findings from This Analysis:**
1. ✅ **Identity plumbing is missing** - Confirmed and will be addressed
2. ✅ **No policy management** - Confirmed and will be addressed (simplified from original vision)
3. ✅ **HELLO message trusted insecurely** - Confirmed and will be addressed
4. ⚠️ **Original three-tier model was over-engineered** - Simplified in revision
5. ⚠️ **Distributed policy was operational complexity** - Centralized in revision

**Next Steps:**
- See `ZZNET_AUTH_ARCHITECTURE_REVISED.md` for the final architecture
- Implementation plan is documented in the revised document
- This document serves as historical context for architectural decisions

---

## Appendix: Certificate Security & Storage

> **Added after revision** - Addressing AI agent access concerns

### Production Certificate Storage

**Problem**: Storing production certificates in the workspace risks accidental exposure:
- AI agents may read them during debugging
- Risk of accidentally committing to version control
- Data could be sent to LLM providers

**Recommended Practices:**

1. **Store outside workspace:**
   ```bash
   # Production certs in home directory
   ~/.zzping/certs/
       ca.pem
       database.pem
       database.key

   # Application points to external location
   # zzping-database.toml
   [network]
   certs_dir = "~/.zzping/certs"
   ```

2. **Use environment variables:**
   ```bash
   export ZZPING_CERT_DIR="/secure/path/certs"

   # Application reads from env var
   let cert_dir = env::var("ZZPING_CERT_DIR")
       .unwrap_or_else(|_| "certs".to_string());
   ```

3. **`.gitignore` production certs:**
   ```gitignore
   # Already in .gitignore
   certs/

   # But still risky if AI agent can read workspace
   ```

4. **Test certs in repo (acceptable):**
   ```
   test_certs/  ← Checked into repo, clearly marked as test-only
   certs/       ← .gitignore'd, but still in workspace (avoid)
   ```

**Best Practice for zzping:**
- Keep `test_certs/` in repo for development (clearly marked unsafe)
- Document that production certs should be in `~/.zzping/certs/` or similar
- Update deployment docs to use external paths
- Consider prompting user on first run if certs are in workspace location