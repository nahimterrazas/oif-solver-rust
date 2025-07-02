use anyhow::Result;
use tracing::{info, error};

use crate::config::AppConfig;
use crate::contracts::ContractFactory;
use crate::models::{Order, OrderStatus, FillResult};
use crate::storage::MemoryStorage;

pub struct CrossChainService {
    storage: MemoryStorage,
    contract_factory: ContractFactory,
    config: AppConfig,
}

impl CrossChainService {
    pub async fn new(storage: MemoryStorage, config: AppConfig) -> Result<Self> {
        let contract_factory = ContractFactory::new(config.clone()).await?;
        
        Ok(Self {
            storage,
            contract_factory,
            config,
        })
    }

    pub async fn process_fill(&self, order_id: uuid::Uuid) -> Result<FillResult> {
        // Get order from storage
        let mut order = match self.storage.get_order(order_id).await? {
            Some(order) => order,
            None => {
                let error_msg = format!("Order not found: {}", order_id);
                error!("{}", error_msg);
                return Ok(FillResult::failure(error_msg));
            }
        };

        info!("Processing fill for order: {}", order_id);

        // Update status to processing
        order.update_status(OrderStatus::Processing);
        self.storage.update_order(order.clone()).await?;

        // Validate order before fill
        if let Err(validation_error) = self.validate_fill_preconditions(&order) {
            let error_msg = format!("Fill validation failed: {}", validation_error);
            error!("{}", error_msg);
            order.set_error(error_msg.clone());
            self.storage.update_order(order).await?;
            return Ok(FillResult::failure(error_msg));
        }

        // Execute fill on destination chain
        match self.execute_fill(&order).await {
            Ok(fill_result) => {
                if fill_result.success {
                    if let Some(tx_hash) = &fill_result.tx_hash {
                        info!("Fill executed successfully: {}", tx_hash);
                        
                        // Update order with fill transaction hash and status
                        order.set_fill_tx(tx_hash.clone());
                        order.update_status(OrderStatus::Filled);
                        self.storage.update_order(order).await?;
                    }
                    Ok(fill_result)
                } else {
                    let error_msg = fill_result.error.unwrap_or("Unknown fill error".to_string());
                    error!("Fill execution failed: {}", error_msg);
                    
                    // Update order with error
                    order.set_error(error_msg.clone());
                    self.storage.update_order(order).await?;
                    Ok(FillResult::failure(error_msg))
                }
            }
            Err(e) => {
                let error_msg = format!("Fill execution error: {}", e);
                error!("{}", error_msg);
                
                // Update order with error
                order.set_error(error_msg.clone());
                self.storage.update_order(order).await?;

                Ok(FillResult::failure(error_msg))
            }
        }
    }

    fn validate_fill_preconditions(&self, order: &Order) -> Result<(), String> {
        let standard_order = &order.standard_order;

        // Check fill deadline
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| format!("Time error: {}", e))?
            .as_secs();

        if standard_order.fill_deadline <= now {
            return Err(format!(
                "Fill deadline has passed: {} <= {}",
                standard_order.fill_deadline, now
            ));
        }

        // Check that we have outputs
        if standard_order.outputs.is_empty() {
            return Err("Order has no outputs".to_string());
        }

        // Check that we have inputs
        if standard_order.inputs.is_empty() {
            return Err("Order has no inputs".to_string());
        }

        // Validate output amounts
        for output in &standard_order.outputs {
            if output.amount.parse::<u128>().is_err() {
                return Err(format!("Invalid output amount: {}", output.amount));
            }
        }

        Ok(())
    }

    async fn execute_fill(&self, order: &Order) -> Result<FillResult> {
        let standard_order = &order.standard_order;
        
        // Get the first output (destination chain)
        let destination_output = &standard_order.outputs[0];
        
        info!(
            "Executing fill for order {} on chain {}",
            order.id, destination_output.chain_id
        );

        // For POC, simulate blockchain transaction
        let tx_hash = self.contract_factory.fill_order(
            destination_output.token,
            destination_output.amount.parse().unwrap_or_default(),
            destination_output.recipient,
        ).await?;

        Ok(FillResult::success(tx_hash, None))
    }
} 