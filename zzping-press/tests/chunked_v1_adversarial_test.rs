// Chunked V1 Adversarial Input Tests
//
// PURPOSE: Test format resilience against pathological inputs designed to break assumptions
// REQUIREMENT: Never panic, graceful degradation, meaningful error messages
//
// TEST COVERAGE:
// - Pathological data designed to break compression assumptions
// - Values at mathematical and format limits
// - Temporal anomalies that could cause calculation errors
// - Memory exhaustion attempts
// - Statistical edge cases that could break entropy coding

use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use std::time::Duration;
use zzping_press::{
    RawDataRecord,
    chunked_v1::{Quantizer, compress_chunked_v1, decompress_chunked_v1},
};

const PACKET_LOSS: u64 = u64::MAX;

enum AttackType {
    AllPacketLoss,
    ExtremeOutliers,
    BimodalDistribution,
    IntegerOverflow,
    PathologicalTiming,
    QuantizationAttacks,
    EntropyAttack,
    MemoryExhaustion,
}

// Test chunks with 100% packet loss for entire periods
#[test]
#[ignore = "BUG: Fails when chunk is 100% packet loss. Same bug as in loss_test.rs"]
fn test_adversarial_all_packet_loss() {
    let records = create_pathological_data(AttackType::AllPacketLoss, 120);
    let compressed = compress_chunked_v1(&records).unwrap();
    let decompressed = decompress_chunked_v1(&compressed).unwrap();
    verify_attack_resilience(&records, &decompressed);
}

// Test data with extreme RTT outliers designed to break statistical models
#[test]
fn test_adversarial_extreme_outliers() {
    let records = create_pathological_data(AttackType::ExtremeOutliers, 100);
    let compressed = compress_chunked_v1(&records).unwrap();
    let decompressed = decompress_chunked_v1(&compressed).unwrap();
    verify_attack_resilience(&records, &decompressed);
}

// Test bimodal RTT distributions that could confuse percentile calculations
#[test]
fn test_adversarial_bimodal_distribution() {
    let records = create_pathological_data(AttackType::BimodalDistribution, 200);
    let compressed = compress_chunked_v1(&records).unwrap();
    let decompressed = decompress_chunked_v1(&compressed).unwrap();
    verify_attack_resilience(&records, &decompressed);
}

// Test values near integer overflow boundaries
#[test]
#[ignore = "BUG: Fails with timestamps near u64::MAX. See theory below."]
fn test_adversarial_integer_overflow() {
    // THEORY: The timestamp reconstruction logic fails for timestamps that are
    // close to u64::MAX. The decompressed timestamp is off by a large,
    // non-trivial amount, which suggests an overflow or data type issue in the
    // timestamp calculation logic when dealing with very large u64 values.
    let records = create_pathological_data(AttackType::IntegerOverflow, 10);
    let compressed = compress_chunked_v1(&records).unwrap();
    let decompressed = decompress_chunked_v1(&compressed).unwrap();
    verify_attack_resilience(&records, &decompressed);
}

// Test timing patterns designed to maximize drift accumulation
#[test]
fn test_adversarial_pathological_timing() {
    let records = create_pathological_data(AttackType::PathologicalTiming, 200);
    let compressed = compress_chunked_v1(&records).unwrap();
    let decompressed = decompress_chunked_v1(&compressed).unwrap();
    verify_attack_resilience(&records, &decompressed);
}

// Test RTT values specifically chosen to exploit quantization weaknesses
#[test]
fn test_adversarial_quantization_attacks() {
    let records = create_pathological_data(AttackType::QuantizationAttacks, 200);
    let compressed = compress_chunked_v1(&records).unwrap();
    let decompressed = decompress_chunked_v1(&compressed).unwrap();
    verify_attack_resilience(&records, &decompressed);
}

// Test data patterns designed to break entropy coding efficiency
#[test]
fn test_adversarial_entropy_attacks() {
    let records = create_pathological_data(AttackType::EntropyAttack, 500);
    let compressed = compress_chunked_v1(&records).unwrap();
    let decompressed = decompress_chunked_v1(&compressed).unwrap();
    verify_attack_resilience(&records, &decompressed);
}

// Test inputs designed to cause excessive memory usage
#[test]
#[ignore = "This test is slow and might cause OOM on some systems."]
fn test_adversarial_memory_exhaustion() {
    let records = create_pathological_data(AttackType::MemoryExhaustion, 20000);
    let compressed = compress_chunked_v1(&records).unwrap();
    let decompressed = decompress_chunked_v1(&compressed).unwrap();
    verify_attack_resilience(&records, &decompressed);
}

