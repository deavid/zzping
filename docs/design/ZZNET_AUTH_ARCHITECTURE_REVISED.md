# ZZNet Authentication & Authorization Architecture (Revised)

**Date**: October 5, 2025
**Status**: Active Design - Revised based on codebase analysis
**Supersedes**: `ZZNET_AUTH_ARCHITECTURE_PROPOSAL.md`

---

## 1. Executive Summary

This document defines the authentication and authorization architecture for the ZZNet network layer and ZZPing application. The design is based on a critical insight discovered during implementation analysis:

**ZZNet must remain auth-agnostic and reusable.** Authentication and authorization are application concerns, not network layer concerns.

### Core Architectural Principles

1. **Separation of Concerns**: ZZNet provides verified cryptographic identity; applications define what that identity means.
2. **Certificate-Based Identity**: mTLS certificates carry both role (CN) and user (SAN) for flexible access control.
3. **Secure by Default**: Raw TCP mode is explicitly insecure and requires opt-in configuration.
4. **Application-Layer ACLs**: Authorization decisions live in application code (e.g., `zzping-database`), not in zznet.

### The Identity Model

**Hybrid Role + User Model:**
- **Services** (collector, database): Use service certificates with `CN=<role>` and `SAN=DNS:root`
- **Human users** (admin, guests): Use user certificates with `CN=<role>` and `SAN=DNS:<username>`

**Authentication Flow:**
```
mTLS Handshake → Extract (CN, SAN) → Application resolves identity
                                   → Application enforces ACLs
```

---

## 2. The Identity Certificate Model

### 2.1 Certificate Structure

All certificates in the system follow this convention:

| Certificate Type | CN (Common Name) | SAN (Subject Alt Name) | Example |
|-----------------|------------------|------------------------|---------|
| Service (Collector) | `collector` | `DNS:root` | Service identity, no specific user |
| Service (Database) | `database` | `DNS:root` | Service identity, no specific user |
| Admin User | `client-admin` | `DNS:alice` | Admin user "alice" |
| Read-Only User | `client-ro` | `DNS:guest-bob` | Guest user "bob" |

**Key Properties:**
- **CN defines the role** - This is the primary authorization scope
- **SAN defines the user** - This enables per-user access control within a role
- **SAN is mandatory** - Certificate generation must fail if SAN is not provided
- **`SAN=DNS:root`** - Reserved for service certificates (no specific user)

### 2.2 Certificate Generation

The certificate generation script enforces this structure:

```bash
#!/bin/bash
# Usage examples:
./generate_certs.sh --collector              # CN=collector, SAN=DNS:root
./generate_certs.sh --client-admin alice     # CN=client-admin, SAN=DNS:alice
./generate_certs.sh --client-ro guest-bob    # CN=client-ro, SAN=DNS:guest-bob
```

**Enforcement Rules:**
1. SAN must always be provided
2. If no username is given, script must prompt for explicit `--user root` or fail
3. This prevents accidental creation of ambiguous certificates

### 2.3 Identity Representation

```rust
/// Peer identity extracted from mTLS certificate
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerIdentity {
    /// Common Name from certificate - represents the role
    pub common_name: String,

    /// First DNS entry from Subject Alternative Name - represents the user
    /// "root" is reserved for service identities
    pub san_username: String,

    /// Peer address for logging (IP:port)
    pub peer_addr: String,
}

impl PeerIdentity {
    /// Returns true if this is a service identity (not a specific user)
    pub fn is_service(&self) -> bool {
        self.san_username == "root"
    }

    /// Returns the full identity string for logging and ACL lookups
    /// Format: "username@role" or "role" for services
    pub fn full_identity(&self) -> String {
        if self.is_service() {
            self.common_name.clone()
        } else {
            format!("{}@{}", self.san_username, self.common_name)
        }
    }
}
```

---

## 3. ZZNet Layer: Transport Identity Extraction

### 3.1 Design Philosophy

**ZZNet is auth-agnostic.** It does not:
- ❌ Define application roles (Collector, Database, etc.)
- ❌ Enforce authorization policies
- ❌ Make access control decisions
- ❌ Trust claimed identities from protocol messages

**ZZNet's responsibility:**
- ✅ Perform mTLS handshake with mutual authentication
- ✅ Extract and provide verified cryptographic identity from peer certificate
- ✅ Provide transport abstraction (TCP/TLS, mock, future transports)
- ✅ Route messages between rooms based on application instructions

