use anyhow::Result;
use tracing::{info, error, warn};
use alloy::providers::Provider;
use alloy::primitives::U256;

use crate::config::AppConfig;
use crate::contracts::ContractFactory;
use crate::models::{Order, OrderStatus, FillResult};
use crate::storage::MemoryStorage;

#[derive(Clone)]
pub struct FinalizationService {
    storage: MemoryStorage,
    contract_factory: ContractFactory,
    config: AppConfig,
}

impl FinalizationService {
    pub async fn new(storage: MemoryStorage, config: AppConfig) -> Result<Self> {
        let contract_factory = ContractFactory::new(config.clone()).await?;
        
        Ok(Self {
            storage,
            contract_factory,
            config,
        })
    }

    pub async fn finalize_order(&self, order_id: uuid::Uuid) -> Result<FillResult> {
        info!("Starting finalization for order: {}", order_id);

        // Get order from storage
        let mut order = match self.storage.get_order(order_id).await? {
            Some(order) => order,
            None => {
                let error_msg = format!("Order not found: {}", order_id);
                error!("{}", error_msg);
                return Ok(FillResult::failure(error_msg));
            }
        };

        // Validate order can be finalized
        if let Err(validation_error) = self.validate_finalization_preconditions(&order) {
            let error_msg = format!("Finalization validation failed: {}", validation_error);
            error!("{}", error_msg);
            order.set_error(error_msg.clone());
            self.storage.update_order(order).await?;
            return Ok(FillResult::failure(error_msg));
        }

        // Update status to finalizing
        order.update_status(OrderStatus::Finalizing);
        self.storage.update_order(order.clone()).await?;

        // Execute finalization
        match self.execute_finalization(&order).await {
            Ok(finalize_result) => {
                if finalize_result.success {
                    if let Some(tx_hash) = &finalize_result.tx_hash {
                        info!("Finalization executed successfully: {}", tx_hash);
                        
                        // Update order with finalization transaction hash and status
                        order.set_finalize_tx(tx_hash.clone());
                        order.update_status(OrderStatus::Finalized);
                        self.storage.update_order(order).await?;
                    }
                    Ok(finalize_result)
                } else {
                    let error_msg = finalize_result.error.unwrap_or("Unknown finalization error".to_string());
                    error!("Finalization failed: {}", error_msg);
                    
                    order.set_error(error_msg.clone());
                    self.storage.update_order(order).await?;
                    Ok(FillResult::failure(error_msg))
                }
            }
            Err(e) => {
                let error_msg = format!("Finalization execution error: {}", e);
                error!("{}", error_msg);
                
                order.set_error(error_msg.clone());
                self.storage.update_order(order).await?;
                Ok(FillResult::failure(error_msg))
            }
        }
    }

    fn validate_finalization_preconditions(&self, order: &Order) -> Result<(), String> {
        // Check order is in correct state for finalization
        match order.status {
            OrderStatus::Filled => {
                info!("Order {} is ready for finalization (status: Filled)", order.id);
            }
            OrderStatus::Finalizing => {
                return Err("Order is already being finalized".to_string());
            }
            OrderStatus::Finalized => {
                return Err("Order is already finalized".to_string());
            }
            _ => {
                return Err(format!("Order cannot be finalized in current status: {:?}", order.status));
            }
        }

        // Check we have a fill transaction hash
        if order.fill_tx_hash.is_none() {
            return Err("Order has no fill transaction hash".to_string());
        }

        // Check order hasn't expired
        let standard_order = &order.standard_order;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| format!("Time error: {}", e))?
            .as_secs();

        if standard_order.expires <= now {
            return Err(format!(
                "Order has expired: {} <= {}",
                standard_order.expires, now
            ));
        }

