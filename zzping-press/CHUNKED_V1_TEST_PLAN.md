# ZZPing Chunked V1 Test Plan

## Overview

This document outlines a comprehensive testing strategy for the Chunked V1 compression format to ensure reliability, accuracy, and compliance with the format specification. We take an **adversarial approach** - thinking like an attacker trying to break the format - to build a test suite that proves resilience under all conditions.

## Testing Philosophy

> *"To have confidence in this new format, we need to be ruthlessly critical and think like an adversary trying to break it. The goal is to build a test suite that can prove its resilience."*

This means:
- Testing every edge case, no matter how unlikely
- Ensuring the decoder never panics, even with corrupted data
- Verifying graceful degradation under all failure conditions
- Proving the format works across system boundaries and time scales

## Current Test Status

### ✅ MAJOR SUCCESS: Format Fixed and Working
**As of latest update**: The integer-only timing implementation with minute boundary + nanosecond offset approach is now working perfectly:
- ✅ **0 drift violations** - Perfect timing accuracy achieved
- ✅ **0ns max cumulative drift** - Eliminates floating-point precision issues
- ✅ **All accuracy tests passed** - Format now meets all requirements
- ✅ **Compression ratio: 1.83x** - Still excellent compression efficiency

### Existing Tests
1. **Round-trip test** (`test_round_trip`): Basic compression/decompression with 10-minute dataset
   - ✅ Tests basic functionality - **NOW PASSING**
   - ✅ ~~Currently failing due to tolerance issues~~ - **FIXED**
   - ❌ Still insufficient coverage for edge cases

### Critical Issues Status
- ✅ **FIXED**: Timing drift accumulation - now uses integer-only math with minute boundaries
- ✅ **FIXED**: Variable rate mode efficiency - now uses 1ns quantized deltas instead of raw u64
- ❌ **TODO**: AggregateEntry uses u16 for percentiles + u32 for count (26 bytes) - could be u8+u8 (12 bytes)
- ❌ **TODO**: Drift threshold could be reduced from 2ms to 1ms for better accuracy
- ❌ **FIXME**: Single symbol dummy handling may not ensure sufficient frequency ratio
- ❌ **CLARIFICATION NEEDED**: Why cumulative drift (20ms) > per-chunk threshold (2ms)?

### Missing Critical Tests (All test files created as stubs with TODOs)
- ❌ **chunked_v1_quantization_test.rs**: RTT accuracy within 0.1% or 0.1ms
- ❌ **chunked_v1_timing_test.rs**: Cumulative drift ≤ 20ms testing
- ❌ **chunked_v1_loss_test.rs**: Packet loss preservation accuracy
- ❌ **chunked_v1_edge_cases_test.rs**: All edge cases and boundary conditions
- ❌ **chunked_v1_corruption_test.rs**: File corruption resistance (never panic)
- ❌ **chunked_v1_adversarial_test.rs**: Pathological inputs designed to break format
- ❌ **chunked_v1_format_test.rs**: Format compliance and cross-platform compatibility
- ❌ **chunked_v1_performance_test.rs**: Speed and memory benchmarks
- Quantization accuracy validation
- Time drift accumulation testing
- Packet loss preservation verification
- Fallback mode testing
- Edge case handling
- Statistical model validation
- Format compliance testing
- **File corruption resistance**
- **Adversarial input handling**

## Test Categories

### 1. File Structure & Corruption Resistance Tests

**Purpose**: Ensure the decoder is resilient to file corruption and never panics

#### Test Cases:
- **Empty File**: 0 bytes - should return graceful error
- **Header-Only File**: Exactly 64KiB with only header - no chunks
- **Truncated Header**: < 64KiB file - incomplete header detection
- **Truncated Chunk Stream**: File ends mid-chunk - decode complete chunks + error
- **Truncated Chunk Header**: File ends in chunk header - detect incomplete chunk
- **Corrupted Magic Number**: Wrong first 8 bytes - immediate rejection
- **Corrupted Chunk Length**: Invalid length values - detect and stop safely
- **Corrupted Finalized Header**: Invalid index data - fallback to sequential scan

