use anyhow::Result;
use tracing::{info, error};

use crate::config::AppConfig;
use crate::contracts::ContractFactory;
use crate::models::{Order, OrderStatus, ExecutionResult};
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

    pub async fn process_fill(&self, order_id: uuid::Uuid) -> Result<ExecutionResult> {
        // Get order from storage
        let mut order = match self.storage.get_order(order_id).await? {
            Some(order) => order,
            None => {
                let error_msg = format!("Order not found: {}", order_id);
                error!("{}", error_msg);
                return Ok(ExecutionResult::failure(error_msg));
            }
        };

        info!("Processing fill for order: {}", order_id);

        // Update status to processing
        order.update_status(OrderStatus::Processing);
        self.storage.update_order(order.clone()).await?;

        // Execute fill on destination chain
        match self.execute_fill(&order).await {
            Ok(tx_hash) => {
                info!("Fill executed successfully: {}", tx_hash);
                
                // Update order with fill transaction hash and status
                order.set_fill_tx(tx_hash.clone());
                order.update_status(OrderStatus::Filled);
                self.storage.update_order(order).await?;

                Ok(ExecutionResult::success(tx_hash, None))
            }
            Err(e) => {
                let error_msg = format!("Fill execution failed: {}", e);
                error!("{}", error_msg);
                
                // Update order with error
                order.set_error(error_msg.clone());
                self.storage.update_order(order).await?;

                Ok(ExecutionResult::failure(error_msg))
            }
        }
    }

    async fn execute_fill(&self, order: &Order) -> Result<String> {
        let standard_order = &order.standard_order;

        // Execute fill on destination chain using ContractFactory
        let tx_hash = self.contract_factory.fill_order(
            standard_order.output_token,
            standard_order.output_amount,
            standard_order.maker,
        ).await?;

        Ok(tx_hash)
    }
} 