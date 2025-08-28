# ZZPing Chunked V1 Format Specification

## Overview

The Chunked V1 format is a lossy compression format designed for ping/network latency data that provides high compression ratios while maintaining acceptable accuracy for network performance analysis. The format is optimized for time-series data with predictable patterns and supports both constant-rate and variable-rate ping sequences.

## Design Goals

1. **Lossy Compression**: Achieve high compression ratios by using quantized RTT values
2. **Time Accuracy**: Maintain timing precision with controlled drift accumulation
3. **Packet Loss Representation**: Accurately preserve packet loss events and timing
4. **Minute-based Chunking**: Organize data in 60-second chunks for efficient access
5. **Fallback Modes**: Graceful degradation when compression assumptions fail

## Data Model

### Input Data
- **RawDataRecord**: Contains `sent_nanos` (u64) and `rtt_nanos` (u64)
- **Packet Loss**: Represented by `rtt_nanos = u64::MAX`
- **Temporal Order**: Records must be sorted by `sent_nanos`

### Key Constants
```rust
FILE_MAGIC: u64 = 0x5A5A504356312020    // "ZZPCV1  "
FORMAT_VERSION: u16 = 1
HEADER_SIZE: usize = 65536               // 64KB header
PACKET_LOST_SYMBOL: u16 = 65535
DRIFT_THRESHOLD_NS: f64 = 2_000_000.0    // 2ms (TODO: consider reducing to 1ms)
```

**TODO**: The drift threshold could be reduced to 1ms for better accuracy while keeping test tolerance at 2ms.

## Compression Strategy

### 1. Chunking
- Data is divided into 60-second chunks based on `sent_nanos`
- Each chunk is compressed independently
- Chunk boundary: `minute_index = sent_nanos / (60 * 1_000_000_000)`

### 2. RTT Quantization
RTT values are quantized using logarithmic compression:
```rust
// Encoding: time_in_ms -> symbol
symbol = ((time_in_ms / 100.0 + 1.0).ln() / ln(1.001)).round()

// Decoding: symbol -> time_in_ms
time_in_ms = (ln(1.001) * symbol).exp() - 1.0) * 100.0
```

**Quantization Properties:**
- Higher precision for smaller RTT values
- Symbol 0 represents very small RTTs (< 0.1ms)
- Symbol 65535 reserved for packet loss
- Precision degrades gracefully with larger RTTs

### 3. Time Delta Compression
Two strategies based on timing regularity:

**Constant Rate Strategy:**
- Used when max drift ≤ 2ms across the chunk
- Stores only: `start_time` + `average_interval`
- Reconstructed as: `sent_time[i] = start_time + i * avg_interval`

**Variable Rate Strategy (Fallback):**
- Used when timing is irregular (drift > 2ms)
- **FIXME**: Currently stores raw delta values as u64 nanoseconds (~8 bytes per record)
- **TODO**: Should use logarithmic quantization like RTT values for better compression
- Much less efficient than constant rate due to raw u64 storage
- Preserves exact timing but at significant storage cost

### 4. Statistical Modeling
Each chunk includes aggregate statistics for entropy coding:
- **Percentiles**: P0, P10, P20, ..., P90, P100 of RTT symbols (u16 each = 22 bytes)
- **Lost Packet Count**: Number of lost packets in chunk (u32 = 4 bytes)
- **Total per chunk**: 26 bytes of statistics
- **Frequency Distribution**: Built from percentile ranges for entropy coding

**Design Decision**: The u16 percentile symbols provide full quantizer symbol range (0-65534) which is essential for precise frequency estimation. Compacting to u8 would limit the symbol range and potentially degrade compression efficiency. The u32 lost packet count ensures accurate frequency modeling for all practical data rates without overflow concerns.

## File Format Structure

### File Header (64KB)
```
Offset | Size | Field                    | Description
-------|------|--------------------------|----------------------------------
0      | 8    | magic                    | FILE_MAGIC constant
8      | 2    | format_version           | FORMAT_VERSION constant
10     | 8    | start_time_unix_ns       | First record timestamp
18     | 4    | aggregate_entry_count    | Number of aggregate entries
22     | 4    | index_entry_count        | Number of chunks
26     | ...  | aggregate_entries        | Per-chunk statistics
...    | ...  | index_entries            | Chunk offset table
```

### Aggregate Entry (26 bytes each)
```
Offset | Size | Field           | Description
-------|------|-----------------|----------------------------------
0      | 2    | p00_symbol      | 0th percentile RTT symbol (u16)
2      | 2    | p10_symbol      | 10th percentile RTT symbol (u16)
...    | ...  | ...             | ... (11 percentiles total)
20     | 2    | p100_symbol     | 100th percentile RTT symbol (u16)
22     | 4    | lost_packet_count| Number of lost packets (u32)
```

**Note**: The 26-byte aggregate entry size is optimized for compression efficiency. u16 percentiles support the full quantizer symbol range, and u32 lost packet count ensures precise frequency modeling.

