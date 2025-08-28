// Chunked V1 Quantization Accuracy Tests
//
// PURPOSE: Verify RTT quantization stays within specified tolerances
// REQUIREMENT: ≤ 0.1% or ≤ 0.1ms error for all RTT values
//
// TEST COVERAGE:
// - Small RTT Values (0.1ms - 1ms): High precision region
// - Medium RTT Values (1ms - 100ms): Common network latencies
// - Large RTT Values (100ms - 1000ms): Degraded precision region
// - Boundary Values: Near quantization boundaries
// - Edge Cases: Zero RTT, maximum valid RTT
// - Roundtrip accuracy: symbol_to_duration(duration_to_symbol(x))

use std::time::Duration;
use zzping_press::chunked_v1::Quantizer;

// Test RTTs from 0.1ms to 1ms with high precision expectations
#[test]
fn test_quantization_accuracy_small_rtts() {
    let quantizer = Quantizer::new();
    for micros in (100..=1000).step_by(10) { // 0.1ms to 1ms
        let original_duration = Duration::from_micros(micros);
        let symbol = quantizer.duration_to_symbol(original_duration);
        let decompressed_duration = quantizer.symbol_to_duration(symbol);
        assert!(
            check_rtt_tolerance(original_duration, decompressed_duration),
            "Tolerance failed for small RTT: {:?}. Decompressed: {:?}",
            original_duration,
            decompressed_duration
        );
    }
}

// Test common network latencies from 1ms to 100ms
#[test]
fn test_quantization_accuracy_medium_rtts() {
    let quantizer = Quantizer::new();
    for millis in 1..=100 { // 1ms to 100ms
        let original_duration = Duration::from_millis(millis);
        let symbol = quantizer.duration_to_symbol(original_duration);
        let decompressed_duration = quantizer.symbol_to_duration(symbol);
        assert!(
            check_rtt_tolerance(original_duration, decompressed_duration),
            "Tolerance failed for medium RTT: {:?}. Decompressed: {:?}",
            original_duration,
            decompressed_duration
        );
    }
}

// Test high latency scenarios from 100ms to 1000ms
#[test]
fn test_quantization_accuracy_large_rtts() {
    let quantizer = Quantizer::new();
    for millis in (100..=1000).step_by(10) { // 100ms to 1000ms
        let original_duration = Duration::from_millis(millis);
        let symbol = quantizer.duration_to_symbol(original_duration);
        let decompressed_duration = quantizer.symbol_to_duration(symbol);
        assert!(
            check_rtt_tolerance(original_duration, decompressed_duration),
            "Tolerance failed for large RTT: {:?}. Decompressed: {:?}",
            original_duration,
            decompressed_duration
        );
    }
}

// Test values near quantization boundaries for precision edge cases
#[test]
#[ignore = "TODO: Implement boundary value testing"]
fn test_quantization_boundary_values() {
    todo!("Test RTT values at quantization boundaries");
}

// Test zero RTT, maximum RTT, and other edge conditions
#[test]
fn test_quantization_edge_cases() {
    let quantizer = Quantizer::new();

    // Test zero RTT
    let zero_duration = Duration::from_secs(0);
    let symbol = quantizer.duration_to_symbol(zero_duration);
    assert_eq!(symbol, 0);
    let decompressed_duration = quantizer.symbol_to_duration(symbol);
    assert_eq!(decompressed_duration.as_nanos(), 0);

    // Test a very large RTT that should be clamped
    let large_duration = Duration::from_secs(10000);
    let symbol = quantizer.duration_to_symbol(large_duration);
    let decompressed_duration = quantizer.symbol_to_duration(symbol);
    assert!(check_rtt_tolerance(large_duration, decompressed_duration));
}

// Verify symbol_to_duration(duration_to_symbol(x)) ≈ x
#[test]
fn test_quantization_roundtrip_accuracy() {
    let quantizer = Quantizer::new();
    let test_durations = [
        Duration::from_micros(50),      // Lower than small
        Duration::from_micros(123),
        Duration::from_millis(1),
        Duration::from_millis(42),
        Duration::from_millis(100),
        Duration::from_millis(567),
        Duration::from_secs(1),
        Duration::from_secs(10),
    ];

    for &original_duration in &test_durations {
        let symbol = quantizer.duration_to_symbol(original_duration);
        let decompressed_duration = quantizer.symbol_to_duration(symbol);
        assert!(
            check_rtt_tolerance(original_duration, decompressed_duration),
            "Roundtrip tolerance failed for: {:?}. Decompressed: {:?}",
            original_duration,
            decompressed_duration
        );
    }
}

// Helper function for tolerance checking
fn check_rtt_tolerance(original: Duration, quantized: Duration) -> bool {
    let tolerance_ns = std::cmp::max(
        100_000,                             // 0.1ms minimum tolerance
        (original.as_nanos() / 1000) as u64, // 0.1% of original
    );
    let error_ns = (original.as_nanos() as i64 - quantized.as_nanos() as i64).abs() as u64;
    error_ns <= tolerance_ns
}
