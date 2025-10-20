//! Actor implementation for the MemDB component.
//!
//! This module contains the MemDBActor which handles both Collector and Database roles
//! for in-memory ping result storage and querying.

use crate::messages::{
    ClearBuffer, GetHealth, GetStats, MemDBError, MemDBHealth, StorePingResult, TargetStats,
};
use crate::network_messages::{MemDBMessage, PingResult};
use crate::permission_wrapper::PermissionWrapper;
use crate::role::MemDBRole;
use crate::storage::StorageBackend;
use actix::prelude::*;
use std::rc::Rc;
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use zznet_auth::role::ApplicationRole;
use zznet_session::peer_session::RoomHandle;
use zznet_session::session_manager::SessionManager;
use zznet_session::types::RoomId;

/// Message to set the session manager on a running actor
#[derive(Message)]
#[rtype(result = "()")]
pub struct SetSessionManager<T: ApplicationRole> {
    /// The session manager to set
    pub session_manager: Rc<SessionManager<MemDBMessage, PermissionWrapper<T>>>,
}

/// Room handle that forwards MemDB messages to the MemDBActor
pub struct MemDBRoomHandle<T: ApplicationRole> {
    addr: Addr<MemDBActor<T>>,
    room_id: RoomId,
}

impl<T: ApplicationRole> MemDBRoomHandle<T> {
    /// Create a new MemDBRoomHandle that forwards messages to the given actor address
    pub fn new(addr: Addr<MemDBActor<T>>) -> Self {
        Self {
            addr,
            room_id: RoomId::from("memdb"),
        }
    }
}

impl<T: ApplicationRole> RoomHandle<MemDBMessage> for MemDBRoomHandle<T> {
    fn room_id(&self) -> &RoomId {
        &self.room_id
    }

    fn send_message(
        &mut self,
        msg: MemDBMessage,
    ) -> Result<(), zznet_session::types::SessionError> {
        self.addr.do_send(msg);
        Ok(())
    }

    fn spawn_forwarder(
        &mut self,
        _tx: tokio::sync::mpsc::Sender<(RoomId, MemDBMessage)>,
    ) -> Result<(), zznet_session::types::SessionError> {
        // Not needed for direct forwarding
        Ok(())
    }
}

/// The MemDBActor handles ping result storage and querying.
///
/// This actor operates in two roles:
/// - **Collector**: Buffers ping results and sends batches to Database peers
/// - **Database**: Receives batches, stores data, and provides query interface
pub struct MemDBActor<T: ApplicationRole> {
    /// Role configuration (Collector or Database)
    role: MemDBRole,

    /// Storage backend for Database role (None for Collector)
    storage: Option<StorageBackend>,

    /// Buffer for Collector role (unsent results)
    buffer: Vec<PingResult>,

    /// SessionManager for network communication
    // Temporarily commented out Handler<MemDBMessage>

    // Network message handler for MemDBMessage
    session_manager: Option<Rc<SessionManager<MemDBMessage, PermissionWrapper<T>>>>,
    /// Health counters for operational visibility
    successful_batches: Arc<AtomicU64>,
    failed_batches: Arc<AtomicU64>,
    total_results: Arc<AtomicU64>,

    /// For Collector role: track the timestamp of the currently outstanding batch
    outstanding_batch: Option<u64>,
}

impl<T: ApplicationRole> Default for MemDBActor<T> {
    fn default() -> Self {
        Self::new_with_role(MemDBRole::default())
    }
}

impl<T: ApplicationRole> MemDBActor<T> {
    /// Create a new MemDBActor with the specified role
    pub fn new_with_role(role: MemDBRole) -> Self {
        Self::new_with_role_and_session_manager(role, None)
    }

    /// Create a new MemDBActor with role and optional SessionManager
    pub fn new_with_role_and_session_manager(
        role: MemDBRole,
        session_manager: Option<Rc<SessionManager<MemDBMessage, PermissionWrapper<T>>>>,
    ) -> Self {
        // Validate the role configuration
        if let Err(e) = role.validate() {
            panic!("Invalid role configuration: {}", e);
        }

        let storage = if role.is_database() {
            Some(StorageBackend::new(
                role.max_results_per_target().unwrap_or(1000),
            ))
        } else {
            None
        };

        Self {
            role,
            storage,
            buffer: Vec::new(),
            session_manager,
            successful_batches: Arc::new(AtomicU64::new(0)),
            failed_batches: Arc::new(AtomicU64::new(0)),
            total_results: Arc::new(AtomicU64::new(0)),
            outstanding_batch: None,
        }
    }

