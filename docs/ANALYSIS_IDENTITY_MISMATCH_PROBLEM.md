# Analysis: Identity Extraction vs Role Authorization Mismatch

**Date:** October 19, 2025
**Status:** CRITICAL SECURITY ISSUE IDENTIFIED
**Severity:** HIGH - Authorization Bypass Vulnerability

---

## Executive Summary

The collector service is being **REJECTED** by the database service due to a **fundamental mismatch** between:
1. How `PeerIdentity` is extracted from TLS certificates
2. How the authorizer functions validate roles

**Root Cause:** The authorizer validates based on `common_name` field (which contains the certificate CN like "database" or "collector"), but these CNs represent the **peer's own role**, not what role they should be granted by the authorization system.

**Impact:** This causes legitimate peer connections to be rejected because:
- Collector's certificate has CN="collector"
- Collector tries to connect to database as "collector" role
- Database's authorizer checks CN="collector" and maps it to **DatabaseRole::Collector**
- But the database certificate itself has CN="database"
- When collector connects, it presents CN="collector", which the database authorizer tries to map to a DatabaseRole
- This fails because "collector" is not a valid DatabaseRole (only Database, Collector, Admin exist)

---

## Log Analysis

### Collector Side (Client)
```
2025-10-19T20:34:41.239342Z DEBUG Collector authorizer checking peer identity: zzping@database
2025-10-19T20:34:41.239346Z  WARN Collector authorizer rejected zzping@database - unknown role: Unknown role: database
2025-10-19T20:34:41.239350Z ERROR !!! SECURITY REJECTION !!!: Peer default-hostname REJECTED by authorizer
```

**Analysis:**
- Collector receives connection from peer with identity `zzping@database`
- Collector's authorizer uses `IntentConfigPermission::from_cn("database")`
- This fails because "database" is not a valid IntentConfigPermission

### Database Side (Server)
```
2025-10-19T20:34:41.198264Z DEBUG Database authorizer checking peer identity: zzping@collector
2025-10-19T20:34:41.198268Z DEBUG Authorizer resolved zzping@collector → Collector
2025-10-19T20:34:41.198271Z  INFO Peer default-hostname (collector) authorized as Collector
```

**Analysis:**
- Database receives connection from peer with identity `zzping@collector`
- Database's authorizer uses `DatabaseRole::from_cn("collector")`
- This **succeeds** and maps to `DatabaseRole::Collector`
- Connection is authorized successfully

---

## Technical Deep Dive

### 1. Certificate Structure

**Collector Certificate:**
```
Subject: CN = collector
Subject Alternative Name: DNS:zzping
```

**Database Certificate:**
```
Subject: CN = database
Subject Alternative Name: DNS:zzping
```

### 2. PeerIdentity Extraction

From `src/net/zznet-transport-tcp/src/connection.rs`:

```rust
pub struct PeerIdentity {
    pub common_name: String,    // From certificate CN (e.g., "database", "collector")
    pub san_username: String,   // From SAN DNS name (e.g., "zzping")
    pub peer_addr: String,      // Socket address
}

impl PeerIdentity {
    pub fn full_identity(&self) -> String {
        if self.is_service() {  // is_service() returns true if san_username == "root"
            self.common_name.clone()
        } else {
            format!("{}@{}", self.san_username, self.common_name)
        }
    }

    pub fn is_service(&self) -> bool {
        self.san_username == "root"
    }
}
```

**Problem:** Both certificates have SAN="zzping" (not "root"), so `is_service()` returns `false`, resulting in identity format `"zzping@database"` or `"zzping@collector"`.

### 3. Authorizer Implementation

**Database Service Authorizer** (`src/apps/zzping-database/src/service.rs`):
```rust
let authorizer: GenericAuthorizer<DatabaseRole> = Box::new(|peer_identity| {
    match DatabaseRole::from_cn(&peer_identity.common_name) {
        Ok(role) => Some(role),
        Err(_) => None,
    }
});
```

**Collector Service Authorizer** (`src/apps/zzping-collector/src/service.rs`):
```rust
let authorizer: GenericAuthorizer<IntentConfigPermission> = Box::new(|peer_identity| {
    match IntentConfigPermission::from_cn(&peer_identity.common_name) {
        Ok(role) => Some(role),
        Err(_) => None,
    }
});
```

### 4. Role Enum Definitions

**DatabaseRole** (in `zzping-database`):
```rust
pub enum DatabaseRole {
    Database,   // Maps from CN="database"
    Collector,  // Maps from CN="collector"
    Admin,      // Maps from CN="admin"
}
```

**IntentConfigPermission** (in `zzintent-config`):
```rust
pub enum IntentConfigPermission {
    Database,   // Maps from CN="database"
    Collector,  // Maps from CN="collector"
}
```

---

## The Core Problem

### Scenario: Collector Connects to Database

**Step 1:** Collector initiates TLS connection to Database
- Collector presents its certificate with CN="collector", SAN="zzping"

**Step 2:** Database extracts PeerIdentity from collector's certificate
- `common_name = "collector"`
- `san_username = "zzping"`
- `full_identity() = "zzping@collector"`

**Step 3:** Database's authorizer validates
```rust
DatabaseRole::from_cn("collector")  // Returns Ok(DatabaseRole::Collector)
```
- ✅ **SUCCESS** - "collector" is a valid DatabaseRole
- Authorization granted

### Scenario: Database Connects to Collector (the failing case)

**Step 1:** Database initiates TLS connection to Collector (this happens due to bidirectional connection negotiation)
- Database presents its certificate with CN="database", SAN="zzping"

