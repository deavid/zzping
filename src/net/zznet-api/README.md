# zznet-api

**Transport abstraction layer for ZZNet**

## Vision

This crate exists to enable **transport-agnostic design** in ZZNet. The entire network stack must work identically whether running on production TCP connections or in-memory mocks for testing. This crate defines the contract that makes that possible.

## Purpose

Define the abstraction boundary between:
- **Above**: Code that doesn't care HOW bytes move (HELLO, SessionManager, Components)
- **Below**: Code that actually moves bytes (TCP/TLS, mock channels, future transports)

## Why This Matters

Without this abstraction:
- ❌ Components would depend on TCP directly
- ❌ Testing would require real network
- ❌ Can't add new transports (WebSocket, QUIC, etc.) without changing everything

With this abstraction:
- ✅ Components never know about TCP
- ✅ Tests run in-memory with deterministic behavior
- ✅ New transports = implement traits, everything else just works
- ✅ **Mock-first development**: Write and test components before TCP implementation exists


## Core Abstractions

### TransportConnection
Represents **one bidirectional connection** that sends/receives frames (delimited byte chunks).

**Key Requirements**:
- Send and receive framed messages (not raw bytes)
- Report peer identity (from mTLS certificate or other auth)
 - Graceful close detection (reported as TransportError::ConnectionClosed)
- Transport errors propagate cleanly

### TransportServer
Accepts incoming connections, producing TransportConnection instances.

**Key Requirements**:
- Bind to address
- Accept connections in a loop
- Each accepted connection is independent

### TransportClient
Initiates outgoing connections.

**Key Requirements**:
- Connect to remote address
- Produce TransportConnection on success

## What This Crate Must NOT Do

- ❌ Implement actual networking (that's zznet-transport-tcp)
- ❌ Know about HELLO protocol (that's zznet-hello)
- ❌ Handle typed messages (that's SessionManager)
- ❌ Manage sessions or rooms (that's zznet-session/zznet-room)

## Shared Types

### PeerIdentity
Represents verified identity of the peer, extracted from mTLS certificate.

**Components**:
- **Common Name (CN)**: Role identifier (e.g., "collector", "database")
- **SAN Username**: First DNS entry from Subject Alternative Name (e.g., "alice", "root" for services)
- **Peer Address**: Socket address for logging

**Purpose**: Passed up the stack for authorization decisions.

### Frame Protocol
All transports must implement the same framing:
- **Format**: `[u32 BE length][payload bytes]`
- **Maximum frame size**: 16 MiB
- **Zero-length frames**: Allowed (for heartbeat/keepalive)

**Why this protocol**:
- Simple and efficient
- Language-agnostic (can implement in other languages later)
- Large enough for any realistic message
- Small enough to prevent memory attacks

## Mock Transport: Why It's Critical

The mock transport is **not just for testing** - it's a proof that the abstraction works.

**Requirements for Mock**:
- Two instances can communicate directly in-memory
- No threads, no async I/O (just channels)
- Deterministic behavior (no races, no timeouts)
- Same trait implementations as real transport

**What Mock Proves**:
- Transport abstraction is sufficient
- No hidden dependencies on TCP
- Components can be fully tested before production transport exists
- Stack integration can be validated without network

## Design Principles

### Pure Abstraction
These traits must describe **what transports do**, not **how they do it**. No TCP-specific concepts leak through.

### Frame-Based
Transports deal in **frames** (delimited messages), not streams. The higher layer (HELLO) handles serialization.

### Error Transparency
Transport errors must be distinguishable:
- Connection closed gracefully is reported as TransportError::ConnectionClosed from recv
- Connection failed (Err with details)
- Frame too large (specific error, not generic)

### Identity at Transport Level
mTLS provides cryptographically verified identity. This must be available to higher layers for authorization.

## Testing Requirements

### Any implementation must support:
- **Bidirectional communication**: Send and receive simultaneously
- **Multiple frames**: Queue multiple sends before receive
- **Large frames**: Up to 16 MiB
- **Graceful close**: Both sides detect clean shutdown
- **Error cases**: Invalid frames, oversized frames, network errors

## Relationship to Other Crates

- **zznet-transport-tcp**: Implements these traits with real TCP/TLS
- **zznet-hello**: Sits above this, converts frames ↔ typed messages
- **zznet-session**: Uses hello connections, never touches these traits directly
- **zznet-builder**: Chooses TCP or mock transport based on config

## Success Criteria

This crate succeeds if:
- ✅ Components can be fully developed and tested using mock transport
- ✅ Switching TCP ↔ mock requires only configuration change
- ✅ New transports can be added without changing upper layers
- ✅ No component ever imports `tokio::net` directly