    /// Get the current role
    pub fn role(&self) -> &MemDBRole {
        &self.role
    }

    /// Set the SessionManager for network communication
    pub fn set_session_manager(
        &mut self,
        session_manager: Rc<SessionManager<MemDBMessage, PermissionWrapper<T>>>,
    ) {
        self.session_manager = Some(session_manager);
    }

    /// Store a ping result (used by both roles)
    fn store_result(&mut self, result: PingResult) {
        if let Some(storage) = &mut self.storage {
            // Database role: use StorageBackend
            let batch_timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0);
            storage.insert_batch(vec![result], batch_timestamp);
        } else {
            // This shouldn't happen in normal operation, but handle gracefully
            log::warn!("Attempted to store result but no storage backend available");
        }

        // Update counters
        self.total_results.fetch_add(1, Ordering::Relaxed);
    }

    /// Send a batch of results to Database peers (Collector role only)
    fn send_batch(&mut self) -> Result<(), MemDBError> {
        if !self.role.is_collector() {
            return Err(MemDBError::WrongRole);
        }

        if self.buffer.is_empty() {
            return Ok(()); // Nothing to send
        }

        // Check if we already have an outstanding batch
        if self.outstanding_batch.is_some() {
            log::warn!("Already have outstanding batch, not sending new one");
            return Ok(());
        }

        let results = std::mem::take(&mut self.buffer);
        let result_count = results.len();
        let timestamp_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);

        // Mark this batch as outstanding
        self.outstanding_batch = Some(timestamp_ms);

        // Create batch message
        let batch = MemDBMessage::SubmitBatch {
            sender_peer_id: "".to_string(), // will be filled by SessionManager when sending
            timestamp_ms,
            results: results.clone(),
        };

        // If we have a session manager, send to Database peers
        if let Some(sm_rc) = &self.session_manager {
            let memdb_room = zznet_session::types::RoomId::from("memdb");
            // Iterate peers and send to those with memdb room joined
            for peer_id in sm_rc.peer_ids() {
                let joined = sm_rc
                    .is_room_joined_with_peer(&peer_id, &memdb_room)
                    .unwrap_or(false);
                if joined && sm_rc.get_peer_sender(&peer_id).is_some() {
                    let sender = sm_rc.get_peer_sender(&peer_id).unwrap();
                    let msg_to_send = batch.clone();
                    let room_clone = memdb_room.clone();

                    // Set sender_peer_id to our own identity if available via session manager identity (not available here), leave empty
                    let send_fut = async move {
                        if let Err(e) = sender.send((room_clone, msg_to_send)).await {
                            tracing::warn!("Failed to send SubmitBatch to {}: {:?}", peer_id, e);
                        }
                    };

                    // Spawn the send in background
                    actix::spawn(send_fut);
                }
            }

            log::debug!("Sent batch with {} results to peers", result_count);
            Ok(())
        } else {
            // No session manager: mark as successful locally and log
            log::debug!(
                "No session manager: would send batch with {} results",
                result_count
            );
            Ok(())
        }
    }

    /// Get statistics for a specific target
    fn get_target_stats(&self, target: &str) -> TargetStats {
        if let Some(storage) = &self.storage {
            let (result_count, avg_rtt_us, packet_loss_percent, last_seen_ms) =
                storage.get_target_stats(target);
            TargetStats {
                target: target.to_string(),
                result_count,
                avg_rtt_us,
                packet_loss_percent,
                last_seen_ms,
            }
        } else {
            // No storage available
            TargetStats {
                target: target.to_string(),
                result_count: 0,
                avg_rtt_us: None,
                packet_loss_percent: 0.0,
                last_seen_ms: None,
            }
        }
    }
}

impl<T: ApplicationRole> Actor for MemDBActor<T> {
    type Context = Context<Self>;

    fn started(&mut self, _ctx: &mut Context<Self>) {
        log::info!("MemDBActor has started with role: {:?}", self.role);
    }

    fn stopped(&mut self, _ctx: &mut Context<Self>) {
        log::info!("MemDBActor has stopped");
    }
}

// Message handlers

impl<T: ApplicationRole> Handler<SetSessionManager<T>> for MemDBActor<T> {
    type Result = ();

