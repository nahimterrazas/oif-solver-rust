use alloy::primitives::Address;
use anyhow::Result;
use async_trait::async_trait;

/// Enum to specify which blockchain to execute transactions on
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainType {
    /// Origin chain (where SettlerCompact is deployed - for finalize operations)
    Origin,
    /// Destination chain (where CoinFiller is deployed - for fill operations)
    Destination,
}

#[derive(Debug, Clone)]
pub struct GasParams {
    pub gas_limit: u64,
    pub gas_price: u64,
}

#[async_trait]
pub trait ExecutionEngine: Send + Sync {
    /// Send a transaction to the specified blockchain
    /// 
    /// # Arguments
    /// * `chain` - Which blockchain to execute on (Origin or Destination)
    /// * `call_data` - The encoded function call data
    /// * `to` - The contract address to call
    /// * `gas` - Gas parameters for the transaction
    async fn send_transaction(&self, chain: ChainType, call_data: Vec<u8>, to: Address, gas: GasParams) -> Result<String>;
    
    /// Perform a static call (read-only) on the specified blockchain
    /// 
    /// # Arguments
    /// * `chain` - Which blockchain to call (Origin or Destination)
    /// * `call_data` - The encoded function call data
    /// * `to` - The contract address to call
    /// * `from` - The address to call from
    async fn static_call(&self, chain: ChainType, call_data: Vec<u8>, to: Address, from: Address) -> Result<Vec<u8>>;
    
    /// Estimate gas for a transaction on the specified blockchain
    /// 
    /// # Arguments
    /// * `chain` - Which blockchain to estimate on (Origin or Destination)
    /// * `call_data` - The encoded function call data
    /// * `to` - The contract address to call
    /// * `from` - The address to call from
    async fn estimate_gas(&self, chain: ChainType, call_data: Vec<u8>, to: Address, from: Address) -> Result<u64>;
    
    /// Get the wallet address used by this executor
    fn wallet_address(&self) -> Address;
    
    /// Get a human-readable description of this executor
    fn description(&self) -> &str;
}

/// High-level abstract trait for order execution
/// 
/// This trait provides a complete abstraction for order processing,
/// hiding the complexity of encoding and low-level transaction execution.
#[async_trait]
pub trait OrderExecutor: Send + Sync {
    /// Execute finalization for an order (high-level interface)
    /// 
    /// # Arguments
    /// * `order` - The order to finalize
    /// 
    /// # Returns
    /// * `Result<String>` - The finalization transaction hash
    async fn execute_finalization(&self, order: &crate::models::Order) -> Result<String>;
    
    /// Estimate gas for finalization
    /// 
    /// # Arguments
    /// * `order` - The order to estimate gas for
    /// 
    /// # Returns
    /// * `Result<u64>` - The estimated gas amount
    async fn estimate_finalization_gas(&self, order: &crate::models::Order) -> Result<u64>;
    
    /// Get the wallet address used by this executor
    /// 
    /// # Returns
    /// * `Address` - The wallet address
    fn wallet_address(&self) -> Address;
    
    /// Get a human-readable description of this executor
    /// 
    /// # Returns
    /// * `&str` - Description of the executor implementation  
    fn description(&self) -> &str;
} 