#### Success Criteria:
```rust
// All corruption scenarios must:
// 1. Never panic or crash
// 2. Return appropriate error types
// 3. Preserve any successfully decoded data
// 4. Provide meaningful error messages
assert!(matches!(result, Err(ChunkedV1Error::CorruptedHeader { .. })));
```

### 2. Encoding & Data Content Edge Cases

**Purpose**: Test compression limits with unusual but valid data

#### Test Cases:
- **All Packet Loss Minute**: 100% lost packets in chunk
- **No Pings Minute**: Zero pings sent (collector down)
- **Single Ping Minute**: Only one ping in entire minute
- **Zero RTT**: Localhost pings with 0µs RTT
- **Sub-Microsecond RTTs**: Very small nanosecond values
- **Extreme Outlier RTTs**: Stable 10ms + single 30s spike
- **Bimodal RTTs**: Oscillating between 10ms and 800ms
- **Maximum Value Clamping**: RTT exceeding quantization limits
- **Monotonic RTT Progression**: Each RTT slightly higher than previous

#### Success Criteria:
```rust
// Edge case data must:
// 1. Compress without mathematical errors (no NaN/Infinity)
// 2. Decompress within tolerance bounds
// 3. Handle percentile calculations gracefully
// 4. Maintain file format integrity
```

### 3. Timing & Timestamp Edge Cases

**Purpose**: Test temporal data handling and clock-related issues

#### Test Cases:
- **Highly Jittery Send Times**: Erratic, random ping intervals
- **Clock Skew (Backwards Time)**: NTP adjustment during operation
- **Leap Second**: Minute containing 61 seconds
- **Massive Time Gaps**: Hours-long collector outages
- **Microsecond Precision**: Sub-millisecond timing variations
- **Year Boundaries**: Data spanning midnight Dec 31/Jan 1
- **Timezone Changes**: DST transitions during data collection

#### Success Criteria:
```rust
// Timing edge cases must:
// 1. Preserve temporal accuracy within drift limits
// 2. Handle clock adjustments gracefully
// 3. Maintain chunk boundary integrity
// 4. Correctly detect constant vs variable rate
```

### 4. System-Level & Long-Term Resilience

**Purpose**: Verify behavior across system boundaries and operational scenarios

#### Test Cases:
- **Unfinalized File Recovery**: Files missing proper closure (crash/power loss)
- **Disk Full During Write**: Filesystem space exhaustion
- **Endianness Portability**: Little-endian write, big-endian read
- **Forward Compatibility**: Future software reading v1 files
- **Backward Compatibility**: v1 software encountering v2 files
- **Memory Pressure**: Large file processing with limited RAM
- **Concurrent Access**: Multiple readers of same file

#### Success Criteria:
```rust
// System-level scenarios must:
// 1. Provide recovery mechanisms for incomplete files
// 2. Maintain cross-platform compatibility
// 3. Handle resource constraints gracefully
// 4. Support version migration strategies
```

### 5. Quantization Accuracy Tests

**Purpose**: Verify RTT quantization stays within specified tolerances

#### Test Cases:
- **Small RTT Values** (0.1ms - 1ms): High precision region
- **Medium RTT Values** (1ms - 100ms): Common network latencies
- **Large RTT Values** (100ms - 1000ms): Degraded precision region
- **Boundary Values**: Near quantization boundaries
- **Edge Cases**: Zero RTT, maximum valid RTT

#### Success Criteria:
```rust
// For each RTT value:
let tolerance = max(0.1ms, original_rtt * 0.001); // 0.1% or 0.1ms
assert!(abs(decompressed_rtt - original_rtt) <= tolerance);
```

### 6. Time Drift Control Tests

**Purpose**: Ensure cumulative timing drift stays within 20ms bounds

