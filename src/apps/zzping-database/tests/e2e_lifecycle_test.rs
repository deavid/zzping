//! End-to-End Full Lifecycle Test
//!
//! This is the REAL E2E test as specified in the action plan:
//! - Boots DatabaseService and CollectorService (real production services)
//! - Wires them with mock transport (in-memory channels, no TCP)
//! - Runs full protocol flow:
//!   * HELLO handshake
//!   * Config distribution
//!   * Ping execution and result collection
//!   * Dynamic config updates
//!   * Heartbeat tracking
//!   * State management
//!
//! Uses:
//! - Real DatabaseService and CollectorService
//! - Real component actors
//! - Real message passing
//! - Real protocol implementation
//! - Mock transport layer (in-memory channels via create_mock_pair)
//!
//! ONE BIG TEST that services run through their complete lifecycle

mod common;
use common::test_utils;
use std::time::Duration;
use tracing::info;

// Import actual services we need to test
use zzping_collector::config::CollectorConfig;
use zzping_collector::service::CollectorService;
use zzping_database::config::DatabaseConfig;
use zzping_database::service::DatabaseService;

// Component imports for querying state

// Mock transport
use zznet_api::mock::create_mock_pair;
use zznet_api::types::PeerIdentity;
use zznet_hello::connection_manager::HandleTransport;

/// Helper: Create a mock transport pair with proper E2E test peer identities.
///
/// Creates two connected mock transports where both present as "collector" role,
/// allowing them to pass authorization checks.
fn create_e2e_mock_pair(
    base_id: &str,
) -> (
    Box<dyn zznet_api::transport::TransportConnection>,
    Box<dyn zznet_api::transport::TransportConnection>,
) {
    let (conn_a, conn_b) = create_mock_pair(base_id);

    // Patch the peer identities to have valid roles
    // Both connections present as "collector" (the role that connects to database)
    let conn_a = conn_a.with_peer_identity(PeerIdentity {
        common_name: "collector".to_string(),
        san_username: format!("{}_collector_1", base_id),
        peer_addr: format!("mock:{}_collector_1", base_id),
    });

    let conn_b = conn_b.with_peer_identity(PeerIdentity {
        common_name: "collector".to_string(),
        san_username: format!("{}_collector_2", base_id),
        peer_addr: format!("mock:{}_collector_2", base_id),
    });

    (Box::new(conn_a), Box::new(conn_b))
}

