#!/bin/bash

# Script to generate CA and certificates for zzping components
# Usage:
#   ./generate_certs.sh --ca                          # Generate CA certificate
#   ./generate_certs.sh --collector                   # Generate collector certificate (CN=collector, SAN=DNS:root)
#   ./generate_certs.sh --database                    # Generate database certificate (CN=database, SAN=DNS:root)
#   ./generate_certs.sh --client-ro [username]        # Generate client-ro certificate (CN=client-ro, SAN=DNS:<username>)
#   ./generate_certs.sh --client-admin [username]     # Generate client-admin certificate (CN=client-admin, SAN=DNS:<username>)
#   ./generate_certs.sh --all                         # Generate CA and all service certificates

set -e

CERTS_FOLDER="test_certs"

# Create certs directory if it doesn't exist
mkdir -p "$CERTS_FOLDER"

CA_KEY="$CERTS_FOLDER/ca.key"
CA_CERT="$CERTS_FOLDER/ca.pem"
COLLECTOR_KEY="$CERTS_FOLDER/collector.key"
COLLECTOR_CERT="$CERTS_FOLDER/collector.pem"
COLLECTOR_CSR="$CERTS_FOLDER/collector.csr"
DATABASE_KEY="$CERTS_FOLDER/database.key"
DATABASE_CERT="$CERTS_FOLDER/database.pem"
DATABASE_CSR="$CERTS_FOLDER/database.csr"
CLIENT_RO_KEY="$CERTS_FOLDER/client-ro.key"
CLIENT_RO_CERT="$CERTS_FOLDER/client-ro.pem"
CLIENT_RO_CSR="$CERTS_FOLDER/client-ro.csr"
CLIENT_ADMIN_KEY="$CERTS_FOLDER/client-admin.key"
CLIENT_ADMIN_CERT="$CERTS_FOLDER/client-admin.pem"
CLIENT_ADMIN_CSR="$CERTS_FOLDER/client-admin.csr"
CA_SERIAL="$CERTS_FOLDER/ca.srl"

# Function to generate CA
generate_ca() {
    echo "Generating CA private key..."
    openssl genrsa -out "$CA_KEY" 2048

    echo "Generating CA certificate..."
    openssl req -new -x509 -days 365 -key "$CA_KEY" -sha256 -out "$CA_CERT" \
        -subj "/C=US/ST=State/L=City/O=zzping/CN=zzping-CA"

    echo "CA certificate generated: $CA_CERT"
}

# Function to generate collector certificate
generate_collector() {
    if [ ! -f "$CA_CERT" ]; then
        echo "CA certificate not found. Generating CA first..."
        generate_ca
    fi

    echo "Generating collector private key..."
    openssl genrsa -out "$COLLECTOR_KEY" 2048

    echo "Generating collector certificate signing request..."
    openssl req -subj "/CN=collector" -new -key "$COLLECTOR_KEY" -out "$COLLECTOR_CSR"

    echo "Signing collector certificate with CA..."
    openssl x509 -req -days 365 -in "$COLLECTOR_CSR" -CA "$CA_CERT" -CAkey "$CA_KEY" \
        -out "$COLLECTOR_CERT" -sha256 -CAcreateserial \
        -extfile <(cat <<EOF
basicConstraints=CA:FALSE
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth,clientAuth
subjectAltName=DNS:root
EOF
)

    rm -f "$COLLECTOR_CSR"

    echo "Collector certificate generated: $COLLECTOR_CERT"
    echo "Collector key generated: $COLLECTOR_KEY"
}

# Function to generate database certificate
generate_database() {
    if [ ! -f "$CA_CERT" ]; then
        echo "CA certificate not found. Generating CA first..."
        generate_ca
    fi

    echo "Generating database private key..."
    openssl genrsa -out "$DATABASE_KEY" 2048

    echo "Generating database certificate signing request..."
    openssl req -subj "/CN=database" -new -key "$DATABASE_KEY" -out "$DATABASE_CSR"

    echo "Signing database certificate with CA..."
    openssl x509 -req -days 365 -in "$DATABASE_CSR" -CA "$CA_CERT" -CAkey "$CA_KEY" \
        -out "$DATABASE_CERT" -sha256 -CAcreateserial \
        -extfile <(cat <<EOF
basicConstraints=CA:FALSE
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=serverAuth,clientAuth
subjectAltName=DNS:root
EOF
)

    rm -f "$DATABASE_CSR"

    echo "Database certificate generated: $DATABASE_CERT"
    echo "Database key generated: $DATABASE_KEY"
}

