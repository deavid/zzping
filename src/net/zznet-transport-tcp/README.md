# zznet-transport-tcp

**Production TCP/TLS transport for ZZNet**

## Vision

Provide a **production-ready, secure transport** implementing zznet-api traits using TCP with optional TLS/mTLS. This is what real ZZPing deployments use.

## Purpose

Be the concrete implementation that:
- Handles real network I/O over TCP
- Optionally provides TLS/mTLS for secure, authenticated connections
- Extracts peer identity from certificates (when using mTLS)
- Implements the frame protocol reliably over a streaming transport

## Why This Matters

The entire ZZNet stack is transport-agnostic. This crate **proves the abstraction works in production**:
- ✅ Same SessionManager code works with this AND mock transport
- ✅ Components never know they're using TCP
- ✅ Security (TLS/mTLS) lives in transport layer, transparent to applications
- ✅ **Mock-first development paid off**: Components fully tested before this existed

## Key Requirements

### TCP Transport
- **Reliable, ordered delivery** (inherent to TCP)
- Works on any TCP-capable network
- Proper connection lifecycle (connect, communicate, close)
- Error handling and propagation

### Optional TLS/mTLS
**NOTE**: Raw TCP must be usable for production too. mTLS (or regular TLS) is overkill for most deployments. mTLS is meant to protect against attacks when exposing the service to the internet, which is not something common.

When TLS is enabled:
- **Server authentication** (TLS): Client verifies server certificate
- **Mutual authentication** (mTLS): Both sides present and verify certificates
- Certificate chain validation with CA
- Identity extraction from certificates → PeerIdentity for authorization

**NOTE**: The crate itself MUST not expect certificates in any particular place. Certificate paths must be entirely configurable.

### Certificate Identity Extraction (mTLS only)

When mTLS is enabled, extract peer identity from certificates:
- **CN (Common Name)**: NOT derived from DNS. **NOTE**: The CN is a typical problem or issue, we need to be careful with this. The CN MUST NOT be DNS derived. zznet does not deal with domain names. In some scenarios or tests we might use "zznet" as a catch-all. The string "zzping" must not exist in "zznet" crates.
- **SAN (Subject Alternative Name)**: First DNS entry becomes username (e.g., "alice", "root")
- **Peer Address**: Socket address for logging

**Certificate pattern examples**:
- Service: `CN=service-name`, `SAN=DNS:root`
- User: `CN=role-name`, `SAN=DNS:username`

**NOTE**: zznet packages must not define roles themselves. The applications (zzping) define roles. The presence of application-specific roles in zznet crates means we leaked the abstraction and must be repaired.

### Frame Protocol Implementation
- **Length-prefixed framing**: `[u32 BE length][payload]`
- Handle partial frames correctly (TCP is stream-based, not message-based)
- Enforce 16 MiB maximum frame size
- Zero-length frames allowed for keepalive

### Connection Management
- Graceful shutdown (both sides close cleanly)
- Error propagation (network errors, TLS errors, frame errors)
- Timeout support
- **TCP keepalive** for detecting dead connections
  - **NOTE**: Keepalive interval must be configurable with millisecond precision. Default should be 1 second.
- Proper resource cleanup

## What This Crate Must NOT Do

- ❌ Handle HELLO protocol (that's zznet-hello)
- ❌ Manage sessions (that's zznet-session)
- ❌ Know about rooms or components
- ❌ Serialize typed messages (only deals with frames/bytes)
- ❌ Implement authorization logic (only extracts identity)
- ❌ Define application roles (that's the application's responsibility)

## Design Principles

### Security is Optional
- **Plain TCP** for trusted networks, local development, simple deployments
- **TLS** for encrypted communication
- **mTLS** for cryptographic peer authentication
- **Application chooses** security level based on deployment needs

### Reliable Framing Over Streams
- TCP provides reliable ordered byte stream
- This layer provides reliable framed message delivery
- Partial frames buffered correctly
- Oversized frames rejected immediately

### Clean Error Handling
- Network errors → TransportError::Io
- TLS errors → TransportError::Tls
- Frame errors → TransportError::FrameTooLarge
- Graceful close → Ok(None), NOT an error

### Identity Extraction (mTLS only)
- CN and SAN extracted from peer certificate
- Available immediately after TLS handshake
- Passed up stack for authorization decisions
- **Cryptographically verified** (not self-reported)
- **NO DNS RESOLUTION** - CN is not a domain name

### Configurability
- Certificate paths configurable
- Keepalive intervals configurable (millisecond precision)
- **NOTE**: Defaults should be sensible but everything should be tunable

## Testing Requirements

**NOTE**: "cargo nextest run" should be used where possible, if it's installed.

### Must Pass
- Connect to localhost with and without TLS
- Extract correct peer identity from certificate (mTLS)
- Send/receive frames bidirectionally
- Handle connection close gracefully
- Reject oversized frames
- Detect network errors
- Partial frame handling (send half a frame, wait, send rest)

### Integration with Mock
- Same test suite should work with both TCP and mock transport
- Proves traits are correctly implemented
- No transport-specific behavior leaks into tests

## Performance Considerations

### Acceptable Overhead
- TLS handshake: ~1-2ms (one-time per connection)
- TLS encryption: ~5-10% CPU (modern hardware with AES-NI)
- Frame protocol: negligible overhead

### Buffering
- Use reasonable buffer sizes
  - **NOTE**: "typical MTU size" of 8 KiB is wrong. Internet MTU is ~1.5 KiB. 8 KiB is fine as a buffer size but don't claim it's MTU-related.
- Don't buffer entire large frames in memory
- Stream frame payload efficiently

### TCP Tuning
- Enable TCP keepalive (detect dead connections)
  - **NOTE**: Configurable, millisecond precision, default 1 second
- Nagle's algorithm: leave at OS default (not latency-critical for our use case)
- Socket buffer sizes: OS defaults are fine

## Relationship to Other Crates

- **zznet-api**: Defines the traits this implements
- **zznet-hello**: Uses this transport to perform HELLO handshake
- **zznet-builder**: Creates instances of this based on config
- **rustls** (or similar): Provides TLS implementation (prefer memory-safe Rust TLS over OpenSSL)

## Success Criteria

This crate succeeds if:
- ✅ Production ZZPing deployments use this transport without modifications
- ✅ Same component code works with mock in tests, this in production
- ✅ Plain TCP works for simple/trusted deployments
- ✅ mTLS provides strong authentication when needed
- ✅ No security incidents due to transport layer
- ✅ Configuration is flexible enough for all deployment scenarios
- ✅ Performance overhead is acceptable (<10% vs raw TCP)

