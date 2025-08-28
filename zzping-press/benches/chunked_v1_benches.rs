use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use std::time::Duration;
use zzping_press::{
    RawDataRecord,
    chunked_v1::{compress_chunked_v1, decompress_chunked_v1},
};

#[derive(Debug)]
struct BenchmarkMetrics {
    size: usize,
    compression_time_us: f64,
    decompression_time_us: f64,
    compressed_size: usize,
    uncompressed_size: usize,
}

impl BenchmarkMetrics {
    fn compression_ratio(&self) -> f64 {
        self.compressed_size as f64 / self.uncompressed_size as f64
    }

    fn compression_throughput_mibs(&self) -> f64 {
        let mb = self.uncompressed_size as f64 / (1024.0 * 1024.0);
        mb / (self.compression_time_us / 1_000_000.0)
    }

    fn decompression_throughput_mibs(&self) -> f64 {
        let mb = self.uncompressed_size as f64 / (1024.0 * 1024.0);
        mb / (self.decompression_time_us / 1_000_000.0)
    }

    fn compression_rtts_per_sec(&self) -> f64 {
        self.size as f64 / (self.compression_time_us / 1_000_000.0)
    }

    fn decompression_rtts_per_sec(&self) -> f64 {
        self.size as f64 / (self.decompression_time_us / 1_000_000.0)
    }

    fn avg_bits_per_rtt(&self) -> f64 {
        (self.compressed_size as f64 * 8.0) / self.size as f64
    }
}

fn generate_test_data<F, G>(
    num_records: usize,
    interval_pattern: F,
    rtt_pattern: G,
    start_time_ns: u64,
) -> Vec<RawDataRecord>
where
    F: Fn(usize) -> u64,
    G: Fn(usize) -> u64,
{
    let mut records = Vec::with_capacity(num_records);
    let mut current_timestamp_ns = start_time_ns;

    for i in 0..num_records {
        records.push(RawDataRecord {
            sent_nanos: current_timestamp_ns,
            rtt_nanos: rtt_pattern(i),
        });
        current_timestamp_ns = current_timestamp_ns.wrapping_add(interval_pattern(i));
    }
    records
}

fn criterion_benchmark(c: &mut Criterion) {
    let mut group = c.benchmark_group("chunked_v1_performance");
    // Configure for faster execution
    group.sample_size(10); // Reduce sample size from default 100
    group.measurement_time(std::time::Duration::from_secs(3)); // Reduce measurement time
    group.warm_up_time(std::time::Duration::from_secs(1)); // Reduce warm-up time

    let start_time = 1_672_531_200_000_000_000; // 2023-01-01 00:00:00 UTC
    let mut metrics_collection = Vec::new();

    for size in [100_000, 1_000_000, 10_000_000].iter() {
        group.throughput(criterion::Throughput::Elements(*size as u64));
        let interval = if *size >= 1_000_000 {
            1_000_000
        } else {
            1_000_000_000
        };
        let records = generate_test_data(
            *size,
            |_| interval, // 1ms for 1M, 1s for others
            |i| Duration::from_millis(20 + (i as u64 % 10)).as_nanos() as u64,
            start_time,
        );
        let compressed_data = compress_chunked_v1(&records).unwrap();
        let uncompressed_size = records.len() * std::mem::size_of::<RawDataRecord>();

        // Quick timing measurement for our summary (reduce iterations for speed)
        let start = std::time::Instant::now();
        for _ in 0..3 {
            std::hint::black_box(compress_chunked_v1(&records).unwrap());
        }
        let compression_time_us = start.elapsed().as_secs_f64() * 1_000_000.0 / 3.0;

        let start = std::time::Instant::now();
        for _ in 0..3 {
            std::hint::black_box(decompress_chunked_v1(&compressed_data).unwrap());
        }
        let decompression_time_us = start.elapsed().as_secs_f64() * 1_000_000.0 / 3.0;

        metrics_collection.push(BenchmarkMetrics {
            size: *size,
            compression_time_us,
            decompression_time_us,
            compressed_size: compressed_data.len(),
            uncompressed_size,
        });

        // Run the actual criterion benchmarks
        group.bench_with_input(BenchmarkId::new("compress", size), &records, |b, r| {
            b.iter(|| compress_chunked_v1(r).unwrap())
        });

        group.bench_with_input(
            BenchmarkId::new("decompress", size),
            &compressed_data,
            |b, d| b.iter(|| decompress_chunked_v1(d).unwrap()),
        );
    }

    group.finish();

    // Print summary table
    print_benchmark_summary(&metrics_collection);

    let mut group = c.benchmark_group("chunked_v1_timing");
    group.sample_size(10);
    group.measurement_time(std::time::Duration::from_secs(2));
    let records = generate_test_data(
        100_000,
        |_| 1_000_000_000,
        |_| Duration::from_millis(20).as_nanos() as u64,
        start_time,
    );
    let compressed = compress_chunked_v1(&records).expect("Compression failed");
    group.bench_function("timing_accuracy_long_sequences", |b| {
        b.iter(|| decompress_chunked_v1(&compressed).unwrap())
    });
    group.finish();
}

fn print_benchmark_summary(metrics: &[BenchmarkMetrics]) {
    println!();
    println!(
        "╔══════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════════╗"
    );
    println!(
        "║                                            ZZPING CHUNKED_V1 PERFORMANCE SUMMARY                                             ║"
    );
    println!(
        "╠═══════════════╤════════════════╤══════════════╤════════════════╤═════════════╤═══════════════════════╤═══════════════════════╣"
    );
    println!(
        "║   RTT Count   │  Uncompressed  │  Compressed  │   Size Ratio   │ Avg Bits/   │     Compression       │    Decompression      ║"
    );
    println!(
        "║               │      Size      │     Size     │                │    RTT      │  MiB/s  │   MRTTs/s   │  MiB/s  │   MRTTs/s   ║"
    );
    println!(
        "╠═══════════════╪════════════════╪══════════════╪════════════════╪═════════════╪═════════╪═════════════╪═════════╪═════════════╣"
    );

    for metric in metrics {
        let ratio_str = format!("{:.2}x", metric.compression_ratio());

        println!(
            "║ {:>13} │ {:>12} B │ {:>10} B │ {:>14} │ {:>11.1} │ {:>7.1} │ {:>11.1} │ {:>7.1} │ {:>11.1} ║",
            format_number(metric.size),
            format_number(metric.uncompressed_size),
            format_number(metric.compressed_size),
            ratio_str,
            metric.avg_bits_per_rtt(),
            metric.compression_throughput_mibs(),
            metric.compression_rtts_per_sec() / 1_000_000.0,
            metric.decompression_throughput_mibs(),
            metric.decompression_rtts_per_sec() / 1_000_000.0
        );
    }

    println!(
        "╚═══════════════╧════════════════╧══════════════╧════════════════╧═════════════╧═════════╧═════════════╧═════════╧═════════════╝"
    );
    println!("Note: MRTTs/s = Million RTTs per second");
    println!(
        "      Size Ratio shows compressed size relative to original (lower % = better compression)"
    );
    println!(
        "      Avg Bits/RTT shows average storage requirement per RTT record in compressed format"
    );
    println!();
}

fn format_number(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
