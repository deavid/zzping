# zznet-auth

**Generic authentication and authorization traits for ZZNet**

## Vision

This crate provides **trait definitions** for auth - not implementations. It defines the interfaces that components use to check authentication and authorization, but applications provide the concrete logic.

## Purpose

Provide **generic abstractions** for auth:

```
Component needs to check auth
    ↓
Uses AuthorizationService trait
    ↓
Application provides concrete implementation
    (ACL-based, role-based, certificate-based, custom, etc.)
```

It provides:
- **Authentication traits**: Verify identity of peers
- **Authorization traits**: Check if peer can perform action
- **AuthContext**: Capture authentication state
- **Generic over auth mechanisms**: Works with any auth implementation

## Why This Matters

Without these traits:
- ❌ Every component implements auth differently
- ❌ Can't swap auth mechanisms
- ❌ Testing requires real auth infrastructure
- ❌ No standard interface

With these traits:
- ✅ Components use standard interface
- ✅ Applications choose auth mechanism
- ✅ Easy to mock for testing
- ✅ Can change auth without changing components

## Key Requirements

### Trait Definitions Only
This crate defines **interfaces**, not implementations:
- `AuthenticationService` trait: Verify peer identity
- `AuthorizationService` trait: Check permissions
- `AuthContext` type: Capture auth state
- **Applications implement these traits**

### Generic Over Auth Mechanisms
Support various approaches:
- **Role-based**: Check if peer has specific role
- **ACL-based**: Check if peer in Access Control List
- **Certificate-based**: Check certificate properties (via mTLS)
- **Custom**: Implement any auth logic

### Async-First
All trait methods are async:
- Allows I/O for auth checks (database, external service)
- Non-blocking auth validation
- Natural fit with async network code

### Testable
Easy to create test implementations:
- Mock auth services for unit tests
- Deterministic test behavior
- No dependency on real auth infrastructure

## What This Crate Must NOT Do

- ❌ Implement concrete auth logic (application responsibility)
- ❌ Provide certificate validation (that's zznet-transport-tcp)
- ❌ Manage user accounts
- ❌ Provide session management (that's zznet-session)
- ❌ Enforce auth (components must call auth services)
- ❌ Define application roles (that's application responsibility)

## AuthContext

Captures authenticated peer state.

**Core fields**:
- `peer_identity`: Verified PeerIdentity
- `role`: Extracted role (from certificate CN or other source)
- `username`: Extracted username (from certificate SAN or other source)
- `authenticated_at`: When authentication occurred

**NOTE**: Optional expiry time and metadata are NOT needed. Keep it simple.

## Design Principles

### Generic Traits
- Traits define interfaces, not implementations
- Applications provide concrete implementations
- Supports multiple auth mechanisms

### Async-First
- All trait methods are async
- Allows I/O for auth checks
- Non-blocking design

### Separation of Concerns
- **Authentication**: "Who are you?"
- **Authorization**: "What can you do?"
- Clear separation in trait design

### Testable
- Easy to mock for unit tests
- No dependency on real infrastructure
- Deterministic test behavior

### Simple
- Don't over-engineer
- **NOTE**: No expiry times, no metadata - not needed
- Focus on core auth concepts

## Common Patterns

### mTLS-Based Auth
With mTLS, identity is cryptographically verified:
1. Transport extracts PeerIdentity from certificate
2. AuthenticationService creates AuthContext from PeerIdentity
3. AuthorizationService checks permissions based on role/username

### Role-Based Authorization
Simple pattern: Role determines permissions.

Example:
- "collector" role can write pings
- "database" role can read/write everything
- "client-admin" role can do anything
- "client-ro" role can only read

### ACL-Based Authorization
More flexible: Check if user/role in specific ACL.

Example:
- ACL "ping-writers" = ["collector1", "collector2"]
- ACL "admins" = ["alice", "bob"]

## Testing Requirements

### Must Pass
- Mock auth service works
- Can approve/deny based on simple rules
- AuthContext created correctly
- Trait methods callable

### Mock-First
- Provide mock auth service for testing
- Components should test with mock first
- Real auth for integration tests

## Relationship to Other Crates

- **zznet-api**: Defines PeerIdentity type
- **zznet-transport-tcp**: Provides mTLS authentication, extracts PeerIdentity
- **zznet-session**: Sessions can use auth services
- **zznet-room**: Rooms can enforce per-message auth
- **Application crates**: Implement concrete auth logic

## Success Criteria

This crate succeeds if:
- ✅ Components use standard auth traits
- ✅ Applications can choose any auth mechanism
- ✅ Easy to mock for testing
- ✅ Clean separation of auth and business logic
- ✅ Simple, focused API (no over-engineering)