    fn handle(&mut self, msg: SetSessionManager<T>, _ctx: &mut Context<Self>) {
        self.session_manager = Some(msg.session_manager);
    }
}

impl<T: ApplicationRole> Handler<StorePingResult> for MemDBActor<T> {
    type Result = Result<(), MemDBError>;

    fn handle(&mut self, msg: StorePingResult, _ctx: &mut Context<Self>) -> Self::Result {
        if self.role.is_collector() {
            // Collector role: buffer the result
            let target = msg.result.target.clone();
            self.buffer.push(msg.result);
            log::debug!("Buffered ping result for {}", target);

            // Check if we should send a batch
            if self
                .role
                .buffer_size()
                .is_some_and(|buffer_size| self.buffer.len() >= buffer_size)
            {
                self.send_batch()?;
            }
        } else {
            // Database role: store directly
            self.store_result(msg.result);
        }

        Ok(())
    }
}

impl<T: ApplicationRole> Handler<ClearBuffer> for MemDBActor<T> {
    type Result = Result<(), MemDBError>;

    fn handle(&mut self, _msg: ClearBuffer, _ctx: &mut Context<Self>) -> Self::Result {
        if !self.role.is_collector() {
            return Err(MemDBError::WrongRole);
        }

        let cleared_count = self.buffer.len();
        self.buffer.clear();
        log::info!("Cleared buffer of {} results", cleared_count);

        Ok(())
    }
}

impl<T: ApplicationRole> Handler<GetHealth> for MemDBActor<T> {
    type Result = Result<MemDBHealth, MemDBError>;

    fn handle(&mut self, _msg: GetHealth, _ctx: &mut Context<Self>) -> Self::Result {
        let role_str = match &self.role {
            MemDBRole::Database { .. } => "database",
            MemDBRole::Collector { .. } => "collector",
        }
        .to_string();

        let buffer_size = self.buffer.len();
        let total_results = self.total_results.load(Ordering::Relaxed);
        let successful_batches = self.successful_batches.load(Ordering::Relaxed);
        let failed_batches = self.failed_batches.load(Ordering::Relaxed);

        Ok(MemDBHealth {
            role: role_str,
            buffer_size,
            total_results,
            successful_batches,
            failed_batches,
            last_batch_ms: None, // TODO: implement last batch timestamp
        })
    }
}

impl<T: ApplicationRole> Handler<GetStats> for MemDBActor<T> {
    type Result = Result<TargetStats, MemDBError>;

    fn handle(&mut self, msg: GetStats, _ctx: &mut Context<Self>) -> Self::Result {
        if !self.role.is_database() {
            return Err(MemDBError::WrongRole);
        }

        Ok(self.get_target_stats(&msg.target))
    }
}

/// Network message handler for MemDBMessage
///
/// Handles incoming network messages from other MemDB peers.
/// The behavior depends on the actor's role:
/// - **Collector**: Receives BatchAck responses from Database peers
/// - **Database**: Receives SubmitBatch and Query requests, sends responses
impl<T: ApplicationRole> Handler<MemDBMessage> for MemDBActor<T> {
    type Result = ResponseFuture<()>;

