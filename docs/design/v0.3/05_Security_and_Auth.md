# ZZPing v0.3: Security & Authentication

- **Status:** Authoritative / Active
- **Date:** November 2025
- **Dependency:** `04_Orchestration.md`

---

## 1. The Threat Model

What are we protecting against?

1. **Rogue Data Injection:** A bad actor submitting fake ping results (corrupting the journal).
2. **Unauthorized Control:** A bad actor changing the target list (e.g., to DDoS a target).
3. **MITM:** Eavesdropping on the data stream.

**Out of Scope:**

- Physical access to the Collector machine (if they have the box, they have the cert).
- Advanced side-channel attacks.

## 2. Authentication Strategy: mTLS

We use Mutual TLS (mTLS) for all inter-process communication.

### The "Pain Reduction" Compromise

To make deployment manageable for home users, we decouple **Identity** from **Authorization**.

- **Strict:** We require valid Certificates signed by our CA.
- **Loose:** We share the same certificate across multiple machines.

### Certificate Roles

The system recognizes four distinct certificate roles (via `CN`):

1. **`collector`**: Can submit data. Can read config. Cannot write config.
2. **`database`**: The Server. Can write config.
3. **`client-admin`**: Can read/write config. Can query data.
4. **`client-ro`**: Read-only. Can query data.

### Deployment Example

- **User generates:** `ca.pem`, `collector.pem`, `database.pem`.
- **User deploys:** Copies `collector.pem` to 5 different Raspberry Pis.
- **Security:** All 5 Pis are authenticated as "A Valid Collector". They are authorized to submit data.
- **Identity:** Each Pi is distinguished by its `installation_id` in `collector.ron`, NOT by its certificate.

## 3. Authorization Enforcement

Auth logic lives in the Application Layer (not the Network Layer).

### 1. Connection Level (Handshake)

- **Database:** Rejects connection if the Client Certificate is invalid.
- **Database:** Rejects connection if the Role is `unknown`.

### 2. Room Level (Access Control)

When a peer attempts to join a room or send a message, the Component checks permissions.

**Example: `IntentConfig` Component**

```rust
fn handle_update(msg: RequestConfigChange, peer_role: Role) {
    if peer_role != Role::ClientAdmin {
        log::warn!("Unauthorized config change attempt from {:?}", peer_role);
        return; // Drop message
    }
    // ... process update
}
```

## 4. The "Insecure" Mode (Dev Only)

For local development, generating certs is friction. We support a raw TCP fallback.

- **Config:** `insecure_trust_hello = true`.
- **Behavior:** The system trusts the `Role` string sent in the HELLO handshake packet.
- **Safety:** This mode prints a loud `WARNING` on startup. It is strictly forbidden in production.

## 5. Secret Management

- **Certificates:** Stored on disk (`/etc/zzping/certs/`).
- **Private Keys:** Stored on disk, read-only by the zzping user.
- **Encryption:** All data on the wire is encrypted by TLS 1.3.

## 6. Summary of Identity vs. Auth

| Context                   | Source of Truth                 | Purpose          |
| :------------------------ | :------------------------------ | :--------------- |
| **Can I connect?**        | mTLS Certificate (Root CA)      | Authentication   |
| **Can I write config?**   | Certificate CN (Role)           | Authorization    |
| **Which Collector am I?** | Config File (`installation_id`) | Logical Identity |
| **Am I the Master?**      | `TCPLock` + Database            | Orchestration    |
