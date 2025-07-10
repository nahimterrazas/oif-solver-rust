use alloy::primitives::Address;
use anyhow::Result;
use async_trait::async_trait;

#[derive(Debug, Clone)]
pub struct GasParams {
    pub gas_limit: u64,
    pub gas_price: u64,
}

#[async_trait]
pub trait ExecutionEngine: Send + Sync {
    /// Send a transaction to the blockchain (low-level)
    async fn send_transaction(&self, call_data: Vec<u8>, to: Address, gas: GasParams) -> Result<String>;
    
    /// Perform a static call (read-only)
    async fn static_call(&self, call_data: Vec<u8>, to: Address, from: Address) -> Result<Vec<u8>>;
    
    /// Estimate gas for a transaction
    async fn estimate_gas(&self, call_data: Vec<u8>, to: Address, from: Address) -> Result<u64>;
    
    /// Get the wallet address used by this executor
    fn wallet_address(&self) -> Address;
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