# Chunked V1 Implementation Issues Summary

## Critical Issues Found

### 1. Variable Rate Storage Inefficiency (FIXME)
**Problem**: Variable rate mode stores raw u64 time deltas (~8 bytes per record)
**Location**: `chunked_v1.rs:456-462`
**Impact**: Extremely poor compression when fallback mode triggers
**Recommendation**: Use logarithmic quantization like RTT values

```rust
// Current inefficient implementation:
for delta in deltas {
    data.write_u64::<BigEndian>(delta.as_nanos() as u64)?; // 8 bytes per record!
}
```

### 2. AggregateEntry Format Waste (TODO)
**Problem**: Uses u16 for percentiles + u32 for lost_packet_count (26 bytes total)
**Location**: `chunked_v1.rs:102-115`
**Impact**: Significant metadata overhead per chunk
**Recommendation**: Use u8 for percentiles + u8/u16 for count (12-13 bytes, 54% savings)

```rust
// Current format: 26 bytes
pub p00_symbol: u16,     // Could be u8
// ... 11 percentiles
pub lost_packet_count: u32,  // Could be u8 for most cases
```

### 3. Drift Threshold Logic Inconsistency (TODO/CLARIFICATION)
**Problem**: Cumulative drift limit (20ms) > per-chunk threshold (2ms)
**Location**: Spec vs test expectations
**Impact**: Unclear behavior and potentially loose accuracy requirements
**Questions**:
- Why allow 20ms cumulative when chunks reset at 2ms?
- Should reduce threshold to 1ms for better accuracy?

### 4. Single Symbol Frequency Handling (FIXME)
**Problem**: Dummy symbol gets frequency 1, but real symbol frequency not guaranteed high
**Location**: `chunked_v1.rs:378-386`
**Impact**: May affect compression efficiency
**Recommendation**: Ensure real symbol frequency >> 1 (orders of magnitude difference)

```rust
// Current: adds dummy with frequency 1, but real symbol might be low too
symbols_with_freq.push(dummy_symbol);
probabilities.push(1);  // Should ensure real symbol freq >> 1
```

## Design Questions Needing Clarification

### 1. Drift Accumulation Strategy
- Should cumulative drift reset at chunk boundaries?
- What's the rationale for 20ms cumulative vs 2ms per-chunk?
- Can we reduce per-chunk threshold to 1ms for better accuracy?

### 2. Storage Format Optimization
- Is the current u16 percentile range (0-65535) necessary?
- Would u8 range (0-255) be sufficient for most RTT distributions?
- Should lost_packet_count be sized based on typical chunk sizes?

### 3. Fallback Mode Efficiency
- Why not use quantized time deltas in variable rate mode?
- What compression ratio is acceptable for fallback scenarios?
- Should there be multiple fallback strategies?

## Immediate Actions Needed

### Code Comments Added ✅
- Added TODO for drift threshold reduction
- Added FIXME for u64 delta storage inefficiency
- Added TODO for AggregateEntry size optimization
- Added FIXME for single symbol frequency handling

### Documentation Updated ✅
- Updated specification with actual format details
- Added TODOs and FIXMEs to spec
- Clarified storage costs and inefficiencies
- Added implementation issue notes

### Testing Strategy Updated ✅
- Added test category for implementation issues
- Added measurements for identified problems
- Updated test plan with efficiency concerns

## Next Steps

1. **Fix Current Test**: Understand why round-trip test is failing
2. **Measure Impact**: Quantify the overhead of identified issues
3. **Design Improvements**: Plan better storage formats for v2
4. **Comprehensive Testing**: Implement the full test battery
5. **Performance Baseline**: Establish current compression ratios and efficiency

## Long-term Considerations

### Format V2 Improvements
- Quantized time deltas for variable rate mode
- Optimized AggregateEntry with u8 fields
- Better frequency distribution for single symbols
- Clearer drift accumulation strategy

### Backwards Compatibility
- V1 format is now documented as-is
- Future improvements should be V2 format
- Migration strategy from V1 to V2 needed
