//! Stress tests for ACL performance and concurrency characteristics.
//!
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Instant;
use zznet_api::types::PeerIdentity;
use zznet_auth::acl::AclManager;
use zznet_auth::role::ApplicationRole;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
enum MockRole {
    X,
    Y,
}

impl ApplicationRole for MockRole {
    fn from_cn(cn: &str) -> Result<Self, zznet_auth::error::AuthError> {
        match cn {
            "x" => Ok(MockRole::X),
            "y" => Ok(MockRole::Y),
            _ => Err(zznet_auth::error::AuthError::UnknownRole(cn.to_string())),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            MockRole::X => "x",
            MockRole::Y => "y",
        }
    }

    fn can_connect_to(&self, _target: &Self) -> bool {
        true
    }

    fn can_access_room(&self, _room_name: &str) -> bool {
        true
    }
}

#[test]
fn stress_large_acl_lookup() {
    // Create a big allow-list
    let mut allowed = HashSet::new();
    for i in 0..10_000u32 {
        allowed.insert(format!("user{}@x", i));
    }

    let manager: AclManager<MockRole> = AclManager::with_allowed_peers(allowed);

    // Measure lookup time for a present and absent entry
    let id_present = PeerIdentity {
        common_name: "x".to_string(),
        san_username: "user9999".to_string(),
        peer_addr: "127.0.0.1:0".to_string(),
    };

    let start = Instant::now();
    let ok = manager.authorize_peer(&id_present).is_ok();
    let dur = start.elapsed();
    assert!(ok);
    // basic perf assertion: lookup should be fast
    assert!(dur.as_millis() < 50, "authorization too slow: {:?}", dur);

    let id_absent = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: "missing".to_string(),
        peer_addr: "127.0.0.1:0".to_string(),
    };
    assert!(manager.authorize_peer(&id_absent).is_err());
}

#[test]
fn test_high_connection_rate() {
    let mut allowed = HashSet::new();
    allowed.insert("x".to_string());
    allowed.insert("alice@y".to_string());
    let manager: AclManager<MockRole> = AclManager::with_allowed_peers(allowed);

    let identities = vec![
        PeerIdentity {
            common_name: "x".to_string(),
            san_username: "root".to_string(),
            peer_addr: "127.0.0.1:8080".to_string(),
        },
        PeerIdentity {
            common_name: "y".to_string(),
            san_username: "alice".to_string(),
            peer_addr: "127.0.0.1:8081".to_string(),
        },
        PeerIdentity {
            common_name: "y".to_string(),
            san_username: "bob".to_string(),
            peer_addr: "127.0.0.1:8082".to_string(),
        },
    ];

    let start = Instant::now();
    let mut success_count = 0;
    let mut total_count = 0;

    // Simulate 100 simultaneous connection attempts
    for _ in 0..100 {
        for id in &identities {
            total_count += 1;
            if manager.authorize_peer(id).is_ok() {
                success_count += 1;
            }
        }
    }

    let dur = start.elapsed();
    let avg_latency = dur.as_nanos() as f64 / total_count as f64;

    // Should handle 100 connections quickly
    assert!(
        dur.as_millis() < 100,
        "high connection rate too slow: {:?}",
        dur
    );
    assert!(
        avg_latency < 1_000_000.0,
        "average latency too high: {} ns",
        avg_latency
    ); // < 1ms

    // Only collector and alice should succeed
    assert_eq!(success_count, 200); // 100 iterations * 2 successful identities
    assert_eq!(total_count, 300); // 100 iterations * 3 identities
}

