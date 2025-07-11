pub mod config;
pub mod contracts;
pub mod handlers;
pub mod models;
pub mod server;
pub mod services;
pub mod storage;

use anyhow::Result;
use tracing::{info, error, warn};
use tracing_subscriber;
use tokio::signal;

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

    // Load persisted data if enabled
    if config.persistence.enabled {
        info!("Loading persisted data from: {}", config.persistence.data_file);
        if let Err(e) = storage.load_from_file(&config.persistence.data_file).await {
            warn!("Failed to load persisted data: {}", e);
            info!("Starting with empty storage");
        } else {
            let count = storage.count().await;
            info!("Successfully loaded {} orders from persistence file", count);
        }
    } else {
        info!("Persistence disabled, starting with empty storage");
    }

    // Initialize monitoring service
    let monitoring_service: OrderMonitoringService = OrderMonitoringService::new(storage.clone(), config.clone()).await?;
    info!("Order monitoring service initialized");

    // Start background monitoring
    let monitoring_handle = tokio::spawn(async move {
        if let Err(e) = monitoring_service.start().await {
            error!("Monitoring service error: {}", e);
        }
    });

    // Start HTTP server
    let server = SolverServer::new(storage.clone(), config.clone()).await?;
    info!("Starting HTTP server on {}:{}", config.server.host, config.server.port);
    
    // Create storage reference for shutdown handling
    let storage_for_shutdown = storage.clone();
    let config_for_shutdown = config.clone();
    
    // Handle shutdown signals
    let shutdown_handle = tokio::spawn(async move {
        // Wait for shutdown signal
        #[cfg(unix)]
        {
            let mut sigterm = signal::unix::signal(signal::unix::SignalKind::terminate()).unwrap();
            let mut sigint = signal::unix::signal(signal::unix::SignalKind::interrupt()).unwrap();
            
            tokio::select! {
                _ = sigterm.recv() => info!("Received SIGTERM"),
                _ = sigint.recv() => info!("Received SIGINT"),
            }
        }
        
        #[cfg(not(unix))]
        {
            let _ = signal::ctrl_c().await;
            info!("Received Ctrl+C");
        }
        
        // Perform graceful shutdown - save data if persistence is enabled
        info!("Shutting down server gracefully...");
        
        if config_for_shutdown.persistence.enabled {
            info!("Saving data to file: {}", config_for_shutdown.persistence.data_file);
            if let Err(e) = storage_for_shutdown.save_to_file(&config_for_shutdown.persistence.data_file).await {
                error!("Failed to save data during shutdown: {}", e);
            } else {
                let count = storage_for_shutdown.count().await;
                info!("Successfully saved {} orders to persistence file", count);
            }
        } else {
            info!("Persistence disabled, skipping data save");
        }
        
        info!("Server shutdown complete");
    });
    
    // Run server, monitoring service, and shutdown handler concurrently
    tokio::select! {
        result = server.run() => {
            if let Err(e) = result {
                error!("Server error: {}", e);
            }
        }
        _ = monitoring_handle => {
            error!("Monitoring service stopped unexpectedly");
        }
        _ = shutdown_handle => {
            info!("Shutdown signal received, stopping server");
        }
    }

    Ok(())
} 