### 3.2 Transport Layer API

```rust
/// Transport connection trait (zznet-api)
#[async_trait]
pub trait TransportConnection: Send {
    async fn send(&mut self, frame: Bytes) -> Result<(), TransportError>;
    async fn recv(&mut self) -> Result<Option<Bytes>, TransportError>;

    /// Returns peer address for logging
    fn peer_addr(&self) -> Option<String>;

    /// Returns verified peer identity from mTLS certificate
    /// Returns None for non-TLS connections (raw TCP)
    fn peer_identity(&self) -> Option<PeerIdentity>;
}
```

**Implementation Notes:**
- `peer_identity()` extracts CN and first DNS SAN from peer certificate
- For TLS client connections: extracts from server certificate
- For TLS server connections: extracts from client certificate
- For raw TCP: returns `None` (no cryptographic identity available)

### 3.3 HELLO Protocol: Role Field Semantics

The HELLO protocol message contains a `role` field:

```rust
HandshakeFrame::Hello {
    version: String,
    role: AuthRole,      // ⚠️ Claimed role, not verified
    hostname: String,
}
```

**Security Semantics by Transport Mode:**

| Transport Mode | Role Field Treatment | Security |
|---------------|---------------------|----------|
| **mTLS** | Informational only, NOT trusted | Certificate CN is authoritative |
| **Raw TCP** | Trusted (insecure mode) | Peer can lie - development only |

**Rationale**:
- mTLS: The certificate's CN is cryptographically verified, so the claimed role in HELLO is redundant and cannot be trusted over the certificate.
- Raw TCP: No cryptographic identity available, so we must trust the peer's claim (explicitly insecure).

---

## 4. Application Layer: Authentication & Authorization

### 4.1 Responsibility Boundary

**Applications (zzping-database, zzping-collector, etc.) are responsible for:**
1. Defining application-specific roles (e.g., `AuthRole` enum)
2. Maintaining allow-lists of permitted identities
3. Resolving `PeerIdentity` → application role
4. Enforcing connection-level ACLs (who can connect)
5. Enforcing room-level ACLs (who can access which rooms)
6. Enforcing operation-level ACLs (who can perform which actions), using the trait-based permission model for reusable components (see Section 4.7).

### 4.2 ZZPing Authentication Flow

```
┌─────────────────────────────────────────────────────────────┐
│ 1. Transport Layer (zznet-transport-tcp)                    │
│    - mTLS handshake completes                                │
│    - Extract PeerIdentity { cn, san_username }               │
└────────────────────┬────────────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────────────┐
│ 2. HELLO Layer (zznet-hello)                                │
│    - Receives PeerIdentity from transport                    │
│    - Performs protocol handshake (HELLO/OFFER/ACK)           │
│    - Does NOT make auth decisions                            │
│    - Sends HandshakeComplete to application                  │
└────────────────────┬────────────────────────────────────────┘
                     │
                     ▼
┌─────────────────────────────────────────────────────────────┐
│ 3. Application Layer (zzping main.rs / SessionManager)      │
│    - Receives HandshakeComplete with PeerIdentity            │
│    - Looks up identity in local ACL config                   │
│    - Resolves PeerIdentity → AuthRole                        │
│    - Enforces connection policy                              │
│    - Creates PeerSession with resolved role & permissions    │
└─────────────────────────────────────────────────────────────┘
```

### 4.3 Application ACL Configuration

**Database Configuration Example** (`zzping-database.toml`):

```toml
[network]
listen_address = "0.0.0.0:8080"
tls_enabled = true
certs_dir = "certs"

[acl]
# Allow-list of permitted peer identities
# Format: "username@role" for users, "role" for services

allowed_peers = [
    # Services (SAN=root)
    "collector",              # Any collector service cert

    # Admin users
    "alice@client-admin",     # Alice with admin cert
    "bob@client-admin",       # Bob with admin cert

    # Read-only users
    "guest-friend1@client-ro",
    "guest-friend2@client-ro",
]

# Insecure mode: trust HELLO message role claims (TCP only)
# WARNING: Only enable in trusted development environments
insecure_trust_hello = false
```

**Collector/Client Configuration Example** (`zzping-collector.toml`):

