use anyhow::Result;
use tracing::{info, error, warn};
use alloy::providers::Provider;
use alloy::primitives::U256;
use std::sync::Arc;

use crate::config::AppConfig;
use crate::contracts::ContractFactory;
use crate::models::{Order, OrderStatus, FillResult, MandateOutput};
use crate::storage::MemoryStorage;

#[derive(Clone)]
pub struct CrossChainService {
    storage: MemoryStorage,
    contract_factory: Arc<ContractFactory>,
    config: AppConfig,
}

impl CrossChainService {
    pub async fn new(storage: MemoryStorage, config: AppConfig) -> Result<Self> {
        let contract_factory = Arc::new(ContractFactory::new(config.clone()).await?);
        
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
                        self.storage.update_order(order.clone()).await?;
                        
                        // Clear logging for manual finalization testing
                        info!("========================================");
                        info!("✅ ORDER {} FILLED SUCCESSFULLY!", order.id);
                        info!("📋 Order ID: {}", order.id);
                        info!("🔗 Finalize with: curl -X POST http://127.0.0.1:3000/api/v1/orders/{}/finalize", order.id);
                        info!("========================================");
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

        // Log the fill parameters that will be sent
        info!("Fill parameters:");
        info!("  Order ID: {}", order.id);
        info!("  Output token: {:?}", destination_output.token);
        info!("  Output amount: {}", destination_output.amount);
        info!("  Recipient: {:?}", destination_output.recipient);
        info!("  Chain ID: {}", destination_output.chain_id);

        // Check chain connectivity before execution
        match self.contract_factory.check_chain_connectivity().await {
            Ok((origin_block, dest_block)) => {
                info!("Chain connectivity OK - Origin: {}, Destination: {}", origin_block, dest_block);
            }
            Err(e) => {
                warn!("Chain connectivity issues: {}", e);
                // Continue with simulation for now
            }
        }

        // Execute real fill using contract factory
        let tx_hash = self.execute_real_fill(order).await?;
        
        Ok(FillResult::success(tx_hash, None))
    }

    async fn execute_real_fill(&self, order: &Order) -> Result<String> {
        info!("Executing real CoinFiller.fill() transaction");

        let destination_output = &order.standard_order.outputs[0];

        // Create order ID as bytes32
        let order_id_bytes32 = self.contract_factory.string_to_order_id(&order.id.to_string());
        
        // Create mandate output for contract call
        let mandate_output_contract = self.create_contract_mandate_output(destination_output)?;
        
        // Get solver identifier
        let solver_identifier = self.get_solver_identifier().await?;
        
        // Use the original fill deadline from the order (cast to u32)
        let fill_deadline = order.standard_order.fill_deadline as u32;

        info!("Contract call parameters:");
        info!("  Fill deadline: {}", fill_deadline);
        info!("  Order ID: {:?}", order_id_bytes32);
        info!("  Solver identifier: {:?}", solver_identifier);

        // For now, delegate to the simplified contract factory method
        // TODO: Replace with direct alloy contract call once we have real contracts
        let tx_hash = self.contract_factory.fill_order(
            &order.id.to_string(),
            fill_deadline,
            destination_output.remote_oracle,
            destination_output.token,
            destination_output.amount.parse().unwrap_or_default(),
            destination_output.recipient,
        ).await?;

        info!("Fill transaction hash: {}", tx_hash);
        Ok(tx_hash)
    }

    fn create_contract_mandate_output(&self, output: &MandateOutput) -> Result<crate::contracts::factory::MandateOutput> {
        Ok(crate::contracts::factory::MandateOutput {
            remoteOracle: self.contract_factory.address_to_bytes32(output.remote_oracle),
            remoteFiller: self.contract_factory.address_to_bytes32(output.remote_filler),
            chainId: alloy::primitives::U256::from(output.chain_id),
            token: self.contract_factory.address_to_bytes32(output.token),
            amount: output.amount.parse().unwrap_or_default(),
            recipient: self.contract_factory.address_to_bytes32(output.recipient),
            remoteCall: output.remote_call.as_ref()
                .and_then(|s| hex::decode(s.strip_prefix("0x").unwrap_or(s)).ok())
                .unwrap_or_default()
                .into(),
            fulfillmentContext: output.fulfillment_context.as_ref()
                .and_then(|s| hex::decode(s.strip_prefix("0x").unwrap_or(s)).ok())
                .unwrap_or_default()
                .into(),
        })
    }

    async fn get_solver_identifier(&self) -> Result<alloy::primitives::FixedBytes<32>> {
        let wallet = self.contract_factory.get_wallet()?;
        let solver_address = wallet.default_signer().address();
        Ok(self.contract_factory.address_to_bytes32(solver_address))
    }

    // Gas estimation functionality (will be enhanced)
    pub async fn estimate_fill_gas(&self, order: &Order) -> Result<GasEstimate> {
        let destination_output = &order.standard_order.outputs[0];
        
        info!("Estimating gas for fill operation on chain {}", destination_output.chain_id);

        // Get destination provider for gas estimation
        let provider = self.contract_factory.get_destination_provider()?;
        
        // Get current gas price
        let gas_price = provider.get_gas_price().await.unwrap_or_default();
        
        // Conservative gas limit estimate for fill operations
        let base_gas_limit = 300_000u64;
        let gas_multiplier = 1.2; // 20% buffer
        let gas_limit = ((base_gas_limit as f64) * gas_multiplier) as u64;
        
        let total_cost = alloy::primitives::U256::from(gas_limit) * alloy::primitives::U256::from(gas_price);
        
        info!("Gas estimation:");
        info!("  Gas limit: {}", gas_limit);
        info!("  Gas price: {}", gas_price);
        info!("  Total cost: {}", total_cost);

        Ok(GasEstimate {
            gas_limit: alloy::primitives::U256::from(gas_limit),
            gas_price: alloy::primitives::U256::from(gas_price),
            total_cost,
            is_affordable: true, // TODO: Add actual affordability check
        })
    }

    // Public accessor for contract factory (for monitoring service)
    pub fn get_contract_factory(&self) -> &ContractFactory {
        &*self.contract_factory
    }
}

#[derive(Debug, Clone)]
pub struct GasEstimate {
    pub gas_limit: alloy::primitives::U256,
    pub gas_price: alloy::primitives::U256,
    pub total_cost: alloy::primitives::U256,
    pub is_affordable: bool,
} 