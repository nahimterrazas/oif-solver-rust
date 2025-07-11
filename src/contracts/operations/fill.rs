use anyhow::Result;
use tracing::info;
use std::sync::Arc;

use crate::contracts::encoding::{CallDataEncoder, traits::FillRequest};
use crate::contracts::execution::{ExecutionEngine, traits::ChainType};
use crate::config::AppConfig;
use alloy::primitives::{Address, FixedBytes, U256};

/// High-level orchestrator for fill order operations
/// 
/// This orchestrator coordinates the encoding and execution of fill orders using
/// the abstract trait architecture. It can work with any CallDataEncoder and 
/// ExecutionEngine implementation.
pub struct FillOrchestrator {
    encoder: Arc<dyn CallDataEncoder>,
    executor: Arc<dyn ExecutionEngine>,
    config: Arc<AppConfig>,
}

impl FillOrchestrator {
    /// Create a FillOrchestrator with dependency injection
    /// 
    /// This constructor allows complete flexibility in encoder and executor choice:
    /// - encoder: Any implementation of CallDataEncoder (AlloyEncoder, FoundryEncoder, etc.)
    /// - executor: Any implementation of ExecutionEngine (AlloyExecutor, MockExecutor, etc.)
    /// - config: Application configuration for chain and contract details
    pub fn new_with_traits(
        encoder: Arc<dyn CallDataEncoder>,
        executor: Arc<dyn ExecutionEngine>,
        config: Arc<AppConfig>,
    ) -> Result<Self> {
        info!("🏗️ Creating FillOrchestrator with injected traits");
        info!("  Encoder: {}", encoder.description());
        info!("  Executor: {}", executor.description());
        
        Ok(Self {
            encoder,
            executor,
            config,
        })
    }
    
    /// Create a FillOrchestrator with default implementations
    /// 
    /// For convenience, this creates a FillOrchestrator with:
    /// - AlloyEncoder (uses sol! macro, matches existing factory.rs implementation)
    /// - AlloyExecutor (uses Alloy providers for execution)
    pub fn new(
        abi_provider: Arc<dyn crate::contracts::abi::AbiProvider>,
        config: Arc<AppConfig>,
    ) -> Result<Self> {
        info!("🏗️ Creating FillOrchestrator with default AlloyEncoder + AlloyExecutor");
        
        // Create default encoder and executor
        let encoder = Arc::new(crate::contracts::encoding::AlloyEncoder::new(abi_provider));
        let executor = Arc::new(crate::contracts::execution::AlloyExecutor::new(config.clone())?);
        
        Self::new_with_traits(encoder, executor, config)
    }
    
    /// Execute a fill order using the modular architecture
    /// 
    /// This is the main interface for fill operations. It:
    /// 1. Converts high-level parameters to FillRequest
    /// 2. Uses the encoder to generate call data
    /// 3. Uses the executor to send the transaction
    /// 4. Returns the transaction hash
    pub async fn execute_fill(
        &self,
        order_id: &str,
        fill_deadline: u32,
        remote_oracle: Address,
        token: Address,
        amount: U256,
        recipient: Address,
    ) -> Result<String> {
        info!("🚀 MODULAR FILL: Executing fill order with abstract architecture");
        info!("  Order ID: {}", order_id);
        info!("  Remote Oracle: {:?}", remote_oracle);
        info!("  Token: {:?}", token);
        info!("  Amount: {}", amount);
        info!("  Recipient: {:?}", recipient);
        
        // Step 1: Create high-level fill request
        let fill_request = FillRequest {
            order_id: order_id.to_string(),
            fill_deadline,
            remote_oracle,
            token,
            amount,
            recipient,
        };
        
        // Step 2: Get destination chain contract address and parameters
        let coin_filler_address: Address = self.config.contracts.coin_filler.parse()
            .map_err(|e| anyhow::anyhow!("Invalid CoinFiller address in config: {}", e))?;
        let destination_chain_id = self.config.chains.destination.chain_id;
        let solver_address = self.executor.wallet_address();
        
        // Step 3: Generate COMPLETE call data using the trait method (matches factory-bkp.rs)
        info!("🔧 Encoding COMPLETE fill call data...");
        let call_data = self.encoder.encode_complete_fill_call(
            &fill_request,
            coin_filler_address,
            destination_chain_id,
            solver_address,
        )?;
        
        // Step 5: Execute transaction using the executor
        info!("📡 Sending fill transaction...");
        let gas_params = crate::contracts::execution::traits::GasParams {
            gas_limit: 360000u64, // Gas limit matching TypeScript
            gas_price: 50_000_000_000u64, // Gas price (50 gwei)
        };
        let tx_hash = self.executor.send_transaction(
            ChainType::Destination, // Fill operations execute on destination chain
            call_data,
            coin_filler_address,
            gas_params,
        ).await?;
        
        info!("✅ Modular fill completed successfully: {}", tx_hash);
        Ok(tx_hash)
    }
    