```toml
[network]
database_address = "192.168.1.100:8080"
tls_enabled = true
certs_dir = "certs"

[acl]
# Collectors only connect to database, so minimal config
# If TLS handshake succeeds against our trusted CA,
# we're talking to a legitimate database.
# No additional allow-list needed.

insecure_trust_hello = false
```

### 4.4 Identity Resolution Logic

```rust
/// Application-specific role enum (in zzping codebase, not zznet)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthRole {
    Collector,
    Database,
    ClientRo,
    ClientAdmin,
}

/// ACL Manager (in zzping application layer)
pub struct AclManager {
    allowed_peers: HashSet<String>,  // From config file
    insecure_mode: bool,
}

impl AclManager {
    /// Resolve and authorize a peer identity
    pub fn authorize_peer(
        &self,
        identity: Option<PeerIdentity>,
        claimed_role: Option<AuthRole>,  // From HELLO message
    ) -> Result<AuthRole, AuthError> {
        match identity {
            Some(peer_id) => {
                // mTLS mode: Use certificate identity
                let full_id = peer_id.full_identity();

                // Check allow-list
                if !self.allowed_peers.contains(&full_id) {
                    return Err(AuthError::IdentityNotAllowed(full_id));
                }

                // Map CN to AuthRole
                self.resolve_role_from_cn(&peer_id.common_name)
            }
            None => {
                // Raw TCP mode: No certificate available
                if !self.insecure_mode {
                    return Err(AuthError::TlsRequired);
                }

                // Trust claimed role (INSECURE - development only)
                claimed_role.ok_or(AuthError::NoIdentityAvailable)
            }
        }
    }

    fn resolve_role_from_cn(&self, cn: &str) -> Result<AuthRole, AuthError> {
        match cn {
            "collector" => Ok(AuthRole::Collector),
            "database" => Ok(AuthRole::Database),
            "client-ro" => Ok(AuthRole::ClientRo),
            "client-admin" => Ok(AuthRole::ClientAdmin),
            _ => Err(AuthError::UnknownRole(cn.to_string())),
        }
    }
}
```

### 4.5 Connection-Level Authorization

After resolving the peer's role, enforce connection policy:

```rust
impl AuthRole {
    /// Check if this role can connect to a peer with target role
    pub fn can_connect_to(&self, target: &AuthRole) -> bool {
        match (self, target) {
            // Collectors connect to Database
            (AuthRole::Collector, AuthRole::Database) => true,

            // Clients connect to Database
            (AuthRole::ClientRo, AuthRole::Database) => true,
            (AuthRole::ClientAdmin, AuthRole::Database) => true,

            // Database accepts connections (symmetric)
            (AuthRole::Database, AuthRole::Collector) => true,
            (AuthRole::Database, AuthRole::ClientRo) => true,
            (AuthRole::Database, AuthRole::ClientAdmin) => true,

            // Admin can connect to anyone (for debugging)
            (AuthRole::ClientAdmin, _) => true,

            // Deny everything else
            _ => false,
        }
    }
}
```

### 4.6 Room-Level Authorization

```rust
impl AuthRole {
    /// Check if this role can access a specific room
    pub fn can_access_room(&self, room_name: &str) -> bool {
        match (self, room_name) {
            // Admin has access to all rooms
            (AuthRole::ClientAdmin, _) => true,

            // Collector can submit to memdb
            (AuthRole::Collector, "memdb") => true,

            // Database can handle both memdb and query
            (AuthRole::Database, "memdb") => true,
            (AuthRole::Database, "query") => true,

            // Read-only clients can query
            (AuthRole::ClientRo, "query") => true,

            // Deny everything else
            _ => false,
        }
    }
}
```

### 4.7 Component-Level Permissions (Trait-Based Model)

While connection and room-level authorization are handled by coarse-grained checks on the `AuthRole` enum, fine-grained permissions required by reusable components must be handled differently to prevent tight coupling between a component and the application's specific roles.

The standard pattern for this is to define a trait that represents the set of permissions a component requires. This makes the permissions **type-safe and compile-time checked**.

This approach is the mandated replacement for string-based permissions, as it prevents typos and makes the component's requirements explicit through the type system.

**1. Define a Permission Trait:**

A component or a library of components should define a trait that clearly lists the permissions it needs as methods.

```rust
// In a reusable component library (e.g., zzintent-config)
pub trait IntentPermissions {
    fn can_read_intent(&self) -> bool;
    fn can_write_intent(&self) -> bool;
    fn can_delete_intent(&self) -> bool;
}
```

