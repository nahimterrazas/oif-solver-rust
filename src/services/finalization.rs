use anyhow::Result;
use tracing::{info, error};

use crate::config::AppConfig;
use crate::contracts::ContractFactory;
use crate::models::{Order, OrderStatus, ExecutionResult};
use crate::storage::MemoryStorage;

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

    pub async fn process_finalization(&self, order_id: uuid::Uuid) -> Result<ExecutionResult> {
        // Get order from storage
        let mut order = match self.storage.get_order(order_id).await? {
            Some(order) => order,
            None => {
                let error_msg = format!("Order not found: {}", order_id);
                error!("{}", error_msg);
                return Ok(ExecutionResult::failure(error_msg));
            }
        };

        // Check if order is in filled state
        if !matches!(order.status, OrderStatus::Filled) {
            let error_msg = format!(
                "Order {} is not in filled state. Current status: {:?}",
                order_id, order.status
            );
            error!("{}", error_msg);
            return Ok(ExecutionResult::failure(error_msg));
        }

        info!("Processing finalization for order: {}", order_id);

        // Execute finalization on origin chain
        match self.execute_finalization(&order).await {
            Ok(tx_hash) => {
                info!("Finalization executed successfully: {}", tx_hash);
                
                // Update order with finalization transaction hash and status
                order.set_finalize_tx(tx_hash.clone());
                order.update_status(OrderStatus::Finalized);
                self.storage.update_order(order).await?;

                Ok(ExecutionResult::success(tx_hash, None))
            }
            Err(e) => {
                let error_msg = format!("Finalization execution failed: {}", e);
                error!("{}", error_msg);
                
                // Update order with error
                order.set_error(error_msg.clone());
                self.storage.update_order(order).await?;

                Ok(ExecutionResult::failure(error_msg))
            }
        }
    }

    async fn execute_finalization(&self, order: &Order) -> Result<String> {
        let standard_order = &order.standard_order;

        // Convert signature string to bytes
        let signature_bytes = hex::decode(order.signature.strip_prefix("0x").unwrap_or(&order.signature))
            .map_err(|e| anyhow::anyhow!("Invalid signature format: {}", e))?;

        // Execute finalization on origin chain using ContractFactory
        let tx_hash = self.contract_factory.finalize_order(
            standard_order.nonce,
            standard_order.maker,
            standard_order.input_token,
            standard_order.input_amount,
            standard_order.output_token,
            standard_order.output_amount,
            standard_order.expiry,
            standard_order.origin_chain_id,
            standard_order.destination_chain_id,
            signature_bytes,
        ).await?;

        Ok(tx_hash)
    }
} 