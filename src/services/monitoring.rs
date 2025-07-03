use anyhow::Result;
use std::time::Duration;
use tokio::time::{interval, sleep};
use tracing::{info, error, warn};

use crate::config::AppConfig;
use crate::models::OrderStatus;
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
                        warn!("Order {} fill failed: {:?}", order.id, result.error);
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

        info!("Found {} filled orders - automatic finalization DISABLED for manual testing", filled_orders.len());
        info!("💡 Use POST /api/v1/orders/{{order-id}}/finalize to manually trigger finalization");

        // AUTOMATIC FINALIZATION DISABLED FOR MANUAL TESTING
        // Uncomment the lines below to re-enable automatic finalization
        /*
        for order in filled_orders {
            info!("Processing finalization for order: {}", order.id);
            
            match self.finalization_service.finalize_order(order.id).await {
                Ok(result) => {
                    if result.success {
                        info!("Order {} finalized successfully", order.id);
                    } else {
                        warn!("Order {} finalization failed: {:?}", order.id, result.error);
                    }
                }
                Err(e) => {
                    error!("Error processing finalization for order {}: {}", order.id, e);
                }
            }

            // Small delay between processing orders
            sleep(Duration::from_millis(100)).await;
        }
        */

        Ok(())
    }

    pub async fn trigger_finalization(&self, order_id: uuid::Uuid) -> Result<bool> {
        info!("Manual finalization triggered for order: {}", order_id);
        
        match self.finalization_service.finalize_order(order_id).await {
            Ok(result) => {
                if result.success {
                    info!("Order {} finalized successfully", order_id);
                    Ok(true)
                } else {
                    warn!("Order {} finalization failed: {:?}", order_id, result.error);
                    Ok(false)
                }
            }
            Err(e) => {
                error!("Error processing finalization for order {}: {}", order_id, e);
                Err(e)
            }
        }
    }

    // Background monitoring task
    pub async fn start_background_monitoring(&self) -> Result<()> {
        info!("Starting background monitoring service...");

        // Clone services for background task
        let cross_chain_service = self.cross_chain_service.clone();
        let finalization_service = self.finalization_service.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(
                std::time::Duration::from_secs(config.monitoring.check_interval_seconds)
            );

            loop {
                interval.tick().await;
                
                info!("Running periodic monitoring check...");

                // Monitor for orders to fill
                if let Err(e) = Self::monitor_fill_processing(&cross_chain_service).await {
                    error!("Error during fill monitoring: {}", e);
                }

                // Monitor for orders to finalize
                // AUTOMATIC FINALIZATION DISABLED FOR MANUAL TESTING
                /*
                if let Err(e) = finalization_service.monitor_and_finalize_pending_orders().await {
                    error!("Error during finalization monitoring: {}", e);
                }
                */

                // Monitor chain health
                if let Err(e) = Self::monitor_chain_health(&cross_chain_service).await {
                    error!("Error during chain health monitoring: {}", e);
                }

                info!("Periodic monitoring check completed");
            }
        });

        info!("Background monitoring service started");
        Ok(())
    }

    async fn monitor_fill_processing(cross_chain_service: &crate::services::cross_chain::CrossChainService) -> Result<()> {
        info!("Monitoring pending orders for fill processing...");

        // This would typically query orders in 'Pending' status
        // For now, we'll implement this as a placeholder since the cross-chain service
        // doesn't have a direct method to get all pending orders

        // TODO: Implement automatic fill processing for pending orders
        // The cross-chain service would need access to storage to query pending orders
        // and then process them automatically

        Ok(())
    }

    async fn monitor_chain_health(cross_chain_service: &crate::services::cross_chain::CrossChainService) -> Result<()> {
        info!("Monitoring blockchain connectivity...");

        // Check if chains are reachable
        match cross_chain_service.get_contract_factory().check_chain_connectivity().await {
            Ok((origin_block, dest_block)) => {
                info!("Chains healthy - Origin: block {}, Destination: block {}", origin_block, dest_block);
            }
            Err(e) => {
                warn!("Chain connectivity issues detected: {}", e);
                // TODO: Implement alerting or recovery mechanisms
            }
        }

        Ok(())
    }
} 