#[test]
fn test_concurrent_acl_updates() {
    let manager = Arc::new(Mutex::new(AclManager::<MockRole>::new()));
    let mut handles = vec![];

    // Spawn threads that add/remove users concurrently
    for i in 0..10 {
        let manager_clone = Arc::clone(&manager);
        let handle = thread::spawn(move || {
            for j in 0..100 {
                let user = format!("user{}{}@x", i, j);
                {
                    let mut mgr = manager_clone.lock().unwrap();
                    mgr.allow_user(&user);
                    // Check it works
                    let id = PeerIdentity {
                        common_name: "x".to_string(),
                        san_username: format!("user{}{}", i, j),
                        peer_addr: "127.0.0.1:0".to_string(),
                    };
                    assert!(mgr.authorize_peer(&id).is_ok());
                    mgr.deny_user(&user);
                    assert!(mgr.authorize_peer(&id).is_err());
                }
            }
        });
        handles.push(handle);
    }

    // Wait for all threads
    for handle in handles {
        handle.join().unwrap();
    }

    // Final state should be empty
    let test_id = PeerIdentity {
        common_name: "x".to_string(),
        san_username: "user00".to_string(),
        peer_addr: "127.0.0.1:0".to_string(),
    };
    let mgr = manager.lock().unwrap();
    assert!(mgr.authorize_peer(&test_id).is_err());
}

#[test]
fn test_long_running_connections() {
    let mut allowed = HashSet::new();
    allowed.insert("y".to_string());
    let manager: AclManager<MockRole> = AclManager::with_allowed_peers(allowed);

    let id = PeerIdentity {
        common_name: "y".to_string(),
        san_username: "root".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };

    let start = Instant::now();

    // Simulate 1000+ authorization checks over time
    for i in 0..1000 {
        assert!(manager.authorize_peer(&id).is_ok());

        // Simulate some time passing (without actual sleep for test speed)
        if i % 100 == 0 {
            let elapsed = start.elapsed();
            assert!(
                elapsed.as_millis() < 1000,
                "too slow after {} iterations",
                i
            );
        }
    }

    let total_dur = start.elapsed();
    assert!(
        total_dur.as_millis() < 500,
        "long-running test too slow: {:?}",
        total_dur
    );
}

#[test]
fn test_memory_usage_large_acl() {
    // Start with small ACL
    let mut allowed = HashSet::new();
    for i in 0..100 {
        allowed.insert(format!("user{}@x", i));
    }
    let mut manager = AclManager::<MockRole>::with_allowed_peers(allowed);

    // Grow the ACL dynamically
    for i in 100..1000 {
        manager.allow_user(&format!("user{}@x", i));
    }

    // Verify lookups still work
    let id = PeerIdentity {
        common_name: "x".to_string(),
        san_username: "user999".to_string(),
        peer_addr: "127.0.0.1:0".to_string(),
    };
    assert!(manager.authorize_peer(&id).is_ok());

    // Remove some
    for i in 500..600 {
        manager.deny_user(&format!("user{}@x", i));
    }

    // Verify removals
    let removed_id = PeerIdentity {
        common_name: "x".to_string(),
        san_username: "user550".to_string(),
        peer_addr: "127.0.0.1:0".to_string(),
    };
    assert!(manager.authorize_peer(&removed_id).is_err());
}

#[test]
fn test_acl_modification_race_conditions() {
    let manager = Arc::new(Mutex::new(AclManager::<MockRole>::new()));
    let mut handles = vec![];

    // Multiple threads modifying the same ACL entries
    for _ in 0..5 {
        let manager_clone = Arc::clone(&manager);
        let handle = thread::spawn(move || {
            for i in 0..50 {
                let user = format!("race{}@y", i);
                {
                    let mut mgr = manager_clone.lock().unwrap();
                    mgr.allow_user(&user);

                    let id = PeerIdentity {
                        common_name: "y".to_string(),
                        san_username: format!("race{}", i),
                        peer_addr: "127.0.0.1:0".to_string(),
                    };
                    // This should not panic or deadlock
                    let _ = mgr.authorize_peer(&id);

                    mgr.deny_user(&user);
                }
            }
        });
        handles.push(handle);
    }

    // Wait for completion
    for handle in handles {
        handle.join().unwrap();
    }
}
