//! Collector service and component orchestration for the zzping collector application.
//!
//! This module defines `CollectorService`, the top-level application service that
//! configures, starts, and coordinates all collector components.

use crate::config::CollectorTlsConfig;
use actix::Addr;
use anyhow::Result;
use zzintent_config::actor::IntentConfigActor;
use zzmem_db::actor::MemDBActor;
use zznet_router::RouterActor;
use zzpinger::scheduler::PingerSchedulerActor;

/// Started components (running actors)
pub struct StartedComponents {
    /// Address of the running IntentConfig actor.
    pub intent_config: Addr<IntentConfigActor>,
    /// Address of the running Pinger scheduler actor.
    pub pinger: Addr<PingerSchedulerActor>,
    /// Address of the running MemDB actor.
    pub memdb_addr: Addr<MemDBActor>,
    /// RouterActor for data-plane message routing.
    pub router_actor: Addr<RouterActor>,
}

/// Convert collector TLS config to transport layer TLS config
pub fn convert_tls_config(
    tls: &CollectorTlsConfig,
) -> Result<zznet_transport_tcp::config::TlsConfig> {
    Ok(zznet_transport_tcp::tls_utils::to_transport_tls_config(
        &tls.client_cert_path,
        &tls.client_key_path,
        Some(&tls.ca_cert_path),
        "zzping-mesh".into(),
    ))
}

#[cfg(test)]
mod tests {
    use crate::config::{CollectorConfig, ComponentConfig};

    use super::*;
    use std::path::Path;

    /// Helper to create a valid test config.
    fn create_test_config() -> CollectorConfig {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let certs_dir = workspace_root.join("test_certs");
        CollectorConfig {
            collector_id: "test-collector".into(),
            database_host: "127.0.0.1".into(),
            database_port: 8443,
            tls: Some(CollectorTlsConfig {
                ca_cert_path: certs_dir.join("ca/ca.pem").to_str().unwrap().to_string(),
                client_cert_path: certs_dir
                    .join("dist/collector.pem")
                    .to_str()
                    .unwrap()
                    .to_string(),
                client_key_path: certs_dir
                    .join("secrets/collector.key")
                    .to_str()
                    .unwrap()
                    .to_string(),
            }),
            components: ComponentConfig {
                heartbeat_interval_ms: 5000,
                memdb_batch_size: 50,
                pinger_backend: crate::config::PingerBackend::Mock,
            },
            reconnect_delay_ms: 5000,
        }
    }

    #[test]
    fn test_convert_tls_config() {
        let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .parent()
            .unwrap();
        let certs_dir = workspace_root.join("test_certs");
        let tls_config = CollectorTlsConfig {
            ca_cert_path: certs_dir.join("ca.pem").to_str().unwrap().to_string(),
            client_cert_path: certs_dir
                .join("collector.pem")
                .to_str()
                .unwrap()
                .to_string(),
            client_key_path: certs_dir
                .join("collector.key")
                .to_str()
                .unwrap()
                .to_string(),
        };

        let result = convert_tls_config(&tls_config);
        assert!(result.is_ok(), "TLS config conversion should succeed");

        let transport_config = result.unwrap();
        assert_eq!(transport_config.server_name, "zzping-mesh");
        assert!(!transport_config.add_native_ca_certs);
        assert!(transport_config.ca_cert_path.is_some());
        assert_eq!(
            transport_config.cert.pem_path,
            Path::new(&tls_config.client_cert_path)
        );
        assert_eq!(
            transport_config.cert.key_path,
            Path::new(&tls_config.client_key_path)
        );
    }

    #[actix::test]
    async fn test_collector_network_creation() {
        // Install crypto provider for tests (required for TcpTransportClient::new)
        // Use Once to handle parallel test execution safely
        static INIT: std::sync::Once = std::sync::Once::new();
        INIT.call_once(|| {
            let _ = rustls::crypto::ring::default_provider().install_default();
        });

        use std::time::Duration;

        // Test network creation without TLS
        let _network = crate::network::CollectorNetwork::new(
            "127.0.0.1:8443",
            None,
            Duration::from_secs(5),
            Duration::from_secs(10),
        )
        .expect("Failed to create network");

        // Test network creation with TLS
        if let Some(tls) = &create_test_config().tls {
            let tls_config = convert_tls_config(tls).unwrap();
            let _network_with_tls = crate::network::CollectorNetwork::new(
                "127.0.0.1:8443",
                Some(tls_config),
                Duration::from_secs(5),
                Duration::from_secs(10),
            )
            .expect("Failed to create network with TLS");
        }
    }
}
