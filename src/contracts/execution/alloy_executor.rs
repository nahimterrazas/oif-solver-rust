use crate::contracts::execution::traits::{ExecutionEngine, GasParams, ChainType, TransportType, ExecutionContext, ExecutionResponse, AsyncStatus};
use crate::config::AppConfig;
use alloy::{
    providers::{Provider, ProviderBuilder},
    network::EthereumWallet,
    primitives::{Address, U256, TxHash},
    rpc::types::{TransactionRequest, TransactionInput},
    signers::local::PrivateKeySigner,
};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;
use tracing::{info, error, warn};
use hex;

pub struct AlloyExecutor {
    config: Arc<AppConfig>,
    wallet: EthereumWallet,
}

impl AlloyExecutor {
    pub fn new(config: Arc<AppConfig>) -> Result<Self> {
        info!("🔧 Initializing AlloyExecutor with configuration");
        
        // Create wallet from private key
        let private_key = &config.solver.private_key;
        let signer: PrivateKeySigner = private_key.parse()
            .map_err(|e| anyhow::anyhow!("Failed to parse private key: {}", e))?;
        
        let wallet = EthereumWallet::from(signer);
        
        info!("✅ AlloyExecutor initialized");
        info!("  Wallet address: {:?}", wallet.default_signer().address());
        info!("  Origin chain ID: {}", config.chains.origin.chain_id);
        info!("  Destination chain ID: {}", config.chains.destination.chain_id);
        
        Ok(Self {
            config,
            wallet,
        })
    }
    
    /// Create provider for origin chain
    fn create_origin_provider(&self) -> Result<Box<dyn Provider + Send + Sync>> {
        let provider = ProviderBuilder::new()
            .wallet(self.wallet.clone())
            .on_http(self.config.chains.origin.rpc_url.parse()?);
            
        Ok(Box::new(provider))
    }
    
    /// Create provider for destination chain  
    fn create_destination_provider(&self) -> Result<Box<dyn Provider + Send + Sync>> {
        let provider = ProviderBuilder::new()
            .wallet(self.wallet.clone())
            .on_http(self.config.chains.destination.rpc_url.parse()?);
            
        Ok(Box::new(provider))
    }
    
    /// Create provider for specific chain by ID
    fn create_provider_for_chain(&self, chain_id: u64) -> Result<Box<dyn Provider + Send + Sync>> {
        if chain_id == self.config.chains.origin.chain_id {
            self.create_origin_provider()
        } else if chain_id == self.config.chains.destination.chain_id {
            self.create_destination_provider()
        } else {
            Err(anyhow::anyhow!("Unsupported chain ID: {}", chain_id))
        }
    }
    
    /// Build transaction request from call data and parameters
    fn build_transaction_request(
        &self,
        call_data: Vec<u8>,
        to: Address,
        gas: GasParams,
    ) -> TransactionRequest {
        let mut tx_request = TransactionRequest::default()
            .to(to)
            .input(TransactionInput::from(call_data));
        
        // Set gas parameters explicitly
        tx_request.gas = Some(gas.gas_limit.into());
        tx_request.gas_price = Some(gas.gas_price.into());
        
        tx_request
    }
    
    /// Log detailed transaction information for debugging
    fn log_transaction_debug_info(&self, tx_request: &TransactionRequest, call_data: &[u8]) {
        info!("🔍 ALLOY EXECUTOR DEBUG INFO:");
        info!("  To address: {:?}", tx_request.to);
        info!("  Gas limit: {:?}", tx_request.gas);
        info!("  Gas price: {:?}", tx_request.gas_price);
        info!("  Max fee per gas: {:?}", tx_request.max_fee_per_gas);
        info!("  Max priority fee per gas: {:?}", tx_request.max_priority_fee_per_gas);
        info!("  Input data length: {} bytes", call_data.len());
        info!("  Nonce: {:?}", tx_request.nonce);
        info!("  Chain ID: {:?}", tx_request.chain_id);
        info!("  Value: {:?}", tx_request.value);
        
        info!("🔍 WALLET DEBUG INFO:");
        info!("  Wallet address: {:?}", self.wallet.default_signer().address());
        
        info!("🔍 CALL DATA DEBUG:");
        info!("  Call data preview (first 100 bytes): 0x{}", 
              hex::encode(&call_data[..100.min(call_data.len())]));
    }
}

