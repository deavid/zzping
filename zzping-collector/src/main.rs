use anyhow::Result;
use zzping_collector::run;

#[tokio::main]
async fn main() -> Result<()> {
    run().await
}
