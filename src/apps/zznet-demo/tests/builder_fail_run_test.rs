#![allow(missing_docs)]
use async_trait::async_trait;
use zznet_builder::builder::AppBuilder;
use zznet_builder::traits::ZZNetService;
use zznet_demo as _; // ensure crate compiles in this test

#[derive(Clone, serde::Deserialize, serde::Serialize)]
struct FailDemoConfig {
    pub name: String,
}
impl zznet_builder::traits::ZZNetConfig for FailDemoConfig {}

struct FailDemoService;

#[async_trait]
impl ZZNetService for FailDemoService {
    type Config = FailDemoConfig;
    type Error = anyhow::Error;

    fn new(_config: Self::Config) -> Result<Self, Self::Error> {
        Err(anyhow::anyhow!("intentional failure in new"))
    }

    async fn startup(&mut self) -> Result<(), Self::Error> {
        Ok(())
    }
}

#[tokio::test]
async fn test_builder_run_fails_when_service_new_errors() {
    let builder = AppBuilder::new("fail-demo", env!("CARGO_PKG_VERSION"));
    let cfg = FailDemoConfig {
        name: "x".to_string(),
    };
    let stop = async { tokio::time::sleep(std::time::Duration::from_millis(10)).await };

    let handle = tokio::task::spawn_blocking(move || {
        builder.run_service_with_config_and_stop::<FailDemoService, _>(cfg, stop)
    });

    let res = handle.await.unwrap();
    assert!(res.is_err());
}
