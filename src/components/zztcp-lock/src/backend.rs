//! Backend abstraction for TCP locking: real TCP or in-memory registry.

use crate::config::LockStrategy;
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tracing::debug;

/// Global in-memory registry of held memory locks.
/// Each entry is an arbitrary ID string representing a lock holder.
static MEMORY_LOCK_REGISTRY: std::sync::OnceLock<Arc<Mutex<HashSet<String>>>> =
    std::sync::OnceLock::new();

/// Get or initialize the global memory lock registry.
fn get_registry() -> Arc<Mutex<HashSet<String>>> {
    MEMORY_LOCK_REGISTRY
        .get_or_init(|| Arc::new(Mutex::new(HashSet::new())))
        .clone()
}

/// RAII guard that holds a memory lock. When dropped, removes the ID from the registry.
pub struct MemoryLockGuard {
    lock_id: String,
}

impl MemoryLockGuard {
    /// Attempt to acquire a memory lock with the given ID.
    /// Returns `Ok(guard)` if successful, `Err(String)` if the ID is already held.
    pub fn try_acquire(lock_id: String) -> Result<Self, String> {
        let registry = get_registry();
        let mut held = registry
            .lock()
            .expect("Memory lock registry should not be poisoned");

        if held.contains(&lock_id) {
            Err(format!("Memory lock {} is already held", lock_id))
        } else {
            held.insert(lock_id.clone());
            Ok(MemoryLockGuard { lock_id })
        }
    }
}

impl Drop for MemoryLockGuard {
    fn drop(&mut self) {
        let registry = get_registry();
        {
            let mut held = registry
                .lock()
                .expect("Memory lock registry should not be poisoned");
            held.remove(&self.lock_id);
            debug!(
                "Memory lock {} released (registry now has {} locks)",
                self.lock_id,
                held.len()
            );
        }
    }
}

/// The backend abstraction for locking.
pub enum LockBackend {
    /// Real TCP socket (holds the OS-level listener).
    Real(TcpListener),
    /// In-memory lock (holds a guard that removes the ID on drop).
    Memory(MemoryLockGuard),
}

impl LockBackend {
    /// Attempt to acquire a lock based on the strategy.
    /// Returns `Ok(backend)` if successful, `Err(e)` if failed.
    pub async fn try_acquire(strategy: &LockStrategy) -> Result<Self, String> {
        match strategy {
            LockStrategy::Tcp(bind_addr) => match TcpListener::bind(bind_addr).await {
                Ok(listener) => Ok(LockBackend::Real(listener)),
                Err(e) => Err(format!("Failed to bind {}: {}", bind_addr, e)),
            },
            LockStrategy::Memory(lock_id) => match MemoryLockGuard::try_acquire(lock_id.clone()) {
                Ok(guard) => {
                    debug!("Memory lock {} acquired", lock_id);
                    Ok(LockBackend::Memory(guard))
                }
                Err(e) => Err(e),
            },
        }
    }
}