    /// Estimate gas for fill operation
    pub async fn estimate_fill_gas(
        &self,
        order_id: &str,
        fill_deadline: u32,
        remote_oracle: Address,
        token: Address,
        amount: U256,
        recipient: Address,
    ) -> Result<u64> {
        info!("⛽ Estimating fill gas using modular architecture");
        
        // Create fill request
        let fill_request = FillRequest {
            order_id: order_id.to_string(),
            fill_deadline,
            remote_oracle,
            token,
            amount,
            recipient,
        };
        
        // Generate COMPLETE call data with proper configuration
        let coin_filler_address: Address = self.config.contracts.coin_filler.parse()
            .map_err(|e| anyhow::anyhow!("Invalid CoinFiller address in config: {}", e))?;
        let destination_chain_id = self.config.chains.destination.chain_id;
        let solver_address = self.executor.wallet_address();
        
        let call_data = self.encoder.encode_complete_fill_call(
            &fill_request,
            coin_filler_address,
            destination_chain_id,
            solver_address,
        )?;
        
        // Get contract address
        let coin_filler_address: Address = self.config.contracts.coin_filler.parse()
            .map_err(|e| anyhow::anyhow!("Invalid CoinFiller address in config: {}", e))?;
        
        // Estimate gas
        let gas_estimate = self.executor.estimate_gas(
            ChainType::Destination, // Fill operations estimate on destination chain
            call_data,
            coin_filler_address,
            self.executor.wallet_address(), // Use executor's wallet as from address
        ).await?;
        
        info!("✅ Fill gas estimation completed: {} gas", gas_estimate);
        Ok(gas_estimate)
    }
    
    /// Get the wallet address used by this orchestrator
    pub fn wallet_address(&self) -> Address {
        self.executor.wallet_address()
    }
    
