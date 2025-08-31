use anyhow::Result;
use zzping_database::run;

#[tokio::main]
async fn main() -> Result<()> {
    run().await
}
