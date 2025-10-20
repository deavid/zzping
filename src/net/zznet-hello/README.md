# zznet-hello

**HELLO protocol and serialization boundary for ZZNet**

## Vision

This crate is the **critical boundary between bytes and typed messages**. Below this layer, everything is frames (bytes). Above this layer, everything is typed Rust structs. HELLO protocol establishes this boundary by exchanging peer identities and protocol versions.

## Purpose

Serve as the **serialization boundary** in the ZZNet stack:

```
SessionManager (100% typed, never touches bytes)
    ↓
zznet-hello (typed ↔ bytes boundary) ← YOU ARE HERE
    ↓
Transport (frames/bytes only)
```

It provides:
- **HELLO protocol**: Mutual identity exchange after transport connection established
- **Serialization**: Typed messages → bytes
- **Deserialization**: Bytes → typed messages
- **Protocol versioning**: Frame format version negotiation for future compatibility
- **Identity verification**: Confirm peer identity or provide fallback

## Why This Matters

Without this layer:
- ❌ Every component would need serialization logic
- ❌ Protocol changes would break everything
- ❌ No version negotiation mechanism
- ❌ Identity exchange would be ad-hoc

With this layer:
- ✅ Components only deal with typed messages
- ✅ Protocol changes handled centrally
- ✅ Version negotiation transparent to applications
- ✅ Identity verified before any application data flows

## Key Requirements

### HELLO Protocol
- **Mutual exchange**: Both sides send HELLO immediately after transport connect
- **Version negotiation**: Both send supported version, agree on lower version
- **Identity exchange**: Each side receives peer's PeerIdentity
  - With mTLS: HELLO confirms transport-provided identity
  - Without mTLS: HELLO provides identity (⚠️ NOT cryptographically verified - this is intended)
- **Timeout support**: Handshake must complete within timeout or fail

### Serialization Layer
- **NOTE**: msgpack should be considered as a replacement for bincode, because it allows for more flexibility when the protocol changes.
- **Current**: serde + bincode for efficient binary serialization
- Frame version prefix for future compatibility
- Generic over message types (`T: Serialize + DeserializeOwned`)
- **NOTE**: Zero-copy deserialization is not needed (don't over-optimize)

### HelloConnection Abstraction
- Wraps any `TransportConnection` implementation
- Provides typed `send<T>()` and `recv<T>()` methods
- Transparent serialization/deserialization
- Preserves peer identity from HELLO handshake

## What This Crate Must NOT Do

- ❌ Implement transport (that's zznet-transport-tcp or mock)
- ❌ Manage sessions (that's zznet-session)
- ❌ Handle rooms (that's zznet-room)
- ❌ Implement authorization (that's zznet-auth)
- ❌ Define message types (that's application responsibility)

## HELLO Protocol Flow

### Successful Handshake

```
Client                                    Server
  |                                         |
  |  --- HELLO { version, identity } --->  |
  |                                         |
  |  <--- HELLO { version, identity } ---  |
  |                                         |
  |     (both verify/store peer identity)  |
  |                                         |
  | <-------> typed messages <-----------> |
```

### Key Properties
1. **Both sides initiate**: Client and server both send HELLO immediately
2. **Symmetric protocol**: No client/server distinction in HELLO (same code)
3. **Version negotiation**: Lower version is used
4. **Identity confirmation**: mTLS identity confirmed OR fallback identity accepted
5. **After HELLO**: Connection ready for typed message exchange

## Serialization Format

### Frame Structure

Every typed message is serialized as:

```
[Version: u8][Payload: serialized message]
```

- **Version** (1 byte): Protocol version for future compatibility
- **Payload** (variable): Message serialized with chosen serializer (bincode currently, msgpack future)

### Version Evolution

**Current: Version 1**
- Uses bincode default configuration
- Network byte order (big-endian)
- Variable-length integer encoding

**Future: Version 2+**
- Could use msgpack for better schema evolution
- Could use different serializer entirely
- Version negotiation ensures compatibility

## Design Principles

### Clear Layering
- Transport layer: Only knows bytes (frames)
- HELLO layer: Converts bytes ↔ typed messages
- Session layer: Only knows typed messages

### Protocol Evolution
- Version field enables future changes
- Version negotiation ensures compatibility
- New versions coexist with old versions

### Security
- Identity verification via HELLO
- Relies on transport (mTLS) for cryptographic security
- Does NOT implement authorization (that's zznet-auth)
- **Without mTLS**: Identity can be spoofed (this is intentional - app decides if that's acceptable)

### Simplicity
- Single serialization format (don't over-engineer)
- Simple HELLO protocol (mutual exchange)
- Minimal configuration needed

## Identity Management

### With mTLS (Recommended)

Transport provides cryptographically verified PeerIdentity from certificate:
- HELLO confirms this identity
- Cannot be spoofed by HELLO message
- No need to trust application-level identity claims

### Without mTLS (Development/Testing)

Identity comes from HELLO message:
- **⚠️ NOT verified** - can be spoofed
- **This is intentional** - some deployments don't need crypto auth
- Application decides if this is acceptable for its threat model

## Version Negotiation

### Backward Compatibility

When peers support different versions:

```
Client supports v1       Server supports v2
         ↓                        ↓
   Sends HELLO(v1)         Sends HELLO(v2)
         ↓                        ↓
    Receives HELLO(v2)     Receives HELLO(v1)
         ↓                        ↓
      Uses v1  ← both agree →  Uses v1
```

**Rule**: Both sides use the **lower** version number.

### Adding New Versions

When adding version 2:
1. Implement version 2 serialization
2. Add version 2 frame handling
3. Deploy to all nodes (they still use v1)
4. Once all nodes support v2, can start using v2 features
5. Version 1 code can eventually be removed

## Testing Requirements

### Must Pass
- HELLO handshake succeeds
- Version negotiation works (v1 ↔ v2)
- Identity extracted correctly (with mTLS)
- Identity provided correctly (without mTLS)
- Typed messages serialize/deserialize correctly
- Connection close detected
- Handshake timeout works

### Integration with Mock
- Same code works with mock and real transport
- No transport-specific behavior
- Deterministic tests

## Relationship to Other Crates

- **zznet-api**: Defines TransportConnection that this wraps
- **zznet-transport-tcp**: Provides transport for production
- **zznet-session**: Uses HelloConnection for typed communication
- **zznet-room**: Uses HelloConnection for typed room messages
- **serde**: Serialization traits
- **bincode** (current) / **msgpack** (future): Actual serializer

## Success Criteria

This crate succeeds if:
- ✅ Components never deal with serialization directly
- ✅ Protocol changes handled centrally
- ✅ Version negotiation transparent to applications
- ✅ Identity exchange works reliably
- ✅ Performance overhead acceptable (<100µs per message)
- ✅ Future serializer changes possible without breaking upper layers

