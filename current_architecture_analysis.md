# Current Architecture Analysis

This document compares the current state of the `zzping` codebase (as of the completion of the v0.3 MVP) with the high-level goals outlined in the "ZZPing Architectural Vision II" document (`ARCHITECTURE_ZZPING.md`). Its purpose is to identify which parts of the vision have been realized and to explicitly note any deviations or features that have been deferred.

## Overall Vision Alignment

The core architectural pattern described in the vision document has been successfully implemented. The system is now composed of three distinct, networked services:

1.  **`zzping-database`**: A central TCP server that ingests and stores data.
2.  **`zzping-collector`**: A TCP client that performs pings and sends data to the database.
3.  **`zzping-gui`**: A GUI application that connects to the database to view live data.

The project has been successfully migrated from a collection of proof-of-concept tools to a networked architecture. The PoC crates (`zzping-capture`, `zzping-press`, `zzping-view`) have been retired from the workspace, and a new `zzping-common` crate has been introduced to share common data structures, adhering to the DRY principle.

## Key Deviations from the Architectural Vision

While the high-level structure is in place, the implementation of each component represents a minimal vertical slice. Many of the advanced features and resilience mechanisms described in the vision document have been deferred, which is appropriate for an MVP.

### Collector Service (`zzping-collector`)

The current collector is functional but significantly simpler than the one envisioned.

-   **Configuration**: Configuration (target IP, rate) is provided via command-line arguments. The envisioned model of the collector receiving its configuration dynamically from the database has **not been implemented**.
-   **Resilience**: The collector attempts to reconnect to the database every 5 seconds if the connection fails. However, it does **not** have the envisioned in-memory buffer to ensure data continuity across restarts or disconnects. Any pings that occur during a disconnection are currently lost. The concepts of "fail-static" and "fail-close" modes have also **not been implemented**.
-   **Health Reporting**: The collector does **not** report its operational health metadata to the database.
-   **Pinger Backend**: The ping functionality uses `surge-ping` directly. The idea of a swappable backend for different ping methods (e.g., raw sockets) has **not been implemented**.

### Database Service (`zzping-database`)

The database fulfills its core MVP function of storing data and serving the most recent minute, but lacks most of the advanced management features.

-   **Data Storage**: The database correctly receives records, uses the `chunked-v1` format for compression, and stores the data in timestamped files. This is well-aligned with the vision.
-   **Offline Aggregation**: The `chunked-v1` format itself stores per-minute percentile data in its headers. However, the database does **not** perform any further offline aggregation to create coarser-grained (e.g., hourly, daily) summary files.
-   **Data Retention**: There is currently **no mechanism** for deleting old data based on a size or time threshold.
-   **Querying**: The database only supports a single, hardcoded query: `GET_LAST_MINUTE`. The vision for a more generic query API to retrieve arbitrary time ranges has **not been implemented**.
-   **Dynamic Configuration & Orchestration**: The database does **not** serve configuration to collectors or orchestrate multiple collectors.
-   **System Health**: The database does **not** track or expose the health status of connected collectors.

### GUI Service (`zzping-gui`)

The GUI successfully displays a live view of the data but is not yet a historical analysis tool.

-   **Data Display**: The GUI correctly connects to the database, fetches the last minute of data, and displays it in the existing plot widget. The evolution from a file-based viewer to a live, networked client is complete.
-   **Historical Data**: The GUI **cannot** query or navigate through historical data. It is limited to the single `GET_LAST_MINUTE` view.
-   **Live Configuration**: There is **no UI** for configuring the collector or database services.
-   **System Tray / Health**: The concepts of a system tray icon and a system health display have **not been implemented**.

### Cross-Cutting Concerns

Several major features from the vision document that span the entire system were deferred for the MVP.

-   **Authentication**: The HMAC challenge-response authentication mechanism has **not been implemented**. All connections are currently unauthenticated and unencrypted, which is a major deviation from the security goals of the vision document (though acceptable for an initial MVP on a trusted LAN).
-   **Zero-Downtime Updates**: The sophisticated mechanisms for ensuring zero data loss during service updates have **not been implemented**.
-   **UDP Heartbeat / Discovery**: The use of UDP for heartbeats or service discovery has **not been implemented**.

## Conclusion

The current codebase successfully implements the v0.3 MVP vertical slice. It provides a solid, functional foundation for the new architecture, with a clear data pipeline from collector to database to GUI. The code has been refactored to be modular and uses a shared common crate to avoid duplication.

However, the implementation intentionally deviates from the full "ZZPing Architectural Vision II" by omitting many advanced features related to resilience, dynamic configuration, historical data analysis, and security. These deferred features represent the logical next steps for evolving the project from an MVP into a robust, feature-complete monitoring solution. The current state is a strong starting point that aligns with the "Simplicity Over Complexity" guiding principle for an initial release.
