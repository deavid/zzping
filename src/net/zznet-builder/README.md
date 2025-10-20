# zznet-builder

**High-level builder API for ZZNet applications**

## Vision

This crate provides **ergonomic, high-level APIs** for building ZZNet applications. It's the entry point that hides all the complexity of coordinating transport, sessions, rooms, and auth.

## Purpose

Be the **application developer's entry point**:

```
Application uses NetworkBuilder/ClientBuilder/ServerBuilder ← YOU ARE HERE
    ↓
Coordinates: Transport + Session + Rooms + Auth
    ↓
Application gets fully-configured network stack
```

It provides:
- **Simple builder API**: Fluent interface for configuration
- **Sensible defaults**: Works out-of-the-box with minimal config
- **Full customization**: Override any component
- **Mock support**: Easy switch between real and mock transport
- **Production-ready**: Handles common patterns correctly

## Why This Matters

Without builders:
- ❌ Applications manually coordinate transport, sessions, rooms
- ❌ Easy to configure incorrectly
- ❌ Lots of boilerplate
- ❌ Hard to test (switching to mock is manual)

With builders:
- ✅ One builder call creates entire stack
- ✅ Correct configuration by default
- ✅ Minimal boilerplate
- ✅ Easy to switch TCP ↔ mock for testing

## Key Requirements

### Fluent Builder API
Configuration should read naturally:

```
NetworkBuilder::new()
    .with_tcp_transport(addr, tls_config)
    .with_role_based_auth()
    .build()
```

### Sensible Defaults
Most applications should work with:

```
ClientBuilder::new(addr).connect()
```

No need to configure everything unless you want to.

### Transport Selection
- **TCP/TLS** for production
- **Mock transport** for testing
- **Automatic selection** based on config

### Integrated Configuration
- Transport (TCP or mock)
- Session management
- Room registration
- Auth services
- All configured together consistently

## What This Crate Must NOT Do

- ❌ Implement transport (that's zznet-transport-tcp)
- ❌ Define message types (application responsibility)
- ❌ Provide business logic
- ❌ Manage application state
- ❌ Implement concrete auth (that's application)
- ❌ Define application roles (application responsibility)

## Builder Types

### ClientBuilder
Build client applications that connect to servers:
- Specify server address
- Configure TLS/mTLS (optional)
- Register rooms
- Connect and get typed client

### ServerBuilder
Build server applications that accept connections:
- Specify bind address
- Configure TLS/mTLS (optional)
- Register rooms
- Start and get running server

### NetworkBuilder
General-purpose builder for custom scenarios:
- Full control over all components
- For advanced use cases
- Most apps use ClientBuilder/ServerBuilder instead

## Design Principles

### Ease of Use
- Minimal boilerplate
- Sensible defaults
- Fluent builder API
- Clear errors

### Type Safety
- Compile-time type checking
- No runtime type confusion
- Generic over message types

### Flexibility
- Override any component
- Custom auth services
- Mock or real transport
- Full or minimal configuration

### Production Ready
- Proper error handling
- Graceful shutdown
- Resource cleanup
- Performance defaults

### Testable
- Easy mock transport integration
- Deterministic tests
- No dependency on real network

## Configuration Levels

### Minimal (Development)
```
ClientBuilder::new("localhost:9001").connect()
```

Plain TCP, no auth, works immediately.

### Standard (Production)
```
ClientBuilder::new("server:9001")
    .with_tls(tls_config)
    .connect()
```

TLS for encryption, cert-based identity.

### Full (Production with mTLS + Custom Auth)
```
ClientBuilder::new("server:9001")
    .with_tls(tls_config)
    .with_auth(my_auth_service)
    .with_room(room_id, handler)
    .connect()
```

Full control over all components.

## Testing Support

### Mock Transport
```
ClientBuilder::new("mock://test")
    .with_mock_transport()
    .connect()
```

Same API, zero network, deterministic tests.

## Relationship to Other Crates

- **zznet-api**: Transport abstraction
- **zznet-transport-tcp**: TCP/TLS implementation
- **zznet-hello**: HELLO protocol
- **zznet-session**: Session management
- **zznet-room**: Room abstraction
- **zznet-auth**: Auth traits

## Success Criteria

This crate succeeds if:
- ✅ Applications use only this crate's API (never lower layers)
- ✅ Minimal config works out of the box
- ✅ Easy to customize when needed
- ✅ Switching to mock for tests is trivial
- ✅ Clear, obvious error messages
- ✅ Production deployments use this without modifications

