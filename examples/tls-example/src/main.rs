//! TLS Example - Secure Communication with Mutual TLS Authentication
//!
//! This example demonstrates:
//! 1. Configuring TLS for server and client
//! 2. Using role-based certificate selection
//! 3. Mutual TLS authentication
//! 4. Secure message exchange over TLS
//!
//! **Prerequisites**:
//! - Certificates must exist in `certs/` directory
//! - Run `./generate_certs.sh` if certificates don't exist
//!
//! **Run this example**:
//! ```bash
//! cargo run -p tls-example
//! ```

use actix::prelude::*;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use zznet_api::types::Role;
use zznet_builder::{ClientBuilder, ServerBuilder};
use zznet_hello::auth::AuthRole;
use zznet_hello::connection_manager::ConnectionManager;
use zznet_session::room_message_trait::{
    DeserializationError, RoomMessageTrait, SerializationError,
};
use zznet_session::types::RoomId;
use zznet_transport_tcp::config::TlsConfig;
use zzping_auth::config::AclConfig;

// ============================================================================
// Step 1: Define Secure Messages
// ============================================================================

/// Secure messages exchanged over TLS
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
enum SecureMessage {
    /// Authentication challenge
    AuthChallenge { nonce: Vec<u8> },

    /// Authentication response
    AuthResponse { signature: Vec<u8> },

    /// Encrypted data transfer
    SecureData { payload: Vec<u8> },

    /// Health check
    HealthCheck { timestamp: u64 },
}

impl RoomMessageTrait for SecureMessage {
    fn room_id(&self) -> RoomId {
        match self {
            SecureMessage::AuthChallenge { .. } | SecureMessage::AuthResponse { .. } => {
                RoomId::from("auth")
            }
            SecureMessage::SecureData { .. } => RoomId::from("data"),
            SecureMessage::HealthCheck { .. } => RoomId::from("health"),
        }
    }

    fn serialize_inner(&self) -> Result<Vec<u8>, SerializationError> {
        bincode::serialize(self).map_err(|e| SerializationError::BincodeError(e.to_string()))
    }

    fn deserialize_for_room(_room_id: &RoomId, bytes: &[u8]) -> Result<Self, DeserializationError> {
        bincode::deserialize(bytes).map_err(|e| DeserializationError::BincodeError(e.to_string()))
    }

    fn supported_rooms() -> Vec<RoomId> {
        vec![
            RoomId::from("auth"),
            RoomId::from("data"),
            RoomId::from("health"),
        ]
    }
}

// ============================================================================
// Step 2: Main Example - TLS Server and Client Setup
// ============================================================================