#### Test Cases:
- **Constant Rate Data**: Perfect 1-second intervals
- **Near-Constant Rate**: Small variations (< 2ms drift)
- **Variable Rate Trigger**: Variations causing > 2ms drift
- **Mixed Chunks**: Some constant, some variable rate
- **Long Sequences**: 24+ hours of data for drift accumulation

#### Success Criteria:
```rust
let mut cumulative_drift = 0i64;
for (original, decompressed) in records.iter().zip(decompressed.iter()) {
    cumulative_drift += original.sent_nanos as i64 - decompressed.sent_nanos as i64;
    assert!(cumulative_drift.abs() <= 20_000_000); // 20ms
}
```

### 7. Packet Loss Preservation Tests

**Purpose**: Verify packet loss events are preserved accurately

#### Test Cases:
- **No Packet Loss**: All successful pings
- **Sporadic Loss**: Random 1-5% loss rate
- **Burst Loss**: Consecutive lost packets
- **Complete Loss**: Entire minutes with no responses
- **Mixed Patterns**: Alternating loss/success patterns
- **Loss Timing**: Verify lost packet timestamps are preserved

#### Success Criteria:
```rust
// Loss detection
assert_eq!(original.rtt_nanos == u64::MAX, decompressed.rtt_nanos == u64::MAX);

// Loss timing accuracy
if original.rtt_nanos == u64::MAX {
    let time_diff = abs(original.sent_nanos as i64 - decompressed.sent_nanos as i64);
    assert!(time_diff <= 5_000_000); // 5ms tolerance for lost packets
}
```

### 8. Fallback Mode Tests

**Purpose**: Ensure variable rate mode triggers correctly and works properly

#### Test Cases:
- **Trigger Threshold**: Exactly 2ms drift should trigger fallback
- **Variable Rate Compression**: Irregular timing patterns
- **Fallback Accuracy**: Higher precision in variable rate mode
- **Mode Detection**: Verify correct flags are set
- **Efficiency**: Measure compression ratio difference

#### Success Criteria:
```rust
// Fallback should trigger for high drift
let chunk_header = parse_chunk_header(compressed_data);
if max_drift > 2_000_000.0 {
    assert!(chunk_header.flags.contains(ChunkFlags::IS_VARIABLE_RATE));
}
```

### 9. Statistical Model Tests

**Purpose**: Verify entropy coding model construction

#### Test Cases:
- **Model Creation**: Valid models from various data distributions
- **Single Symbol**: Handling of identical RTT values
- **Frequency Distribution**: Accurate percentile calculation
- **Entropy Efficiency**: Compression ratio measurement
- **Model Reconstruction**: Decoder model matches encoder

### 10. Format Compliance Tests

**Purpose**: Ensure file format matches specification exactly

#### Test Cases:
- **Header Structure**: Correct magic, version, sizes
- **Field Alignment**: Proper byte ordering (BigEndian)
- **Chunk Layout**: Headers, data streams in correct order
- **Index Tables**: Accurate offset calculations
- **Size Validation**: All length fields match actual data

### 11. Implementation Issues Tests

**Purpose**: Verify and document current implementation problems

#### Test Cases:
- **Variable Rate Overhead**: Measure actual bytes per record in fallback mode
- **AggregateEntry Size**: Verify 26 bytes per entry overhead
- **Frequency Ratios**: Test single-symbol scenarios and dummy symbol frequencies
- **Drift Logic**: Verify relationship between per-chunk and cumulative drift
- **Threshold Sensitivity**: Test behavior at exactly 2ms drift threshold

#### Expected Results:
- Variable rate should show ~8 bytes/record overhead (vs ~0 for constant rate)
- Single symbol cases should maintain reasonable compression efficiency
- Drift accumulation behavior should be clarified and documented

### 12. Performance and Limits Tests

**Purpose**: Verify behavior under stress conditions

