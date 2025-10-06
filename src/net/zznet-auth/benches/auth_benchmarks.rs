//! Benchmarks for ACL and authorization performance.
//!
//! These benchmarks exercise typical ACL operations to detect regressions in
//! authorization performance.
use criterion::Criterion;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::hint::black_box;
use zznet_api::types::PeerIdentity;
use zznet_auth::acl::AclManager;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
enum MockRole {
    Client,
    Service,
}
impl zznet_auth::ApplicationRole for MockRole {
    fn from_cn(cn: &str) -> Result<Self, zznet_auth::error::AuthError> {
        match cn {
            "client" => Ok(MockRole::Client),
            "service" => Ok(MockRole::Service),
            other => Err(zznet_auth::error::AuthError::UnknownRole(other.to_string())),
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            MockRole::Client => "client",
            MockRole::Service => "service",
        }
    }

    fn can_connect_to(&self, _target: &Self) -> bool {
        true
    }

    fn can_access_room(&self, _room_name: &str) -> bool {
        true
    }
}
use zznet_auth::config::AclConfig;

/// Benchmark: authorize with a small ACL.
fn bench_authorization_small_acl(c: &mut Criterion) {
    let mut allowed = HashSet::new();
    allowed.insert("alice@client-admin".to_string());
    allowed.insert("collector".to_string());
    let acl: AclManager<MockRole> = AclManager::with_allowed_peers(allowed);

    let identity = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "alice".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };

    c.bench_function("authorize_peer_small_acl", |b| {
        b.iter(|| {
            let _ = black_box(acl.authorize_peer(black_box(&identity)));
        })
    });
}

/// Benchmark: authorize with a large ACL.
fn bench_authorization_large_acl(c: &mut Criterion) {
    let mut allowed = HashSet::new();
    for i in 0..1000 {
        allowed.insert(format!("user{}@client-ro", i));
    }
    let acl: AclManager<MockRole> = AclManager::with_allowed_peers(allowed);

    let identity = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: "user500".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };

    c.bench_function("authorize_peer_large_acl", |b| {
        b.iter(|| {
            let _ = black_box(acl.authorize_peer(black_box(&identity)));
        })
    });
}

/// Benchmark: ACL allow/deny modification performance.
fn bench_acl_modification(c: &mut Criterion) {
    let mut acl: AclManager<MockRole> = AclManager::new();

    c.bench_function("allow_user", |b| {
        b.iter(|| {
            acl.allow_user(black_box("testuser@client-ro"));
            acl.deny_user(black_box("testuser@client-ro"));
        })
    });
}

/// Benchmark: parsing ACL TOML config.
fn bench_config_parsing(c: &mut Criterion) {
    let toml_content = r#"
allowed_peers = ["user1@client-ro", "user2@client-admin", "collector"]
"#
    .to_string();

    c.bench_function("parse_acl_config", |b| {
        b.iter(|| {
            let config: AclConfig = toml::from_str(black_box(&toml_content)).unwrap();
            black_box(config);
        })
    });
}

/// Benchmark: ACL config validation.
fn bench_config_validation(c: &mut Criterion) {
    let mut allowed = Vec::new();
    for i in 0..100 {
        allowed.push(format!("user{}@client-ro", i));
    }
    let config = AclConfig::new(allowed, false);

    c.bench_function("validate_acl_config", |b| {
        b.iter(|| {
            let _ = black_box(config.validate());
        })
    });
}

/// Benchmark: authorization when ACL contains only roles.
fn bench_role_only_authorization(c: &mut Criterion) {
    let mut allowed = HashSet::new();
    allowed.insert("client-ro".to_string());
    let acl: AclManager<MockRole> = AclManager::with_allowed_peers(allowed);

    let identity = PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: "anyuser".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };

    c.bench_function("authorize_peer_role_only", |b| {
        b.iter(|| {
            let _ = black_box(acl.authorize_peer(black_box(&identity)));
        })
    });
}

/// Benchmark: repeated authorization calls (concurrent scenario).
fn bench_concurrent_authorization(c: &mut Criterion) {
    let mut allowed = HashSet::new();
    allowed.insert("alice@client-admin".to_string());
    let acl: AclManager<MockRole> = AclManager::with_allowed_peers(allowed);

    let identity = PeerIdentity {
        common_name: "client-admin".to_string(),
        san_username: "alice".to_string(),
        peer_addr: "127.0.0.1:8080".to_string(),
    };

    c.bench_function("authorize_peer_concurrent", |b| {
        b.iter(|| {
            // Simulate concurrent access (though not truly concurrent in bench)
            let _ = black_box(acl.authorize_peer(black_box(&identity)));
        })
    });
}

/// Public entrypoint that runs the local benchmark functions.
pub fn benches_entry(c: &mut Criterion) {
    bench_authorization_small_acl(c);
    bench_authorization_large_acl(c);
    bench_acl_modification(c);
    bench_config_parsing(c);
    bench_config_validation(c);
    bench_role_only_authorization(c);
    bench_concurrent_authorization(c);
}

/// Simple main to run the benchmarks without macro-generated public items.
fn main() {
    let mut c = Criterion::default();
    benches_entry(&mut c);
}
