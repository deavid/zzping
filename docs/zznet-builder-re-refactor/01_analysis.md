# Analysis of the `zznet-builder` Crate

**Date:** 2025-11-15

## 1. Executive Summary

This report analyzes the state of the `zznet-builder` crate. The crate was created with the ambitious goal of abstracting away boilerplate code for creating new `zznet` applications. However, the implementation is incomplete, its primary API is not used by any production application, and it is not validated by the project's main integration test suite (`zznet-demo`).

This has resulted in a significant amount of dead and unused code within the `zznet-builder` crate, as flagged by `cargo check`. The crate is a relic of an unfinished refactoring effort, leaving the applications it was meant to simplify still burdened with significant code duplication.

## 2. The Original Vision

The `zznet-builder` was conceived to solve a clear problem: **code duplication and boilerplate** in creating `zznet` applications. The `docs/zznet-builder-research/01_research_report.md` document explicitly states the goal:

> A `zznet-builder` (or `zznet-app-builder`) crate would provide substantial value by eliminating repetitive patterns and reducing the barrier to creating new zznet-based applications.

The intended scope of the builder was comprehensive, aiming to handle:
-   CLI argument parsing (`cli.rs`)
-   Logging initialization (`logging.rs`)
-   TLS configuration (`tls.rs`)
-   Actix runtime setup (`runtime.rs`)
-   Graceful shutdown signal handling (`signals.rs`)

The vision was to have a fluent builder API that would allow a new application to be created with just a few lines of code, hiding the complex and repetitive setup logic.

## 3. The Current State: An Unfinished Refactor

While the `zznet-builder` crate exists, it has not fulfilled its original vision. The `cargo check` command reveals the extent of the problem, with numerous warnings for unused code:

-   `error.rs`: `BuilderError` enum is never used.
-   `tls.rs`: `ClientTlsConfig`, `ServerTlsConfig`, and all associated functions (`load_client_tls`, `load_server_tls`) are unused.
-   `cli.rs`: The `StandardCliArgs` struct is unreachable.
-   `logging.rs`: `init_logging` and `init_logging_from_args` are unreachable.
-   `runtime.rs`: `install_crypto_provider` and `run_actix` are unreachable.
-   `signals.rs`: The `ShutdownSignals` struct and its methods are unreachable.

This confirms that the core features of the builder are not being called from any other crate. The document `docs/zznet-builder-research/04_COMPLETE_DRY_PLAN.md` acknowledges this, stating:

> The current `zznet-builder` implementation (v0.5 at best) **only eliminates setup boilerplate** but does NOT eliminate the **service lifecycle pattern duplication**.

## 4. The Integration Test Gap

A major contributor to the current situation is a critical gap in testing. The `zznet-demo` crate, which serves as the primary integration test suite for the entire `zznet` framework, **does not use `zznet-builder`**.

The audit in `docs/zznet-demo/04_audit_unused.md` highlights this failure:

> **`zznet-builder`**: The demo **does not use the intended public API** for building applications. It uses manual, low-level wiring, which fails to validate the primary entry point for developers.

And the critical findings in `docs/zznet-demo/05b_CRITICAL_FINDINGS.md` are even more direct:

> The `zznet-demo` integration test **does not use `zznet-builder` at all**.

Without test coverage, the builder's API was never validated, and its development appears to have stalled. This lack of validation is the root cause of why the crate is in its current, half-finished state.

## 5. Analysis of Code Duplication

The consequences of the incomplete builder are evident in the application code. Both `zzping-collector` and `zzping-database` contain significant duplicated code for application setup, which the builder was designed to eliminate.

A review of `src/apps/zzping-collector/src/main.rs` and `src/apps/zzping-database/src/main.rs` shows nearly identical code for:

-   **CLI Parsing:** Both use `clap::Parser::parse()`.
-   **Logging:** Both call `zzping_logging::init_logging()`.
-   **Configuration Loading:** Both have logic to load `collector.ron` or `database.ron`.
-   **Runtime and Signal Handling:** Both contain boilerplate for setting up the `actix::System` and handling shutdown signals.

This is precisely the duplication the builder was meant to prevent.

## 6. Conclusion and Recommendation

The `zznet-builder` crate is a case of an ambitious and well-intentioned refactor that was left incomplete. It was never fully integrated, and the lack of a testing feedback loop meant its development stalled before it could fulfill its purpose.

The chosen path forward is to **Finish the Refactor**. We will commit to the original vision and refactor `zzping-collector`, `zzping-database`, and the `zznet-demo` test suite to use the `zznet-builder` API. This will involve completing the builder's functionality and deleting the now-redundant code from the applications. This will finally achieve the goal of a DRY, simplified application creation process.

## Execution Update

I began executing this plan by implementing key builder APIs for testability and background execution (`run_service_with_config_and_stop` and `run_service_with_stop`), adding unit tests, adding a demo test harness helper to spawn builder-run services, and centralizing transport TLS config construction into `zznet-builder::tls`. Existing app services (`CollectorService` and `DatabaseService`) were refactored to use the builder's transport TLS helper.

Next steps: expand test coverage to exercise more of the builder API, migrate any remaining application duplication to the builder where appropriate, and update documentation to prioritize builder integration tests — run these tests locally since this project does not use CI.