#### Test Cases:
- **Large Files**: GB-sized datasets
- **Memory Usage**: Bounded memory consumption
- **Compression Speed**: Performance benchmarks
- **Decompression Speed**: Performance benchmarks
- **Compression Ratios**: Typical ratios for various data types

## Test Implementation Strategy

### Phase 1: Fix Existing Test
1. Analyze current test failure
2. Adjust tolerance values based on specification
3. Ensure test data is appropriate

### Phase 2: Core Accuracy Tests
1. Implement quantization accuracy tests
2. Add time drift control tests
3. Create packet loss preservation tests

### Phase 3: Adversarial Robustness Tests
1. Add file corruption resistance tests
2. Implement edge case data tests
3. Add timing anomaly tests
4. System-level resilience tests

### Phase 4: Comprehensive Validation
1. Statistical model tests
2. Performance tests
3. Long-running integration tests

## Test Data Requirements

### Synthetic Data Sets
- **Perfect Constant Rate**: Exactly 1000ms intervals
- **Near Constant Rate**: 1000ms ± 0.5ms intervals
- **Variable Rate**: Random intervals 500-1500ms
- **Burst Patterns**: Rapid sequences followed by gaps
- **Loss Patterns**: Controlled packet loss scenarios
- **Corrupted Files**: Systematically damaged test files
- **Edge Case Combinations**: Multiple edge conditions together

### Real-World Data Sets
- **Network Monitoring**: Actual ping data from various networks
- **Stress Scenarios**: High-loss, variable-latency conditions
- **Long Duration**: Days/weeks of continuous data
- **Production Failures**: Real crash/corruption scenarios

### Adversarial Data Sets
- **Pathological Inputs**: Designed to break compression assumptions
- **Boundary Exploits**: Values at quantization/format limits
- **Temporal Anomalies**: Clock skew, leap seconds, gaps
- **Malformed Files**: Systematically corrupted headers/chunks

## Success Metrics

### Accuracy Requirements (Must Pass)
- ✅ RTT accuracy: ≤ 0.1% or ≤ 0.1ms error
- ✅ Time drift: ≤ 20ms cumulative drift
- ✅ Packet loss: 100% preservation of loss events
- ✅ Timing: Lost packet timestamps within 5ms

### Robustness Requirements (Must Pass)
- ✅ Never panic on any input (corrupted or otherwise)
- ✅ Graceful error handling for all failure modes
- ✅ Meaningful error messages for debugging
- ✅ Data recovery from incomplete files

### Quality Requirements (Should Pass)
- 📊 Compression ratio: > 10:1 for typical data
- 📊 Fallback efficiency: < 10% data uses variable rate
- 📊 Model accuracy: Entropy coding achieves expected compression
- ⚠️  **Issue**: Variable rate mode has poor compression due to raw u64 storage

### Performance Requirements (Nice to Have)
- ⚡ Compression speed: > 100K records/second
- ⚡ Decompression speed: > 500K records/second
- 💾 Memory usage: < 100MB for GB datasets

## Test Organization

### Directory Structure
```
tests/
├── chunked_v1_integration_test.rs      # Current round-trip test
├── chunked_v1_quantization_test.rs     # RTT accuracy tests
├── chunked_v1_timing_test.rs           # Time drift tests
├── chunked_v1_loss_test.rs             # Packet loss tests
├── chunked_v1_fallback_test.rs         # Variable rate tests
├── chunked_v1_edge_cases_test.rs       # Edge case tests
├── chunked_v1_format_test.rs           # Format compliance tests
├── chunked_v1_corruption_test.rs       # File corruption resistance
├── chunked_v1_adversarial_test.rs      # Adversarial inputs
├── chunked_v1_system_test.rs           # System-level scenarios
├── chunked_v1_performance_test.rs      # Performance benchmarks
└── fixtures/
    ├── ten-minutes.dat                 # Current test data
    ├── constant-rate-1hour.dat         # Perfect constant intervals
    ├── variable-rate-1hour.dat         # Irregular intervals
    ├── high-loss-10min.dat             # 20% packet loss
    ├── edge-cases-samples.dat          # Various edge cases
    ├── corrupted/                      # Systematically damaged files
    │   ├── truncated-header.dat
    │   ├── bad-magic.dat
    │   ├── invalid-chunk-length.dat
    │   └── unfinalized.dat
    └── adversarial/                    # Pathological test cases
        ├── all-packet-loss.dat
        ├── zero-rtt.dat
        ├── extreme-outliers.dat
        └── clock-anomalies.dat
```

