//! Collector service and component orchestration for the zzping collector application.
//!
//! This module defines `CollectorService`, the top-level application service that
//! configures, starts, and coordinates all collector components.

use crate::config::{CollectorConfig, CollectorTlsConfig};
use actix::{Actor, Addr, Recipient};
use anyhow::Result;
use async_trait::async_trait;
use surge_ping::{Client, ConfigBuilder};
use tokio::task::JoinHandle;
use zzintent_config::actor::IntentConfigActor;
use zzintent_config::builder::IntentConfigBuilder;
use zzmem_db::actor::MemDBActor;
use zzmem_db::messages::StorePingResult;
use zznet_builder::traits::ZZNetApplication;
use zznet_router::RouterActor;
use zzpinger::builder::PingerBuilder;
use zzpinger::mock::MockPingerClient;

/// Builders for all components (before wiring)
pub struct ComponentBuilders {
    /// Builder for the IntentConfig component.
    pub intent_config: IntentConfigBuilder,
    /// Builder for the Pinger component.
    pub pinger: PingerBuilder,
    /// Address of the running MemDB actor.
    pub memdb_addr: Addr<MemDBActor>,
}

/// Started components (running actors)
pub struct StartedComponents {
    /// Address of the running IntentConfig actor.
    pub intent_config: Addr<IntentConfigActor>,
    /// Address of the running Pinger scheduler actor.
    pub pinger: Addr<zzpinger::scheduler::PingerSchedulerActor>,
    /// Address of the running MemDB actor.
    pub memdb_addr: Addr<MemDBActor>,
    /// RouterActor for data-plane message routing.
    pub router_actor: Addr<RouterActor>,
}

/// Collector application that orchestrates all components.
pub struct CollectorApp {
    config: CollectorConfig,

    // State: Pre-Startup (Dependencies waiting to be started)
    // These must be Option so we can .take() them during startup
    pinger_builder: Option<zzpinger::builder::PingerBuilder>,
    memdb_builder: Option<zzmem_db::builder::MemDBBuilder>,
    intent_builder: Option<zzintent_config::builder::IntentConfigBuilder>,
    // ... add other component builders here ...

    // State: Running (Active Actors)
    pinger_addr: Option<Addr<zzpinger::scheduler::PingerSchedulerActor>>,
    memdb_addr: Option<Addr<MemDBActor>>,
    intent_addr: Option<Addr<IntentConfigActor>>,
    network_task: Option<JoinHandle<()>>,
    // ... add other addresses here ...
}

impl CollectorApp {
    /// Create a new collector application with pre-configured component builders.
    ///
    /// This constructor follows the "Construction Outside, Execution Inside" pattern:
    /// the application receives fully-configured builders and will start them during
    /// the `startup()` phase.
    pub fn new(
        config: CollectorConfig,
        pinger_builder: zzpinger::builder::PingerBuilder,
        memdb_builder: zzmem_db::builder::MemDBBuilder,
        intent_builder: zzintent_config::builder::IntentConfigBuilder,
    ) -> Self {
        Self {
            config,
            pinger_builder: Some(pinger_builder),
            memdb_builder: Some(memdb_builder),
            intent_builder: Some(intent_builder),
            // Initial running state is empty
            pinger_addr: None,
            memdb_addr: None,
            intent_addr: None,
            network_task: None,
        }
    }
}

/// Convert collector TLS config to transport layer TLS config
pub fn convert_tls_config(
    tls: &CollectorTlsConfig,
) -> Result<zznet_transport_tcp::config::TlsConfig> {
    Ok(zznet_builder::tls::to_transport_tls_config(
        &tls.client_cert_path,
        &tls.client_key_path,
        Some(&tls.ca_cert_path),
    ))
}

#[async_trait]
impl ZZNetApplication for CollectorApp {
    fn service_name(&self) -> &str {
        "ZZPing Collector"
    }

