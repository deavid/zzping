//! Manages collector identities, roles, and the zero-downtime handoff process.
//!
//! The `Scheduler` is the brain of the high-availability system. Its primary
//! responsibility is to ensure that for any given collector `uuid`, there is always
//! one `Primary` instance and, if others are connected, one `Standby` instance
//! ready to take over. It handles new collectors joining, old ones becoming stale,
//! and the graceful handoff from a `Primary` to a `Standby`.

use dashmap::DashMap;
use log::info;
use std::time::{Duration, Instant};
use zzping_proto::zzping::CollectorRole;

/// The duration after which a collector is considered stale if it hasn't sent a
/// heartbeat. This is used for production environments.
const PROD_STALE_THRESHOLD: Duration = Duration::from_secs(5);

/// The delay before a `Standby` instance is promoted to `Primary` during a
/// graceful handoff. This gives the old `Primary` time to wind down.
const PROD_SWAP_DELAY: Duration = Duration::from_secs(3);

/// Represents a single connected instance of a collector.
#[derive(Debug, Clone)]
pub struct CollectorInstance {
    /// The process ID of the collector instance. This, combined with the `uuid`,
    /// uniquely identifies a running collector process.
    pub pid: u64,
    /// The role currently assigned to this collector instance by the scheduler.
    pub role: CollectorRole,
    /// The timestamp of the last heartbeat received from this instance. This is
    /// used to detect stale collectors.
    pub last_seen: Instant,
}

/// Stores the state of a handoff currently in progress for a given `uuid`.
#[derive(Debug, Clone)]
struct HandoffState {
    /// The process ID of the `Standby` collector that is being promoted.
    new_primary_pid: u64,
    /// The timestamp at which the promotion will take effect.
    swap_at: Instant,
}

/// The central scheduler for managing collector roles and high availability.
///
/// This struct holds the state for all connected collectors, grouped by their
/// `uuid`. It uses `DashMap` for thread-safe interior mutability, as it is
/// accessed concurrently by multiple gRPC worker threads.
#[derive(Debug)]
pub struct Scheduler {
    /// A map from a collector's `uuid` to a list of its running instances.
    ///
    /// For any given `uuid`, there can be multiple processes running (e.g., during
    /// a deployment). The `Vec` stores all known instances, and the scheduler's
    /// logic determines their roles.
    collectors: DashMap<String, Vec<CollectorInstance>>,
    /// A map from a `uuid` to the state of a handoff in progress.
    ///
    /// The presence of an entry in this map signifies that a `Standby` is being
    /// promoted to `Primary`.
    handoffs: DashMap<String, HandoffState>,
    /// The duration after which a collector is considered stale.
    stale_threshold: Duration,
    /// The delay for a graceful handoff.
    swap_delay: Duration,
}

impl Default for Scheduler {
    fn default() -> Self {
        Self::new()
    }
}

impl Scheduler {
    /// Creates a new `Scheduler` with production-ready timing constants.
    pub fn new() -> Self {
        Self::new_with_durations(PROD_STALE_THRESHOLD, PROD_SWAP_DELAY)
    }

    /// Creates a new `Scheduler` with custom timing durations for testing.
    ///
    /// This is crucial for writing fast and reliable unit tests, as it allows
    /// for testing stale logic without long `sleep` calls.
    pub fn new_with_durations(stale_threshold: Duration, swap_delay: Duration) -> Self {
        Self {
            collectors: DashMap::new(),
            handoffs: DashMap::new(),
            stale_threshold,
            swap_delay,
        }
    }

