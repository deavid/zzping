#!/bin/bash

# Script to generate CA and server certificates for zzping-database
# Usage:
#   ./generate_certs.sh --ca          # Generate CA certificate
#   ./generate_certs.sh --server      # Generate server certificate (requires CA)
#   ./generate_certs.sh --all         # Generate both CA and server certificates

set -e

CA_KEY="ca.key"
CA_CERT="ca.pem"
SERVER_KEY="server.key"
SERVER_CERT="server.pem"
SERVER_CSR="server.csr"
CA_SERIAL="ca.srl"

# Function to generate CA
generate_ca() {
    echo "Generating CA private key..."
    openssl genrsa -out "$CA_KEY" 2048

    echo "Generating CA certificate..."
    openssl req -new -x509 -days 365 -key "$CA_KEY" -sha256 -out "$CA_CERT" \
        -subj "/C=US/ST=State/L=City/O=zzping/CN=zzping-CA"

    echo "CA certificate generated: $CA_CERT"
}

# Function to generate server certificate
generate_server() {
    if [ ! -f "$CA_CERT" ]; then
        echo "CA certificate not found. Generating CA first..."
        generate_ca
    fi

    echo "Generating server private key..."
    openssl genrsa -out "$SERVER_KEY" 2048

    echo "Generating server certificate signing request..."
    openssl req -subj "/CN=zzping" -new -key "$SERVER_KEY" -out "$SERVER_CSR"

    echo "Signing server certificate with CA..."
    openssl x509 -req -days 365 -in "$SERVER_CSR" -CA "$CA_CERT" -CAkey "$CA_KEY" \
        -out "$SERVER_CERT" -sha256 -CAcreateserial \
        -extfile <(cat <<EOF
basicConstraints=CA:FALSE
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth
subjectAltName=DNS:zzping,IP:127.0.0.1,IP:192.168.0.200
EOF
)

    # Clean up temporary files
    rm -f "$SERVER_CSR"

    echo "Server certificate generated: $SERVER_CERT"
    echo "Server key generated: $SERVER_KEY"
}

# Main logic
case "$1" in
    --ca)
        generate_ca
        ;;
    --server)
        generate_server
        ;;
    --all)
        generate_ca
        generate_server
        ;;
    *)
        echo "Usage: $0 {--ca|--server|--all}"
        echo "  --ca     Generate CA certificate"
        echo "  --server Generate server certificate (will generate CA if missing)"
        echo "  --all    Generate both CA and server certificates"
        exit 1
        ;;
esac

echo "Certificate generation complete."