        Ok(())
    }

    async fn execute_finalization(&self, order: &Order) -> Result<FillResult> {
        let standard_order = &order.standard_order;
        
        info!("Executing finalization for order {} on origin chain", order.id);

        // Log finalization parameters
        info!("Finalization parameters:");
        info!("  Order ID: {}", order.id);
        info!("  User: {:?}", standard_order.user);
        info!("  Nonce: {}", standard_order.nonce);
        info!("  Origin Chain ID: {}", standard_order.origin_chain_id);
        info!("  Expires: {}", standard_order.expires);

        // Check origin chain connectivity before execution
        match self.contract_factory.check_chain_connectivity().await {
            Ok((origin_block, dest_block)) => {
                info!("Chain connectivity OK - Origin: {}, Destination: {}", origin_block, dest_block);
            }
            Err(e) => {
                warn!("Chain connectivity issues: {}", e);
                // Continue with simulation for now
            }
        }

        // Estimate gas for finalization
        let gas_estimate = self.estimate_finalization_gas(order).await?;
        info!("Finalization gas estimate: {} gwei", gas_estimate.total_cost);

        // Execute real finalization
        let tx_hash = self.execute_real_finalization(order).await?;
        
        Ok(FillResult::success(tx_hash, None))
    }

    async fn execute_real_finalization(&self, order: &Order) -> Result<String> {
        info!("Executing real SettlerCompact.finalise() transaction");

        let standard_order = &order.standard_order;

        // Create order ID as bytes32
        let order_id_bytes32 = self.contract_factory.string_to_order_id(&order.id.to_string());

        info!("Contract finalization parameters:");
        info!("  User: {:?}", standard_order.user);
        info!("  Nonce: {}", standard_order.nonce);
        info!("  Expires: {}", standard_order.expires);
        info!("  Origin Chain ID: {}", standard_order.origin_chain_id);
        info!("  Order ID (bytes32): {:?}", order_id_bytes32);

        // Execute finalization
        let tx_hash = self.contract_factory.finalize_order(order).await?;

        info!("Finalization transaction hash: {}", tx_hash);
        Ok(tx_hash)
    }

    async fn estimate_finalization_gas(&self, order: &Order) -> Result<GasEstimate> {
        info!("Estimating gas for finalization operation");

        // Get origin provider for gas estimation
        let provider = self.contract_factory.get_origin_provider()?;
        
        // Get current gas price
        let gas_price = provider.get_gas_price().await.unwrap_or_default();
        
        // Conservative gas limit estimate for finalization operations
        let base_gas_limit = 500_000u64; // Higher than fill due to more complex logic
        let gas_multiplier = 1.3; // 30% buffer for finalization
        let gas_limit = ((base_gas_limit as f64) * gas_multiplier) as u64;
        
        let total_cost = alloy::primitives::U256::from(gas_limit) * alloy::primitives::U256::from(gas_price);
        
        info!("Finalization gas estimation:");
        info!("  Gas limit: {}", gas_limit);
        info!("  Gas price: {}", gas_price);
        info!("  Total cost: {}", total_cost);

        Ok(GasEstimate {
            gas_limit: alloy::primitives::U256::from(gas_limit),
            gas_price: alloy::primitives::U256::from(gas_price),
            total_cost,
            is_affordable: true, // TODO: Add actual affordability check based on solver balance
        })
    }

    // Monitor fill status and trigger finalization when appropriate
    pub async fn monitor_and_finalize_pending_orders(&self) -> Result<()> {
        info!("Monitoring filled orders for automatic finalization");

        let filled_orders = self.storage.get_orders_by_status(OrderStatus::Filled).await?;
        
        for order in filled_orders {
            info!("Checking if order {} should be auto-finalized", order.id);
            
            // Check if enough time has passed since fill (finalization delay)
            let finalization_delay = self.config.solver.finalization_delay_seconds;
            
            let elapsed = std::time::SystemTime::now()
                .duration_since(order.updated_at.into())
                .unwrap_or_default()
                .as_secs();
                
            if elapsed >= finalization_delay {
                info!("Auto-finalizing order {} after {} seconds", order.id, elapsed);
                
                match self.finalize_order(order.id).await {
                    Ok(result) => {
                        if result.success {
                            info!("Successfully auto-finalized order {}", order.id);
                        } else {
                            warn!("Failed to auto-finalize order {}: {:?}", order.id, result.error);
                        }
                    }
                    Err(e) => {
                        error!("Error during auto-finalization of order {}: {}", order.id, e);
                    }
                }
            } else {
                info!("Order {} needs {} more seconds before auto-finalization", 
                      order.id, finalization_delay - elapsed);
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct GasEstimate {
    pub gas_limit: alloy::primitives::U256,
    pub gas_price: alloy::primitives::U256,
    pub total_cost: alloy::primitives::U256,
    pub is_affordable: bool,
} 