    /// Processes a heartbeat from a collector and determines its role.
    ///
    /// This is the main entry point into the scheduler's logic. It is a complex,
    /// multi-phase function that performs several actions:
    /// 1. Prunes any instances that have not sent a heartbeat recently.
    /// 2. Prunes any handoffs whose participants are no longer active.
    /// 3. Updates the `last_seen` time for the calling instance or adds it if new.
    /// 4. If a handoff is due, it performs the role swap (`Primary` -> `Shutdown`, `Standby` -> `Primary`).
    /// 5. If no `Primary` exists, it promotes a `Standby`.
    /// 6. If a new instance joins and a `Primary` already exists, it initiates a new handoff.
    ///
    /// It returns the calculated role for the calling instance and the time remaining
    /// until a pending handoff occurs.
    pub fn process_heartbeat(&self, uuid: &str, pid: u64, now: Instant) -> (CollectorRole, u64) {
        // Use a clone-and-replace strategy to avoid holding a lock for the whole function.
        let mut instances = self
            .collectors
            .get(uuid)
            .map(|v| v.value().clone())
            .unwrap_or_default();

        // Phase 1: Prune stale collectors.
        instances.retain(|i| now.duration_since(i.last_seen) < self.stale_threshold);

        // Phase 1b: Prune handoffs whose participants are gone.
        // This is done in two steps to avoid holding a read lock from .get() while
        // trying to acquire a write lock from .remove().
        let mut prune_handoff = false;
        if let Some(handoff) = self.handoffs.get(uuid) {
            let primary_pid = instances
                .iter()
                .find(|i| i.role == CollectorRole::Primary)
                .map(|i| i.pid);
            let handoff_participants_exist = instances
                .iter()
                .any(|i| i.pid == handoff.new_primary_pid || Some(i.pid) == primary_pid);
            if !handoff_participants_exist {
                prune_handoff = true;
            }
        }
        if prune_handoff {
            self.handoffs.remove(uuid);
        }

        // Phase 1c: Update current instance or add it if it's new.
        let is_new_instance = if let Some(instance) = instances.iter_mut().find(|i| i.pid == pid) {
            instance.last_seen = now;
            false
        } else {
            instances.push(CollectorInstance {
                pid,
                role: CollectorRole::Standby,
                last_seen: now,
            });
            true
        };

        // Phase 2: Check for and apply a completed handoff.
        if let Some(handoff) = self.handoffs.get(uuid).map(|h| h.value().clone())
            && now >= handoff.swap_at
        {
            if let Some(old_primary) = instances
                .iter_mut()
                .find(|i| i.role == CollectorRole::Primary)
            {
                info!(
                    "Verification success for uuid='{}'. Commanding shutdown for pid={}",
                    uuid, old_primary.pid
                );
                old_primary.role = CollectorRole::Shutdown;
            }
            if let Some(new_primary) = instances
                .iter_mut()
                .find(|i| i.pid == handoff.new_primary_pid)
            {
                info!(
                    "Promoting standby to primary for uuid='{}', pid={}",
                    uuid, new_primary.pid
                );
                new_primary.role = CollectorRole::Primary;
            }
            self.handoffs.remove(uuid);
        }

        // Phase 3: Steady-state role assignment (if no handoff is in progress).
        if !self.handoffs.contains_key(uuid)
            && !instances.iter().any(|i| i.role == CollectorRole::Primary)
            && let Some(candidate) = instances
                .iter_mut()
                .find(|i| i.role == CollectorRole::Standby)
        {
            info!(
                "No primary found for uuid='{}'. Promoting standby pid={} to primary.",
                uuid, candidate.pid
            );
            candidate.role = CollectorRole::Primary;
        }

        // Phase 4: Initiate a new handoff if required.
        if is_new_instance
            && instances.len() > 1
            && instances.iter().any(|i| i.role == CollectorRole::Primary)
            && !self.handoffs.contains_key(uuid)
        {
            let incumbent_pid = instances
                .iter()
                .find(|i| i.role == CollectorRole::Primary)
                .map(|i| i.pid)
                .unwrap_or(0);
            info!(
                "Handoff detected for uuid='{}': incumbent_pid={}, new_pid={}",
                uuid, incumbent_pid, pid
            );
            let handoff = HandoffState {
                new_primary_pid: pid,
                swap_at: now + self.swap_delay,
            };
            self.handoffs.insert(uuid.to_string(), handoff);
        }

        // Phase 5: Calculate return values and write the final state back to the map.
        let role = instances.iter().find(|i| i.pid == pid).unwrap().role;
        let nanos = self.handoffs.get(uuid).map_or(0, |h| {
            h.swap_at.saturating_duration_since(now).as_nanos() as u64
        });

        self.collectors.insert(uuid.to_string(), instances);

        (role, nanos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ntest::timeout;

    fn test_scheduler() -> Scheduler {
        Scheduler::new_with_durations(Duration::from_millis(100), Duration::from_millis(50))
    }

    #[test]
    #[timeout(100)]
    fn test_first_instance_is_primary() {
        let scheduler = test_scheduler();
        let now = Instant::now();
        let (role, _) = scheduler.process_heartbeat("c1", 1111, now);
        assert_eq!(role, CollectorRole::Primary);
    }

    #[test]
    #[timeout(100)]
    fn test_second_instance_is_standby_and_triggers_handoff() {
        let scheduler = test_scheduler();
        let now = Instant::now();
        scheduler.process_heartbeat("c1", 1111, now);
        let (role2, swap_time) = scheduler.process_heartbeat("c1", 2222, now);
        assert_eq!(role2, CollectorRole::Standby);
        assert!(swap_time > 0);
    }

    #[test]
    #[timeout(100)]
    fn test_graceful_handoff_flow() {
        let scheduler = test_scheduler();
        let t0 = Instant::now();

        scheduler.process_heartbeat("c1", 1111, t0);
        scheduler.process_heartbeat("c1", 2222, t0);

        let t1 = t0 + scheduler.swap_delay + Duration::from_millis(10);

        // Heartbeat from the new primary first to trigger the swap
        let (role_c2, _) = scheduler.process_heartbeat("c1", 2222, t1);
        assert_eq!(role_c2, CollectorRole::Primary);

        // Heartbeat from the old primary to confirm it's shutting down
        let (role_c1, _) = scheduler.process_heartbeat("c1", 1111, t1);
        assert_eq!(role_c1, CollectorRole::Shutdown);
    }

    #[test]
    #[timeout(100)]
    fn test_standby_is_promoted_when_primary_goes_stale() {
        let scheduler = test_scheduler();
        let t0 = Instant::now();
        scheduler.process_heartbeat("c1", 1111, t0);
        let (role_c2_before, _) = scheduler.process_heartbeat("c1", 2222, t0);
        assert_eq!(role_c2_before, CollectorRole::Standby);

        let t1 = t0 + scheduler.stale_threshold + Duration::from_millis(10);

        // Primary (1111) is now stale and will be pruned.
        // Standby (2222) will be promoted.
        let (role_c2_after, _) = scheduler.process_heartbeat("c1", 2222, t1);
        assert_eq!(role_c2_after, CollectorRole::Primary);
    }

    #[test]
    #[timeout(100)]
    fn test_stale_standby_is_pruned() {
        let scheduler = test_scheduler();
        let t0 = Instant::now();
        scheduler.process_heartbeat("c1", 1111, t0);
        scheduler.process_heartbeat("c1", 2222, t0);

        let t1 = t0 + scheduler.stale_threshold + Duration::from_millis(10);

        // 1111 and 2222 are now stale and will be pruned.
        // 3333 joins and should become primary immediately.
        let (role_c3, _) = scheduler.process_heartbeat("c1", 3333, t1);
        assert_eq!(role_c3, CollectorRole::Primary);

        let instances = scheduler.collectors.get("c1").unwrap();
        assert_eq!(instances.len(), 1);
        assert_eq!(instances[0].pid, 3333);
    }
}
