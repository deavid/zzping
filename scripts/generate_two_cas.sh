#!/bin/bash
# Generate two CAs (old and new) and sign database + collector certs with each CA
set -euo pipefail

CERT_DIR="test_certs_rotation"
mkdir -p "$CERT_DIR"
cd "$CERT_DIR"

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

# CA1 (old)
if [ ! -f ca1.key ]; then
  echo "Generating CA1..."
  openssl genrsa -out ca1.key 2048
  openssl req -new -x509 -days 3650 -key ca1.key -out ca1.pem -subj "/C=US/ST=Test/L=Test/O=ZZPing/CN=CA1"
fi

# CA2 (new)
if [ ! -f ca2.key ]; then
  echo "Generating CA2..."
  openssl genrsa -out ca2.key 2048
  openssl req -new -x509 -days 3650 -key ca2.key -out ca2.pem -subj "/C=US/ST=Test/L=Test/O=ZZPing/CN=CA2"
fi

# Database cert (signed by both CA1 and CA2) - generate two server certs with SAN
if [ ! -f database.key ]; then
  echo "Generating database key/csr..."
  openssl genrsa -out database.key 2048
  openssl req -new -key database.key -out database.csr -subj "/C=US/ST=Test/L=Test/O=ZZPing/CN=127.0.0.1"
  openssl x509 -req -in database.csr -CA ca1.pem -CAkey ca1.key -CAcreateserial -out database-ca1.pem -days 365 -extfile openssl_ext.cnf -extensions v3_req
  openssl x509 -req -in database.csr -CA ca2.pem -CAkey ca2.key -CAcreateserial -out database-ca2.pem -days 365 -extfile openssl_ext.cnf -extensions v3_req
  rm -f database.csr
fi

# Collector cert signed by CA1 (old)
if [ ! -f collector-old.key ]; then
  echo "Generating collector-old cert..."
  openssl genrsa -out collector-old.key 2048
  openssl req -new -key collector-old.key -out collector-old.csr -subj "/C=US/ST=Test/L=Test/O=ZZPing/CN=collector-old"
  openssl x509 -req -in collector-old.csr -CA ca1.pem -CAkey ca1.key -CAcreateserial -out collector-old.pem -days 365 -extfile openssl_ext.cnf -extensions v3_req
  rm -f collector-old.csr
fi

# Collector cert signed by CA2 (new)
if [ ! -f collector-new.key ]; then
  echo "Generating collector-new cert..."
  openssl genrsa -out collector-new.key 2048
  openssl req -new -key collector-new.key -out collector-new.csr -subj "/C=US/ST=Test/L=Test/O=ZZPing/CN=collector-new"
  openssl x509 -req -in collector-new.csr -CA ca2.pem -CAkey ca2.key -CAcreateserial -out collector-new.pem -days 365 -extfile openssl_ext.cnf -extensions v3_req
  rm -f collector-new.csr
fi

echo "Generated rotation certs in $CERT_DIR"
echo "CA1: ca1.pem"
echo "CA2: ca2.pem"
echo "Database certs: database-ca1.pem, database-ca2.pem"
echo "Collector old: collector-old.pem"
echo "Collector new: collector-new.pem"

# Cleanup temp config
rm -f openssl_ext.cnf