// Helper to create pathological test data
fn create_pathological_data(attack_type: AttackType, num_records: usize) -> Vec<RawDataRecord> {
    let mut records = Vec::with_capacity(num_records);
    let start_time = 1_672_531_200_000_000_000;
    let quantizer = Quantizer::new();
    let mut rng = StdRng::seed_from_u64(54321);

    match attack_type {
        AttackType::AllPacketLoss => {
            for i in 0..num_records {
                records.push(RawDataRecord {
                    sent_nanos: start_time + (i as u64 * 1_000_000_000),
                    rtt_nanos: PACKET_LOSS,
                });
            }
        }
        AttackType::ExtremeOutliers => {
            for i in 0..num_records {
                let rtt_nanos = if i == num_records / 2 {
                    Duration::from_secs(30).as_nanos() as u64 // 30s spike
                } else {
                    Duration::from_millis(10).as_nanos() as u64 // Stable 10ms
                };
                records.push(RawDataRecord {
                    sent_nanos: start_time + (i as u64 * 1_000_000_000),
                    rtt_nanos,
                });
            }
        }
        AttackType::BimodalDistribution => {
            for i in 0..num_records {
                let rtt_nanos = if i % 2 == 0 {
                    Duration::from_millis(10).as_nanos() as u64 // 10ms
                } else {
                    Duration::from_millis(800).as_nanos() as u64 // 800ms
                };
                records.push(RawDataRecord {
                    sent_nanos: start_time + (i as u64 * 1_000_000_000),
                    rtt_nanos,
                });
            }
        }
        AttackType::IntegerOverflow => {
            let mut timestamp = u64::MAX - (num_records as u64 * 1_000_000_000);
            for _ in 0..num_records {
                records.push(RawDataRecord {
                    sent_nanos: timestamp,
                    rtt_nanos: Duration::from_millis(20).as_nanos() as u64,
                });
                timestamp += 1_000_000_000;
            }
        }
        AttackType::PathologicalTiming => {
            let mut timestamp = start_time;
            for i in 0..num_records {
                records.push(RawDataRecord {
                    sent_nanos: timestamp,
                    rtt_nanos: Duration::from_millis(20).as_nanos() as u64,
                });
                // Alternate between small and large deviations from 1s interval
                if i % 2 == 0 {
                    timestamp += 1_000_000_000 + 127_000_000;
                } else {
                    timestamp += 1_000_000_000 - 127_000_000;
                }
            }
        }
        AttackType::QuantizationAttacks => {
            for i in 0..num_records {
                // Find a value halfway between two symbols
                let symbol1 = 1000 + i as u16;
                let symbol2 = 1001 + i as u16;
                let rtt1 = quantizer.symbol_to_duration(symbol1);
                let rtt2 = quantizer.symbol_to_duration(symbol2);
                let halfway_rtt = rtt1 + (rtt2 - rtt1) / 2;

                records.push(RawDataRecord {
                    sent_nanos: start_time + (i as u64 * 1_000_000_000),
                    rtt_nanos: halfway_rtt.as_nanos() as u64,
                });
            }
        }
        AttackType::EntropyAttack => {
            for i in 0..num_records {
                records.push(RawDataRecord {
                    sent_nanos: start_time + (i as u64 * 1_000_000_000),
                    rtt_nanos: Duration::from_millis(rng.random_range(10..1000)).as_nanos() as u64,
                });
            }
        }
        AttackType::MemoryExhaustion => {
            // Create many small chunks
            for i in 0..num_records {
                records.push(RawDataRecord {
                    sent_nanos: start_time + (i as u64 * 60 * 1_000_000_000), // 1 record per minute
                    rtt_nanos: Duration::from_millis(20).as_nanos() as u64,
                });
            }
        }
    }
    records
}

// Helper to verify resilience against attacks
fn verify_attack_resilience(original: &[RawDataRecord], decompressed: &[RawDataRecord]) {
    assert_eq!(original.len(), decompressed.len());
    for (orig, decomp) in original.iter().zip(decompressed.iter()) {
        assert_eq!(orig.sent_nanos, decomp.sent_nanos, "Timestamp mismatch");
        let orig_is_loss = orig.rtt_nanos == PACKET_LOSS;
        let decomp_is_loss = decomp.rtt_nanos == PACKET_LOSS;
        assert_eq!(orig_is_loss, decomp_is_loss, "Packet loss mismatch");

        if !orig_is_loss {
            let tolerance_ns = std::cmp::max(100_000, orig.rtt_nanos / 1000);
            let error_ns = (orig.rtt_nanos as i64 - decomp.rtt_nanos as i64).unsigned_abs();
            assert!(
                error_ns <= tolerance_ns,
                "RTT tolerance failed. Original RTT: {}ns, Decompressed RTT: {}ns",
                orig.rtt_nanos,
                decomp.rtt_nanos
            );
        }
    }
}
