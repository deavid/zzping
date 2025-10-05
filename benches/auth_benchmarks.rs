use criterion::{criterion_group, criterion_main, Criterion};
use std::collections::HashSet;
use zzping_auth::acl::AclManager;
use zzping_auth::config::AclConfig;

fn bench_authorize_peer(c: &mut Criterion) {
    let mut allowed = HashSet::new();
    for i in 0..5000u32 {
        allowed.insert(format!("user{}@client-ro", i));
    }
    let manager = AclManager::with_allowed_peers(allowed);

    let id = zznet_api::types::PeerIdentity {
        common_name: "client-ro".to_string(),
        san_username: "user4000".to_string(),
        peer_addr: "127.0.0.1:0".to_string(),
    };

    c.bench_function("authorize_peer_5k", |b| {
        b.iter(|| manager.authorize_peer(&id))
    });
}

fn bench_config_from_file(c: &mut Criterion) {
    let cfg = AclConfig::new(vec!["a@client-ro".to_string(); 1000], false);
    c.bench_function("acl_config_validate_1k", |b| b.iter(|| cfg.validate()));
}

criterion_group!(benches, bench_authorize_peer, bench_config_from_file);
criterion_main!(benches);