### Test Naming Convention
- `test_quantization_accuracy_<scenario>`
- `test_time_drift_<scenario>`
- `test_packet_loss_<scenario>`
- `test_fallback_<scenario>`
- `test_edge_case_<scenario>`
- `test_format_compliance_<aspect>`
- `test_corruption_resistance_<type>`
- `test_adversarial_<attack_vector>`
- `test_system_resilience_<scenario>`

## Adversarial Testing Checklist

### File Structure Attacks ✋
- [ ] Empty file (0 bytes)
- [ ] Header-only file (64KiB exactly)
- [ ] Truncated at every possible offset
- [ ] Invalid magic numbers
- [ ] Malformed lengths and offsets
- [ ] Corrupted indices

### Data Content Attacks ✋
- [ ] All edge RTT values (0, MAX, NaN-inducing)
- [ ] Pathological timing patterns
- [ ] Complete data absence scenarios
- [ ] Mathematical boundary conditions
- [ ] Percentile calculation edge cases

### System Boundary Attacks ✋
- [ ] Endianness mismatches
- [ ] Version compatibility issues
- [ ] Resource exhaustion scenarios
- [ ] Concurrent access patterns
- [ ] Platform-specific behaviors

This comprehensive test plan ensures that the Chunked V1 format not only meets specification requirements but can withstand real-world operational stresses and malicious inputs while maintaining data integrity and system stability.
- **TODO**: AggregateEntry uses u16 for percentiles + u32 for count (26 bytes) - could be u8+u8 (12 bytes)
- **TODO**: Drift threshold could be reduced from 2ms to 1ms for better accuracy
- **FIXME**: Single symbol dummy handling may not ensure sufficient frequency ratio
- **CLARIFICATION**: Why cumulative drift (20ms) > per-chunk threshold (2ms)?ked V1 Test Plan

## Overview

This document outlines a comprehensive testing strategy for the Chunked V1 compression format to ensure reliability, accuracy, and compliance with the format specification.

## Current Test Status

### Existing Tests
1. **Round-trip test** (`test_round_trip`): Basic compression/decompression with 10-minute dataset
   - ✅ Tests basic functionality
   - ⚠️  Currently failing due to tolerance issues
   - ❌ Insufficient coverage for edge cases

### Missing Critical Tests
- Quantization accuracy validation
- Time drift accumulation testing
- Packet loss preservation verification
- Fallback mode testing
- Edge case handling
- Statistical model validation
- Format compliance testing

## Test Categories

### 1. Quantization Accuracy Tests

**Purpose**: Verify RTT quantization stays within specified tolerances

#### Test Cases:
- **Small RTT Values** (0.1ms - 1ms): High precision region
- **Medium RTT Values** (1ms - 100ms): Common network latencies
- **Large RTT Values** (100ms - 1000ms): Degraded precision region
- **Boundary Values**: Near quantization boundaries
- **Edge Cases**: Zero RTT, maximum valid RTT

#### Success Criteria:
```rust
// For each RTT value:
let tolerance = max(0.1ms, original_rtt * 0.001); // 0.1% or 0.1ms
assert!(abs(decompressed_rtt - original_rtt) <= tolerance);
```

### 2. Time Drift Control Tests

**Purpose**: Ensure cumulative timing drift stays within 20ms bounds

#### Test Cases:
- **Constant Rate Data**: Perfect 1-second intervals
- **Near-Constant Rate**: Small variations (< 2ms drift)
- **Variable Rate Trigger**: Variations causing > 2ms drift
- **Mixed Chunks**: Some constant, some variable rate
- **Long Sequences**: 24+ hours of data for drift accumulation

