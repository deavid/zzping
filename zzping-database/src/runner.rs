use crate::{
    grpc_server::IngestionServiceImpl, ingestion_item::IngestionItem, storage_engine, DATA_DIR,
    INGESTION_ADDR,
};
use anyhow::Result;
use log::{error, info};
use tokio::sync::mpsc;
use zzping_proto::zzping::ingestion_server::IngestionServer;

/// The main function for the database service.
///
/// Sets up logging, creates a channel for data ingestion, and starts the gRPC server.
pub async fn run() -> Result<()> {
    env_logger::builder()
        .filter_level(log::LevelFilter::Info)
        .try_init()
        .ok();
    info!("starting zzping-database server");

    // This channel is the central pipeline for all incoming data from collectors.
    let (tx, rx) = mpsc::channel::<IngestionItem>(1024);

    // The storage task runs in the background, consuming from the channel.
    tokio::spawn(storage_engine::storage_task(rx));

    // Run finalization for any old files on startup.
    if let Err(e) = run_startup_finalization(DATA_DIR) {
        error!("Startup finalization failed: {e}");
    }

    let addr = INGESTION_ADDR.parse()?;
    let ingestion_service = IngestionServiceImpl::new(tx);
    let server = IngestionServer::new(ingestion_service);

    info!("gRPC server listening on {addr}");
    tonic::transport::Server::builder()
        .add_service(server)
        .serve(addr)
        .await?;

    Ok(())
}

/// Scans the data directory for `.zzp1` files from previous days and finalizes them.
fn run_startup_finalization(data_dir: &str) -> Result<()> {
    use crate::finalization;
    use chrono::{Local, NaiveDate};
    use std::fs;

    info!("Running startup finalization check...");
    let today = Local::now().date_naive();
    fs::create_dir_all(data_dir)?;

    for entry in fs::read_dir(data_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "zzp1") {
            if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                let parts: Vec<&str> = stem.split('-').collect();
                if let Some(date_str) = parts.last() {
                    if let Ok(file_date) = NaiveDate::parse_from_str(date_str, "%Y%m%d") {
                        if file_date < today {
                            // This file is from a previous day, attempt to finalize it.
                            if let Err(e) = finalization::finalize_file(&path) {
                                error!("Failed to finalize file {path:?}: {e}");
                            }
                        }
                    }
                }
            }
        }
    }

    info!("Startup finalization check complete.");
    Ok(())
}
