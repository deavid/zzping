#!/bin/bash
# Generate certificates for N collectors + database

set -e

NUM_COLLECTORS=${1:-3}
CERT_DIR="test_certs"

echo "Generating certificates for $NUM_COLLECTORS collectors..."

# Create directory
mkdir -p "$CERT_DIR"
cd "$CERT_DIR"

# 1. Generate CA
if [ ! -f ca.key ]; then
    echo "Generating CA certificate..."
    openssl genrsa -out ca.key 2048
    openssl req -new -x509 -days 3650 -key ca.key -out ca.pem \
        -subj "/C=US/ST=Test/L=Test/O=ZZPing Test/CN=Test CA"
fi

# 2. Generate database certificate
if [ ! -f database.key ]; then
    echo "Generating database certificate..."
    openssl genrsa -out database.key 2048
    openssl req -new -key database.key -out database.csr \
        -subj "/C=US/ST=Test/L=Test/O=ZZPing/CN=database"
    openssl x509 -req -in database.csr -CA ca.pem -CAkey ca.key \
        -CAcreateserial -out database.pem -days 365
    rm database.csr
fi

# 3. Generate collector certificates
for i in $(seq 1 $NUM_COLLECTORS); do
    COLLECTOR_ID=$(printf "collector-%02d" $i)
    if [ ! -f "${COLLECTOR_ID}.key" ]; then
        echo "Generating certificate for ${COLLECTOR_ID}..."
        openssl genrsa -out "${COLLECTOR_ID}.key" 2048
        openssl req -new -key "${COLLECTOR_ID}.key" -out "${COLLECTOR_ID}.csr" \
            -subj "/C=US/ST=Test/L=Test/O=ZZPing/CN=${COLLECTOR_ID}"
        openssl x509 -req -in "${COLLECTOR_ID}.csr" -CA ca.pem -CAkey ca.key \
            -CAcreateserial -out "${COLLECTOR_ID}.pem" -days 365
        rm "${COLLECTOR_ID}.csr"
    fi
done

echo "Certificate generation complete!"
echo "CA: ca.pem"
echo "Database: database.pem, database.key"
for i in $(seq 1 $NUM_COLLECTORS); do
    COLLECTOR_ID=$(printf "collector-%02d" $i)
    echo "Collector $i: ${COLLECTOR_ID}.pem, ${COLLECTOR_ID}.key"
done
