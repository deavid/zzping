# Operations Runbook

## Daily Operations

### Starting the System

1. Ensure certificates are valid:
   ```bash
   ./generate_certs.sh
   openssl verify -CAfile test_certs/ca.pem test_certs/database.pem
   ```

2. Start database:
   ```bash
   nohup ./target/release/zzping-database --config database.ron > db.log 2>&1 &
   ```

3. Start collectors:
   ```bash
   for config in collector-*.ron; do
       nohup ./target/release/zzping-collector --config "$config" > "${config%.ron}.log" 2>&1 &
   done
   ```

### Monitoring

1. Check processes:
   ```bash
   ps aux | grep zzping
   ```

2. Check logs:
   ```bash
   tail -f db.log
   tail -f collector-*.log
   ```

3. Check connections:
   ```bash
   netstat -tlnp | grep 9443
   ```

### Stopping the System

```bash
# Graceful shutdown
pkill -TERM zzping-database
pkill -TERM zzping-collector

# Wait for shutdown
sleep 5

# Force kill if needed
pkill -KILL zzping-database
pkill -KILL zzping-collector
```

## Certificate Rotation

### Zero-Downtime Rotation

1. Generate new CA:
   ```bash
   ./scripts/generate_two_cas.sh
   ```

2. Update database config to trust both CAs:
   ```ron
   tls: TlsConfig(
       ca_cert_paths: ["test_certs/ca.pem", "test_certs/ca-new.pem"],
       server_cert_path: "test_certs/database.pem",
       server_key_path: "test_certs/database.key",
   ),
   ```

3. Restart database:
   ```bash
   pkill -TERM zzping-database
   sleep 2
   ./target/release/zzping-database --config database.ron &
   ```

4. Rotate collector certificates:
   ```bash
   # Generate new certs for each collector
   for i in {1..10}; do
       # Sign with new CA
       openssl req -new -key "collector-$i.key" -out "collector-$i.csr" \
           -subj "/C=US/ST=Test/L=Test/O=ZZPing/CN=collector-$i"
       openssl x509 -req -in "collector-$i.csr" -CA test_certs/ca-new.pem \
           -CAkey test_certs/ca-new.key -out "collector-$i-new.pem" -days 365
   done
   ```

5. Update collector configs and restart:
   ```bash
   # Update config files
   sed -i 's/collector.pem/collector-new.pem/g' collector-*.ron
   sed -i 's/ca.pem/ca-new.pem/g' collector-*.ron

   # Restart collectors
   pkill -TERM zzping-collector
   sleep 2
   for config in collector-*.ron; do
       ./target/release/zzping-collector --config "$config" &
   done
   ```

6. Remove old CA from database config after all collectors updated.

## Scaling

### Adding Collectors

1. Generate certificate:
   ```bash
   ./scripts/generate_multi_certs.sh 1  # For one more
   ```

2. Create config:
   ```bash
   cp collector.example.ron collector-new.ron
   # Edit collector-new.ron with new ID and cert paths
   ```

3. Start collector:
   ```bash
   ./target/release/zzping-collector --config collector-new.ron &
   ```

### Performance Monitoring

Run load test periodically:
```bash
./scripts/load_test.sh 100 300  # 100 collectors, 5 minutes
```

Monitor metrics:
- Memory usage per process
- CPU usage
- Network bandwidth
- Connection count

## Backup and Recovery

### Data Backup

```bash
# If database persists data
cp -r data/ backup/$(date +%Y%m%d)
```

### Disaster Recovery

1. Restore certificates:
   ```bash
   ./generate_certs.sh
   ```

2. Start database with clean state

3. Restart collectors

4. Verify connections

## Alerts and Monitoring

### Key Metrics to Monitor

- Process uptime
- Memory usage trends
- Connection count
- TLS handshake failures
- Data ingestion rate

### Log Monitoring

```bash
# Errors
grep ERROR *.log

# Connections
grep "Accepted connection" db.log | wc -l

# Heartbeats
grep heartbeat collector-*.log | wc -l
```

## Emergency Procedures

### Database Crash

1. Check logs for crash reason
2. Restart database
3. Collectors will reconnect automatically
4. Verify data integrity

### Network Issues

1. Check connectivity: `ping database-host`
2. Restart affected collectors
3. Check firewall rules

### Certificate Expiry

1. Monitor certificate validity:
   ```bash
   openssl x509 -in cert.pem -text | grep "Not After"
   ```

2. Rotate certificates before expiry (see rotation procedure)

## Maintenance

### Weekly Tasks

- Review logs for errors
- Check certificate expiry
- Run stability test: `./scripts/run_short_stability.sh`
- Update dependencies: `cargo update`

### Monthly Tasks

- Full system restart
- Performance benchmarking
- Security audit of certificates