# ZZNet Application Builder (`zznet-builder`)

## Vision

To provide a standardized, DRY (Don't Repeat Yourself) framework for bootstrapping `zznet` applications. The builder enforces a consistent architecture, eliminating boilerplate code and reducing the complexity of creating new services.

## Concept

The `zznet-builder` is a fluent API that handles all common application setup and lifecycle concerns, including:

-   **CLI Parsing:** Standardized command-line argument handling.
-   **Logging:** Centralized logging initialization.
-   **Configuration:** Loading and parsing of application-specific configuration files.
-   **TLS:** Simplified and consistent TLS setup for clients and servers.
-   **Runtime:** Management of the Actix actor runtime.
-   **Graceful Shutdown:** Handling of OS signals for clean application termination.

## Core Abstractions

Applications integrate with the builder by implementing two core traits:

-   `ZZNetConfig`: Defines the application's configuration structure.
-   `ZZNetService`: Implements the application's startup and runtime logic.

By using these traits, an application can be launched with minimal code in `main.rs`, delegating all the setup and lifecycle management to the builder.