# Function to generate client-ro certificate
generate_client_ro() {
    local username="$1"
    if [ -z "$username" ]; then
        echo "Error: Username required for user certificates"
        echo "Use: ./generate_certs.sh --client-ro <username>"
        exit 1
    fi

    if [ ! -f "$CA_CERT" ]; then
        echo "CA certificate not found. Generating CA first..."
        generate_ca
    fi

    echo "Generating client-ro private key..."
    openssl genrsa -out "$CLIENT_RO_KEY" 2048

    echo "Generating client-ro certificate signing request..."
    openssl req -subj "/CN=client-ro" -new -key "$CLIENT_RO_KEY" -out "$CLIENT_RO_CSR"

    echo "Signing client-ro certificate with CA..."
    openssl x509 -req -days 365 -in "$CLIENT_RO_CSR" -CA "$CA_CERT" -CAkey "$CA_KEY" \
        -out "$CLIENT_RO_CERT" -sha256 -CAcreateserial \
        -extfile <(cat <<EOF
basicConstraints=CA:FALSE
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=clientAuth
subjectAltName=DNS:$username
EOF
)

    rm -f "$CLIENT_RO_CSR"

    echo "Client-ro certificate generated: $CLIENT_RO_CERT"
    echo "Client-ro key generated: $CLIENT_RO_KEY"
}

# Function to generate client-admin certificate
generate_client_admin() {
    local username="$1"
    if [ -z "$username" ]; then
        echo "Error: Username required for user certificates"
        echo "Use: ./generate_certs.sh --client-admin <username>"
        exit 1
    fi

    if [ ! -f "$CA_CERT" ]; then
        echo "CA certificate not found. Generating CA first..."
        generate_ca
    fi

    echo "Generating client-admin private key..."
    openssl genrsa -out "$CLIENT_ADMIN_KEY" 2048

    echo "Generating client-admin certificate signing request..."
    openssl req -subj "/CN=client-admin" -new -key "$CLIENT_ADMIN_KEY" -out "$CLIENT_ADMIN_CSR"

    echo "Signing client-admin certificate with CA..."
    openssl x509 -req -days 365 -in "$CLIENT_ADMIN_CSR" -CA "$CA_CERT" -CAkey "$CA_KEY" \
        -out "$CLIENT_ADMIN_CERT" -sha256 -CAcreateserial \
        -extfile <(cat <<EOF
basicConstraints=CA:FALSE
keyUsage=digitalSignature,keyEncipherment
extendedKeyUsage=clientAuth
subjectAltName=DNS:$username
EOF
)

    rm -f "$CLIENT_ADMIN_CSR"

    echo "Client-admin certificate generated: $CLIENT_ADMIN_CERT"
    echo "Client-admin key generated: $CLIENT_ADMIN_KEY"
}

# Main logic
case "$1" in
    --ca)
        generate_ca
        ;;
    --collector)
        generate_collector
        ;;
    --database)
        generate_database
        ;;
    --client-ro)
        generate_client_ro "$2"
        ;;
    --client-admin)
        generate_client_admin "$2"
        ;;
    --all)
        generate_ca
        generate_collector
        generate_database
        ;;
    *)
        echo "Usage: $0 {--ca|--collector|--database|--client-ro|--client-admin|--all}"
        echo "  --ca                    Generate CA certificate"
        echo "  --collector             Generate collector certificate (CN=collector, SAN=DNS:root)"
        echo "  --database              Generate database certificate (CN=database, SAN=DNS:root)"
        echo "  --client-ro <username>  Generate client-ro certificate (CN=client-ro, SAN=DNS:<username>)"
        echo "  --client-admin <username> Generate client-admin certificate (CN=client-admin, SAN=DNS:<username>)"
        echo "  --all                   Generate CA and all service certificates"
        exit 1
        ;;
esac

echo "Certificate generation complete."
