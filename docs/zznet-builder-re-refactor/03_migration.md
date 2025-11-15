# Migration Guide: Moving an App to `zznet-builder`

**Date:** 2025-11-15

This guide provides a high-level checklist for migrating an existing `zznet` application to use the `zznet-builder` crate.

## 1. Implement `ZZNetConfig` and `ZZNetService`

- Ensure your `config` struct derives `Serialize`, `Deserialize`, and implements `ZZNetConfig`.
- Implement `ZZNetService` for your application's service struct.
  - `new(config: Config) -> Result<Self, Error>`: Construct the service and perform any non-validating checks.
  - `async run(self) -> Result<(), Error>`: Start actors/handlers and return once startup is complete.
  - Optionally override `service_name()`.

## 2. Replace `main.rs` with `AppBuilder`

- Replace the manual wiring in `main.rs` with a minimal builder

```rust
use anyhow::Result;
use zznet_builder::builder::AppBuilder;
use my_app::service::MyService;

fn main() -> Result<()> {
    AppBuilder::new("MyApp", env!("CARGO_PKG_VERSION"))
        .with_default_config("myapp.ron")
        .run_service::<MyService>()
}
```

- Confirm CLI options and logging are preserved.

## 3. Use Builder TLS Helpers

- Centralize transport TLS creation using `zznet-builder::tls::to_transport_tls_config`.
- Replace per-app TLS PEM manipulations with builder helper where possible.

> Note: The lower-level helpers `load_client_tls` and `load_server_tls` were intentionally made internal
> (`pub(crate)`) to avoid leaking internal TLS parsing APIs. If an application needs a `rustls::ClientConfig`
> or `rustls::ServerConfig` directly, consider converting to `transport::TlsConfig` via
> `to_transport_tls_config` and let `zznet-transport-tcp` handle `rustls` specifics.

## 4. Update Tests to Use Builder

- Migrate critical integration tests to use `spawn_*_service_with_builder` helpers where applicable, or use builder API to start the application in a test runtime via `run_service_with_config_and_stop`.
- Keep low-level manual wiring tests for deep behavior testing, but **make builder-based tests the primary integration test**.

## 5. Clean Up Unused Code

- Remove duplicated CLI parsing, logging initialization, TLS PEM parsing, and signal handling logic from the app.
- Ensure `Cargo.toml` only depends on crates needed by the app and that builder-managed dependencies are removed.

## 6. Validate & Commit

- Run `cargo test` and `cargo check` across the workspace.
- Run integration tests that exercise builder API paths.
-- Ensure that the developer running tests locally executes the builder-run integration test as part of the validation gate (the project does not use CI pipelines).

## 7. Optional: Deleting Obsolete Files

- If you're confident the app fully uses `zznet-builder`, remove any old helpers in the service that are now replaced by builder logic.
- Keep a backup branch until tests run locally validate behavior remains consistent.

## 8. Notes & Edge Cases

- If an app needs low-level, non-standard initialization, provide a `run_custom` escape hatch or pass closures to the builder to handle custom runtime wiring.
- For Windows, signal handling works differently; ensure a path exists for test harnesses using `run_service_with_config_and_stop`.
- For binary releases, keep logging/arg parsing order consistent with existing expectations.

---

This guide prioritizes breadth and coverage to help teams migrate their apps fully. Adjust as needed for corner cases specific to application architecture.