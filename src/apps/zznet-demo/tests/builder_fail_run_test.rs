#![allow(missing_docs)]
use async_trait::async_trait;
use zznet_builder::harness::AppHarness;
use zznet_builder::traits::ZZNetApplication;
use zznet_demo as _; // ensure crate compiles in this test

struct FailDemoService;

#[async_trait]
impl ZZNetApplication for FailDemoService {
    fn service_name(&self) -> &str {
        "fail-demo"
    }

    async fn startup(&mut self) -> Result<(), anyhow::Error> {
        Err(anyhow::anyhow!("intentional failure in startup"))
    }

    async fn shutdown(&mut self) -> Result<(), anyhow::Error> {
        Ok(())
    }
}

#[tokio::test]
async fn test_harness_run_fails_when_service_startup_errors() {
    let harness = AppHarness::new().log_level("info");
    harness.init_logging();

    let service = FailDemoService;

    let handle = tokio::task::spawn_blocking(move || harness.run(service));

    let res = handle.await.unwrap();
    assert!(res.is_err());
}