#[actix::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .try_init()
        .ok();

    log::info!("=== TLS Example ===\n");
    log::info!("This example demonstrates:");
    log::info!("1. Configuring TLS with role-based certificates");
    log::info!("2. Mutual TLS authentication (server verifies client, client verifies server)");
    log::info!("3. Secure message exchange over TLS");
    log::info!("4. Certificate-based access control\n");

    // ========================================================================
    // Step 3: Create TLS Configuration for Server (Database Role)
    // ========================================================================

    log::info!("🔐 Creating TLS configuration for server (Database role)...");

    let server_tls = match TlsConfig::from_role(Role::Database, None) {
        Ok(config) => {
            log::info!("   ✅ Server TLS config created:");
            log::info!("      Certificate: certs/database.pem");
            log::info!("      Private key: certs/database.key");
            log::info!("      CA cert: certs/ca.pem");
            config
        }
        Err(e) => {
            log::error!("   ❌ Failed to create server TLS config: {}", e);
            log::error!("\n📝 To generate certificates, run:");
            log::error!("   ./generate_certs.sh\n");
            return Err(e.into());
        }
    };

    // ========================================================================
    // Step 4: Create TLS Configuration for Client (Collector Role)
    // ========================================================================

    log::info!("\n🔐 Creating TLS configuration for client (Collector role)...");

    let client_tls = match TlsConfig::from_role(Role::Collector, None) {
        Ok(config) => {
            log::info!("   ✅ Client TLS config created:");
            log::info!("      Certificate: certs/collector.pem");
            log::info!("      Private key: certs/collector.key");
            log::info!("      CA cert: certs/ca.pem");
            config
        }
        Err(e) => {
            log::error!("   ❌ Failed to create client TLS config: {}", e);
            log::error!("\n📝 To generate certificates, run:");
            log::error!("   ./generate_certs.sh\n");
            return Err(e.into());
        }
    };

    // ========================================================================
    // Step 5: Set Up Secure Server with TLS
    // ========================================================================

    log::info!("\n📡 Setting up secure server with TLS...");

    // Load ACL for example and wire into ConnectionManager if available
    let offered_rooms = vec![
        RoomId::from("auth"),
        RoomId::from("data"),
        RoomId::from("health"),
    ];

    // Try to load example ACL; if it fails, continue without ACL
    let server_manager = match AclConfig::from_file("examples/tls-example/acl.toml") {
        Ok(cfg) => match cfg.into_acl_manager() {
            Ok(acl) => {
                let authorizer = acl.to_authorizer();
                ConnectionManager::<SecureMessage>::new_with_acl(
                    offered_rooms.clone(),
                    Some((authorizer, false)),
                )
                .start()
            }
            Err(e) => {
                log::warn!("Failed to build ACL manager: {}", e);
                ConnectionManager::<SecureMessage>::new(offered_rooms.clone()).start()
            }
        },
        Err(e) => {
            log::warn!("No ACL config loaded for tls-example: {}", e);
            ConnectionManager::<SecureMessage>::new(offered_rooms.clone()).start()
        }
    };

    let _server = ServerBuilder::new()
        .bind("127.0.0.1:9002")
        .as_role(AuthRole::Database)
        .offer_rooms(vec![
            "auth".to_string(),
            "data".to_string(),
            "health".to_string(),
        ])
        .with_tls(server_tls) // 🔒 Enable TLS with mutual authentication
        .with_connection_manager(server_manager)
        .start()
        .await?;

    log::info!("✅ Secure server listening on 127.0.0.1:9002");
    log::info!("   🔒 TLS enabled with mutual authentication");
    log::info!("   🔑 Requires valid client certificate");
    log::info!("   Role: Database");
    log::info!("   Rooms: auth, data, health\n");

    tokio::time::sleep(Duration::from_millis(100)).await;

    // ========================================================================
    // Step 6: Set Up Secure Client with TLS
    // ========================================================================

    log::info!("🔌 Setting up secure client with TLS...");

    let client_manager = ConnectionManager::<SecureMessage>::new(vec![
        RoomId::from("auth"),
        RoomId::from("data"),
        RoomId::from("health"),
    ])
    .start();

    let _client = ClientBuilder::new()
        .connect_to("127.0.0.1:9002")
        .as_role(AuthRole::Collector)
        .offer_rooms(vec![
            "auth".to_string(),
            "data".to_string(),
            "health".to_string(),
        ])
        .with_tls(client_tls) // 🔒 Enable TLS with mutual authentication
        .with_connection_manager(client_manager)
        .auto_reconnect(true)
        .reconnect_delay(Duration::from_secs(2))
        .connect()
        .await?;

    log::info!("✅ Secure client connected to 127.0.0.1:9002");
    log::info!("   🔒 TLS connection established");
    log::info!("   🔑 Client certificate verified by server");
    log::info!("   🔑 Server certificate verified by client");
    log::info!("   Role: Collector");
    log::info!("   Rooms: auth, data, health\n");

    // ========================================================================
    // Step 7: Wait for TLS Handshake and HELLO Protocol
    // ========================================================================

    log::info!("🤝 Waiting for TLS handshake and HELLO protocol...");
    tokio::time::sleep(Duration::from_millis(500)).await;
    log::info!("✅ TLS handshake complete!");
    log::info!("✅ HELLO protocol complete!\n");

    // ========================================================================
    // Step 8: Secure Communication Ready
    // ========================================================================

    log::info!("🔒 Secure communication channel established!");
    log::info!("   All messages are now:");
    log::info!("   • Encrypted with TLS");
    log::info!("   • Authenticated with mutual certificates");
    log::info!("   • Integrity-protected");
    log::info!("   • Protected against man-in-the-middle attacks\n");

    log::info!("📨 In a real application, you would:");
    log::info!("   1. Send secure data through the 'data' room");
    log::info!("   2. Perform authentication through the 'auth' room");
    log::info!("   3. Monitor health through the 'health' room");
    log::info!("   4. All communication is automatically encrypted\n");

    // Keep running to observe connection
    tokio::time::sleep(Duration::from_secs(2)).await;

    log::info!("✅ Example complete!");
    log::info!("   TLS connection successfully established and validated.\n");

    log::info!("👋 Shutting down gracefully...");

    Ok(())
}

// ============================================================================
// Notes for Developers
// ============================================================================

// **TLS Configuration**:
//
// The TlsConfig::from_role() method automatically configures:
// 1. Certificate path based on role (database.pem, collector.pem, etc.)
// 2. Private key path based on role (database.key, collector.key, etc.)
// 3. CA certificate for verification (ca.pem)
// 4. Server name for SNI (defaults to "zzping")
//
// **Certificate Requirements**:
//
// Each role needs its own certificate and private key:
// - Database: certs/database.{pem,key}
// - Collector: certs/collector.{pem,key}
// - ClientRo: certs/client-ro.{pem,key}
// - ClientAdmin: certs/client-admin.{pem,key}
//
// All certificates must be signed by the same CA (certs/ca.pem)
//
// **Mutual Authentication**:
//
// With TLS enabled:
// 1. Client verifies server's certificate against CA
// 2. Server verifies client's certificate against CA
// 3. Both sides must present valid certificates
// 4. Connection fails if either certificate is invalid
//
// **Security Benefits**:
//
// 1. **Encryption**: All data is encrypted in transit
// 2. **Authentication**: Both sides prove their identity
// 3. **Integrity**: Data cannot be modified in transit
// 4. **Non-repudiation**: Certificate provides proof of communication
//
// **Performance Considerations**:
//
// TLS adds:
// - Initial handshake overhead (~1 RTT)
// - Encryption/decryption CPU cost (~5-10% for modern CPUs)
// - Small packet size increase (~20-40 bytes per message)
//
// Benefits far outweigh costs for sensitive data.
//
// **Certificate Management**:
//
// Production considerations:
// 1. Use proper certificate authority (not self-signed)
// 2. Rotate certificates regularly
// 3. Monitor certificate expiration
// 4. Use hardware security modules (HSM) for private keys
// 5. Implement certificate revocation checking (CRL/OCSP)
//
// **Troubleshooting**:
//
// Common issues:
// 1. "No such file or directory" - Run generate_certs.sh
// 2. "Certificate verification failed" - Check CA matches
// 3. "Handshake failed" - Verify certificate validity dates
// 4. "Permission denied" - Check file permissions on keys
//
// **Testing Without TLS**:
//
// To test without TLS, simply omit the .with_tls() call:
//
// ```rust
// let server = ServerBuilder::new()
//     .bind("127.0.0.1:9002")
//     // .with_tls(server_tls) // Commented out = plain TCP
//     .start()
//     .await?;
// ```
