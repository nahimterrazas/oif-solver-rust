mod config;
mod server;
mod models;
mod services;
mod contracts;
mod storage;
mod handlers;

use anyhow::Result;
use tracing::{info, error};
use tracing_subscriber;

use crate::config::AppConfig;
use crate::server::SolverServer;
use crate::storage::memory::MemoryStorage;
use crate::services::monitoring::OrderMonitoringService;

#[actix_web::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    info!("Starting OIF Solver Rust POC");

    // Load configuration
    let config = AppConfig::load().await?;
    info!("Configuration loaded successfully");

    // Initialize storage
    let storage = MemoryStorage::new();
    info!("Storage initialized");

    // Initialize monitoring service
    let monitoring_service = OrderMonitoringService::new(storage.clone(), config.clone()).await?;
    info!("Order monitoring service initialized");

    // Start background monitoring
    let monitoring_handle = tokio::spawn(async move {
        if let Err(e) = monitoring_service.start().await {
            error!("Monitoring service error: {}", e);
        }
    });

    // Start HTTP server
    let server = SolverServer::new(storage, config.clone()).await?;
    info!("Starting HTTP server on {}:{}", config.server.host, config.server.port);
    
    // Run server and monitoring service concurrently
    tokio::select! {
        result = server.run() => {
            if let Err(e) = result {
                error!("Server error: {}", e);
            }
        }
        _ = monitoring_handle => {
            error!("Monitoring service stopped unexpectedly");
        }
    }

    Ok(())
} 