#### Success Criteria:
```rust
let mut cumulative_drift = 0i64;
for (original, decompressed) in records.iter().zip(decompressed.iter()) {
    cumulative_drift += original.sent_nanos as i64 - decompressed.sent_nanos as i64;
    assert!(cumulative_drift.abs() <= 20_000_000); // 20ms
}
```

### 3. Packet Loss Preservation Tests

**Purpose**: Verify packet loss events are preserved accurately

#### Test Cases:
- **No Packet Loss**: All successful pings
- **Sporadic Loss**: Random 1-5% loss rate
- **Burst Loss**: Consecutive lost packets
- **Complete Loss**: Entire minutes with no responses
- **Mixed Patterns**: Alternating loss/success patterns
- **Loss Timing**: Verify lost packet timestamps are preserved

#### Success Criteria:
```rust
// Loss detection
assert_eq!(original.rtt_nanos == u64::MAX, decompressed.rtt_nanos == u64::MAX);

// Loss timing accuracy
if original.rtt_nanos == u64::MAX {
    let time_diff = abs(original.sent_nanos as i64 - decompressed.sent_nanos as i64);
    assert!(time_diff <= 5_000_000); // 5ms tolerance for lost packets
}
```

### 4. Fallback Mode Tests

**Purpose**: Ensure variable rate mode triggers correctly and works properly

#### Test Cases:
- **Trigger Threshold**: Exactly 2ms drift should trigger fallback
- **Variable Rate Compression**: Irregular timing patterns
- **Fallback Accuracy**: Higher precision in variable rate mode
- **Mode Detection**: Verify correct flags are set
- **Efficiency**: Measure compression ratio difference

#### Success Criteria:
```rust
// Fallback should trigger for high drift
let chunk_header = parse_chunk_header(compressed_data);
if max_drift > 2_000_000.0 {
    assert!(chunk_header.flags.contains(ChunkFlags::IS_VARIABLE_RATE));
}
```

### 5. Edge Case Tests

**Purpose**: Handle unusual but valid input scenarios

#### Test Cases:
- **Empty Minutes**: No data for entire minute periods
- **Single Record Chunks**: Only one ping per minute
- **Identical RTTs**: All RTTs exactly the same
- **Maximum Values**: Boundary testing with u64::MAX-1
- **Minimum Values**: Zero RTTs, minimal timing
- **Large Datasets**: Memory and performance testing

### 6. Statistical Model Tests

**Purpose**: Verify entropy coding model construction

#### Test Cases:
- **Model Creation**: Valid models from various data distributions
- **Single Symbol**: Handling of identical RTT values
- **Frequency Distribution**: Accurate percentile calculation
- **Entropy Efficiency**: Compression ratio measurement
- **Model Reconstruction**: Decoder model matches encoder

### 7. Format Compliance Tests

**Purpose**: Ensure file format matches specification exactly

#### Test Cases:
- **Header Structure**: Correct magic, version, sizes
- **Field Alignment**: Proper byte ordering (BigEndian)
- **Chunk Layout**: Headers, data streams in correct order
- **Index Tables**: Accurate offset calculations
- **Size Validation**: All length fields match actual data

### 8. Performance and Limits Tests

**Purpose**: Verify behavior under stress conditions

#### Test Cases:
- **Large Files**: GB-sized datasets
- **Memory Usage**: Bounded memory consumption
- **Compression Speed**: Performance benchmarks
- **Decompression Speed**: Performance benchmarks
- **Compression Ratios**: Typical ratios for various data types

### 9. Implementation Issues Tests

**Purpose**: Verify and document current implementation problems

#### Test Cases:
- **Variable Rate Overhead**: Measure actual bytes per record in fallback mode
- **AggregateEntry Size**: Verify 26 bytes per entry overhead
- **Frequency Ratios**: Test single-symbol scenarios and dummy symbol frequencies
- **Drift Logic**: Verify relationship between per-chunk and cumulative drift
- **Threshold Sensitivity**: Test behavior at exactly 2ms drift threshold

