# zznet-session

**Transport-agnostic session management for ZZNet**

## Vision

This crate provides the **SessionManager** - the core of ZZNet's transport-agnostic design. It is **100% typed** and never touches bytes, providing the foundation for component communication.

## Purpose

Be the **heart of ZZNet's transport-agnostic design**:

```
Components (use SessionManager API)
    ↓
SessionManager (100% typed, transport-agnostic) ← YOU ARE HERE
    ↓
HelloConnection (typed ↔ bytes)
    ↓
Transport (bytes only)
```

It provides:
- **Session lifecycle management**: Connection establishment, maintenance, cleanup
- **Room registration**: Components register room handlers for message types
- **Message routing**: Incoming messages → appropriate room handlers
- **Connection monitoring**: Track active sessions, detect disconnects
- **Error handling**: Propagate errors to components gracefully

## Why This Matters

Without SessionManager:
- ❌ Components would deal with connections directly
- ❌ Room registration would be manual and error-prone
- ❌ Message routing would be application logic
- ❌ No central place to monitor connections

With SessionManager:
- ✅ Components just register rooms and send/receive typed messages
- ✅ All connection complexity hidden
- ✅ Same API for client and server
- ✅ Same code works with TCP and mock transport

## Key Requirements

### 100% Typed
- **Never touches bytes** - that's HELLO's job
- All APIs use Rust types
- Type safety enforced at compile time
- Serialization is internal implementation detail

### Transport-Agnostic
- Works with TCP/TLS transport
- Works with mock transport
- Same code for both
- No transport-specific behavior

### Room-Based Architecture
- Components register room handlers for specific message types
- Each room has a unique RoomId
- Incoming messages routed to correct room handler
- Automatic room lifecycle management

### Same Component, Different Config
**Critical principle from vision docs**: Same component code runs on both sides with different config.

```
Process A: Component(config=Client)
Process B: Component(config=Server)
```

**NOT** separate client and server components. All networking code for a component lives in ONE place.

## What This Crate Must NOT Do

- ❌ Implement transport (that's zznet-transport-tcp)
- ❌ Handle HELLO protocol (that's zznet-hello)
- ❌ Define room abstraction (that's zznet-room - this crate is lower level)
- ❌ Implement authorization (that's zznet-auth)
- ❌ Touch bytes (100% typed interface)

## Core Concepts

### SessionManager
The main API that components use to manage network sessions.

**Responsibilities**:
- Accept new connections (server-side)
- Initiate connections (client-side)
- Route messages to registered room handlers
- Track active sessions
- Handle connection cleanup

### Room Handler
Components implement handlers to process incoming messages.

**Per message type**, not per connection. When a message of type T arrives, the registered handler for T is called.

### SessionId and RoomId
- **SessionId**: Identifies one network connection
- **RoomId**: Identifies one message type/channel within a session

**Key insight**: Rooms are 1:1 per connection, NOT broadcast channels. Each connection has its own set of rooms.

## Design Principles

### Transport Agnostic
- Never depends on TCP-specific features
- Works with any transport implementing the traits
- Same code for mock and real transport

### 100% Typed
- All APIs use Rust types, never bytes
- Serialization is internal implementation detail
- Type errors caught at compile time

### Actor-Based
- Each session is independent actor
- No shared mutable state
- Natural async concurrency

### Component-Focused
- API designed for component developers
- Hide networking complexity
- Focus on message passing

## Testing Requirements

### Must Pass
- Work with mock transport
- Work with TCP transport
- Route messages to correct rooms
- Handle session establishment
- Handle graceful session close
- Handle abrupt disconnects
- Multiple concurrent sessions

### Mock-First
- All tests should use mock transport first
- TCP tests validate production behavior
- No test should depend on real network

## Relationship to Other Crates

- **zznet-api**: Transport abstraction SessionManager builds on
- **zznet-hello**: Provides typed connections SessionManager uses
- **zznet-room**: Higher-level room abstraction for components
- **zznet-auth**: Authorization services (used by components, not SessionManager)
- **zznet-builder**: High-level API that uses SessionManager
- **actix**: Actor framework for session actors

## Success Criteria

This crate succeeds if:
- ✅ Components never deal with connections directly
- ✅ Same component code works as client and server
- ✅ Switching TCP ↔ mock requires only config change
- ✅ 100% typed - no bytes in API
- ✅ Clear, simple API for component developers

