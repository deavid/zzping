# High-Level Plan for `zznet-builder` Refactor

**Date:** 2025-11-15
**Status:** Proposed

## 1. Objective

This document outlines a high-level plan to complete the `zznet-builder` refactoring. The goal is to fulfill the original vision of a unified, DRY (Don't Repeat Yourself) application framework by making `zznet-builder` the standard, validated, and documented way to create all `zznet` applications. This plan focuses on breadth over depth, identifying all areas of work and their potential ramifications.

## 2. Phase 1: Enhance `zznet-builder` Core Functionality

This phase focuses on making the builder feature-complete and ready for consumption.

-   **Flesh out the Builder API:**
    -   Integrate the currently unused modules (`cli.rs`, `logging.rs`, `tls.rs`, `runtime.rs`, `signals.rs`) into the primary `AppBuilder` logic.
    -   The builder must handle the full application lifecycle: CLI parsing, logging setup, configuration loading, TLS initialization, runtime management, and graceful shutdown.

-   **Implement the Service Abstraction Traits:**
    -   Define and implement the `ZZNetService` and `ZZNetConfig` traits as outlined in existing design documents (`docs/zznet-builder-research/04_COMPLETE_DRY_PLAN.md`).
    -   These traits will form the contract between the builder and the applications, allowing the builder to manage any service that adheres to the interface.

-   **Consider Ramifications & Edge Cases:**
    -   **Flexibility:** The builder must be flexible enough to support different application types (e.g., client-only, server-only, or both) and varying configuration needs.
    -   **Error Handling:** The builder must have robust error handling for all stages (e.g., invalid config path, failed TLS certificate loading, port already in use). Errors must be propagated clearly.
    -   **Configuration:** The builder should handle relative paths in configuration files correctly (e.g., TLS cert paths relative to the config file's location).

## 3. Phase 2: Refactor Production Applications

This phase involves migrating the existing applications to use the newly enhanced builder.

-   **Target Applications:** `zzping-collector` and `zzping-database`.

-   **Migration Steps:**
    -   Implement the `ZZNetService` and `ZZNetConfig` traits for each application's `Service` and `Config` structs.
    -   Rewrite the `main.rs` file in each application to delegate entirely to the `AppBuilder`. The target is to reduce `main.rs` to fewer than 10 lines.
    -   Remove the now-redundant, duplicated code from each application crate, including manual CLI parsing, logging setup, signal handling, and TLS loading logic.

-   **Consider Ramifications & Edge Cases:**
    -   **Behavioral Parity:** The refactored applications must be functionally identical to their previous versions. All existing command-line arguments, configuration options, and logging behavior must be preserved.
    -   **Dependency Cleanup:** `Cargo.toml` files for the applications should be reviewed to remove dependencies that are now managed by `zznet-builder` (e.g., `clap`, `tokio`, `rustls`).

## 4. Phase 3: Refactor the `zznet-demo` Integration Test Suite

This phase addresses the critical testing gap identified in the analysis.

-   **Create a Builder-Centric Test:**
    -   Develop a new, primary integration test within the `zznet-demo` crate that uses `zznet-builder` to construct the services under test.
    -   This test will replace the old manual wiring and serve as the primary validation for the builder's API.

-   **Test Coverage Goals:**
    -   The new test suite must aim for >95% line coverage on the `zznet-builder` crate. These coverage goals are measured manually via local tooling; the project does not use CI to enforce coverage.
    -   It should validate the entire application lifecycle as managed by the builder, including successful startup and graceful shutdown.
    -   Negative test cases must be included: test for builder errors when provided with invalid configurations, non-existent certificate paths, etc.

-   **Consider Ramifications & Edge Cases:**
    -   **Test Complexity:** The test will need to manage multiple application instances (e.g., a "database" and a "collector") built via the builder, and assert that they can connect and communicate.
    -   **Note:** The project does not use an automated CI/CD pipeline. Developers should run the integration tests and coverage checks locally and ensure they pass prior to review.

## 5. Phase 4: Documentation and Cleanup

This final phase ensures the work is maintainable and accessible to developers.

-   **Update Documentation:**
    -   Update the main `README.md` and any developer guides to reflect that `zznet-builder` is the canonical way to create new applications.
    -   Provide clear, concise examples of using the `AppBuilder`.
    -   Add comprehensive documentation to the `zznet_builder::traits` module.

-   **Code Cleanup:**
    -   Run `cargo check`, `cargo clippy`, and `cargo fmt` across the workspace to ensure the new code adheres to quality standards.
    -   Remove any old, commented-out code or temporary files related to the refactor.
    -   Confirm that `cargo check` no longer reports dead code warnings for the `zznet-builder` modules.

-   **Consider Ramifications:**
    -   **Onboarding:** The new, simplified process should significantly lower the barrier for new developers to create `zznet` applications. The documentation must be clear enough to support this.
    -   **Future Development:** All future applications within the project must use the builder, enforcing a consistent architectural pattern across the codebase.

## Progress Update

✅ Implemented run_service_with_stop and run_service_with_config_and_stop to run builder-managed apps with a programmatic stop future (useful for tests).

✅ Added unit tests in `zznet-builder` to validate building services from config and running them with a programmatic stop.

✅ Added `spawn_demo_service_with_builder` and `builder_run_integration_test.rs` to `zznet-demo` to exercise the builder-run workflow in tests.

✅ Centralized transport TLS config creation into `zznet-builder::tls::to_transport_tls_config` and refactored `zzping-collector` and `zzping-database` to use the helper.

Remaining work includes: updating all application docs to favor builder usage, removing remaining duplicate TLS-loading code where safe, and ensuring integration tests are run locally as part of developer workflows; CI is not used for this repository.

### How to run the builder-run tests locally

Run the demo builder-run integration test specifically:

```bash
cargo test -p zznet-demo --test builder_run_integration_test
```

Run all builder-based tests across the workspace:

```bash
cargo test --workspace --all-features
```

### Next steps (short-term)

- Remove remaining `cli.rs` application definitions if they are unused (we re-exported the builder `StandardCliArgs` to keep code compatible). Ensure `Cargo.toml` does not list `clap` unless the app requires custom CLI options.
- Consider moving `load_client_tls`/`load_server_tls` into public API or a smaller `zznet-app-utils` crate if other components need direct `rustls` configs.
- No automated workflows (e.g. GitHub Actions) are used in this repository; testing and coverage must be run locally by contributors.
