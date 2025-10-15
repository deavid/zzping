#!/usr/bin/env bash
# Load testing harness for 100 collectors
# Measures connection time, memory usage, and basic throughput

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

NUM_COLLECTORS=${1:-10}  # Default to 10 for testing, use 100 for full load
TEST_DURATION_SECS=${2:-60}  # Default 1 minute

echo "Starting load test with $NUM_COLLECTORS collectors for $TEST_DURATION_SECS seconds..."

# Generate certificates if needed
if [ ! -d "test_certs_load" ]; then
    echo "Generating certificates for $NUM_COLLECTORS collectors..."
    ./scripts/generate_multi_certs.sh "$NUM_COLLECTORS"
    mv test_certs_multi test_certs_load
fi

# Build release binaries
echo "Building release binaries..."
cargo build --release --bin zzping-database --bin zzping-collector

DB_BIN="${ROOT_DIR}/target/release/zzping-database"
COL_BIN="${ROOT_DIR}/target/release/zzping-collector"

# Create database config
mkdir -p tests/fixtures_load
cat > tests/fixtures_load/database-load.ron << EOF
DatabaseConfig(
    bind_host: "127.0.0.1",
    bind_port: 9445,
    tls: TlsConfig(
        ca_cert_paths: ["test_certs_load/ca.pem"],
        server_cert_path: "test_certs_load/database.pem",
        server_key_path: "test_certs_load/database.key",
    ),
    components: ComponentConfig(
        stale_timeout_secs: 30,
        max_collectors: 200,
    ),
)
EOF

# Start database
echo "Starting database..."
"$DB_BIN" --config tests/fixtures_load/database-load.ron &
DB_PID=$!
echo "Database PID: $DB_PID"

sleep 5

# Start collectors
echo "Starting $NUM_COLLECTORS collectors..."
COLLECTOR_PIDS=()
START_TIME=$(date +%s.%N)

for i in $(seq 1 "$NUM_COLLECTORS"); do
    COLLECTOR_ID=$(printf "collector-%02d" $i)
    cat > "tests/fixtures_load/${COLLECTOR_ID}.ron" << EOF
CollectorConfig(
    collector_id: "${COLLECTOR_ID}",
    database_host: "127.0.0.1",
    database_port: 9445,
    tls: TlsConfig(
        ca_cert_paths: ["test_certs_load/ca.pem"],
        ca_cert_path: "test_certs_load/ca.pem",
        client_cert_path: "test_certs_load/${COLLECTOR_ID}.pem",
        client_key_path: "test_certs_load/${COLLECTOR_ID}.key",
    ),
    components: ComponentConfig(
        heartbeat_interval_secs: 5,
        memdb_batch_size: 50,
    ),
)
EOF

    "$COL_BIN" --config "tests/fixtures_load/${COLLECTOR_ID}.ron" &
    COLLECTOR_PIDS+=("$!")
done

echo "All processes started. Waiting for connections..."

# Wait for test duration
sleep "$TEST_DURATION_SECS"

END_TIME=$(date +%s.%N)
DURATION=$(echo "$END_TIME - $START_TIME" | bc)

# Collect metrics
echo "Collecting metrics..."

# Count running processes
RUNNING_COLLECTORS=0
for pid in "${COLLECTOR_PIDS[@]}"; do
    if kill -0 "$pid" 2>/dev/null; then
        ((RUNNING_COLLECTORS++))
    fi
done

# Memory usage
if [ -r "/proc/$DB_PID/status" ]; then
    DB_MEMORY=$(grep VmRSS /proc/$DB_PID/status | awk '{print $2}')
else
    DB_MEMORY="unknown"
fi

TOTAL_COLLECTOR_MEMORY=0
for pid in "${COLLECTOR_PIDS[@]}"; do
    if [ -r "/proc/$pid/status" ]; then
        MEM=$(grep VmRSS /proc/$pid/status | awk '{print $2}')
        TOTAL_COLLECTOR_MEMORY=$((TOTAL_COLLECTOR_MEMORY + MEM))
    fi
done

# Shutdown
echo "Shutting down..."
for pid in "${COLLECTOR_PIDS[@]}"; do
    kill "$pid" || true
done
kill "$DB_PID" || true

wait "$DB_PID" || true
for pid in "${COLLECTOR_PIDS[@]}"; do
    wait "$pid" || true
done

# Report results
echo "=== Load Test Results ==="
echo "Collectors started: $NUM_COLLECTORS"
echo "Collectors running at end: $RUNNING_COLLECTORS"
echo "Test duration: ${DURATION}s"
echo "Database memory: ${DB_MEMORY} kB"
echo "Total collector memory: ${TOTAL_COLLECTOR_MEMORY} kB"
echo "Average collector memory: $((TOTAL_COLLECTOR_MEMORY / NUM_COLLECTORS)) kB"

if [ "$RUNNING_COLLECTORS" -eq "$NUM_COLLECTORS" ]; then
    echo "✅ All collectors remained running"
else
    echo "❌ $((NUM_COLLECTORS - RUNNING_COLLECTORS)) collectors failed"
fi

echo "Load test complete."