use alloy::primitives::{Address, U256, FixedBytes, Bytes};
use anyhow::Result;

#[derive(Debug, Clone)]
pub struct FinaliseParams {
    pub order: StandardOrderParams,
    pub sponsor_sig: Bytes,
    pub allocator_sig: Bytes,
    pub timestamps: Vec<u32>,
    pub solvers: Vec<FixedBytes<32>>,
    pub destination: FixedBytes<32>,
    pub calls: Bytes,
}

#[derive(Debug, Clone)]
pub struct StandardOrderParams {
    pub user: Address,
    pub nonce: U256,
    pub origin_chain_id: U256,
    pub expires: u32,        // uint32 for correct ABI
    pub fill_deadline: u32,  // uint32 for correct ABI
    pub local_oracle: Address,
    pub inputs: Vec<(U256, U256)>,  // Will be encoded as uint256[2][]
    pub outputs: Vec<MandateOutputParams>,
}

#[derive(Debug, Clone)]
pub struct MandateOutputParams {
    pub remote_oracle: FixedBytes<32>,
    pub remote_filler: FixedBytes<32>,
    pub chain_id: U256,
    pub token: FixedBytes<32>,
    pub amount: U256,
    pub recipient: FixedBytes<32>,
    pub remote_call: Bytes,
    pub fulfillment_context: Bytes,
}

#[derive(Debug, Clone)]
pub struct FillParams {
    pub fill_deadline: u32,
    pub order_id: FixedBytes<32>,
    pub output: MandateOutputParams,
    pub proposed_solver: FixedBytes<32>,
}

/// High-level fill request parameters
#[derive(Debug, Clone)]
pub struct FillRequest {
    pub order_id: String,
    pub fill_deadline: u32,
    pub remote_oracle: Address,
    pub token: Address,
    pub amount: U256,
    pub recipient: Address,
}

/// Abstract trait for call data encoding
/// 
/// This trait provides a clean interface for different encoding implementations:
/// - FoundryEncoder: Uses Foundry cast for encoding
/// - Future: Direct Alloy encoding, ethers-rs encoding, etc.
pub trait CallDataEncoder: Send + Sync {
    /// Encode finalization call data for a given order
    /// 
    /// This is the main interface - implementations handle the conversion
    /// from Order to their specific parameter format internally.
    fn encode_finalize_call(&self, order: &crate::models::Order) -> Result<Vec<u8>>;
    
    /// Get the function selector for the finalise function
    fn get_finalize_selector(&self) -> [u8; 4];
    
    /// Get a human-readable description of this encoder
    fn description(&self) -> &str;
    
    /// Encode CoinFiller.fill() call data from high-level request
    /// 
    /// This is the main interface for fill operations - implementations handle
    /// the conversion from FillRequest to their specific parameter format internally.
    fn encode_fill_call(&self, request: &FillRequest) -> Result<Vec<u8>> {
        // Default implementation for encoders that don't support fill
        Err(anyhow::anyhow!("Fill encoding not supported by this encoder"))
    }
    
    /// Get the function selector for the fill function
    fn get_fill_selector(&self) -> [u8; 4] {
        // Default fill selector - can be overridden by implementations
        [0x00, 0x00, 0x00, 0x00]
    }
    
    /// Encode complete fill call with configuration parameters
    /// 
    /// This method takes all the necessary context to create a complete
    /// fill call without requiring post-processing. This is used by
    /// FillOrchestrator to ensure all fields are correctly populated.
    fn encode_complete_fill_call(
        &self,
        request: &FillRequest,
        coin_filler_address: Address,
        destination_chain_id: u64,
        solver_address: Address,
    ) -> Result<Vec<u8>> {
        // Default implementation: use the basic method and hope the implementor
        // handles the configuration correctly
        self.encode_fill_call(request)
    }
} 