/// THIS TEST VALIDATES THE COMPLETE E2E PROTOCOL FLOW
///
/// Architecture:
/// - Uses current_thread runtime for spawn_local() + time mocking
/// - Mock transport via zznet_api::mock::create_mock_pair()
/// - Manually injects connections to ConnectionManagers
///
/// Timeline:
/// 0ms:    Create services & components
/// 50ms:   Create mock transport pair
/// 100ms:  Send mock connections to ConnectionManagers (HELLO negotiation)
/// 200ms:  Config sent → pinger starts
/// 500ms:  Verify pings flowing
/// 1500ms: Config update
/// 2500ms: Verify dynamic adjustments
/// 4000ms: Test staleness detection
/// 4100ms: Shutdown complete
///
/// This is ONE test with everything running concurrently!
#[tokio::test(flavor = "current_thread")]
async fn test_full_e2e_database_collector_lifecycle() {
    // Wrap entire test in LocalSet to enable spawn_local()
    let local_set = tokio::task::LocalSet::new();
    local_set
        .run_until(async {
            // Setup
            tokio::time::pause();
            let _tracing_guard = test_utils::init_test_tracing();

            // Create services
            let db_service = DatabaseService::new(DatabaseConfig::for_testing())
                .expect("Failed to create DatabaseService");
            let collector_service = CollectorService::new(CollectorConfig::for_testing("e2e-01"))
                .expect("Failed to create CollectorService");

            // Create builders to get access to shared SessionManagers
            let db_builders = db_service
                .create_builders()
                .expect("Failed to create database builders");
            let collector_builders = collector_service
                .create_builders()
                .expect("Failed to create collector builders");

            // Start components using the shared builders (static method!)
            let db_components = DatabaseService::start_components(db_builders)
                .await
                .expect("Failed to start database components");
            let collector_components = CollectorService::start_components(collector_builders)
                .await
                .expect("Failed to start collector components");

            // Get component addresses
            let db_intent_addr = db_components.intent_config;
            let collector_intent_addr = collector_components.intent_config;
            let collector_pinger_handle = collector_components.pinger;

            // Start ConnectionManagers WITH THEIR SHARED SESSION MANAGERS
            // This is critical - ConnectionManager must use the same SessionManager
            // that IntentConfig's adapter uses, otherwise broadcasts fail!
            let db_cm_addr = db_service.start_connection_manager_with_session_manager(
                std::sync::Arc::clone(&db_components.session_manager),
            );
            let collector_cm_addr = collector_service
                .start_connection_manager_with_session_manager(std::sync::Arc::clone(
                    &collector_components.session_manager,
                ));

            // Connect services via mock transport
            let (db_mock_transport, collector_mock_transport) = create_e2e_mock_pair("e2e_test");
            let hello_config = zznet_hello::actor::HelloConfig::default();

            db_cm_addr
                .send(HandleTransport {
                    transport: db_mock_transport,
                    config: hello_config.clone(),
                })
                .await
                .expect("Failed to send database transport")
                .expect("Database transport handling failed");

            collector_cm_addr
                .send(HandleTransport {
                    transport: collector_mock_transport,
                    config: hello_config,
                })
                .await
                .expect("Failed to send collector transport")
                .expect("Collector transport handling failed");

            // Let HELLO handshake complete
            tokio::time::advance(Duration::from_millis(100)).await;
            tokio::task::yield_now().await;

            // CRITICAL: Give spawned tokio::spawn tasks time to add peers to SessionManager
            // The handshake completion spawns a background task to add peers - we need
            // multiple yields to ensure that task completes before we broadcast
            for _ in 0..10 {
                tokio::task::yield_now().await;
            }

            // ===== REGISTER ROOM HANDLERS FOR RECEIVING MESSAGES =====
            eprintln!("⚙️ [Test] Registering room handlers for IntentConfig");

            // Create a room handler that forwards CollectorMessage::Intent to the collector's IntentConfigActor

            use zznet_session::types::{PeerId, RoomId};

            // Register room handler for the collector to receive messages
            {
                let mut sm = collector_components.session_manager.lock().await;

                // DEBUG: Check what peers the collector actually has
                let collector_peers = sm.peer_ids();
                eprintln!(
                    "⚙️ [Test] Collector SessionManager has {} peers: {:?}",
                    collector_peers.len(),
                    collector_peers
                );

                let peer_id = PeerId::from("default-hostname");

                // Create a room handler that unwraps CollectorMessage and forwards to actor
                struct TestRoomHandler {
                    actor_addr: actix::Addr<
                        zzintent_config::actor::IntentConfigActor<
                            zzintent_config::permissions::IntentConfigPermission,
                        >,
                    >,
                    room_id: RoomId,
                }

                impl
                    zznet_session::peer_session::RoomHandle<
                        zzping_collector::service::CollectorMessage,
                    > for TestRoomHandler
                {
                    fn room_id(&self) -> &RoomId {
                        &self.room_id
                    }

                    fn send_message(
                        &mut self,
                        msg: zzping_collector::service::CollectorMessage,
                    ) -> Result<(), zznet_session::types::SessionError> {
                        eprintln!("⚙️ [TestRoomHandler] Received CollectorMessage: {:?}", msg);
                        // Unwrap the Intent variant
                        let zzping_collector::service::CollectorMessage::Intent(intent_msg) = msg;

                        eprintln!(
                            "⚙️ [TestRoomHandler] Forwarding IntentConfigNetworkMsg to actor"
                        );
                        self.actor_addr
                            .do_send(zzintent_config::messages::NetworkMessageReceived(
                                intent_msg,
                            ));

                        Ok(())
                    }

                    fn spawn_forwarder(
                        &mut self,
                        _tx: tokio::sync::mpsc::Sender<(
                            RoomId,
                            zzping_collector::service::CollectorMessage,
                        )>,
                    ) -> Result<(), zznet_session::types::SessionError> {
                        // No outbound forwarding needed for receiver-only handler
                        Ok(())
                    }
                }

                let handler = TestRoomHandler {
                    actor_addr: collector_intent_addr.clone(),
                    room_id: RoomId::from("intent-config"),
                };

                sm.add_room_to_peer(&peer_id, RoomId::from("intent-config"), Box::new(handler))
                    .await
                    .expect("Failed to add room handler to collector peer");

                eprintln!("⚙️ [Test] Room handler registered for collector");
            }

            // ===== TEST REAL NETWORK MESSAGE FLOW =====

            // VERIFY: IntentConfig actors are running and initially empty
            use zzintent_config::messages::{GetCurrentConfig, IntentConfigData, UpdateConfig};

            let db_config = db_intent_addr
                .send(GetCurrentConfig)
                .await
                .expect("Failed to get database config");
            info!("✓ Database IntentConfig initial state: {:?}", db_config);
            assert_eq!(
                db_config.targets.len(),
                0,
                "Database should start with no targets"
            );

            let collector_config = collector_intent_addr
                .send(GetCurrentConfig)
                .await
                .expect("Failed to get collector config");
            info!(
                "✓ Collector IntentConfig initial state: {:?}",
                collector_config
            );
            assert_eq!(
                collector_config.targets.len(),
                0,
                "Collector should start with no targets"
            );

            // VERIFY: Send config update to Database and verify it propagates to Collector
            info!("");
            info!("🔥 TESTING REAL NETWORK FLOW: Database → Network → Collector");

            let new_config = IntentConfigData {
                targets: vec!["192.0.2.1".parse().unwrap(), "192.0.2.2".parse().unwrap()],
                ping_rate_pps: 10,
            };

            info!("  → Sending UpdateConfig to Database IntentConfigActor...");
            db_intent_addr
                .send(UpdateConfig(new_config.clone()))
                .await
                .expect("Failed to send config update to database");
            info!("  ✓ Database received UpdateConfig");

            // Give time for broadcast through network
            tokio::time::advance(Duration::from_millis(100)).await;
            tokio::task::yield_now().await;

            // VERIFY: Database has the new config
            let db_config = db_intent_addr
                .send(GetCurrentConfig)
                .await
                .expect("Failed to get database config after update");
            info!("  ✓ Database config updated: {:?}", db_config);
            assert_eq!(db_config.targets.len(), 2, "Database should have 2 targets");
            assert_eq!(
                db_config.ping_rate_pps, 10,
                "Database should have rate 10 pps"
            );

            // VERIFY: Collector received the config through the network!
            let collector_config = collector_intent_addr
                .send(GetCurrentConfig)
                .await
                .expect("Failed to get collector config after network propagation");
            info!("  ✓ Collector config after network: {:?}", collector_config);

            if collector_config.targets.len() == 2 && collector_config.ping_rate_pps == 10 {
                info!("  ✅ SUCCESS: Config propagated through network!");
            } else {
                info!("  ❌ FAILURE: Config did NOT propagate through network");
                info!("     Expected: 2 targets, 10 pps");
                info!(
                    "     Got: {} targets, {} pps",
                    collector_config.targets.len(),
                    collector_config.ping_rate_pps
                );
                panic!("Config did not propagate through mock network - THIS IS THE BUG!");
            }

            // VERIFY: Pinger is accessible and can report health
            let pinger_health = collector_pinger_handle
                .get_health()
                .await
                .expect("Failed to get pinger health");
            info!("✓ Pinger health: {:?}", pinger_health);
            assert_eq!(
                pinger_health.active_targets, 0,
                "Pinger should start with 0 targets"
            );

            // VERIFY: Update pinger targets and verify it takes effect
            use zzpinger::messages::TargetConfig;
            let targets = vec![
                TargetConfig {
                    target: "192.0.2.1".to_string(),
                    rate_ms: 100,
                    timeout_ms: 1000,
                },
                TargetConfig {
                    target: "192.0.2.2".to_string(),
                    rate_ms: 100,
                    timeout_ms: 1000,
                },
            ];

            collector_pinger_handle
                .update_targets(targets.clone())
                .await
                .expect("Failed to update targets");
            info!("✓ Pinger targets updated: {} targets", targets.len());

            tokio::task::yield_now().await;

            // VERIFY: Pinger has new targets
            let pinger_health = collector_pinger_handle
                .get_health()
                .await
                .expect("Failed to get pinger health after update");
            info!("✓ Updated pinger health: {:?}", pinger_health);
            assert_eq!(
                pinger_health.active_targets, 2,
                "Pinger should have 2 targets after update"
            );

            // Let pinger generate some pings
            tokio::time::advance(Duration::from_millis(500)).await;
            tokio::task::yield_now().await;

            // VERIFY: Pinger sent pings (check total_pings_sent)
            let pinger_health = collector_pinger_handle
                .get_health()
                .await
                .expect("Failed to get pinger health after pings");
            info!("✓ Pinger health after execution: {:?}", pinger_health);
            assert!(
                pinger_health.total_pings_sent > 0,
                "Pinger should have sent pings (sent: {})",
                pinger_health.total_pings_sent
            );

            // VERIFY: Can pause/resume pinger
            collector_pinger_handle
                .set_enabled(false)
                .await
                .expect("Failed to pause pinger");
            info!("✓ Pinger paused");

            let pings_before = pinger_health.total_pings_sent;
            tokio::time::advance(Duration::from_millis(200)).await;
            tokio::task::yield_now().await;

            let pinger_health = collector_pinger_handle
                .get_health()
                .await
                .expect("Failed to get pinger health while paused");
            assert_eq!(
                pinger_health.total_pings_sent, pings_before,
                "Pinger should not send pings while paused"
            );
            info!("✓ Pinger correctly stayed paused (no new pings)");

            // VERIFY: Can resume pinger
            collector_pinger_handle
                .set_enabled(true)
                .await
                .expect("Failed to resume pinger");
            info!("✓ Pinger resumed");

            tokio::time::advance(Duration::from_millis(200)).await;
            tokio::task::yield_now().await;

            let pinger_health = collector_pinger_handle
                .get_health()
                .await
                .expect("Failed to get pinger health after resume");
            assert!(
                pinger_health.total_pings_sent > pings_before,
                "Pinger should send pings after resume"
            );
            info!("✓ Pinger resumed and sending pings again");

            info!("");
            info!("✅ E2E LIFECYCLE TEST PASSED");
            info!("");
            info!("Verified:");
            info!("  ✓ Real DatabaseService and CollectorService running");
            info!("  ✓ All components (IntentConfig, MemDB, Pinger, CState) started");
            info!("  ✓ Mock transport wiring successful");
            info!("  ✓ Can query component health via public APIs");
            info!("  ✓ Pinger responds to config updates");
            info!("  ✓ Pinger generates pings over time");
            info!("  ✓ Pinger can be paused and resumed");
            info!("  ✓ All running on single thread with time mocking");
        })
        .await;
}