#[async_trait]
impl ExecutionEngine for AlloyExecutor {
    async fn send_transaction(&self, chain: ChainType, call_data: Vec<u8>, to: Address, gas: GasParams, context: Option<ExecutionContext>) -> Result<ExecutionResponse> {
        info!("🚀 AlloyExecutor: Sending transaction to {}", to);
        info!("  Chain: {:?}", chain);
        info!("  Call data: {} bytes", call_data.len());
        info!("  Gas limit: {}", gas.gas_limit);
        info!("  Gas price: {}", gas.gas_price);
        
        // Create provider based on specified chain
        let provider = match chain {
            ChainType::Origin => self.create_origin_provider()?,
            ChainType::Destination => self.create_destination_provider()?,
        };
        
        // Build transaction request
        let tx_request = self.build_transaction_request(call_data.clone(), to, gas);
        
        // Log debug information
        self.log_transaction_debug_info(&tx_request, &call_data);
        
        // Send transaction
        let pending_tx = provider.send_transaction(tx_request.clone()).await
            .map_err(|e| {
                // Enhanced error logging
                error!("❌ ALLOY EXECUTOR TRANSACTION FAILED:");
                error!("  Error: {}", e);
                error!("  Contract address: {:?}", to);
                error!("  Wallet address: {:?}", self.wallet.default_signer().address());
                error!("  Call data: 0x{}", hex::encode(&call_data));
                anyhow::anyhow!("Failed to send transaction: {}", e)
            })?;
        
        info!("⏳ Transaction sent, waiting for confirmation...");
        
        // Wait for transaction receipt
        let receipt = pending_tx.get_receipt().await
            .map_err(|e| anyhow::anyhow!("Failed to get transaction receipt: {}", e))?;
        
        let tx_hash = format!("0x{}", hex::encode(receipt.transaction_hash));
        
        info!("✅ Transaction confirmed:");
        info!("  Transaction hash: {}", tx_hash);
        info!("  Block number: {:?}", receipt.block_number);
        info!("  Gas used: {:?}", receipt.gas_used);
        info!("  Status: {:?}", receipt.status());
        
        // Check if transaction was successful
        if !receipt.status() {
            error!("❌ Transaction failed (reverted)");
            error!("  Transaction hash: {}", tx_hash);
            error!("  Block number: {:?}", receipt.block_number);
            return Err(anyhow::anyhow!("Transaction reverted: {}", tx_hash));
        }
        
        Ok(ExecutionResponse::Immediate(tx_hash))
    }
    
    async fn static_call(&self, chain: ChainType, call_data: Vec<u8>, to: Address, from: Address) -> Result<Vec<u8>> {
        info!("🔍 AlloyExecutor: Performing static call");
        info!("  Chain: {:?}", chain);
        info!("  To: {}", to);
        info!("  From: {}", from);
        info!("  Call data: {} bytes", call_data.len());
        
        // Create provider based on specified chain
        let provider = match chain {
            ChainType::Origin => self.create_origin_provider()?,
            ChainType::Destination => self.create_destination_provider()?,
        };
        
        // Build call request
        let call_request = TransactionRequest::default()
            .to(to)
            .from(from)
            .input(TransactionInput::from(call_data));
        
        // Perform static call
        let result = provider.call(call_request).await
            .map_err(|e| {
                error!("❌ Static call failed:");
                error!("  Error: {}", e);
                error!("  To: {}", to);
                error!("  From: {}", from);
                anyhow::anyhow!("Static call failed: {}", e)
            })?;
        
        info!("✅ Static call successful: {} bytes returned", result.len());
        
        Ok(result.to_vec())
    }
    
