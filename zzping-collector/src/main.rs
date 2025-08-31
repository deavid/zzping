use anyhow::Result;
use zzping_collector::runner::run;

#[tokio::main]
async fn main() -> Result<()> {
    run().await
}
