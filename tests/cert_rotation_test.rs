//! Certificate rotation tests

use std::process::Command;
use std::time::Duration;
use tokio::time::sleep;
use std::error::Error;
use std::fs::File;
use std::io::BufReader;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use rustls::{ClientConfig, Certificate, PrivateKey, RootCertStore};
use rustls_pemfile::{certs, pkcs8_private_keys, rsa_private_keys};
use std::sync::Arc;
use rustls::client::ServerName;
use tonic::transport::{Certificate as TonicCertificate, ClientTlsConfig, Channel, Identity as TonicIdentity};
use zzping_proto::zzping::ingestion_client::IngestionClient;
use zzping_proto::zzping::HeartbeatRequest;
use base64::engine::general_purpose;
use base64::Engine as _;

#[tokio::test]
async fn test_database_accepts_old_and_new_ca_certs() {
    // Generate two CAs and certificates
    assert!(Command::new("sh")
        .args(&["-c", "scripts/generate_two_cas.sh"])
        .status()
        .expect("Failed to run generate_two_cas.sh")
        .success());

    // Start database with BOTH CAs trusted
    let mut db = Command::new("./target/debug/zzping-database")
        .arg("--config")
        .arg("tests/fixtures/database-dual-ca.ron")
        .spawn()
        .expect("Failed to start database");

    sleep(Duration::from_secs(2)).await;

    // Start collector with OLD CA-signed cert
    let mut collector_old = Command::new("./target/debug/zzping-collector")
        .arg("--config")
        .arg("tests/fixtures/collector-old-ca.ron")
        .spawn()
        .expect("Failed to start collector old");

    sleep(Duration::from_secs(3)).await;

    // Start collector with NEW CA-signed cert
    let mut collector_new = Command::new("./target/debug/zzping-collector")
        .arg("--config")
        .arg("tests/fixtures/collector-new-ca.ron")
        .spawn()
        .expect("Failed to start collector new");

    sleep(Duration::from_secs(3)).await;

    // Verify both collectors can perform an mTLS handshake with the database
    let db_addr = "127.0.0.1:9444";

    // Helper: perform TLS handshake using given CA + client cert/key
    async fn verify_mtls_handshake(
        addr: &str,
        ca_path: &str,
        client_cert_path: &str,
        client_key_path: &str,
    ) -> Result<(), Box<dyn Error>> {
        // Load CA
        let ca_file = File::open(ca_path)?;
        let mut ca_reader = BufReader::new(ca_file);
        let ca_certs = certs(&mut ca_reader)?;
        let mut root_store = RootCertStore::empty();
        for cert in ca_certs {
            root_store.add(&Certificate(cert)).map_err(|e| {
                format!("Failed to add CA cert: {:?}", e)
            })?;
        }

        // Load client cert
        let cert_file = File::open(client_cert_path)?;
        let mut cert_reader = BufReader::new(cert_file);
        let client_certs = certs(&mut cert_reader)?;
        if client_certs.is_empty() {
            return Err("No client certs found".into());
        }
        let client_certs: Vec<Certificate> = client_certs.into_iter().map(Certificate).collect();

        // Load client key (try pkcs8 then rsa)
        let key_file = File::open(client_key_path)?;
        let mut key_reader = BufReader::new(key_file);
        let mut keys = pkcs8_private_keys(&mut key_reader)?;
        if keys.is_empty() {
            // try RSA keys
            let key_file = File::open(client_key_path)?;
            let mut key_reader = BufReader::new(key_file);
            let rsa_keys = rsa_private_keys(&mut key_reader)?;
            if rsa_keys.is_empty() {
                return Err("No private keys found".into());
            }
            keys = rsa_keys;
        }
        let key = PrivateKey(keys.remove(0));

        // Build client config
        let mut cfg = ClientConfig::builder()
            .with_safe_defaults()
            .with_root_certificates(root_store)
            .with_single_cert(client_certs, key)
            .map_err(|e| format!("Failed to build client config: {:?}", e))?;

        let cfg = Arc::new(cfg);
        let connector = TlsConnector::from(cfg);

        // Connect TCP
        let stream = TcpStream::connect(addr).await?;

        // Domain for SNI: use "database" which matches generated CN in scripts
        let server_name = ServerName::try_from("database").map_err(|_| "Invalid DNS name")?;
        let tls_stream = connector.connect(server_name, stream).await?;
        // If handshake succeeds, drop stream and return Ok
        drop(tls_stream);
        Ok(())
    }

    // Verify old collector (signed by CA1)
    verify_mtls_handshake(
        db_addr,
        "test_certs_rotation/ca1.pem",
        "test_certs_rotation/collector-old.pem",
        "test_certs_rotation/collector-old.key",
    )
    .await
    .expect("Old CA client failed TLS handshake");

    // Verify new collector (signed by CA2)
    verify_mtls_handshake(
        db_addr,
        "test_certs_rotation/ca2.pem",
        "test_certs_rotation/collector-new.pem",
        "test_certs_rotation/collector-new.key",
    )
    .await
    .expect("New CA client failed TLS handshake");

    // Also perform a minimal authenticated gRPC call (heartbeat) for both clients
    async fn verify_rpc_call(
        addr: &str,
        ca_path: &str,
        client_cert_path: &str,
        client_key_path: &str,
        subject: &str,
    ) -> Result<(), Box<dyn Error>> {
        // Load CA PEM for tonic
        let ca_pem = tokio::fs::read(ca_path).await?;
        let ca_cert = TonicCertificate::from_pem(ca_pem);

        // Load client cert and key PEM
        let client_cert = tokio::fs::read(client_cert_path).await?;
        let client_key = tokio::fs::read(client_key_path).await?;
        let identity = TonicIdentity::from_pem(client_cert.clone(), client_key.clone());

        let tls = ClientTlsConfig::new()
            .domain_name("database")
            .ca_certificate(ca_cert)
            .identity(identity);

        let endpoint = format!("https://{}", addr);
        let channel = Channel::from_shared(endpoint)?.tls_config(tls)?.connect().await?;
        let mut client = IngestionClient::new(channel);

        // Create simple token with collector role
        let token_json = format!(r#"{{"sub":"{}","roles":["collector"]}}"#, subject);
        let token = general_purpose::STANDARD.encode(token_json.as_bytes());

        let mut req = tonic::Request::new(HeartbeatRequest {
            collector_uuid: subject.to_string(),
            pid: 0,
        });
        req.metadata_mut().insert("authorization", format!("Bearer {}", token).parse()?);

        let resp = client.heartbeat(req).await?;
        println!("Heartbeat RPC received: {:?}", resp);
        Ok(())
    }

    verify_rpc_call(
        db_addr,
        "test_certs_rotation/ca1.pem",
        "test_certs_rotation/collector-old.pem",
        "test_certs_rotation/collector-old.key",
        "collector-old",
    )
    .await
    .expect("Old CA RPC failed");

    verify_rpc_call(
        db_addr,
        "test_certs_rotation/ca2.pem",
        "test_certs_rotation/collector-new.pem",
        "test_certs_rotation/collector-new.key",
        "collector-new",
    )
    .await
    .expect("New CA RPC failed");

    // Cleanup
    collector_old.kill().ok();
    collector_new.kill().ok();
    db.kill().ok();
}