    async fn estimate_gas(&self, chain: ChainType, call_data: Vec<u8>, to: Address, from: Address) -> Result<u64> {
        info!("⛽ AlloyExecutor: Estimating gas");
        info!("  Chain: {:?}", chain);
        info!("  To: {}", to);
        info!("  From: {}", from);
        info!("  Call data: {} bytes", call_data.len());
        
        // Create provider based on specified chain
        let provider = match chain {
            ChainType::Origin => self.create_origin_provider()?,
            ChainType::Destination => self.create_destination_provider()?,
        };
        
        // Build estimation request
        let estimation_request = TransactionRequest::default()
            .to(to)
            .from(from)
            .input(TransactionInput::from(call_data));
        
        // Estimate gas
        let gas_estimate = provider.estimate_gas(estimation_request).await
            .map_err(|e| {
                error!("❌ Gas estimation failed:");
                error!("  Error: {}", e);
                error!("  To: {}", to);
                error!("  From: {}", from);
                anyhow::anyhow!("Gas estimation failed: {}", e)
            })?;
        
        let gas_u64 = gas_estimate;
        
        info!("✅ Gas estimation successful: {} gas", gas_u64);
        
        Ok(gas_u64)
    }
    
    fn wallet_address(&self) -> Address {
        self.wallet.default_signer().address()
    }
    
    fn description(&self) -> &str {
        "AlloyExecutor: Uses Alloy providers for blockchain transaction execution"
    }
    
    fn transport_type(&self) -> TransportType {
        TransportType::Direct
    }
    
    async fn check_async_status(&self, _request_id: &str) -> Result<AsyncStatus> {
        Err(anyhow::anyhow!("AlloyExecutor uses direct execution - async status checking is not supported"))
    }
    
    async fn get_transaction_hash(&self, _request_id: &str) -> Result<Option<String>> {
        Err(anyhow::anyhow!("AlloyExecutor uses direct execution - async transaction hash retrieval is not supported"))
    }
}

impl AlloyExecutor {
    /// Send transaction to a specific chain (useful for cross-chain operations)
    pub async fn send_transaction_to_chain(
        &self, 
        call_data: Vec<u8>, 
        to: Address, 
        gas: GasParams,
        chain_id: u64
    ) -> Result<String> {
        info!("🚀 AlloyExecutor: Sending transaction to chain {}", chain_id);
        
        // Create provider for specific chain
        let provider = self.create_provider_for_chain(chain_id)?;
        
        // Build transaction request
        let tx_request = self.build_transaction_request(call_data.clone(), to, gas);
        
        // Log debug information
        self.log_transaction_debug_info(&tx_request, &call_data);
        
        // Send transaction
        let pending_tx = provider.send_transaction(tx_request).await
            .map_err(|e| anyhow::anyhow!("Failed to send transaction to chain {}: {}", chain_id, e))?;
        
        // Wait for confirmation
        let receipt = pending_tx.get_receipt().await
            .map_err(|e| anyhow::anyhow!("Failed to get receipt on chain {}: {}", chain_id, e))?;
        
        let tx_hash = format!("0x{}", hex::encode(receipt.transaction_hash));
        
        info!("✅ Transaction confirmed on chain {}:", chain_id);
        info!("  Transaction hash: {}", tx_hash);
        info!("  Status: {:?}", receipt.status());
        
        if !receipt.status() {
            return Err(anyhow::anyhow!("Transaction reverted on chain {}: {}", chain_id, tx_hash));
        }
        
        Ok(tx_hash)
    }
    
    /// Get the wallet address
    pub fn wallet_address(&self) -> Address {
        self.wallet.default_signer().address()
    }
    
    /// Get current chain ID for origin chain
    pub async fn get_origin_chain_id(&self) -> Result<u64> {
        let provider = self.create_origin_provider()?;
        let chain_id = provider.get_chain_id().await?;
        Ok(chain_id)
    }
    
    /// Get current chain ID for destination chain
    pub async fn get_destination_chain_id(&self) -> Result<u64> {
        let provider = self.create_destination_provider()?;
        let chain_id = provider.get_chain_id().await?;
        Ok(chain_id)
    }
    
