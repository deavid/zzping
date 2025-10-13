# zzcollector-state Component

The `zzcollector-state` component manages collector identity and health state. It supports three roles — Collector, Database, and Admin — and a small room-based message protocol for heartbeat and queries.

This README provides a short overview, the message protocol, usage examples, and testing notes.

## Overview

Responsibilities:
- Maintain a unique collector identity and connection nonce
- Periodically send heartbeat messages to the Database role
- Aggregate health metrics from other components (pinger, mem-db)
- Database role tracks active collectors and exposes a query interface for Admin
- Stale detection and cleanup of inactive collectors

Design goals:
- Transport-agnostic: uses `zznet-session` room messages
- Testable: SessionManager abstraction can be mocked for unit tests
- Single component codebase handles all roles via `CStateRole`

## Roles

- Collector
  - Sends periodic `Heartbeat` messages with uptime and metrics
  - Receives `HeartbeatAck` from Database (contains `server_time_ms`)
  - Exposes `UpdateHealthMetrics` and `GetCollectorState` messages for other components

- Database
  - Receives `Heartbeat` messages
  - Tracks active collectors and their last-seen timestamps
  - Sends `HeartbeatAck` to collectors on receipt
  - Responds to `QueryCollectors` with `CollectorList`
  - Periodically cleans stale collectors (configurable via role)

- Admin
  - Sends `QueryCollectors` and receives `CollectorList`

## Protocol

All protocol messages are defined in `network_messages.rs`, room `cstate`:

- `CStateMessage::Heartbeat { collector_id, uptime_secs, pings_sent, pings_received, batches_sent, last_config_update_ms, connection_nonce }` — Collector → Database
- `CStateMessage::HeartbeatAck { timestamp_ms, server_time_ms }` — Database → Collector
- `CStateMessage::QueryCollectors` — Admin → Database
- `CStateMessage::CollectorList { collectors }` — Database → Admin

Messages implement `RoomMessageTrait` and are serialized with RON.

## Quick example usage

Examples are in `src/components/zzcollector-state/examples/`. They are small, self-contained programs that demonstrate how to start the component in different roles. To run an example, switch to the workspace root and run:

```bash
cargo run --package zzcollector-state --example collector_heartbeat
```

## Testing

Unit and integration tests for the component are available under `src/components/zzcollector-state/src/` and use an in-repo `MockSessionManager` for deterministic behavior. To run tests for this crate:

```bash
cargo test --package zzcollector-state --lib
```

See `PHASE3_CHECKLIST.md` for the full development & test plan.
