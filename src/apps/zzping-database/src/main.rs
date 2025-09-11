use anyhow::Result;
use zzping_database::runner::run;

#[tokio::main]
async fn main() -> Result<()> {
    run().await
}
