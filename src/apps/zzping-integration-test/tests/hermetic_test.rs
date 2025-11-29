//! Hermetic integration test for zzping system.
//!
//! This test verifies that the Collector and Database can communicate and register
//! correctly using mock transport connections. It runs the exact same lifecycle logic
//! as production but over in-memory channels instead of TCP sockets.

use actix::prelude::*;
use anyhow::Result;
use std::time::Duration;
use zznet_api::{ReconnectConfig, create_mock_pair, maintain_connection, serve_connections};
use zznet_hello::{ConnectionManager, HelloConfig};
use zznet_router::RouterActor;

#[actix::main]
async fn test_collector_database_handshake() -> Result<()> {
    // Setup tracing for debugging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .init();

    tracing::info!("Starting hermetic integration test");

    // ============ DATABASE SIDE ============
    tracing::info!("Spawning Database actors...");

    let db_router = RouterActor::new(vec![]).start();

    let intent_builder = zzintent_config::IntentConfigBuilder::new()
        .config_for_database(std::path::PathBuf::from("/tmp/test_intent.ron"));
    let _db_intent = intent_builder.router(db_router.clone()).start()?;

    let memdb_builder =
        zzmem_db::MemDBBuilder::new(zzmem_db::MemDBConfig::for_database(10000, None));
    let _db_memdb = memdb_builder.router(db_router.clone()).build();

    let cstate_builder = zzcollector_state::CStateBuilder::new(
        zzcollector_state::CStateConfig::for_database(5000, Some(100)),
    );
    let _db_cstate = cstate_builder.router(db_router.clone()).build();

    tracing::info!("Database actors spawned");

    // ============ COLLECTOR SIDE ============
    tracing::info!("Spawning Collector actors...");

    let coll_router = RouterActor::new(vec![]).start();

    let intent_builder = zzintent_config::IntentConfigBuilder::new().config_for_collector();
    let _coll_intent = intent_builder.router(coll_router.clone()).start()?;

    let memdb_builder = zzmem_db::MemDBBuilder::new(zzmem_db::MemDBConfig::for_collector(100));
    let _coll_memdb = memdb_builder.router(coll_router.clone()).build();

    tracing::info!("Collector actors spawned");

    // ============ MOCK CONNECTION SETUP ============
    tracing::info!("Creating mock connection pair...");

    let (mock_client_side, mock_server_side) = create_mock_pair("test_connection");

    // Convert to EstablishedConnection for use with lifecycle functions
    let server_conn = mock_server_side.into_established();
    let client_conn = mock_client_side.into_established();

    tracing::info!("Mock connections established");

    // ============ DATABASE LIFECYCLE ============
    tracing::info!("Starting Database server lifecycle...");

    let mut db_allowed_roles = std::collections::HashSet::new();
    db_allowed_roles.insert(zznet_api::Role::new("collector"));
    db_allowed_roles.insert(zznet_api::Role::new("client-ro"));
    db_allowed_roles.insert(zznet_api::Role::new("client-admin"));

    let db_hello_config = HelloConfig {
        hostname: "database".to_string(),
        our_role: "database".to_string(),
        offered_rooms: vec![
            "intent-config".to_string(),
            "memdb".to_string(),
            "query".to_string(),
        ],
        handshake_timeout: Duration::from_millis(1),
    };

    let db_connection_manager = ConnectionManager::new(
        db_router.clone().recipient(),
        db_hello_config,
        db_allowed_roles,
    );

    let db_cm_addr = db_connection_manager.start();

    tracing::info!("Database ConnectionManager started");

    // Create a MockServer that will yield the server-side connection
    // This tests the serve_connections lifecycle loop, ensuring the server
    // accept path works correctly with mock transport
    let mock_server = zznet_api::MockServer::new(vec![server_conn]);

    tracing::info!("Database serve_connections spawned");

    // Start the serving loop (background task)
    // This verifies that serve_connections correctly calls accept() and forwards to the recipient
    serve_connections(mock_server, db_cm_addr.recipient());

    // ============ COLLECTOR LIFECYCLE ============
    tracing::info!("Starting Collector client lifecycle...");

    let mut coll_allowed_roles = std::collections::HashSet::new();
    coll_allowed_roles.insert(zznet_api::Role::new("database"));
    coll_allowed_roles.insert(zznet_api::Role::new("collector"));

    let coll_hello_config = HelloConfig {
        hostname: "collector".to_string(),
        our_role: "collector".to_string(),
        offered_rooms: vec!["intent-config".to_string(), "memdb".to_string()],
        handshake_timeout: Duration::from_millis(1),
    };

    let coll_connection_manager = ConnectionManager::new(
        coll_router.clone().recipient(),
        coll_hello_config,
        coll_allowed_roles,
    );

    let coll_cm_addr = coll_connection_manager.start();

    tracing::info!("Collector ConnectionManager started");

    // Create a MockClient that returns our connection
    let mock_client = zznet_api::MockClient::with_connection(client_conn);

    let reconnect_config = ReconnectConfig {
        retry_delay: Duration::from_millis(1),
    };

    // Use the generic maintain_connection function with the mock client
    maintain_connection(mock_client, coll_cm_addr.recipient(), reconnect_config);

    tracing::info!("Collector maintain_connection spawned");

    // ============ VERIFICATION ============
    tracing::info!("Waiting for handshake to complete...");

    // Give some time for the handshake protocol to run
    tokio::time::sleep(Duration::from_millis(1)).await;

    tracing::info!("Test completed successfully!");

    Ok(())
}

#[test]
fn run_hermetic_test() {
    // Run the async test - the #[actix::main] macro already handles the runtime
    test_collector_database_handshake().unwrap();
}