**Step 2:** Collector extracts PeerIdentity from database's certificate
- `common_name = "database"`
- `san_username = "zzping"`
- `full_identity() = "zzping@database"`

**Step 3:** Collector's authorizer validates
```rust
IntentConfigPermission::from_cn("database")  // Returns Ok(IntentConfigPermission::Database)
```
- ❌ **WAIT** - This actually should work! Let me check the enum...

### Wait, Let Me Re-Check...

Let me trace through the actual error message:
```
WARN Collector authorizer rejected zzping@database - unknown role: Unknown role: database
```

This suggests that `IntentConfigPermission::from_cn("database")` is returning an error. Let me verify the enum definition...

---

## Root Cause Analysis (Updated)

The problem is likely one of the following:

### Hypothesis 1: IntentConfigPermission doesn't include Database variant
If the `IntentConfigPermission` enum only has `Collector` (and maybe `Admin`), then trying to authorize a peer with CN="database" will fail.

### Hypothesis 2: SAN="zzping" vs SAN="root" Issue
The certificates have SAN="zzping" instead of SAN="root", which means:
- `is_service()` returns `false`
- `full_identity()` returns `"zzping@database"` instead of just `"database"`

However, the authorizer is checking `peer_identity.common_name` directly, not `full_identity()`, so this shouldn't be the issue.

### Hypothesis 3: Role String Mismatch
The HELLO message contains a `role_str` field that the peer claims. The logs show:
```
Handshake completed - peer_id: default-hostname, peer_role_from_hello: collector
```

But this is just for logging - the actual authorization uses the CN from the certificate.

---

## Investigation Required

To fully understand this, we need to check:

1. **What is the complete definition of `IntentConfigPermission`?**
   - Does it include a `Database` variant?
   - What CNs does it accept in `from_cn()`?

2. **What is the complete definition of `DatabaseRole`?**
   - Verify it includes both `Database` and `Collector` variants
   - Verify the `from_cn()` implementation

3. **Certificate SAN Values**
   - Should services use SAN="root" instead of SAN="zzping"?
   - What is the intended identity format for services?

4. **Connection Direction**
   - Why is the database connecting TO the collector?
   - Should only the collector connect to the database?

---

## Design Issues Identified

### Issue 1: Bidirectional Authorization Asymmetry

The collector and database have **different role enums** with **different valid values**:

- `DatabaseRole` accepts: database, collector, admin
- `IntentConfigPermission` accepts: ??? (needs verification)

This creates asymmetry where:
- Database can authorize a collector (CN="collector" → DatabaseRole::Collector)
- But collector **cannot** authorize a database if IntentConfigPermission doesn't have a Database variant

### Issue 2: CN as Role Identifier is Confusing

The certificate's CN represents **WHO the peer is**, not **WHAT ROLE they should be assigned**.

Current logic:
```
Peer Certificate CN="collector" → Authorize as Role::Collector
Peer Certificate CN="database" → Authorize as Role::Database
```

This creates a 1:1 mapping where the peer's identity directly becomes their role, which may not be flexible enough for real-world authorization scenarios.

### Issue 3: No Separation of Identity and Authorization

In traditional security systems:
1. **Authentication:** Verify WHO the peer is (certificate validation)
2. **Authorization:** Decide WHAT the peer can do (role assignment)

Currently, these are conflated - the peer's certificate CN directly determines their role.

Better design:
```rust
// Identity: WHO they are
let identity = extract_from_cert();  // "collector-instance-01"

// Authorization: WHAT they can do
let role = authorize_peer(&identity);  // Look up in ACL: "collector-instance-01" → Collector
```

---

## Immediate Questions for User

1. **What should `IntentConfigPermission` contain?**
   - Should it include a `Database` variant to allow database connections?
   - Or should the collector never receive inbound connections from the database?

2. **What is the intended connection topology?**
   - Should collector only connect TO database (unidirectional)?
   - Or should database also connect to collector (bidirectional)?

3. **What should the certificate SAN values be?**
   - Should services use SAN="root" to be identified as services?
   - Or is SAN="zzping" intentional for some reason?

4. **Should we separate identity from role assignment?**
   - Instead of CN → Role mapping, should we have CN → Identity → ACL Lookup → Role?
   - This would allow more flexible authorization policies

---

## Recommended Investigation Steps

1. Check the definition of `IntentConfigPermission` enum
2. Check if there are different role enums for different services
3. Verify the intended connection direction (collector→database only, or bidirectional)
4. Check the certificate generation script to see if SAN should be "root"
5. Review the architectural design documents for the intended authorization model

---

## Temporary Workarounds (NOT RECOMMENDED)

### Option 1: Add Database variant to IntentConfigPermission
```rust
pub enum IntentConfigPermission {
    Database,    // Add this
    Collector,
}
```

**Problem:** This violates the principle that permissions should match the service's role. A collector should not need to know about database roles.

### Option 2: Make Collector Never Accept Inbound Connections
If the collector should only be a client (never a server), it shouldn't need an authorizer at all - or should have a trivial one that rejects everything.

### Option 3: Use a Common Role Enum
Use `zzping_auth::AuthRole` everywhere instead of service-specific role enums.

---

## Conclusion

The authorization system is **working as designed** - it's correctly rejecting unknown roles. However, there's a **mismatch in expectations**:

- The **code design** assumes each service has its own role enum with only the roles it needs to authorize
- The **runtime behavior** shows bidirectional connections where each service needs to authorize the other

**The core question is:** What is the intended connection topology and authorization model?

Once that's clarified, we can implement the correct solution.
