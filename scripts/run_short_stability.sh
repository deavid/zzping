#!/usr/bin/env bash
# Short stability runner: builds release binaries, starts DB + 3 collectors,
# monitors process liveness and VmRSS for ~70s, then stops them.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

echo "Building release binaries..."
cargo build --release --bin zzping-database --bin zzping-collector

DB_BIN="${ROOT_DIR}/target/release/zzping-database"
COL_BIN="${ROOT_DIR}/target/release/zzping-collector"

if [ ! -x "$DB_BIN" ] || [ ! -x "$COL_BIN" ]; then
  echo "Missing binaries: $DB_BIN or $COL_BIN not found or not executable"
  exit 1
fi

echo "Starting database..."
"$DB_BIN" --config tests/fixtures/database-stability.ron &
DB_PID=$!
echo "DB pid=$DB_PID"

sleep 2

COL_PIDS=()
for id in 01 02 03; do
  echo "Starting collector $id..."
  "$COL_BIN" --config tests/fixtures/collector-${id}-stability.ron &
  COL_PIDS+=("$!")
  echo "collector $id pid=${COL_PIDS[-1]}"
  sleep 1
done

TOTAL_SECONDS=70
INTERVAL=10
ELAPSED=0

echo "Monitoring for $TOTAL_SECONDS seconds (interval ${INTERVAL}s)..."
while [ $ELAPSED -lt $TOTAL_SECONDS ]; do
  sleep $INTERVAL
  ELAPSED=$((ELAPSED + INTERVAL))
  echo "--- elapsed ${ELAPSED}s ---"

  if ! kill -0 "$DB_PID" 2>/dev/null; then
    echo "Database exited unexpectedly"
    wait "$DB_PID" || true
    exit 2
  fi

  # print DB VmRSS
  if [ -r "/proc/$DB_PID/status" ]; then
    grep VmRSS /proc/$DB_PID/status || true
  fi

  for p in "${COL_PIDS[@]}"; do
    if ! kill -0 "$p" 2>/dev/null; then
      echo "Collector pid $p exited unexpectedly"
      wait "$p" || true
      exit 3
    fi
    if [ -r "/proc/$p/status" ]; then
      grep VmRSS /proc/$p/status || true
    fi
  done
done

echo "Test interval complete; shutting down processes..."
for p in "${COL_PIDS[@]}"; do
  kill "$p" || true
  wait "$p" || true
done
kill "$DB_PID" || true
wait "$DB_PID" || true

echo "Short stability run completed successfully."
