#!/bin/bash
set -e

# Check that Main-actor components do not depend on network infrastructure crates
MAIN_ACTOR_CRATES=(
    "src/components/zzintent-config"
    "src/components/zzmem-db"
    "src/components/zzcollector-state"
)

FORBIDDEN_DEPS=(
    "zznet-session"
    "zznet-peer-manager"
    "zznet-router"
)

VIOLATIONS=0

for crate_path in "${MAIN_ACTOR_CRATES[@]}"; do
    cargo_toml="$crate_path/Cargo.toml"

    if [ ! -f "$cargo_toml" ]; then
        echo "ERROR: $cargo_toml not found"
        exit 1
    fi

    for dep in "${FORBIDDEN_DEPS[@]}"; do
        if grep -q "^$dep\\s*=" "$cargo_toml" || grep -q "^$dep\\s*{" "$cargo_toml"; then
            echo "VIOLATION: $crate_path depends on forbidden crate: $dep"
            VIOLATIONS=$((VIOLATIONS + 1))
        fi
    done
done

if [ $VIOLATIONS -eq 0 ]; then
    echo "✅ All Main-actor components follow dependency rules"
    exit 0
else
    echo "❌ Found $VIOLATIONS dependency violations"
    echo "Main-actor components must only depend on zznet-api (and component-local crates)"
    exit 1
fi