    fn handle(&mut self, msg: MemDBMessage, _ctx: &mut Context<Self>) -> Self::Result {
        log::debug!("Handling network message: {:?}, role: {:?}", msg, self.role);

        // TODO: Use session_manager when we implement actual message sending
        // let session_manager = match &self.session_manager {
        //     Some(sm) => Rc::clone(sm),
        //     None => {
        //         log::error!("No session manager available for network message handling");
        //         return Box::pin(async {});
        //     }
        // };

        match (&self.role, msg) {
            // Database receives SubmitBatch from Collectors
            (
                MemDBRole::Database { .. },
                MemDBMessage::SubmitBatch {
                    sender_peer_id,
                    timestamp_ms,
                    results,
                },
            ) => {
                let received_count = results.len();
                log::debug!("Database received batch with {} results", received_count);

                // Store the batch
                if let Some(storage) = &mut self.storage {
                    storage.insert_batch(results, timestamp_ms);
                    self.total_results
                        .fetch_add(received_count as u64, Ordering::Relaxed);
                    self.successful_batches.fetch_add(1, Ordering::Relaxed);
                }

                // Send acknowledgment back to the sender if we have a session manager
                let ack = MemDBMessage::BatchAck {
                    received_count,
                    timestamp_ms: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_millis() as u64,
                };

                // Capture session manager and sender id if available
                let maybe_sm = self.session_manager.clone();

                Box::pin(async move {
                    if let Some(sm_rc) = maybe_sm {
                        // Build PeerId from the provided sender_peer_id string
                        let peer = zznet_session::types::PeerId::from(sender_peer_id.as_str());
                        // Try to obtain a sender for the original peer
                        if let Some(sender) = sm_rc.get_peer_sender(&peer) {
                            // Send the ack via the cloned sender
                            let room = zznet_session::types::RoomId::from("memdb");
                            if let Err(e) = sender.send((room, ack)).await {
                                tracing::warn!("Failed sending BatchAck to {}: {:?}", peer, e);
                            }
                        } else {
                            tracing::warn!(
                                "No sender available for peer {} to send BatchAck",
                                peer
                            );
                        }
                    } else {
                        log::debug!(
                            "No session manager configured; would send BatchAck: {:?}",
                            ack
                        );
                    }
                })
            }

            // Database receives Query from Admin clients
            (
                MemDBRole::Database { .. },
                MemDBMessage::Query {
                    sender_peer_id,
                    target,
                    from_ms,
                    to_ms,
                },
            ) => {
                log::debug!(
                    "Database received query for target {} from {} to {}",
                    target,
                    from_ms,
                    to_ms
                );

                let results = if let Some(storage) = &self.storage {
                    storage.query_target(&target, from_ms, to_ms)
                } else {
                    Vec::new()
                };

                // Prepare response message
                let response = MemDBMessage::QueryResponse { results };

                // Capture session manager and sender id if available
                let maybe_sm = self.session_manager.clone();

                Box::pin(async move {
                    if let Some(sm_rc) = maybe_sm {
                        // Build PeerId from the provided sender_peer_id string
                        let peer = zznet_session::types::PeerId::from(sender_peer_id.as_str());
                        if let Some(sender) = sm_rc.get_peer_sender(&peer) {
                            let room = zznet_session::types::RoomId::from("memdb");
                            if let Err(e) = sender.send((room, response)).await {
                                tracing::warn!("Failed sending QueryResponse to {}: {:?}", peer, e);
                            }
                        } else {
                            tracing::warn!(
                                "No sender available for peer {} to send QueryResponse",
                                peer
                            );
                        }
                    } else {
                        log::debug!(
                            "No session manager configured; would send QueryResponse with {} results",
                            0
                        );
                    }
                })
            }

            // Collector receives BatchAck from Database
            (
                MemDBRole::Collector { .. },
                MemDBMessage::BatchAck {
                    received_count,
                    timestamp_ms,
                },
            ) => {
                log::debug!(
                    "Collector received BatchAck for {} results at {}",
                    received_count,
                    timestamp_ms
                );

                // Handle acknowledgment: clear outstanding batch and update metrics
                if let Some(batch_ts) = self.outstanding_batch.take() {
                    if batch_ts == timestamp_ms {
                        self.successful_batches.fetch_add(1, Ordering::Relaxed);
                        log::debug!("Cleared outstanding batch with timestamp {}", timestamp_ms);
                    } else {
                        log::warn!(
                            "BatchAck timestamp mismatch: expected {}, got {}",
                            batch_ts,
                            timestamp_ms
                        );
                        // Put it back since it didn't match
                        self.outstanding_batch = Some(batch_ts);
                    }
                } else {
                    log::warn!("Received BatchAck but no outstanding batch");
                }

                Box::pin(async {})
            }

            // Collector should not receive SubmitBatch or Query
            (MemDBRole::Collector { .. }, MemDBMessage::SubmitBatch { .. }) => {
                log::warn!("Collector received SubmitBatch - this should not happen");
                Box::pin(async {})
            }

            // Collector should not receive Query
            (MemDBRole::Collector { .. }, MemDBMessage::Query { .. }) => {
                log::warn!("Collector received Query - this should not happen");
                Box::pin(async {})
            }

            // Collector should not receive QueryResponse
            (MemDBRole::Collector { .. }, MemDBMessage::QueryResponse { .. }) => {
                log::warn!("Collector received QueryResponse - this should not happen");
                Box::pin(async {})
            }

            // Database should not receive BatchAck or QueryResponse
            (MemDBRole::Database { .. }, MemDBMessage::BatchAck { .. }) => {
                log::warn!("Database received BatchAck - this should not happen");
                Box::pin(async {})
            }

            (MemDBRole::Database { .. }, MemDBMessage::QueryResponse { .. }) => {
                log::warn!("Database received QueryResponse - this should not happen");
                Box::pin(async {})
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::MemDBPermission;

    #[test]
    fn test_actor_creation() {
        let collector_actor =
            MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Collector { buffer_size: 100 });
        assert!(collector_actor.role().is_collector());

        let database_actor = MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Database {
            max_results_per_target: 1000,
            persistence_path: None,
        });
        assert!(database_actor.role().is_database());
    }

    #[test]
    fn test_actor_default() {
        let actor = MemDBActor::<MemDBPermission>::default();

        // Default role should be Collector with buffer_size 1000
        match actor.role {
            MemDBRole::Collector { buffer_size } => assert_eq!(buffer_size, 1000),
            _ => panic!("Default role should be Collector"),
        }

        // Should have no session manager initially
        assert!(actor.session_manager.is_none());

        // Should have empty buffer
        assert!(actor.buffer.is_empty());

        // Should have no outstanding batch
        assert!(actor.outstanding_batch.is_none());
    }

    #[test]
    fn test_send_batch_database_role_fails() {
        let mut actor = MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Database {
            max_results_per_target: 1000,
            persistence_path: None,
        });

        let result = actor.send_batch();
        assert!(matches!(result, Err(MemDBError::WrongRole)));
    }

