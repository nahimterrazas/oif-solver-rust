use anyhow::Result;
use tracing::{info, error};

use crate::config::AppConfig;
use crate::contracts::ContractFactory;
use crate::models::{Order, OrderStatus, FillResult};
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

    pub async fn process_finalization(&self, order_id: uuid::Uuid) -> Result<FillResult> {
        // Get order from storage
        let mut order = match self.storage.get_order(order_id).await? {
            Some(order) => order,
            None => {
                let error_msg = format!("Order not found: {}", order_id);
                error!("{}", error_msg);
                return Ok(FillResult::failure(error_msg));
            }
        };

        // Check if order is in filled state
        if !matches!(order.status, OrderStatus::Filled) {
            let error_msg = format!(
                "Order {} is not in filled state. Current status: {:?}",
                order_id, order.status
            );
            error!("{}", error_msg);
            return Ok(FillResult::failure(error_msg));
        }

        info!("Processing finalization for order: {}", order_id);

        // Execute finalization on origin chain
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
                    error!("Finalization execution failed: {}", error_msg);
                    
                    // Update order with error
                    order.set_error(error_msg.clone());
                    self.storage.update_order(order).await?;
                    Ok(FillResult::failure(error_msg))
                }
            }
            Err(e) => {
                let error_msg = format!("Finalization execution error: {}", e);
                error!("{}", error_msg);
                
                // Update order with error
                order.set_error(error_msg.clone());
                self.storage.update_order(order).await?;

                Ok(FillResult::failure(error_msg))
            }
        }
    }

    async fn execute_finalization(&self, order: &Order) -> Result<FillResult> {
        let standard_order = &order.standard_order;

        info!(
            "Executing finalization for order {} on origin chain {}",
            order.id, standard_order.origin_chain_id
        );

        // Convert signature string to bytes
        let signature_bytes = hex::decode(order.signature.strip_prefix("0x").unwrap_or(&order.signature))
            .map_err(|e| anyhow::anyhow!("Invalid signature format: {}", e))?;

        // Get first input (tokenId, amount)
        let (token_id_str, input_amount_str) = standard_order.inputs.get(0)
            .ok_or_else(|| anyhow::anyhow!("No inputs found in order"))?;

        // Get first output for settlement details
        let destination_output = standard_order.outputs.get(0)
            .ok_or_else(|| anyhow::anyhow!("No outputs found in order"))?;

        // For POC, simulate blockchain transaction
        let tx_hash = self.contract_factory.finalize_order(
            token_id_str.parse().unwrap_or_default(),
            standard_order.user,
            alloy::primitives::Address::ZERO, // input_token - simplified for POC
            input_amount_str.parse().unwrap_or_default(),
            destination_output.token,
            destination_output.amount.parse().unwrap_or_default(),
            standard_order.expires,
            standard_order.origin_chain_id,
            destination_output.chain_id,
            signature_bytes,
        ).await?;

        Ok(FillResult::success(tx_hash, None))
    }
} 