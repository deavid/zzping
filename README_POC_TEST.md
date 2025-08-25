# ZZPing v0.3 PoC Lab - Developer Testing Guide

Enable:

sudo sysctl -w net.ipv4.ping_group_range="0 2147483647"


----


This document provides instructions for compiling, running, and validating the three proof-of-concept (PoC) tools for the zzping v0.3 architecture.

## 1. Project Setup

Before you begin, ensure the workspace is correctly configured to build the new PoC tools. The legacy `zzping-gui` crate conflicts with the new dependencies and must be excluded from the build.

1.  Open the `Cargo.toml` file at the root of the project.
2.  Locate the `[workspace]` section and its `members` array.
3.  Ensure that `"zzping-gui"` is **removed** from the `members` array.
4.  Add the new PoC crates to the `members` array: `"zzping-capture"`, `"zzping-press"`, and `"zzping-view"`.

The `members` array should look similar to this:

```toml
[workspace]
resolver = "2"
members = [
    "zzping-lib",
    "zzping-daemon",
    "zzping-capture", # New
    "zzping-press",   # New
    "zzping-view",    # New
]
```

## 2. Testing Workflow

The testing process follows a three-step pipeline, using each tool to generate data for the next.

### Step 1: Generate Raw Data with `zzping-capture`

This tool will perform a live, high-frequency ping test and save the raw results. This will become our "ground truth" dataset.

**Action:**

1.  Navigate to the project root in your terminal.
2.  Compile the tool in release mode for best performance:
    ```bash
    cargo build --release --bin zzping-capture
    ```
3.  Run the capture tool. Choose a reliable target like `1.1.1.1` or `8.8.8.8`. A rate of 100 pps is a good test. Let it run for at least **1-2 minutes** to generate a reasonably sized data file.
    ```bash
    # This may require sudo on the first run to set capabilities,
    # or you may need to configure unprivileged ICMP on your system.
    ./target/release/zzping-capture --target 1.1.1.1 --rate 100
    ```
4.  After a few minutes, stop the process with **Ctrl+C**.

**Expected Outcome:**

*   You will see status messages printed to the console (e.g., "Starting capture...", "Creating new log file...").
*   A new directory named `capture_logs/` will be created in your project root.
*   Inside this directory, there will be a new data file named similar to `1.1.1.1-20250827.dat`. This file contains the raw, high-precision ping data.

### Step 2: Compress Data with `zzping-press`

This tool will take the raw data file from Step 1 and compress it using our target `delta_quantized_v1` format.

**Action:**

1.  Compile the tool in release mode:
    ```bash
    cargo build --release --bin zzping-press
    ```
2.  Run the tool, providing the raw data file as input and specifying an output path.
    ```bash
    ./target/release/zzping-press \
      --input ./capture_logs/1.1.1.1-20250827.dat \
      --output ./capture_logs/1.1.1.1-20250827.compressed.dat \
      --strategy delta_quantized_v1
    ```

**Expected Outcome:**

*   A new compressed file (`.compressed.dat`) will be created in the `capture_logs/` directory.
*   The tool will print verification metrics to the console. The output should look similar to this:

    ```
    Successfully wrote 12000 compressed records to capture_logs/1.1.1.1-20250827.compressed.dat.

    --- Verification Metrics ---
    Original size:    192016 bytes
    Compressed size:  48016 bytes
    Compression ratio: 4.00:1
    RTT Precision Loss (RMSE): 0.00 microseconds
    ```
*   **Key Validation:** The **Compression ratio** should be exactly **4.00:1**. The **RMSE** should be **0.00 microseconds**, as our `delta_quantized_v1` strategy (for RTTs under ~65ms) is lossless.

### Step 3: Visualize Data with `zzping-view`

This final step validates the GUI's performance by loading and rendering the compressed data file.

**Action:**

1.  Compile the tool in release mode:
    ```bash
    cargo build --release --bin zzping-view
    ```
2.  Run the viewer, passing the path to the **compressed** data file as the argument.
    ```bash
    ./target/release/zzping-view ./capture_logs/1.1.1.1-20250827.compressed.dat
    ```

**Expected Outcome:**

*   A GUI window will open, displaying the ping data as a graph.
*   **Interactivity Test:**
    *   Manipulate the **Zoom** slider. The graph should smoothly zoom in and out. At high zoom levels, you should see the rendering switch from min/max bars to individual points.
    *   Manipulate the **Pan** slider. The view should scroll smoothly across the dataset, even at high zoom levels.
*   **Performance Validation:** The application should feel responsive. Panning and zooming should not cause noticeable lag or stuttering. The primary goal is to confirm that the `egui`-based renderer can handle the dataset without performance issues.

If all three steps complete successfully and the outcomes match these descriptions, the PoC Lab has successfully validated the core technical risks of the v0.3 architecture.