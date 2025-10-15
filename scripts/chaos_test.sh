#!/usr/bin/env bash
# Chaos testing script: starts DB + collectors, then kills processes to test resilience

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

NUM_COLLECTORS=${1:-3}
CHAOS_DURATION_SECS=${2:-60}

echo "Starting chaos test with $NUM_COLLECTORS collectors for $CHAOS_DURATION_SECS seconds..."

# Use existing test_certs for simplicity
if [ ! -d "test_certs" ]; then
    echo "test_certs not found, run ./generate_certs.sh first"
    exit 1
fi

# Build binaries
cargo build --release --bin zzping-database --bin zzping-collector

DB_BIN="${ROOT_DIR}/target/release/zzping-database"
COL_BIN="${ROOT_DIR}/target/release/zzping-collector"

# Create configs
mkdir -p tests/fixtures_chaos
cat > tests/fixtures_chaos/database-chaos.ron << EOF
DatabaseConfig(
    bind_host: "127.0.0.1",
    bind_port: 9446,
    tls: TlsConfig(
        ca_cert_paths: ["test_certs/ca.pem"],
        server_cert_path: "test_certs/database.pem",
        server_key_path: "test_certs/database.key",
    ),
    components: ComponentConfig(
        stale_timeout_secs: 30,
        max_collectors: 10,
    ),
)
EOF

# Start database
echo "Starting database..."
"$DB_BIN" --config tests/fixtures_chaos/database-chaos.ron &
DB_PID=$!
echo "Database PID: $DB_PID"

sleep 5

# Start collectors
echo "Starting $NUM_COLLECTORS collectors..."
COLLECTOR_PIDS=()
for i in $(seq 1 "$NUM_COLLECTORS"); do
    COLLECTOR_ID="collector-chaos-$i"
    cat > "tests/fixtures_chaos/${COLLECTOR_ID}.ron" << EOF
CollectorConfig(
    collector_id: "${COLLECTOR_ID}",
    database_host: "127.0.0.1",
    database_port: 9446,
    tls: TlsConfig(
        ca_cert_paths: ["test_certs/ca.pem"],
        ca_cert_path: "test_certs/ca.pem",
        client_cert_path: "test_certs/collector.pem",
        client_key_path: "test_certs/collector.key",
    ),
    components: ComponentConfig(
        heartbeat_interval_secs: 5,
        memdb_batch_size: 50,
    ),
)
EOF

    "$COL_BIN" --config "tests/fixtures_chaos/${COLLECTOR_ID}.ron" &
    COLLECTOR_PIDS+=("$!")
    echo "Collector $i PID: ${COLLECTOR_PIDS[-1]}"
    sleep 2
done

echo "All processes started. Running chaos for $CHAOS_DURATION_SECS seconds..."

START_TIME=$(date +%s)
END_TIME=$((START_TIME + CHAOS_DURATION_SECS))

while [ $(date +%s) -lt $END_TIME ]; do
    sleep 10

    # Randomly kill a collector
    if [ ${#COLLECTOR_PIDS[@]} -gt 0 ]; then
        RANDOM_INDEX=$((RANDOM % ${#COLLECTOR_PIDS[@]}))
        VICTIM_PID=${COLLECTOR_PIDS[RANDOM_INDEX]}
        if kill -0 "$VICTIM_PID" 2>/dev/null; then
            echo "Killing collector PID $VICTIM_PID"
            kill "$VICTIM_PID" || true
            # Restart it
            COLLECTOR_ID="collector-chaos-$((RANDOM_INDEX + 1))"
            "$COL_BIN" --config "tests/fixtures_chaos/${COLLECTOR_ID}.ron" &
            NEW_PID=$!
            COLLECTOR_PIDS[RANDOM_INDEX]=$NEW_PID
            echo "Restarted collector as PID $NEW_PID"
        fi
    fi

    # Check if database is still running
    if ! kill -0 "$DB_PID" 2>/dev/null; then
        echo "Database crashed, restarting..."
        "$DB_BIN" --config tests/fixtures_chaos/database-chaos.ron &
        DB_PID=$!
        echo "Database restarted as PID $DB_PID"
        sleep 5
    fi

    # Check collectors
    for i in "${!COLLECTOR_PIDS[@]}"; do
        if ! kill -0 "${COLLECTOR_PIDS[i]}" 2>/dev/null; then
            echo "Collector $i crashed, restarting..."
            COLLECTOR_ID="collector-chaos-$((i + 1))"
            "$COL_BIN" --config "tests/fixtures_chaos/${COLLECTOR_ID}.ron" &
            COLLECTOR_PIDS[i]=$!
            echo "Collector $i restarted as PID ${COLLECTOR_PIDS[i]}"
        fi
    done
done

echo "Chaos test complete. Shutting down..."

# Shutdown
for pid in "${COLLECTOR_PIDS[@]}"; do
    kill "$pid" || true
done
kill "$DB_PID" || true

wait "$DB_PID" || true
for pid in "${COLLECTOR_PIDS[@]}"; do
    wait "$pid" || true
done

echo "Chaos test finished."