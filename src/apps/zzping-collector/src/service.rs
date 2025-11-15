//! Collector service and component orchestration for the zzping collector application.
//!
//! This module defines `CollectorService`, the top-level application service that
//! configures, starts, and coordinates all collector components.

use crate::config::{CollectorConfig, CollectorTlsConfig};
use crate::error::CollectorError;
use actix::{Actor, Addr};
use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use zzintent_config::actor::IntentConfigActor;
use zzintent_config::builder::IntentConfigBuilder;
use zzintent_config::permissions::IntentConfigPermissions;
use zzmem_db::actor::MemDBActor;
use zzmem_db::builder::MemDBBuilder;
use zzmem_db::config::MemDBConfig;
use zznet_builder::traits::ZZNetService;
use zznet_router::RouterActor;
use zzpinger::api::PingerHandle;
use zzpinger::builder::PingerBuilder;
use zzpinger::permissions::PingerPermissions;

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
    /// Handle to the running Pinger actor.
    pub pinger: PingerHandle,
    /// Address of the running MemDB actor.
    pub memdb_addr: Addr<MemDBActor>,
    /// RouterActor for data-plane message routing.
    pub router_actor: Addr<RouterActor>,
}

#[derive(Debug)]
/// Collector service that orchestrates all components.
pub struct CollectorService {
    config: CollectorConfig,
}

impl CollectorService {
    /// Creates component builders for all collector components.
    pub fn create_builders(&self) -> Result<ComponentBuilders> {
        // Create permissions policy for intent-config (collector has read-only access)
        let mut intent_config_permissions = HashMap::new();
        intent_config_permissions.insert(
            "collector".to_string(),
            IntentConfigPermissions::new(true, false), // can read but not write
        );

        let intent_config = IntentConfigBuilder::new()
            .config_for_collector()
            .permissions_map(intent_config_permissions);

        // Create permissions policy for pinger (collector can update targets)
        let mut pinger_permissions = HashMap::new();
        pinger_permissions.insert(
            "collector".to_string(),
            PingerPermissions::new(true), // can update targets
        );

        let pinger = PingerBuilder::new()
            .enabled(true)
            .permissions_map(pinger_permissions);

        let memdb_addr = MemDBBuilder::new(MemDBConfig::for_collector(
            self.config.components.memdb_batch_size,
        ))
        .build();
        let pinger = pinger.memdb_addr(memdb_addr.clone());

        Ok(ComponentBuilders {
            intent_config,
            pinger,
            memdb_addr,
        })
    }

    /// Starts all collector components from their builders.
    pub async fn start_components(builders: ComponentBuilders) -> Result<StartedComponents> {
        let router_actor = RouterActor::new(vec![]).start();
        let intent_addr = builders
            .intent_config
            .router(router_actor.clone())
            .start()
            .map_err(|e| CollectorError::Component(format!("IntentConfig start failed: {}", e)))?;
        let memdb_addr = builders.memdb_addr;
        let pinger_handle = builders.pinger.router_actor(router_actor.clone()).start()?;

        Ok(StartedComponents {
            intent_config: intent_addr,
            pinger: pinger_handle,
            memdb_addr,
            router_actor,
        })
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
}

#[async_trait]
impl ZZNetService for CollectorService {
    type Config = CollectorConfig;
    type Error = CollectorError;

    fn new(config: Self::Config) -> Result<Self, Self::Error> {
        // Validation is now handled by the AppBuilder before `new` is called.
        Ok(Self { config })
    }

    async fn run(self) -> Result<(), Self::Error> {
        tracing::info!("Collector service starting");

        let builders = self.create_builders()?;
        let started = Self::start_components(builders).await?;

        tracing::info!("All components started successfully");
        tracing::info!("Starting network wiring");

        let tls_cfg = if let Some(tls) = &self.config.tls {
            tracing::info!("TLS enabled - using mTLS connection");
            Some(Self::convert_tls_config(tls).map_err(|e| {
                CollectorError::Service(format!("Failed to convert TLS config: {}", e))
            })?)
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

        tracing::info!("Collector service connecting to database");

        // Spawn network connect in a background task so `run()` returns quickly
        // and the builder can report successful startup. The network connect loop
        // will continue to run and log errors/retries; we do not await it here.
        let connect_started = StartedComponents {
            intent_config: started.intent_config.clone(),
            pinger: started.pinger.clone(),
            memdb_addr: started.memdb_addr.clone(),
            router_actor: started.router_actor.clone(),
        };

        let network_clone = network;
        tokio::spawn(async move {
            if let Err(e) = network_clone.connect(&connect_started).await {
                tracing::error!("Collector network task failed: {}", e);
            }
        });

        // The AppBuilder will hold the process open until a shutdown signal is received.
        // We just need to return Ok(()) here to indicate successful startup.
        Ok(())
    }

    fn service_name() -> &'static str {
        "ZZPing Collector"
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
            },
            reconnect_delay_ms: 5000,
        }
    }

    #[test]
    fn test_service_creation() {
        let config = create_test_config();
        let service = CollectorService::new(config);
        assert!(service.is_ok());
    }

    #[test]
    fn test_service_creation_with_invalid_config_is_handled_by_builder() {
        // This test is now conceptual. The builder calls `validate` before `new`.
        // If we were to call `new` directly with invalid config, it should succeed
        // because `new` no longer validates.
        let mut config = create_test_config();
        config.collector_id = String::new(); // Invalid!

        // Direct call to `new` should not fail, as validation is deferred to the builder.
        let result = CollectorService::new(config);
        assert!(result.is_ok());
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

        let result = CollectorService::convert_tls_config(&tls_config);
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
            let tls_config = CollectorService::convert_tls_config(tls).unwrap();
            let _network_with_tls = crate::network::CollectorNetwork::new(
                "127.0.0.1:8443",
                Some(tls_config),
                Duration::from_secs(5),
                Duration::from_secs(10),
            );
        }
    }
}
