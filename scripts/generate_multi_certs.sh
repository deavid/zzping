
#!/bin/bash
# Generate certificates for N collectors + database (with SAN and extensions)

set -euo pipefail

# Number of collectors to create certs for
NUM_COLLECTORS=${1:-3}
# Consolidate into a single test_certs directory
CERT_DIR="test_certs"

echo "Generating certificates for $NUM_COLLECTORS collectors into ${CERT_DIR}..."

# Create directory
mkdir -p "${CERT_DIR}"
cd "${CERT_DIR}"

# 1. Generate CA
if [ ! -f ca.key ]; then
    echo "Generating CA certificate..."
    openssl genrsa -out ca.key 2048
    openssl req -new -x509 -days 3650 -key ca.key -out ca.pem \
        -subj "/C=US/ST=Test/L=Test/O=ZZPing Test/CN=Test CA"
fi

# Create a reusable openssl config to include SAN and proper extensions
cat > openssl_ext.cnf <<'EOF'
[ req ]
distinguished_name = req_distinguished_name
req_extensions = v3_req

[ req_distinguished_name ]

[ v3_req ]
basicConstraints = CA:FALSE
keyUsage = digitalSignature, keyEncipherment
extendedKeyUsage = serverAuth, clientAuth
subjectAltName = @alt_names

[ alt_names ]
IP.1 = 127.0.0.1
DNS.1 = localhost
EOF

# 2. Generate database certificate (with SAN and extensions)
if [ ! -f database.key ]; then
    echo "Generating database certificate..."
    openssl genrsa -out database.key 2048
    openssl req -new -key database.key -out database.csr \
        -subj "/C=US/ST=Test/L=Test/O=ZZPing/CN=127.0.0.1"
    openssl x509 -req -in database.csr -CA ca.pem -CAkey ca.key \
        -CAcreateserial -out database.pem -days 365 -extfile openssl_ext.cnf -extensions v3_req
    rm -f database.csr
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
            -CAcreateserial -out "${COLLECTOR_ID}.pem" -days 365 -extfile openssl_ext.cnf -extensions v3_req
        rm -f "${COLLECTOR_ID}.csr"
    fi
done

echo "Certificate generation complete!"
echo "CA: ca.pem"
echo "Database: database.pem, database.key"
for i in $(seq 1 $NUM_COLLECTORS); do
    COLLECTOR_ID=$(printf "collector-%02d" $i)
    echo "Collector $i: ${COLLECTOR_ID}.pem, ${COLLECTOR_ID}.key"
done

# Cleanup temp config
rm -f openssl_ext.cnf