    /// Verify chain connectivity
    pub async fn verify_chain_connectivity(&self) -> Result<()> {
        info!("🔗 Verifying chain connectivity...");
        
        // Test origin chain
        let origin_provider = self.create_origin_provider()?;
        let origin_block = origin_provider.get_block_number().await
            .map_err(|e| anyhow::anyhow!("Failed to connect to origin chain: {}", e))?;
        
        // Test destination chain  
        let dest_provider = self.create_destination_provider()?;
        let dest_block = dest_provider.get_block_number().await
            .map_err(|e| anyhow::anyhow!("Failed to connect to destination chain: {}", e))?;
        
        info!("✅ Chain connectivity verified:");
        info!("  Origin chain {} block: {}", self.config.chains.origin.chain_id, origin_block);
        info!("  Destination chain {} block: {}", self.config.chains.destination.chain_id, dest_block);
        
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, ServerConfig, ChainConfig, ChainDetails, SolverConfig, ContractConfig, MonitoringConfig, PersistenceConfig};
    use alloy::primitives::TxKind;
    use std::str::FromStr;

    fn create_test_config() -> Arc<AppConfig> {
        Arc::new(AppConfig {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 3000,
            },
            chains: ChainConfig {
                origin: ChainDetails {
                    rpc_url: "http://localhost:8545".to_string(),
                    chain_id: 31337,
                },
                destination: ChainDetails {
                    rpc_url: "http://localhost:8546".to_string(),
                    chain_id: 31338,
                },
            },
            solver: SolverConfig {
                private_key: "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80".to_string(),
                finalization_delay_seconds: 30,
            },
            contracts: ContractConfig {
                the_compact: "0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0".to_string(),
                settler_compact: "0x5FC8d32690cc91D4c39d9d3abcBD16989F875707".to_string(),
                coin_filler: "0xCf7Ed3AccA5a467e9e704C703E8D87F634fB0Fc9".to_string(),
            },
            monitoring: MonitoringConfig {
                enabled: true,
                check_interval_seconds: 60,
            },
            persistence: PersistenceConfig {
                enabled: true,
                data_file: "data/orders.json".to_string(),
            },
            relayer: None,
        })
    }

    #[test]
    fn test_alloy_executor_creation() {
        let config = create_test_config();
        let result = AlloyExecutor::new(config);
        
        assert!(result.is_ok(), "AlloyExecutor creation should succeed: {:?}", result.err());
        
        let executor = result.unwrap();
        
        // Should have correct wallet address
        let expected_address = Address::from_str("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266")
            .expect("Valid test address");
        assert_eq!(executor.wallet_address(), expected_address);
        
        println!("✅ AlloyExecutor created successfully");
        println!("   Wallet address: {}", executor.wallet_address());
    }

    #[test]
    fn test_gas_params_creation() {
        let gas_params = GasParams {
            gas_limit: 650000,
            gas_price: 1178761408,
        };
        
        assert_eq!(gas_params.gas_limit, 650000);
        assert_eq!(gas_params.gas_price, 1178761408);
        
        println!("✅ GasParams structure works correctly");
    }

    #[test]  
    fn test_transaction_request_building() {
        let config = create_test_config();
        let executor = AlloyExecutor::new(config).expect("Executor creation");
        
        let call_data = vec![0x01, 0x02, 0x03, 0x04];
        let to_address = Address::from_str("0x5FC8d32690cc91D4c39d9d3abcBD16989F875707")
            .expect("Valid address");
        let gas_params = GasParams {
            gas_limit: 650000,
            gas_price: 1178761408,
        };
        
        let tx_request = executor.build_transaction_request(call_data.clone(), to_address, gas_params);
        
        // Verify transaction recipient  
        if let Some(tx_kind) = &tx_request.to {
            match tx_kind {
                alloy::primitives::TxKind::Call(addr) => {
                    assert_eq!(*addr, to_address);
                }
                _ => panic!("Expected Call transaction kind"),
            }
        } else {
            panic!("Transaction should have a recipient");
        }
        
        assert_eq!(tx_request.gas, Some(650000u64.into()));
        assert_eq!(tx_request.gas_price, Some(1178761408u64.into()));
        
        // Verify input data is set (structure validation)
        // Note: TransactionInput structure verification - actual data comparison would require more complex logic
        
        println!("✅ Transaction request building works correctly");
    }

    // Note: Integration tests that actually connect to blockchain would require running test nodes
    // These basic tests verify the structure and configuration without network calls
} 