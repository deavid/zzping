use std::{collections::HashSet, net::IpAddr, str::FromStr};
use zzping_collector::{
    database_client::DatabaseClient,
    ping_surge_client::PingSurgeClient,
    task_supervisor::{SupervisorConfig, TaskSupervisor},
};

mod common;
use common::MockIngestionService;

#[tokio::test]
async fn test_supervisor_reconciliation_with_client_updates() {
    let server_addr = common::spawn_mock_server(MockIngestionService::default()).await;
    let db_client =
        DatabaseClient::connect(format!("http://{server_addr}"), "token".to_string())
            .await
            .unwrap();

    // Check if the environment supports creating raw sockets. If not, our assertions will be different.
    let can_create_workers = PingSurgeClient::new("127.0.0.1".parse().unwrap()).is_ok();
    if !can_create_workers {
        println!("NOTE: Test environment does not support raw socket creation. Skipping worker count assertions.");
    }

    // Supervisor starts with no client
    let mut supervisor = TaskSupervisor::new("test-uuid".to_string());
    assert!(supervisor.db_client.is_none());

    // 1. Send config with one target, but no client yet.
    let mut targets1 = HashSet::new();
    let target1_ip = IpAddr::from_str("127.0.0.1").unwrap();
    targets1.insert(target1_ip);
    let config1 = Some(SupervisorConfig {
        targets: targets1.clone(),
        ping_rate_pps: 10,
    });
    supervisor.reconcile(config1).await;
    assert!(
        supervisor.workers.is_empty(),
        "Worker should not be created without a database client"
    );

    // 2. Give it a client. Then re-reconcile with the same config.
    supervisor.db_client = Some(db_client.clone());
    supervisor.reconcile(Some(SupervisorConfig { targets: targets1, ping_rate_pps: 10 })).await;
    if can_create_workers {
        assert_eq!(supervisor.workers.len(), 1, "Worker should be created after client is received");
    }

    // 3. Lose the client.
    supervisor.db_client = None;

    // 4. Send new config. New worker should not be created.
    let mut targets2 = HashSet::new();
    let target2_ip = IpAddr::from_str("127.0.0.2").unwrap();
    targets2.insert(target1_ip);
    targets2.insert(target2_ip);
    let config2 = Some(SupervisorConfig {
        targets: targets2.clone(),
        ping_rate_pps: 10,
    });
    supervisor.reconcile(config2.clone()).await;
    if can_create_workers {
        assert_eq!(supervisor.workers.len(), 1, "New worker should not be created without a client");
    }

    // 5. Get a new client. Second worker should be created.
    supervisor.db_client = Some(db_client);
    supervisor.reconcile(config2.clone()).await;
    if can_create_workers {
        assert_eq!(supervisor.workers.len(), 2, "Second worker should be created after new client");
    }

    // 6. Remove all targets.
    supervisor.reconcile(None).await;
    assert!(supervisor.workers.is_empty(), "All workers should be removed");
}