### Index Entry (8 bytes each)
```
Offset | Size | Field                | Description
-------|------|----------------------|---------------------------
0      | 8    | chunk_offset_bytes   | Byte offset to chunk data
```

### Chunk Format
```
Offset | Size | Field                     | Description
-------|------|---------------------------|----------------------------------
0      | 8    | minute_boundary_unix_ns   | Unix timestamp rounded to minute boundary
8      | 4    | first_ping_offset_ns      | Nanoseconds from boundary to first ping (0-59999999999)
12     | 4    | rtt_symbol_count          | Number of RTT symbols
16     | 4    | send_time_symbol_count    | Number of time delta symbols
20     | 4    | rtt_stream_len_bytes      | RTT encoded data length
24     | 4    | send_time_stream_len_bytes| Time delta data length
28     | 1    | flags                     | Chunk flags (see below)
29     | 8    | base_interval_ns          | Base interval in nanoseconds (exact integer)
37     | 26   | rtt_stats                 | RTT aggregate entry
63     | 26?  | send_time_stats           | Time delta stats (if variable rate)
...    | ...  | rtt_encoded_data          | Entropy-coded RTT symbols
...    | ...  | send_time_encoded_data    | Quantized time deltas (1ns precision)
```

**✅ FIXED**: Now uses minute boundary + nanosecond offset for perfect timing accuracy
**✅ FIXED**: send_time_encoded_data now uses quantized 1ns deltas instead of raw u64 values
```

### Chunk Flags
```
Bit | Name           | Description
----|----------------|--------------------------------------------------
0   | IS_VARIABLE_RATE| Use variable rate (raw deltas) vs constant rate
1   | RAW_DELTAS     | Time deltas stored as raw u64 values
```

## Quality Guarantees

### RTT Accuracy
- **Target Tolerance**: ≤ 0.1% relative error OR ≤ 0.1ms absolute error
- **Implementation**: `max(0.5ms, original_rtt * 0.002)` tolerance
- **Packet Loss**: Preserved exactly (no quantization error)

### Time Drift Control
- **Maximum Cumulative Drift**: ≤ 20ms over entire dataset
- **Per-Chunk Drift**: ≤ 2ms triggers fallback to variable rate
- **Drift Accumulation**: Tracked across all records in sequence

**CLARIFICATION NEEDED**: Why is cumulative drift (20ms) larger than per-chunk threshold (2ms)?
**EXPECTED**: Each chunk should reset drift accumulation, so cumulative should not exceed per-chunk limits.
**TODO**: Review drift accumulation logic and specification for consistency.

### Packet Loss Preservation
- **Loss Detection**: `rtt_nanos == u64::MAX` → symbol 65535
- **Timing Accuracy**: Lost packet timestamps maintain same precision as successful packets
- **Count Accuracy**: Exact count preserved in aggregate statistics

## Fallback Mechanisms

### Variable Rate Fallback
- **Trigger**: When send time drift > 2ms in any chunk
- **Behavior**: Store raw u64 time deltas instead of constant rate
- **Overhead**: ~8 bytes per record vs ~0 bytes for constant rate (**FIXME**: Very inefficient)

### Single Symbol Handling
- **Issue**: Entropy coding requires ≥2 symbols
- **Solution**: Add dummy symbol with frequency 1
- **Impact**: Minimal compression efficiency loss
- **FIXME**: Need to ensure real symbol has frequency >> 1 (orders of magnitude higher)
- **TODO**: Verify frequency ratios are sufficient for good compression efficiency

### Empty Chunk Handling
- **Minutes without data**: Supported via empty chunks
- **Index entries**: Still created with appropriate offsets
- **Reconstruction**: No records generated for empty chunks

## Implementation Notes

### Entropy Coding
- Uses ANS (Asymmetric Numeral Systems) for RTT symbol compression
- Probability model derived from percentile statistics
- Symbols with zero frequency excluded from model

### Precision Trade-offs
- RTT quantization provides ~1000:1 precision/compression trade-off
- Time quantization provides high compression for regular ping intervals
- Statistical modeling adapts to actual data distribution

### Error Accumulation
- Quantization errors are independent per record
- Time drift accumulates linearly but is bounded per chunk
- Worst-case error bounds are deterministic and measurable

## Test Requirements

Based on this specification, comprehensive tests must verify:

1. **Quantization Accuracy**: RTT reconstruction within tolerance bounds
2. **Time Drift Control**: Cumulative drift stays within 20ms limit
3. **Packet Loss Fidelity**: Exact preservation of loss events and timing
4. **Fallback Behavior**: Variable rate triggering and operation
5. **Edge Cases**: Empty chunks, single symbols, extreme values
6. **Round-trip Consistency**: Compress→decompress yields acceptable results
7. **Statistical Model Validity**: Entropy coding model construction
8. **Format Compliance**: File structure matches specification exactly

## Version History

- **v1.0**: Initial specification with logarithmic RTT quantization and dual time strategies