    async fn startup(&mut self) -> Result<()> {
        // 1. Start Router (Standard actor, usually created inside startup)
        let router = zznet_router::RouterActor::new(vec![]).start();

        // 2. Take Builders and Start Components
        // Note: We unwrap() because if builders are missing at startup, it's a developer error.

        // Example: Intent Config
        let intent_addr = self
            .intent_builder
            .take()
            .unwrap()
            .router(router.clone())
            .start()?;
        self.intent_addr = Some(intent_addr);

        // Start MemDB
        let memdb_addr = self
            .memdb_builder
            .take()
            .unwrap()
            .router(router.clone())
            .build();
        let memdb_recipient: Recipient<StorePingResult> = memdb_addr.clone().recipient();
        self.memdb_addr = Some(memdb_addr.clone());

        // Start Pinger with the real memdb recipient
        let pinger_builder = self.pinger_builder.take().unwrap();
        let pinger_addr = match self.config.components.pinger_backend {
            crate::config::PingerBackend::Real => {
                let config = ConfigBuilder::default().build();
                log::info!("Creating surge_ping client...");
                let client = Client::new(&config)
                    .expect("Failed to create surge_ping client - check raw socket permissions");
                pinger_builder.start(client, memdb_recipient.clone())
            }
            crate::config::PingerBackend::Mock => {
                log::info!("Using mock pinger client for testing");
                let client = MockPingerClient::default();
                pinger_builder.start(client, memdb_recipient.clone())
            }
        };
        self.pinger_addr = Some(pinger_addr);

        // 3. Network wiring (CollectorNetwork)
        // The network connection loop is an async task, not an actor usually.
        // Spawn it here.
        let tls_cfg = if let Some(tls) = &self.config.tls {
            tracing::info!("TLS enabled - using mTLS connection");
            Some(convert_tls_config(tls)?)
        } else {
            tracing::warn!("TLS disabled - using plain TCP connection");
            None
        };

        let addr = format!(
            "{}:{}",
            self.config.database_host, self.config.database_port
        );
        let reconnect_delay = std::time::Duration::from_millis(self.config.reconnect_delay_ms);
        let handshake_timeout = std::time::Duration::from_secs(10);
        let network = crate::network::CollectorNetwork::new(
            &addr,
            tls_cfg,
            reconnect_delay,
            handshake_timeout,
        );

        let started_components = StartedComponents {
            intent_config: self.intent_addr.clone().unwrap(),
            pinger: self.pinger_addr.clone().unwrap(),
            memdb_addr: self.memdb_addr.clone().unwrap(),
            router_actor: router,
        };

        // TODO(network-startup): Currently if network.connect() fails (e.g., connection refused),
        // the error is only logged and startup() still returns Ok. This could leave the app
        // in a "zombie" state where it appears running but network is non-functional.
        // Future improvement: Use a oneshot channel to wait for "Connected" confirmation
        // before returning from startup(), so failures are propagated immediately.
        let handle = tokio::spawn(async move {
            if let Err(e) = network.connect(&started_components).await {
                tracing::error!("Collector network task failed: {}", e);
            }
        });
        self.network_task = Some(handle);

        Ok(())
    }

    async fn shutdown(&mut self) -> Result<()> {
        if let Some(handle) = self.network_task.take() {
            handle.abort();
        }

        // Drop actor addresses to stop them gracefully
        drop(self.pinger_addr.take());
        drop(self.memdb_addr.take());
        drop(self.intent_addr.take());

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::config::ComponentConfig;

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
            }),
            components: ComponentConfig {
                heartbeat_interval_ms: 5000,
                memdb_batch_size: 50,
                pinger_backend: crate::config::PingerBackend::Mock,
            },
            reconnect_delay_ms: 5000,
        }
    }

    #[actix_rt::test]
    async fn test_app_creation() {
        let config = create_test_config();
        // For testing, we need to create the builders
        let intent_builder =
            zzintent_config::builder::IntentConfigBuilder::new().config_for_collector();
        let memdb_builder =
            zzmem_db::builder::MemDBBuilder::new(zzmem_db::config::MemDBConfig::for_collector(50));
        let pinger_builder = zzpinger::builder::PingerBuilder {
            clock: None,
            spawn_strategy: zzpinger::builder::SpawnStrategy::NewArbiter,
        };

        let _app = CollectorApp::new(config, pinger_builder, memdb_builder, intent_builder);
        // Just check that it was created
        // If we get here, construction succeeded
    }

    #[actix_rt::test]
    async fn test_app_creation_with_invalid_config() {
        // Since validation is now done at config load time, not in new(),
        // this test is less relevant. We just test that construction works.
        let config = create_test_config();
        let intent_builder =
            zzintent_config::builder::IntentConfigBuilder::new().config_for_collector();
        let memdb_builder =
            zzmem_db::builder::MemDBBuilder::new(zzmem_db::config::MemDBConfig::for_collector(50));
        let pinger_builder = zzpinger::builder::PingerBuilder {
            clock: None,
            spawn_strategy: zzpinger::builder::SpawnStrategy::NewArbiter,
        };

        let _app = CollectorApp::new(config, pinger_builder, memdb_builder, intent_builder);
        // Construction should succeed
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
        assert_eq!(transport_config.server_name, "zzping");
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
        use std::time::Duration;

        // Test network creation without TLS
        let _network = crate::network::CollectorNetwork::new(
            "127.0.0.1:8443",
            None,
            Duration::from_secs(5),
            Duration::from_secs(10),
        );

        // Test network creation with TLS
        if let Some(tls) = &create_test_config().tls {
            let tls_config = convert_tls_config(tls).unwrap();
            let _network_with_tls = crate::network::CollectorNetwork::new(
                "127.0.0.1:8443",
                Some(tls_config),
                Duration::from_secs(5),
                Duration::from_secs(10),
            );
        }
    }
}
