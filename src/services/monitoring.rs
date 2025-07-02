use anyhow::Result;
use std::time::Duration;
use tokio::time::{interval, sleep};
use tracing::{info, error, warn};

use crate::config::AppConfig;
use crate::models::{OrderStatus};
use crate::storage::MemoryStorage;
use crate::services::{CrossChainService, FinalizationService};

pub struct OrderMonitoringService {
    storage: MemoryStorage,
    cross_chain_service: CrossChainService,
    finalization_service: FinalizationService,
    config: AppConfig,
}

impl OrderMonitoringService {
    pub async fn new(storage: MemoryStorage, config: AppConfig) -> Result<Self> {
        let cross_chain_service = CrossChainService::new(storage.clone(), config.clone()).await?;
        let finalization_service = FinalizationService::new(storage.clone(), config.clone()).await?;

        Ok(Self {
            storage,
            cross_chain_service,
            finalization_service,
            config,
        })
    }

    pub async fn start(&self) -> Result<()> {
        info!("Starting order monitoring service");

        // Create interval timer for periodic checks
        let mut interval = interval(Duration::from_secs(5)); // Check every 5 seconds

        loop {
            interval.tick().await;

            // Process pending orders
            if let Err(e) = self.process_pending_orders().await {
                error!("Error processing pending orders: {}", e);
            }

            // Process filled orders for finalization
            if let Err(e) = self.process_filled_orders().await {
                error!("Error processing filled orders: {}", e);
            }
        }
    }

    async fn process_pending_orders(&self) -> Result<()> {
        let pending_orders = self.storage.get_pending_orders().await?;
        
        if pending_orders.is_empty() {
            return Ok(());
        }

        info!("Processing {} pending orders", pending_orders.len());

        for order in pending_orders {
            info!("Processing fill for order: {}", order.id);
            
            match self.cross_chain_service.process_fill(order.id).await {
                Ok(result) => {
                    if result.success {
                        info!("Order {} filled successfully", order.id);
                    } else {
                        warn!("Order {} fill failed: {:?}", order.id, result.error_message);
                    }
                }
                Err(e) => {
                    error!("Error processing fill for order {}: {}", order.id, e);
                }
            }

            // Small delay between processing orders to avoid overwhelming the system
            sleep(Duration::from_millis(100)).await;
        }

        Ok(())
    }

    async fn process_filled_orders(&self) -> Result<()> {
        let filled_orders = self.storage.get_orders_by_status(OrderStatus::Filled).await?;
        
        if filled_orders.is_empty() {
            return Ok(());
        }

        info!("Processing {} filled orders for finalization", filled_orders.len());

        for order in filled_orders {
            info!("Processing finalization for order: {}", order.id);
            
            match self.finalization_service.process_finalization(order.id).await {
                Ok(result) => {
                    if result.success {
                        info!("Order {} finalized successfully", order.id);
                    } else {
                        warn!("Order {} finalization failed: {:?}", order.id, result.error_message);
                    }
                }
                Err(e) => {
                    error!("Error processing finalization for order {}: {}", order.id, e);
                }
            }

            // Small delay between processing orders
            sleep(Duration::from_millis(100)).await;
        }

        Ok(())
    }

    pub async fn trigger_finalization(&self, order_id: uuid::Uuid) -> Result<bool> {
        info!("Manual finalization triggered for order: {}", order_id);
        
        match self.finalization_service.process_finalization(order_id).await {
            Ok(result) => {
                if result.success {
                    info!("Order {} finalized successfully", order_id);
                    Ok(true)
                } else {
                    warn!("Order {} finalization failed: {:?}", order_id, result.error_message);
                    Ok(false)
                }
            }
            Err(e) => {
                error!("Error processing finalization for order {}: {}", order_id, e);
                Err(e)
            }
        }
    }
} 