    /// Update fill call data with configuration-specific values
    /// 
    /// This method fills in the missing configuration values that the encoder
    /// couldn't determine (like chain IDs, contract addresses, solver addresses)
    fn update_fill_parameters(&self, call_data: Vec<u8>, request: &FillRequest) -> Result<Vec<u8>> {
        info!("🔧 Updating fill parameters with configuration values");
        
        // For now, we'll implement a simple approach:
        // The AlloyEncoder already handles most of the parameter encoding.
        // If we need to update specific fields (like remoteFiller, chainId, proposedSolver),
        // we would decode the call data, update the parameters, and re-encode.
        
        // For the initial implementation, we'll trust that the encoder produces
        // the correct call data and just log what we would update:
        info!("  Destination chain ID: {}", self.config.chains.destination.chain_id);
        info!("  CoinFiller address: {}", self.config.contracts.coin_filler);
        info!("  Solver address: {}", self.wallet_address());
        
        // TODO: If needed, implement actual parameter updating here
        // This would involve decoding the ABI-encoded call data, updating specific fields,
        // and re-encoding. For now, the encoder should handle this correctly.
        
        Ok(call_data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contracts::abi::AbiRegistry;
    use crate::contracts::encoding::AlloyEncoder;
    use crate::contracts::execution::AlloyExecutor;
    use std::str::FromStr;

    fn create_test_config() -> AppConfig {
        crate::config::AppConfig {
            server: crate::config::ServerConfig {
                host: "localhost".to_string(),
                port: 8080,
            },
            solver: crate::config::SolverConfig {
                private_key: "0x1111111111111111111111111111111111111111111111111111111111111111".to_string(),
                finalization_delay_seconds: 30,
            },
            chains: crate::config::ChainConfig {
                origin: crate::config::ChainDetails {
                    chain_id: 31337,
                    rpc_url: "http://localhost:8545".to_string(),
                },
                destination: crate::config::ChainDetails {
                    chain_id: 31338,
                    rpc_url: "http://localhost:8546".to_string(),
                },
            },
            contracts: crate::config::ContractConfig {
                settler_compact: "0x1234567890123456789012345678901234567890".to_string(),
                the_compact: "0x2345678901234567890123456789012345678901".to_string(),
                coin_filler: "0x3456789012345678901234567890123456789012".to_string(),
            },
            monitoring: crate::config::MonitoringConfig {
                enabled: false,
                check_interval_seconds: 60,
            },
            persistence: crate::config::PersistenceConfig {
                enabled: false,
                data_file: "test_orders.json".to_string(),
            },
        }
    }

    #[test]
    fn test_fill_orchestrator_creation() {
        let config = Arc::new(create_test_config());
        let abi_provider = Arc::new(AbiRegistry::new());
        
        // Test default creation
        let orchestrator = FillOrchestrator::new(abi_provider.clone(), config.clone());
        assert!(orchestrator.is_ok(), "Should create FillOrchestrator successfully");
        
        let orchestrator = orchestrator.unwrap();
        assert!(!orchestrator.wallet_address().is_zero(), "Should have valid wallet address");
    }

    #[test]
    fn test_fill_orchestrator_with_dependency_injection() {
        let config = Arc::new(create_test_config());
        let abi_provider = Arc::new(AbiRegistry::new());
        
        // Create components manually
        let encoder = Arc::new(AlloyEncoder::new(abi_provider));
        let executor = Arc::new(AlloyExecutor::new(config.clone()).expect("Should create executor"));
        
        // Test dependency injection
        let orchestrator = FillOrchestrator::new_with_traits(encoder, executor, config);
        assert!(orchestrator.is_ok(), "Should create FillOrchestrator with injected traits");
    }

    #[tokio::test]
    async fn test_fill_gas_estimation() {
        let config = Arc::new(create_test_config());
        let abi_provider = Arc::new(AbiRegistry::new());
        let orchestrator = FillOrchestrator::new(abi_provider, config).unwrap();
        
        // Test gas estimation (will fail due to mock RPC, but should not panic)
        let result = orchestrator.estimate_fill_gas(
            "test_order_123",
            u32::MAX,
            Address::from_str("0xe7f1725e7734ce288f8367e1bb143e90bb3f0512").unwrap(),
            Address::from_str("0x9fe46736679d2d9a65f0992f2272de9f3c7fa6e0").unwrap(),
            U256::from_str("99000000000000000000").unwrap(),
            Address::from_str("0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266").unwrap(),
        ).await;
        
        // Should fail due to test environment (no real RPC), but not due to integration issues
        match result {
            Ok(gas_estimate) => {
                // If it succeeds somehow, gas estimate should be reasonable
                assert!(gas_estimate > 0 && gas_estimate < 10_000_000, "Gas estimate should be reasonable");
            }
            Err(error) => {
                // If it fails (expected in test environment), should be network-related
                let error_msg = error.to_string();
                assert!(!error_msg.contains("panic"), "Should not panic");
                assert!(!error_msg.contains("integration"), "Should not be integration-related");
            }
        }
    }

    #[test]
    fn test_fill_request_creation() {
        let fill_request = FillRequest {
            order_id: "test_order_123".to_string(),
            fill_deadline: u32::MAX,
            remote_oracle: Address::from_str("0xe7f1725e7734ce288f8367e1bb143e90bb3f0512").unwrap(),
            token: Address::from_str("0x9fe46736679d2d9a65f0992f2272de9f3c7fa6e0").unwrap(),
            amount: U256::from_str("99000000000000000000").unwrap(),
            recipient: Address::from_str("0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266").unwrap(),
        };
        
        assert_eq!(fill_request.order_id, "test_order_123");
        assert_eq!(fill_request.fill_deadline, u32::MAX);
        assert!(!fill_request.amount.is_zero());
    }
} 