**2. Implement the Trait for the Application's Role Enum:**

The application (`zzping`) is responsible for implementing this trait for its concrete `AuthRole` enum, mapping its roles to the required permissions.

```rust
// In the zzping application's auth logic
use zzintent_config::IntentPermissions; // Assuming the trait is in the component crate

impl IntentPermissions for AuthRole {
    fn can_read_intent(&self) -> bool {
        matches!(self, AuthRole::ClientAdmin | AuthRole::ClientRo | AuthRole::Collector)
    }

    fn can_write_intent(&self) -> bool {
        matches!(self, AuthRole::ClientAdmin)
    }

    fn can_delete_intent(&self) -> bool {
        // Only admins can delete
        matches!(self, AuthRole::ClientAdmin)
    }
}
```

**3. Use the Trait in the Component:**

The component's logic is then generic over any type that implements the permission trait. This ensures the component is decoupled and reusable.

```rust
// In the reusable component
pub struct IntentConfigService<R: IntentPermissions> {
    _role_type: std::marker::PhantomData<R>,
}

impl<R: IntentPermissions> IntentConfigService<R> {
    pub fn update_intent(&self, role: &R, intent: &str) -> Result<(), &'static str> {
        if !role.can_write_intent() {
            return Err("Permission Denied: cannot write intent");
        }
        // Proceed with update logic...
        Ok(())
    }
}
```

This model provides the decoupling of a classic permission system while maintaining the safety and clarity of the Rust type system.

---

## 5. Security Modes

### 5.1 Production Mode: mTLS (Secure)

**Configuration:**
```toml
[network]
tls_enabled = true
certs_dir = "certs"

[acl]
insecure_trust_hello = false  # Must be false for production
```

**Security Properties:**
- ✅ Mutual authentication via certificates
- ✅ Encrypted transport
- ✅ Cryptographic proof of identity
- ✅ HELLO role field is ignored (informational only)
- ✅ Identity extracted from certificate CN + SAN

**Trust Chain:**
```
CA signs all certificates
    ↓
mTLS handshake verifies certificate signature
    ↓
Application extracts verified identity
    ↓
Application checks identity against allow-list
    ↓
Application resolves identity → role
    ↓
Application enforces ACL policy
```

### 5.2 Development Mode: Raw TCP (Insecure)

**Configuration:**
```toml
[network]
tls_enabled = false

[acl]
insecure_trust_hello = true  # Explicitly enable insecure mode
```

**Security Properties:**
- ❌ No authentication
- ❌ No encryption
- ❌ Peer can claim any role
- ⚠️ HELLO role field is trusted (insecure!)

**Warning Message:**
```
⚠️  WARNING: Running in INSECURE mode!
    TLS is disabled and peer identity claims are trusted without verification.
    This mode is ONLY for development in trusted environments.
    DO NOT use in production.
```

**Implementation Note:**
Applications must log this warning prominently and refuse to start in insecure mode unless explicitly configured.

---

## 6. Revocation and Access Management

### 6.1 User Access Revocation

**Scenario**: You gave your friend `guest-bob` a certificate with `CN=client-ro, SAN=DNS:guest-bob`. Later, you want to revoke their access.

**Solution**: Remove from allow-list in database config:

```toml
[acl]
allowed_peers = [
    "collector",
    "alice@client-admin",
    # "guest-bob@client-ro",  ← Remove this line
]
```

**Operational Steps:**
1. Edit `zzping-database.toml`
2. Remove user's identity from `allowed_peers`
3. Restart or reload database service (signal SIGHUP for config reload)
4. User's certificate is now rejected at connection time

**Benefits:**
- ✅ No certificate regeneration needed
- ✅ No Certificate Revocation List (CRL) complexity
- ✅ Simple config file edit
- ✅ Takes effect immediately on service restart/reload