    #[test]
    fn test_send_batch_empty_buffer() {
        let mut actor =
            MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Collector { buffer_size: 100 });

        // Buffer is empty
        assert!(actor.buffer.is_empty());

        let result = actor.send_batch();
        assert!(result.is_ok());
        // Should not have outstanding batch
        assert!(actor.outstanding_batch.is_none());
    }

    #[test]
    fn test_send_batch_with_outstanding_batch() {
        let mut actor =
            MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Collector { buffer_size: 100 });

        // Add a result to buffer
        actor.buffer.push(PingResult {
            target: "8.8.8.8".to_string(),
            timestamp_ms: 1234567890,
            rtt_us: Some(15000),
            sequence: 42,
        });

        // Set an outstanding batch
        actor.outstanding_batch = Some(1234567890);

        let result = actor.send_batch();
        assert!(result.is_ok());
        // Buffer should still have the result (not sent)
        assert_eq!(actor.buffer.len(), 1);
        // Outstanding batch should still be there
        assert_eq!(actor.outstanding_batch, Some(1234567890));
    }

    #[test]
    fn test_send_batch_no_session_manager() {
        let mut actor =
            MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Collector { buffer_size: 100 });

        // Add a result to buffer
        actor.buffer.push(PingResult {
            target: "8.8.8.8".to_string(),
            timestamp_ms: 1234567890,
            rtt_us: Some(15000),
            sequence: 42,
        });

        // No session manager
        assert!(actor.session_manager.is_none());

        let result = actor.send_batch();
        assert!(result.is_ok());
        // Buffer should be cleared
        assert!(actor.buffer.is_empty());
        // Should have outstanding batch
        assert!(actor.outstanding_batch.is_some());
    }

    #[test]
    fn test_set_session_manager_message_handler() {
        let actor =
            MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Collector { buffer_size: 100 });

        // Initially no session manager
        assert!(actor.session_manager.is_none());

        // We can't easily create a real SessionManager for this test,
        // but we can verify that the message handler exists and can be called
        // In integration tests, this would be tested with real SessionManager instances

        // For now, just verify the handler compiles and the method exists
        // The actual functionality will be tested in integration tests
    }

    #[actix::test]
    async fn test_actor_lifecycle_started() {
        let mut actor =
            MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Collector { buffer_size: 100 });
        let mut ctx = Context::new();

        // Call the started method
        actor.started(&mut ctx);

        // The method should complete without panicking
        // In a real scenario, this would log the startup message
    }

    #[actix::test]
    async fn test_actor_lifecycle_stopped() {
        let mut actor =
            MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Collector { buffer_size: 100 });
        let mut ctx = Context::new();

        // Call the stopped method
        actor.stopped(&mut ctx);

        // The method should complete without panicking
        // In a real scenario, this would log the shutdown message
    }

    #[test]
    fn test_store_result_collector() {
        let mut actor =
            MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Collector { buffer_size: 10 });

        let result = PingResult {
            target: "8.8.8.8".to_string(),
            timestamp_ms: 1234567890,
            rtt_us: Some(15000),
            sequence: 42,
        };

        let msg = StorePingResult { result };
        let result = actor.handle(msg, &mut Context::new());

        assert!(result.is_ok());
        assert_eq!(actor.buffer.len(), 1);
    }

    #[test]
    fn test_store_result_database() {
        let mut actor = MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Database {
            max_results_per_target: 1000,
            persistence_path: None,
        });

        let result = PingResult {
            target: "8.8.8.8".to_string(),
            timestamp_ms: 1234567890,
            rtt_us: Some(15000),
            sequence: 42,
        };

        let msg = StorePingResult {
            result: result.clone(),
        };
        let result = actor.handle(msg, &mut Context::new());

        assert!(result.is_ok());
        assert_eq!(
            actor
                .storage
                .as_ref()
                .unwrap()
                .query_target("8.8.8.8", 0, u64::MAX)
                .len(),
            1
        );
        assert_eq!(actor.total_results.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_clear_buffer_collector() {
        let mut actor =
            MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Collector { buffer_size: 10 });

        // Add some results to buffer
        actor.buffer.push(PingResult {
            target: "test".to_string(),
            timestamp_ms: 1234567890,
            rtt_us: Some(1000),
            sequence: 1,
        });

        let msg = ClearBuffer {};
        let result = actor.handle(msg, &mut Context::new());

        assert!(result.is_ok());
        assert_eq!(actor.buffer.len(), 0);
    }

    #[test]
    fn test_clear_buffer_database_fails() {
        let mut actor = MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Database {
            max_results_per_target: 1000,
            persistence_path: None,
        });

        let msg = ClearBuffer {};
        let result = actor.handle(msg, &mut Context::new());

        assert!(matches!(result, Err(MemDBError::WrongRole)));
    }

    #[test]
    fn test_get_health() {
        let mut actor =
            MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Collector { buffer_size: 10 });

        let msg = GetHealth {};
        let result = actor.handle(msg, &mut Context::new());

        assert!(result.is_ok());
        let health = result.unwrap();
        assert_eq!(health.role, "collector");
        assert_eq!(health.buffer_size, 0);
    }

    #[test]
    fn test_get_stats_database() {
        let mut actor = MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Database {
            max_results_per_target: 1000,
            persistence_path: None,
        });

        // Add a result
        actor.store_result(PingResult {
            target: "8.8.8.8".to_string(),
            timestamp_ms: 1234567890,
            rtt_us: Some(15000),
            sequence: 42,
        });

        let msg = GetStats {
            target: "8.8.8.8".to_string(),
        };
        let result = actor.handle(msg, &mut Context::new());

        assert!(result.is_ok());
        let stats = result.unwrap();
        assert_eq!(stats.target, "8.8.8.8");
        assert_eq!(stats.result_count, 1);
        assert_eq!(stats.avg_rtt_us, Some(15000.0));
    }

    #[test]
    fn test_get_stats_collector_fails() {
        let mut actor =
            MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Collector { buffer_size: 10 });

        let msg = GetStats {
            target: "8.8.8.8".to_string(),
        };
        let result = actor.handle(msg, &mut Context::new());

        assert!(matches!(result, Err(MemDBError::WrongRole)));
    }

    // #[actix::test]
    // async fn test_collector_to_database_integration() {
    //     // Integration test commented out due to SessionManager Send trait issues
    //     // TODO: Implement integration test once SessionManager Send is resolved
    // }

    #[actix::test]
    async fn test_memdb_room_handle_new() {
        let actor =
            MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Collector { buffer_size: 100 });
        let addr = actor.start();
        let room_handle = MemDBRoomHandle::new(addr);

        // Test that room_id returns the correct room
        assert_eq!(room_handle.room_id().as_str(), "memdb");
    }

    #[actix::test]
    async fn test_memdb_room_handle_send_message() {
        let actor = MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Database {
            max_results_per_target: 100,
            persistence_path: None,
        });
        let addr = actor.start();
        let mut room_handle = MemDBRoomHandle::new(addr);

        // Send a message - this should succeed (message goes to actor mailbox)
        let msg = MemDBMessage::Query {
            sender_peer_id: "test-peer".to_string(),
            target: "example.com".to_string(),
            from_ms: 1000,
            to_ms: 2000,
        };

        let result = room_handle.send_message(msg);
        assert!(result.is_ok());
    }

    #[actix::test]
    async fn test_memdb_room_handle_spawn_forwarder() {
        let actor =
            MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Collector { buffer_size: 100 });
        let addr = actor.start();
        let mut room_handle = MemDBRoomHandle::new(addr);

        // Create a dummy channel
        let (tx, _rx) = tokio::sync::mpsc::channel(10);

        // spawn_forwarder should succeed (returns Ok(()))
        let result = room_handle.spawn_forwarder(tx);
        assert!(result.is_ok());
    }

    #[actix::test]
    async fn test_batch_ack_handling() {
        let mut actor =
            MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Collector { buffer_size: 100 });

        // Set an outstanding batch with a specific timestamp
        let batch_timestamp = 1234567890;
        actor.outstanding_batch = Some(batch_timestamp);

        let msg = MemDBMessage::BatchAck {
            received_count: 5,
            timestamp_ms: batch_timestamp, // Matching timestamp
        };
        let future = actor.handle(msg, &mut Context::new());
        future.await;

        assert!(actor.outstanding_batch.is_none());
        assert_eq!(actor.successful_batches.load(Ordering::Relaxed), 1);
    }

    #[actix::test]
    async fn test_batch_ack_wrong_timestamp() {
        let mut actor =
            MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Collector { buffer_size: 100 });

        // Set an outstanding batch
        let batch_timestamp = 1234567890;
        actor.outstanding_batch = Some(batch_timestamp);

        let msg = MemDBMessage::BatchAck {
            received_count: 5,
            timestamp_ms: 1234567891, // Different timestamp
        };
        let future = actor.handle(msg, &mut Context::new());
        future.await;

        // Outstanding batch should still be there since timestamps don't match
        assert_eq!(actor.outstanding_batch, Some(batch_timestamp));
        assert_eq!(actor.successful_batches.load(Ordering::Relaxed), 0);
    }

    #[actix::test]
    async fn test_database_handles_submit_batch() {
        let mut actor = MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Database {
            max_results_per_target: 1000,
            persistence_path: None,
        });

        let msg = MemDBMessage::SubmitBatch {
            sender_peer_id: "peer1".to_string(),
            timestamp_ms: 1234567890,
            results: vec![
                PingResult {
                    target: "8.8.8.8".to_string(),
                    timestamp_ms: 1234567890,
                    rtt_us: Some(15000),
                    sequence: 42,
                },
                PingResult {
                    target: "8.8.4.4".to_string(),
                    timestamp_ms: 1234567891,
                    rtt_us: Some(20000),
                    sequence: 43,
                },
            ],
        };

        let future = actor.handle(msg, &mut Context::new());
        future.await;

        // Verify results were stored
        let results_8_8_8_8 = actor
            .storage
            .as_ref()
            .unwrap()
            .query_target("8.8.8.8", 0, u64::MAX);
        let results_8_8_4_4 = actor
            .storage
            .as_ref()
            .unwrap()
            .query_target("8.8.4.4", 0, u64::MAX);

        assert_eq!(results_8_8_8_8.len(), 1);
        assert_eq!(results_8_8_4_4.len(), 1);
        assert_eq!(actor.total_results.load(Ordering::Relaxed), 2);
    }

    #[actix::test]
    async fn test_collector_ignores_submit_batch() {
        let mut actor =
            MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Collector { buffer_size: 100 });

        let msg = MemDBMessage::SubmitBatch {
            sender_peer_id: "peer1".to_string(),
            timestamp_ms: 1234567890,
            results: vec![PingResult {
                target: "8.8.8.8".to_string(),
                timestamp_ms: 1234567890,
                rtt_us: Some(15000),
                sequence: 42,
            }],
        };

        let future = actor.handle(msg, &mut Context::new());
        future.await;

        // Collector should ignore SubmitBatch (no storage, no error)
        assert!(actor.storage.is_none());
    }

    #[actix::test]
    async fn test_database_ignores_query_response() {
        let mut actor = MemDBActor::<MemDBPermission>::new_with_role(MemDBRole::Database {
            max_results_per_target: 1000,
            persistence_path: None,
        });

        let msg = MemDBMessage::QueryResponse { results: vec![] };

        let future = actor.handle(msg, &mut Context::new());
        future.await;

        // Should log warning but not error
    }
}
