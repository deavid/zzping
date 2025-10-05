use criterion::{black_box, criterion_group, criterion_main, Criterion};
use std::collections::HashSet;
use zznet_api::types::PeerIdentity;
use zzping_auth::acl::AclManager;
use zzping_auth::config::AclConfig;

fn bench_authorization_small_acl(c: &mut Criterion) {
    let mut allowed = HashSet::new();
    allowed.insert("alice@client-admin".to_string());
    allowed.insert("collector".to_string());
    let acl = AclManager::with_allowed_peers(allowed);

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

fn bench_authorization_large_acl(c: &mut Criterion) {
    let mut allowed = HashSet::new();
    for i in 0..1000 {
        allowed.insert(format!("user{}@client-ro", i));
    }
    let acl = AclManager::with_allowed_peers(allowed);

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

fn bench_acl_modification(c: &mut Criterion) {
    let mut acl = AclManager::new();

    c.bench_function("allow_user", |b| {
        b.iter(|| {
            acl.allow_user(black_box("testuser@client-ro"));
            acl.deny_user(black_box("testuser@client-ro"));
        })
    });
}

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

fn bench_role_only_authorization(c: &mut Criterion) {
    let mut allowed = HashSet::new();
    allowed.insert("client-ro".to_string());
    let acl = AclManager::with_allowed_peers(allowed);

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

fn bench_concurrent_authorization(c: &mut Criterion) {
    let mut allowed = HashSet::new();
    allowed.insert("alice@client-admin".to_string());
    let acl = AclManager::with_allowed_peers(allowed);

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

criterion_group!(
    benches,
    bench_authorization_small_acl,
    bench_authorization_large_acl,
    bench_acl_modification,
    bench_config_parsing,
    bench_config_validation,
    bench_role_only_authorization,
    bench_concurrent_authorization
);
criterion_main!(benches);