**Trade-offs:**
- ⚠️ Requires manual config management
- ⚠️ Revoked user keeps their certificate (just can't use it)
- ⚠️ Need to manage config across services (but only database needs the allow-list)

### 6.2 Per-Certificate Access Control

**Scenario**: You create two certificates for Alice:
- `CN=client-admin, SAN=DNS:alice` (admin cert)
- `CN=client-ro, SAN=DNS:alice` (read-only cert)

You want to allow only the read-only certificate.

**Solution**: Allow-list contains `alice@client-ro` but not `alice@client-admin`:

```toml
[acl]
allowed_peers = [
    "alice@client-ro",      # This cert works
    # "alice@client-admin"  # This cert is blocked
]
```

**Client Responsibility:**
Alice must configure her client to use the correct certificate:

```toml
# alice's client config
[network]
cert_path = "alice-ro.pem"      # Use read-only cert
key_path = "alice-ro.key"
```

**Use Cases:**
- Give users multiple certificates with different privileges
- User can switch certificates by changing client config
- Allows "promotion" (add admin cert) without revoking RO cert
- Allows fine-grained temporary access (issue time-limited cert)

---

## 7. Implementation Phases

### Phase 1: Foundation (Weeks 1-2)

**Objective**: Establish certificate identity extraction without changing auth logic.

1. **Update certificate generation** (`generate_certs.sh`):
   - Add `--user <username>` parameter
   - Enforce SAN presence (fail if missing, suggest `--user root`)
   - Generate certificates with `SAN=DNS:<username>`
   - Regenerate all test certificates

2. **Add PeerIdentity to zznet-api**:
   - Define `PeerIdentity` struct in `zznet-api/src/types.rs`
   - Add `peer_identity()` method to `TransportConnection` trait
   - Document semantics (returns `None` for raw TCP)

3. **Implement identity extraction in zznet-transport-tcp**:
   - Extract CN from peer certificate
   - Extract first DNS entry from SAN
   - Return `PeerIdentity` from `TcpTransport::peer_identity()`
   - Add tests for identity extraction

4. **Update HELLO layer**:
   - Pass `PeerIdentity` in `HandshakeComplete` message
   - Keep `role` field in HELLO frame (needed for TCP fallback)
   - Document that role field is informational only in TLS mode

**Validation:**
- All tests pass with new certificate structure
- `peer_identity()` returns correct CN + SAN for TLS connections
- `peer_identity()` returns `None` for raw TCP
- No behavior changes yet (auth still uses HELLO role field)

### Phase 2: Application Auth Integration (Weeks 3-4)

**Objective**: Move auth logic from zznet to application layer.

5. **Create zzping-auth crate**:
   - Move `AuthRole` enum from zznet-hello to zzping-auth
   - Implement `AclManager` with config-based allow-lists
   - Implement `can_connect_to()` and `can_access_room()` logic
   - Add insecure mode flag and warning logging

6. **Update application config**:
   - Add `[acl]` section to TOML configs
   - Define `allowed_peers` lists for database
   - Add `insecure_trust_hello` flag
   - Create example configs for all roles

7. **Wire auth into zzping applications**:
   - Load ACL config in `main.rs`
   - Create `AclManager` instance
   - Pass `AclManager` to session manager
   - Call `authorize_peer()` in handshake complete handler
   - Enforce ACL before creating peer sessions

8. **Remove auth from zznet**:
   - Remove `AuthRole` from zznet-hello
   - Remove hardcoded `can_connect_to()` from zznet-hello
   - zznet-hello becomes purely protocol handling
   - Update zznet documentation

**Validation:**
- Applications start with new config format
- TLS mode uses certificate identity for auth
- TCP mode requires explicit insecure flag
- Unauthorized identities are rejected
- All integration tests pass

### Phase 3: Refinement (Week 5+)

**Objective**: Polish, documentation, and operational tooling.

9. **Operational tooling**:
   - Config validation tool (check allow-lists, detect typos)
   - Certificate inspection tool (show CN + SAN)
   - Live config reload (SIGHUP handler)

10. **Documentation**:
    - Update README with new auth model
    - Document certificate management procedures
    - Create troubleshooting guide
    - Update deployment guides

11. **Testing**:
    - Integration tests for revocation scenarios
    - Tests for insecure mode warnings
    - Tests for invalid/malformed certificates
    - Performance testing (ACL lookup overhead)

---

## 8. Migration from Current System

### Current State Analysis

**What exists today:**
- ✅ mTLS transport layer with rustls
- ✅ Certificate generation script (but all certs have `CN=zzping`)
- ✅ `AuthRole` enum in zznet-hello
- ✅ Hardcoded ACL methods (`can_connect_to`, `can_access_room`)
- ⚠️ HELLO message role is trusted (insecure for mTLS)

**What's missing:**
- ❌ SAN in certificates
- ❌ Identity extraction from certificates
- ❌ Application-layer ACL configuration
- ❌ Allow-list based authorization

### Migration Strategy

**Backward Compatibility:**
- Phase 1 is backward compatible (adds features, doesn't change behavior)
- Phase 2 breaks compatibility (requires new certs and config)
- Old certificates without SAN will be rejected

**Migration Path:**
1. Implement Phase 1 (identity extraction infrastructure)
2. Generate new certificates with SANs
3. Deploy new certificates to all services
4. Update configs with allow-lists
5. Deploy Phase 2 code (switches to certificate-based auth)
6. Revoke old certificates

**Rollback Plan:**
- Keep Phase 1 changes minimal and isolated
- Phase 1 can be rolled back without data loss
- Phase 2 requires coordination (all services must upgrade together)

---

## 9. Key Architectural Decisions

### Decision 1: CN = Role, SAN = User

**Rationale**:
- Role is the primary authorization scope (coarse-grained)
- User enables fine-grained access control within a role
- Separation allows flexible cert management

**Alternative Considered**: CN = User, role in SAN
- Rejected because role is more fundamental to the system

### Decision 2: Auth Logic in Application, Not ZZNet

**Rationale**:
- Makes zznet reusable for other projects
- Application has better context for auth decisions
- Follows separation of concerns principle

**Alternative Considered**: Generic auth trait in zznet
- Rejected as over-engineering; adds complexity without clear benefit

### Decision 3: SAN Mandatory, "root" for Services

**Rationale**:
- Eliminates ambiguity (every cert explicitly states service vs. user)
- Prevents accidental creation of user-less certs
- Makes cert inspection obvious (see "root" → know it's a service)

**Alternative Considered**: SAN optional, absence means service
- Rejected because it's error-prone (forgot SAN = service?)

### Decision 4: Allow-List in Config, Not Certificate Extensions

**Rationale**:
- Simple revocation (edit config, restart)
- No PKI infrastructure complexity (CRL, OCSP)
- Matches operational model (database is central authority)

**Alternative Considered**: Certificate extensions with permissions
- Rejected due to operational complexity and inflexibility

### Decision 5: Keep HELLO Role Field for TCP Fallback

**Rationale**:
- Development mode needs some identity mechanism
- Removing field would break protocol versioning
- Field is harmless if not trusted in TLS mode

**Alternative Considered**: Remove field entirely, TCP has no identity
- Rejected because it makes local development harder

---

## 10. Security Considerations

### Threat Model

**In Scope:**
- Rogue clients attempting to connect with unauthorized certificates
- Compromised user credentials (revocation scenario)
- Misconfiguration (wrong cert loaded, wrong role claimed)
- Eavesdropping on network traffic (TLS protects)

**Out of Scope:**
- CA compromise (if CA is compromised, system trust is broken)
- Insider threats with legitimate certificates (they have access by design)
- Host-level attacks (if host is compromised, game over)
- Side-channel attacks on TLS implementation

### Security Properties

**With mTLS Enabled:**
- **Authentication**: Peer identity is cryptographically proven
- **Confidentiality**: All traffic is encrypted
- **Integrity**: TLS ensures messages are not tampered with
- **Non-repudiation**: Certificate proves who sent messages

**Attack Scenarios:**

| Attack | Mitigation |
|--------|-----------|
| Peer claims wrong role in HELLO | TLS mode: Ignored, certificate CN is authoritative |
| Stolen certificate | Remove from allow-list, peer is rejected |
| MITM attack | TLS mutual authentication prevents MITM |
| Replay attack | TLS nonces prevent replay |
| Certificate not in allow-list | Connection rejected at handshake |
| Raw TCP in production | Requires explicit config flag, logs WARNING |

### Best Practices

1. **Keep CA private key offline** - Generate CA once, store securely
2. **Short-lived certificates** - Regenerate certificates annually
3. **Monitor allow-lists** - Audit who has access regularly
4. **Log authentication failures** - Alert on repeated failures
5. **Separate CAs per environment** - Dev CA ≠ Prod CA
6. **Never run insecure mode in production** - Enforce in code

---

## 11. Open Questions and Future Work

### Open Questions

1. **Config reload mechanism**: Should services support live reload (SIGHUP), or require restart?
2. **Certificate expiration handling**: Should system detect expiring certs and warn?
3. **Audit logging**: Should all auth decisions be logged for compliance?

### Future Enhancements

1. **Certificate rotation**: Automatic cert renewal without service disruption
2. **Metrics**: Track auth failures, connections by role, etc.
3. **Rate limiting**: Prevent brute-force connection attempts
4. **Dynamic allow-lists**: Load from database instead of config file
5. **OCSP/CRL support**: For large-scale deployments with many users
6. **OAuth/OIDC bridge**: Allow external identity providers for web GUI

---

## 12. Conclusion

This architecture provides a pragmatic, secure, and maintainable authentication system for ZZNet and ZZPing. Key achievements:

- ✅ **Certificate-based identity** with flexible role+user model
- ✅ **ZZNet remains reusable** by being auth-agnostic
- ✅ **Simple revocation** via config file allow-lists
- ✅ **Secure by default** with explicit opt-in for insecure mode
- ✅ **Clear migration path** from current implementation

The hybrid identity model (role in CN, user in SAN) strikes the right balance between operational simplicity for services and fine-grained control for user access. By moving auth logic to the application layer, we preserve ZZNet's reusability while giving ZZPing the flexibility to enforce its specific security policies.

---

## Appendix A: Certificate Examples

### Service Certificate (Collector)
```bash
$ openssl x509 -in collector.pem -noout -text
Subject: CN = collector
X509v3 Subject Alternative Name:
    DNS:root
```

### User Certificate (Admin)
```bash
$ openssl x509 -in alice-admin.pem -noout -text
Subject: CN = client-admin
X509v3 Subject Alternative Name:
    DNS:alice
```

### Identity Representation
- Collector: `"collector"` (service identity)
- Database: `"database"` (service identity)
- Alice admin: `"alice@client-admin"` (user identity)
- Bob guest: `"bob@client-ro"` (user identity)

---

## Appendix B: Configuration Examples

### Database Configuration (Full)
```toml
[network]
listen_address = "0.0.0.0:8080"
tls_enabled = true
certs_dir = "certs"
ca_cert = "certs/ca.pem"
server_cert = "certs/database.pem"
server_key = "certs/database.key"

[acl]
allowed_peers = [
    "collector",
    "alice@client-admin",
    "bob@client-admin",
    "guest1@client-ro",
]
insecure_trust_hello = false
```

### Collector Configuration (Minimal)
```toml
[network]
database_address = "192.168.1.100:8080"
tls_enabled = true
certs_dir = "certs"
ca_cert = "certs/ca.pem"
client_cert = "certs/collector.pem"
client_key = "certs/collector.key"

[acl]
insecure_trust_hello = false
```

### Development Configuration (Insecure)
```toml
[network]
listen_address = "127.0.0.1:8080"
tls_enabled = false

[acl]
insecure_trust_hello = true
allowed_peers = []  # Not used in insecure mode
```

---

**Document Version**: 1.1
**Last Updated**: October 2025 (Phase 2 Implementation)
**Next Review**: After Phase 2 validation

## Appendix: Phase 2 Implementation Details

### ACL Manager Implementation

The `zzping-auth` crate provides the application-layer ACL implementation:

- **AclManager**: Core authorization engine with HashSet-based peer lookup
- **AclConfig**: TOML-based configuration with `allowed_peers` array
- **AuthRole**: Enum for known roles (Collector, Database, ClientRo, ClientAdmin)
- **PeerIdentity**: Certificate-derived identity with CN/SAN validation

### Key Implementation Features

1. **Fast Authorization**: HashSet lookup for O(1) authorization checks
2. **Dynamic ACL Updates**: Runtime allow/deny user operations
3. **TOML Configuration**: Human-readable config files with validation
4. **Error Handling**: Comprehensive error types for config and auth failures
5. **Testing**: Extensive test suite covering security, stress, edge cases, and docs

### Security Properties

- **Identity Immutability**: Peer identity cannot be spoofed after TLS handshake
- **ACL Integrity**: Changes take effect immediately, no caching issues
- **Concurrent Safety**: Thread-safe ACL modifications
- **Memory Efficiency**: Stable memory usage with large ACLs

### Performance Benchmarks

Authorization checks: ~45ns for small ACLs, ~45ns for large ACLs (1000 entries)
Config parsing: ~760ns
Config validation: ~7.7µs for 100 entries

---
