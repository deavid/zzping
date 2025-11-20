#!/bin/bash

# ZZNet Certificate Generator (v3.0 - Production Ready)
#
# ARCHITECTURE SPECIFICATION:
# 1. Cryptography: ECDSA P-256 (Prime256v1).
# 2. Identity (The Directory Model):
#    - O  (Organization)       = "zzping" (System Scope)
#    - OU (OrganizationalUnit) = ROLE     (e.g., "collector", "database")
#    - CN (CommonName)         = USERNAME (e.g., "root", "alice")
# 3. Topology (The Mesh ID):
#    - SAN (SubjectAltName)    = "DNS:zzping-mesh"
#
# FILE LAYOUT:
#   test_certs/
#     ├── ca/       # The Root Authority (Protect this!)
#     ├── secrets/  # Private Keys (Don't share)
#     └── dist/     # Public Certificates (Distribute these)

set -euo pipefail

# --- Configuration ---
BASE_DIR="test_certs"
DIR_CA="$BASE_DIR/ca"
DIR_SECRETS="$BASE_DIR/secrets"
DIR_DIST="$BASE_DIR/dist"

# Lifetimes (in days)
DAYS_INFRA=10950 # ~30 Years (Services/Root)
DAYS_USER=90     # ~3 months (Friends/Humans)

# --- Prerequisites Check ---
if ! command -v openssl &> /dev/null; then
    echo "Error: openssl not found."
    exit 1
fi

# Simple version check (warn only, as format varies wildly between distros)
OPENSSL_VERSION=$(openssl version | awk '{print $2}')
echo "Using OpenSSL version: $OPENSSL_VERSION"

# --- Setup ---
mkdir -p "$DIR_CA" "$DIR_SECRETS" "$DIR_DIST"
# Secure the secrets directory immediately
chmod 700 "$DIR_CA" "$DIR_SECRETS"

# --- Helper Functions ---

gen_key() {
    local out_path=$1
    echo "    [Key] Generating ECDSA P-256 Key..."
    openssl ecparam -name prime256v1 -genkey -noout -out "$out_path"
    chmod 600 "$out_path"
}

generate_ca() {
    local key="$DIR_CA/ca.key"
    local cert="$DIR_CA/ca.pem"

    if [[ -f "$key" && -f "$cert" ]]; then
        echo "(!) CA already exists. Skipping generation to preserve trust."
        echo "    To regenerate, delete: $DIR_CA"
        return
    fi

    echo "=== Generating Root CA ==="
    gen_key "$key"

    echo "    [Cert] Signing Self-Signed Root CA ($DAYS_INFRA days)..."
    openssl req -new -x509 -days "$DAYS_INFRA" -key "$key" -out "$cert" \
        -subj "/O=zzping/CN=zzping-Root-CA" \
        -addext "basicConstraints=critical,CA:TRUE" \
        -addext "keyUsage=critical,keyCertSign,cRLSign"

    echo "    -> CA Ready: $cert"
    echo ""
}

# issue_cert <role> <username> <filename_override_optional>
issue_cert() {
    local role=$1
    local username=$2
    local custom_name=${3:-}

    # Determine Filename: if custom name not set, use role_username (or just role if root)
    local filename
    if [[ -n "$custom_name" ]]; then
        filename="$custom_name"
    elif [[ "$username" == "root" ]]; then
        filename="$role"
    else
        filename="${role}_${username}"
    fi

    # Determine Expiry
    local days
    if [[ "$username" == "root" ]]; then
        days=$DAYS_INFRA
    else
        days=$DAYS_USER
    fi

    local key_path="$DIR_SECRETS/$filename.key"
    local csr_path="$DIR_SECRETS/$filename.csr" # CSRs are temp secrets
    local cert_path="$DIR_DIST/$filename.pem"

    echo "=== Issuing Cert: $filename ==="
    echo "    Role: $role | User: $username | Expiry: $days days"

    if [[ ! -f "$DIR_CA/ca.pem" ]]; then
        echo "Error: CA not found. Run --ca first."
        exit 1
    fi

    # 1. Generate Key
    gen_key "$key_path"

    # 2. Generate CSR
    # Subject: O=System, OU=Role, CN=User
    openssl req -new -key "$key_path" -out "$csr_path" \
        -subj "/O=zzping/OU=$role/CN=$username"

    # 3. Sign
    # SAN: zzping-mesh (The magic token)
    if ! openssl x509 -req -days "$days" -in "$csr_path" \
        -CA "$DIR_CA/ca.pem" -CAkey "$DIR_CA/ca.key" -CAcreateserial \
        -out "$cert_path" -sha256 \
        -extfile <(cat <<EOF
basicConstraints=CA:FALSE
keyUsage=digitalSignature,keyAgreement
extendedKeyUsage=serverAuth,clientAuth
subjectAltName=DNS:zzping-mesh
EOF
) > /dev/null 2>&1; then
        echo "Error: Failed to sign certificate."
        exit 1
    fi

    # Cleanup
    rm -f "$csr_path"

    echo "    -> Public:  $cert_path"
    echo "    -> Private: $key_path"
    echo ""
}

# --- Main Logic ---

case "${1:-}" in
    --ca)
        generate_ca
        ;;
    --collector)
        issue_cert "collector" "root"
        ;;
    --database)
        issue_cert "database" "root"
        ;;
    --client)
        # Usage: --client <username> [filename]
        if [ -z "${2:-}" ]; then echo "Error: Username required"; exit 1; fi
        issue_cert "client-ro" "$2" "${3:-}"
        ;;
    --admin)
        # Usage: --admin <username> [filename]
        if [ -z "${2:-}" ]; then echo "Error: Username required"; exit 1; fi
        issue_cert "client-admin" "$2" "${3:-}"
        ;;
    --all)
        # Nuke and rebuild dev environment
        echo "!!! DELETING ALL EXISTING CERTIFICATES !!!"
        rm -rf "$BASE_DIR"
        mkdir -p "$DIR_CA" "$DIR_SECRETS" "$DIR_DIST"

        generate_ca
        issue_cert "collector" "root"
        issue_cert "database" "root"
        # Generate a sample user cert for testing
        issue_cert "client-ro" "testuser"
        ;;
    *)
        echo "ZZNet Certificate Generator (v3.0)"
        echo "Usage:"
        echo "  $0 --all                   Regenerate CA and all infrastructure certs"
        echo "  $0 --ca                    Generate/Ensure CA exists"
        echo "  $0 --collector             Generate Collector service cert (30 years)"
        echo "  $0 --database              Generate Database service cert (30 years)"
        echo "  $0 --client <user> [name]  Generate Read-Only cert (90 days)"
        echo "  $0 --admin  <user> [name]  Generate Admin cert (90 days)"
        exit 1
        ;;
esac