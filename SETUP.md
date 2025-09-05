# zzping Setup Guide

This document provides basic setup instructions for running the `zzping` services locally.

## 1. TLS Certificates

The `zzping-database` gRPC server uses TLS for secure communication. A helper script is provided to generate the necessary self-signed certificates.

From the root of the repository, run:

```bash
./generate_certs.sh
```

This will create `server.pem`, `server.key`, and `ca.pem` files in the root directory. These files are required by the database and collector services.

## 2. Database Setup

The `zzping-database` service requires a data directory and a configuration file to run.

### Create Data Directory

Create a `data/` directory in the root of the repository:

```bash
mkdir data
```

### Create Intent Configuration

The database uses an `intent.ron` file to define the global configuration for all collectors. Create a file named `data/intent.ron` with the following content:

```ron
// data/intent.ron
(
  // The global ping rate (pings per second) for all collectors.
  ping_rate_pps: 50,

  // The global list of targets for all collectors to ping.
  targets: [
    "8.8.8.8",   // Google DNS
    "1.1.1.1",   // Cloudflare DNS
    "9.9.9.9",   // Quad9 DNS
  ],
)
```

You can now run the database service:

```bash
cargo run -p zzping-database
```

## 3. Collector Setup

Once the database is running, you can start one or more collector instances. The collector will connect to the database, receive the configuration from `intent.ron`, and start pinging.

```bash
cargo run -p zzping-collector
```
