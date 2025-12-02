//! Tests for the memory lock strategy to ensure deterministic, hermetic locking.

use zztcp_lock::backend::LockBackend;
use zztcp_lock::config::LockStrategy;

#[tokio::test]
async fn test_memory_lock_acquisition() {
    // Test that a memory lock can be acquired
    let strategy = LockStrategy::Memory("test-lock-1".to_string());
    let backend1 = LockBackend::try_acquire(&strategy).await;
    assert!(backend1.is_ok(), "First lock acquisition should succeed");

    // Test that a second attempt with the same ID fails (contention)
    let backend2 = LockBackend::try_acquire(&strategy).await;
    assert!(
        backend2.is_err(),
        "Second lock acquisition with same ID should fail (already held)"
    );

    // Drop the first lock and try again
    drop(backend1);

    // Now the lock should be available again
    let backend3 = LockBackend::try_acquire(&strategy).await;
    assert!(
        backend3.is_ok(),
        "After releasing, lock acquisition should succeed again"
    );

    drop(backend3);
}

#[tokio::test]
async fn test_tcp_lock_acquisition() {
    // Test that TCP lock can bind to a real port
    let strategy = LockStrategy::Tcp("127.0.0.1:0".to_string()); // 0 = auto-select available port
    let backend1 = LockBackend::try_acquire(&strategy).await;
    assert!(
        backend1.is_ok(),
        "TCP lock acquisition to port 0 should succeed"
    );

    drop(backend1);
}

#[tokio::test]
async fn test_memory_locks_are_independent() {
    // Test that locks with different IDs don't interfere
    let strategy1 = LockStrategy::Memory("lock-a".to_string());
    let strategy2 = LockStrategy::Memory("lock-b".to_string());

    let backend1 = LockBackend::try_acquire(&strategy1).await;
    assert!(backend1.is_ok(), "First lock should succeed");

    let backend2 = LockBackend::try_acquire(&strategy2).await;
    assert!(
        backend2.is_ok(),
        "Second lock with different ID should also succeed"
    );

    drop(backend1);
    drop(backend2);
}