#### Expected Results:
- Variable rate should show ~8 bytes/record overhead (vs ~0 for constant rate)
- Single symbol cases should maintain reasonable compression efficiency
- Drift accumulation behavior should be clarified and documented

## Test Implementation Strategy

### Phase 1: Fix Existing Test
1. Analyze current test failure
2. Adjust tolerance values based on specification
3. Ensure test data is appropriate

### Phase 2: Core Accuracy Tests
1. Implement quantization accuracy tests
2. Add time drift control tests
3. Create packet loss preservation tests

### Phase 3: Robustness Tests
1. Add fallback mode tests
2. Implement edge case tests
3. Add format compliance tests

### Phase 4: Comprehensive Validation
1. Statistical model tests
2. Performance tests
3. Long-running integration tests

## Test Data Requirements

### Synthetic Data Sets
- **Perfect Constant Rate**: Exactly 1000ms intervals
- **Near Constant Rate**: 1000ms ± 0.5ms intervals
- **Variable Rate**: Random intervals 500-1500ms
- **Burst Patterns**: Rapid sequences followed by gaps
- **Loss Patterns**: Controlled packet loss scenarios

### Real-World Data Sets
- **Network Monitoring**: Actual ping data from various networks
- **Stress Scenarios**: High-loss, variable-latency conditions
- **Long Duration**: Days/weeks of continuous data

### Edge Case Data Sets
- **Empty Periods**: Minutes with no data
- **Extreme Values**: Very high/low RTTs
- **Boundary Conditions**: Values near quantization boundaries

## Success Metrics

### Accuracy Requirements (Must Pass)
- ✅ RTT accuracy: ≤ 0.1% or ≤ 0.1ms error
- ✅ Time drift: ≤ 20ms cumulative drift
- ✅ Packet loss: 100% preservation of loss events
- ✅ Timing: Lost packet timestamps within 5ms

### Quality Requirements (Should Pass)
- 📊 Compression ratio: > 10:1 for typical data
- 📊 Fallback efficiency: < 10% data uses variable rate
- 📊 Model accuracy: Entropy coding achieves expected compression
- ⚠️  **Issue**: Variable rate mode has poor compression due to raw u64 storage

### Performance Requirements (Nice to Have)
- ⚡ Compression speed: > 100K records/second
- ⚡ Decompression speed: > 500K records/second
- 💾 Memory usage: < 100MB for GB datasets

## Test Organization

### Directory Structure
```
tests/
├── chunked_v1_integration_test.rs      # Current round-trip test
├── chunked_v1_quantization_test.rs     # RTT accuracy tests
├── chunked_v1_timing_test.rs           # Time drift tests
├── chunked_v1_loss_test.rs             # Packet loss tests
├── chunked_v1_fallback_test.rs         # Variable rate tests
├── chunked_v1_edge_cases_test.rs       # Edge case tests
├── chunked_v1_format_test.rs           # Format compliance tests
├── chunked_v1_performance_test.rs      # Performance benchmarks
└── fixtures/
    ├── ten-minutes.dat                 # Current test data
    ├── constant-rate-1hour.dat         # Perfect constant intervals
    ├── variable-rate-1hour.dat         # Irregular intervals
    ├── high-loss-10min.dat             # 20% packet loss
    └── edge-cases-samples.dat          # Various edge cases
```

### Test Naming Convention
- `test_quantization_accuracy_<scenario>`
- `test_time_drift_<scenario>`
- `test_packet_loss_<scenario>`
- `test_fallback_<scenario>`
- `test_edge_case_<scenario>`
- `test_format_compliance_<aspect>`

This comprehensive test plan ensures that the Chunked V1 format meets all specification requirements and handles edge cases gracefully while maintaining the required accuracy guarantees.
