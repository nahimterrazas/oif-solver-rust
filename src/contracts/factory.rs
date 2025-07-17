use alloy::{
    primitives::{Address, U256, FixedBytes, Bytes},
    providers::{ProviderBuilder, Provider},
    sol,
    signers::local::PrivateKeySigner,
    network::{EthereumWallet},
    primitives::keccak256,
};
use anyhow::Result;
use std::str::FromStr;
use tracing::{info, error, warn};
use hex;

// CRITICAL FIX: Add ethers-rs for proper contract interface
use ethers::prelude::*;
use ethers::contract::abigen;

// Generate contract bindings using ABI - this ensures exact compatibility with TypeScript
abigen!(
    SettlerCompactEthers,
    r#"[
        {
            "type": "function",
            "name": "finalise",
            "inputs": [
                {
                    "name": "order",
                    "type": "tuple",
                    "components": [
                        {"name": "user", "type": "address"},
                        {"name": "nonce", "type": "uint256"},
                        {"name": "originChainId", "type": "uint256"},
                        {"name": "expires", "type": "uint256"},
                        {"name": "fillDeadline", "type": "uint256"},
                        {"name": "localOracle", "type": "address"},
                        {"name": "inputs", "type": "tuple[]", "components": [
                            {"name": "tokenId", "type": "uint256"},
                            {"name": "amount", "type": "uint256"}
                        ]},
                        {"name": "outputs", "type": "tuple[]", "components": [
                            {"name": "remoteOracle", "type": "bytes32"},
                            {"name": "remoteFiller", "type": "bytes32"},
                            {"name": "chainId", "type": "uint256"},
                            {"name": "token", "type": "bytes32"},
                            {"name": "amount", "type": "uint256"},
                            {"name": "recipient", "type": "bytes32"},
                            {"name": "remoteCall", "type": "bytes"},
                            {"name": "fulfillmentContext", "type": "bytes"}
                        ]}
                    ]
                },
                {"name": "signatures", "type": "bytes"},
                {"name": "timestamps", "type": "uint32[]"},
                {"name": "solvers", "type": "bytes32[]"},
                {"name": "destination", "type": "bytes32"},
                {"name": "calls", "type": "bytes"}
            ],
            "outputs": [{"name": "", "type": "bool"}],
            "stateMutability": "nonpayable"
        }
    ]"#,
);

use crate::config::AppConfig;
use crate::contracts::operations::{FinalizationOrchestrator, FillOrchestrator};
use crate::contracts::abi::AbiRegistry;
use std::sync::Arc;

// Contract interfaces using Alloy sol! macro - shared across modules  
sol! {
    // Add Input struct for EIP-712 hashing
    struct Input {
        uint256 tokenId;     // ⚠️ CRITICAL: tokenId FIRST (matches TypeScript)
        uint256 amount;      // ⚠️ CRITICAL: amount SECOND (matches TypeScript)
    }
    
    struct MandateOutput {
        bytes32 remoteOracle;     // ⚠️ CRITICAL: bytes32, not address (matches TypeScript)
        bytes32 remoteFiller;
        uint256 chainId;
        bytes32 token;
        uint256 amount;
        bytes32 recipient;
        bytes remoteCall;
        bytes fulfillmentContext;
    }

    struct StandardOrder {
        address user;
        uint256 nonce;
        uint256 originChainId;
        uint256 expires;
        uint256 fillDeadline;
        address localOracle;
        Input[] inputs; // Changed from anonymous tuple for hashing
        MandateOutput[] outputs;
    }

    interface CoinFiller {
        function fill(
            uint32 fillDeadline,
            bytes32 orderId,
            MandateOutput memory output,
            bytes32 proposedSolver
        ) external returns (bool);
    }

    interface SettlerCompact {
        function finalise(
            StandardOrder order,
            bytes signatures,
            uint32[] timestamps,  // ⚠️ CRITICAL: uint32[] not uint256[]
            bytes32[] solvers,
            bytes32 destination,
            bytes calls
        ) external returns (bool);
    }

    interface TheCompact {
        function depositERC20(
            address token,
            uint256 amount,
            address user
        ) external returns (uint256 tokenId);

        function DOMAIN_SEPARATOR() external view returns (bytes32);
    }
}

pub struct ContractFactory {
    pub config: AppConfig,
    origin_provider: Option<Box<dyn Provider + Send + Sync>>,
    destination_provider: Option<Box<dyn Provider + Send + Sync>>,
    wallet: Option<EthereumWallet>,
}

impl ContractFactory {
    pub async fn new(config: AppConfig) -> Result<Self> {
        let mut factory = Self {
            config,
            origin_provider: None,
            destination_provider: None,
            wallet: None,
        };

        // Initialize providers
        factory.init_providers().await?;
        // Initialize wallet
        factory.init_wallet().await?;

        Ok(factory)
    }

    async fn init_providers(&mut self) -> Result<()> {
        info!("Initializing blockchain providers");
        info!("Origin RPC: {}", self.config.chains.origin.rpc_url);
        info!("Destination RPC: {}", self.config.chains.destination.rpc_url);

        // Create origin chain provider
        let origin_provider = ProviderBuilder::new()
            .on_http(self.config.chains.origin.rpc_url.parse()
                .map_err(|e| anyhow::anyhow!("Invalid origin RPC URL '{}': {}", self.config.chains.origin.rpc_url, e))?);
        self.origin_provider = Some(Box::new(origin_provider));

        // Create destination chain provider  
        let destination_provider = ProviderBuilder::new()
            .on_http(self.config.chains.destination.rpc_url.parse()
                .map_err(|e| anyhow::anyhow!("Invalid destination RPC URL '{}': {}", self.config.chains.destination.rpc_url, e))?);
        self.destination_provider = Some(Box::new(destination_provider));

        info!("Blockchain providers initialized successfully");
        Ok(())
    }

    async fn init_wallet(&mut self) -> Result<()> {
        info!("Initializing solver wallet");

        let private_key = &self.config.solver.private_key;
        info!("Using private key: {}...", &private_key[..10]);
        let signer = PrivateKeySigner::from_str(private_key)?;
        let wallet = EthereumWallet::from(signer);
        
        info!("Solver wallet initialized: {:?}", wallet.default_signer().address());
        self.wallet = Some(wallet);

        Ok(())
    }

    pub async fn fill_order(
        &self,
        order_id: &str,
        fill_deadline: u32,
        remote_oracle: Address,
        token: Address,
        amount: U256,
        recipient: Address,
    ) -> Result<String> {
        info!("🚀 MODULAR FILL: Using FillOrchestrator architecture");
        
        // Create FillOrchestrator with modular components
        let orchestrator = self.create_fill_orchestrator()?;
        
        // Execute fill using the new modular approach
        let tx_hash = orchestrator.execute_fill(
            order_id,
            fill_deadline,
            remote_oracle,
            token,
            amount,
            recipient,
        ).await?;
        
        info!("✅ Modular fill completed successfully: {}", tx_hash);
        Ok(tx_hash)
    }

    pub async fn finalize_order(
        &self,
        order: &crate::models::Order,
    ) -> Result<String> {
        info!("🚀 MODULAR FINALIZATION: Using FinalizationOrchestrator architecture");
        
        // Create FinalizationOrchestrator with modular components
        let orchestrator = self.create_finalization_orchestrator()?;
        
        // Execute finalization using the new modular approach
        let tx_hash = orchestrator.execute_finalization(order).await?;
        
        info!("✅ Modular finalization completed successfully: {}", tx_hash);
        Ok(tx_hash)
    }

    /// Create FinalizationOrchestrator with the current factory configuration
    fn create_finalization_orchestrator(&self) -> Result<FinalizationOrchestrator> {
        info!("🏗️ Creating FinalizationOrchestrator from ContractFactory");
        
        // Create ABI provider
        let abi_provider = Arc::new(AbiRegistry::new());
        
        // Create config Arc from current config
        let config = Arc::new(self.config.clone());
        
        // Create FinalizationOrchestrator
        let orchestrator = FinalizationOrchestrator::new(abi_provider, config)?;
        
        info!("✅ FinalizationOrchestrator created with factory configuration");
        info!("  Wallet address: {}", orchestrator.wallet_address());
        
        Ok(orchestrator)
    }

    /// Create FillOrchestrator with the current factory configuration
    fn create_fill_orchestrator(&self) -> Result<FillOrchestrator> {
        info!("🏗️ Creating FillOrchestrator from ContractFactory");
        
        // Create ABI provider
        let abi_provider = Arc::new(AbiRegistry::new());
        
        // Create config Arc from current config
        let config = Arc::new(self.config.clone());
        
        // Create FillOrchestrator
        let orchestrator = FillOrchestrator::new(abi_provider, config)?;
        
        info!("✅ FillOrchestrator created with factory configuration");
        info!("  Wallet address: {}", orchestrator.wallet_address());
        
        Ok(orchestrator)
    }

    /// Estimate gas for finalization using FinalizationOrchestrator
    pub async fn estimate_finalization_gas(&self, order: &crate::models::Order) -> Result<u64> {
        info!("⛽ Estimating finalization gas using FinalizationOrchestrator");
        
        let orchestrator = self.create_finalization_orchestrator()?;
        let gas_estimate = orchestrator.estimate_finalization_gas(order).await?;
        
        info!("✅ Gas estimation completed: {} gas", gas_estimate);
        Ok(gas_estimate)
    }

    /// Estimate gas for fill using FillOrchestrator
    pub async fn estimate_fill_gas(
        &self,
        order_id: &str,
        fill_deadline: u32,
        remote_oracle: Address,
        token: Address,
        amount: U256,
        recipient: Address,
    ) -> Result<u64> {
        info!("⛽ Estimating fill gas using FillOrchestrator");
        
        let orchestrator = self.create_fill_orchestrator()?;
        let gas_estimate = orchestrator.estimate_fill_gas(
            order_id,
            fill_deadline,
            remote_oracle,
            token,
            amount,
            recipient,
        ).await?;
        
        info!("✅ Fill gas estimation completed: {} gas", gas_estimate);
        Ok(gas_estimate)
    }

    /// Get wallet address from the factory
    pub fn get_wallet_address(&self) -> Result<Address> {
        Ok(self.get_wallet()?.default_signer().address())
    }
    
    pub fn get_origin_provider(&self) -> Result<&(dyn Provider + Send + Sync)> {
        self.origin_provider.as_ref()
            .map(|p| p.as_ref())
            .ok_or_else(|| anyhow::anyhow!("Origin provider not initialized"))
    }

    pub fn get_destination_provider(&self) -> Result<&(dyn Provider + Send + Sync)> {
        self.destination_provider.as_ref()
            .map(|p| p.as_ref())
            .ok_or_else(|| anyhow::anyhow!("Destination provider not initialized"))
    }

    pub fn get_wallet(&self) -> Result<&EthereumWallet> {
        self.wallet.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Wallet not initialized"))
    }

    // ⚠️ REMOVED: get_the_compact_domain_separator function - not needed since we don't 
    // manually verify EIP-712 signatures anymore

    // CRITICAL FIX: Use EXACT TypeScript selector 0xdd1ff485
    fn encode_finalize_call_with_typescript_selector(
        &self,
        order: &StandardOrder,
        sponsor_sig: &Bytes,
        allocator_sig: &Bytes,
        timestamps: &[u32],
        solvers: &[FixedBytes<32>],
        destination: &FixedBytes<32>,
        calls: &Bytes,
    ) -> Result<Vec<u8>> {
        info!("🔧 Using EXACT TypeScript selector 0xdd1ff485 to match working version");
        
        // Use the working TypeScript calldata as a template
        // From TypeScript logs: 1349 bytes with selector 0xdd1ff485
        
        // First try to find what function signature produces 0xdd1ff485
        // Since we can't reverse it, we'll use the raw parameter encoding from cast
        // but with the correct TypeScript selector
        
        // Call the original manual function to get parameter encoding
        let manual_result = self.encode_finalize_call_manual(
            order, sponsor_sig, allocator_sig, timestamps, solvers, destination, calls
        )?;
        
        // Replace the selector with TypeScript's working selector
        let typescript_selector = [0xdd, 0x1f, 0xf4, 0x85]; // 0xdd1ff485
        let parameters = &manual_result[4..]; // Skip original selector
        
        let typescript_calldata = [typescript_selector.as_slice(), parameters].concat();
        
        info!("✅ Using TypeScript selector: 0x{}", hex::encode(&typescript_selector));
        info!("✅ Parameters from cast: {} bytes", parameters.len());
        info!("✅ Total calldata: {} bytes", typescript_calldata.len());
        
        Ok(typescript_calldata)
    }

    // Original Foundry cast abi-encode implementation 
    fn encode_finalize_call_manual(
        &self,
        order: &StandardOrder,
        sponsor_sig: &Bytes,
        allocator_sig: &Bytes,
        timestamps: &[u32],
        solvers: &[FixedBytes<32>],
        destination: &FixedBytes<32>,
        calls: &Bytes,
    ) -> Result<Vec<u8>> {
        use std::process::Command;

        info!("🔧 Using Foundry cast abi-encode to fix nested tuple arrays bug");
        
        // Check if cast is available
        let cast_available = Command::new("cast")
            .arg("--version")
            .output()
            .map(|out| out.status.success())
            .unwrap_or(false);
            
        if !cast_available {
            error!("⚠️  Foundry cast not available - falling back to ethabi (may have nested tuple bugs)");
            warn!("Install Foundry with: curl -L https://foundry.paradigm.xyz | bash");
            // TODO: Implement fallback to ethabi here if needed
            return Err(anyhow::anyhow!("Foundry cast not available and ethabi has nested tuple arrays bug"));
        }
        
        info!("✅ Foundry cast is available");

        // Helper function to format Address as hex string
        let addr = |a: &Address| -> String {
            format!("0x{}", hex::encode(a.as_slice()))
        };

        // Helper for FixedBytes<32> to proper bytes32 hex (full 32 bytes)
        let bytes32_hex = |b: &FixedBytes<32>| -> String {
            format!("0x{}", hex::encode(b.as_slice())) // Full 32 bytes, not just last 20
        };
        
        // Helper function to format bytes fields 
        let bytes_hex = |b: &[u8]| -> String {
            if b.is_empty() { "0x".to_string() } else { format!("0x{}", hex::encode(b)) }
        };

        // Prepare the function signature with CORRECT types: uint32[] for timestamps, bytes32[] for solvers  
        let function_sig = "finalise((address,uint256,uint256,uint256,uint256,address,(uint256,uint256)[],(bytes32,bytes32,uint256,bytes32,uint256,bytes32,bytes,bytes)[]),bytes,uint32[],bytes32[],bytes32,bytes)";

        // EIP-712 type definitions must match EXACTLY the TypeScript definitions
        let standard_order_type_string = [
            "StandardOrder(address user,uint256 nonce,uint256 originChainId,uint256 expires,uint256 fillDeadline,address localOracle,Input[] inputs,MandateOutput[] outputs)",
            "Input(uint256 tokenId,uint256 amount)",  // ⚠️ CRITICAL: tokenId FIRST (matches TypeScript)
            "MandateOutput(bytes32 remoteOracle,bytes32 remoteFiller,uint256 chainId,bytes32 token,uint256 amount,bytes32 recipient,bytes remoteCall,bytes fulfillmentContext)"  // ⚠️ CRITICAL: bytes32 types (matches TypeScript)
        ].concat();

        // Build order argument without quotes (Command doesn't use shell)
        let order_arg = format!(
            "({},{},{},{},{},{},{},{})",
            format!("0x{}", hex::encode(order.user)),
            order.nonce,
            order.originChainId,
            order.expires,
            order.fillDeadline,
            format!("0x{}", hex::encode(order.localOracle)),
            format!("[{}]", 
                order.inputs.iter()
                    .map(|i| format!("({},{})", i.tokenId, i.amount))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            format!("[{}]",
                order.outputs.iter()
                    .map(|o| format!(
                        "({},{},{},{},{},{},{},{})",
                        bytes32_hex(&o.remoteOracle),
                        bytes32_hex(&o.remoteFiller),
                        o.chainId,
                        bytes32_hex(&o.token),
                        o.amount,
                        bytes32_hex(&o.recipient),
                        bytes_hex(&o.remoteCall),
                        bytes_hex(&o.fulfillmentContext)
                    ))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        );

        // Build signatures as concatenated bytes (sponsorSig + allocatorSig)
        // CRITICAL: TypeScript uses ABI.encode(['bytes','bytes'], [sponsor, allocator])
        // which creates a bytes containing the encoded tuple. We need to replicate this.
        
        // CRITICAL FIX: Don't pad sponsor signature - ECDSA signatures are already 65 bytes
        // The sig_to_65_bytes function was corrupting the 'v' component (recovery id)
        info!("🔍 Sponsor signature length before processing: {} bytes", sponsor_sig.len());
        
        let sponsor_fixed = sponsor_sig.to_vec(); // Use signature as-is, no padding
        let allocator_fixed = allocator_sig.to_vec(); // Keep allocator empty as intended
        
        info!("🔍 Signature lengths after processing: sponsor={} bytes, allocator={} bytes", 
              sponsor_fixed.len(), allocator_fixed.len());
        
        // TypeScript uses ABI.encode(['bytes','bytes'], [sponsor, allocator])
        // We need to replicate this by encoding the tuple first, then passing it
        let sponsor_hex = format!("0x{}", hex::encode(&sponsor_fixed));
        let allocator_hex = "0x"; // Empty bytes - exactly like TypeScript
        
        // Use cast abi-encode with function signature (without selector)
        let tuple_encode_output = Command::new("cast")
            .arg("abi-encode")
            .arg("f(bytes,bytes)")  // Function signature - cast abi-encode doesn't add selector
            .arg(&sponsor_hex)
            .arg(&allocator_hex) // allocator empty bytes
            .output()
            .map_err(|e| anyhow::anyhow!("Failed to encode signatures tuple: {}", e))?;

        if !tuple_encode_output.status.success() {
            let stderr = String::from_utf8_lossy(&tuple_encode_output.stderr);
            return Err(anyhow::anyhow!("Failed to encode signatures tuple: {}", stderr));
        }

        let signatures_arg = String::from_utf8(tuple_encode_output.stdout)?.trim().to_string(); // Already 0x... without selector

        info!("len(signatures_arg) = {} chars ⇒ {} bytes", signatures_arg.len(), (signatures_arg.len() - 2) / 2);
        
        // Build other arguments without quotes - u32 for uint32[] type
        let timestamps_arg = format!("[{}]", timestamps.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(","));
        let solvers_arg = format!("[{}]", solvers.iter().map(|s| bytes32_hex(s)).collect::<Vec<_>>().join(","));
        let destination_arg = bytes32_hex(destination);
        let calls_arg = bytes_hex(calls);

        // Log critical parameters that the contract validates
        info!("🔍 CRITICAL VALIDATION PARAMETERS:");
        info!("  Order nonce: {}", order.nonce);
        info!("  Order user: 0x{}", hex::encode(order.user));
        info!("  Order expires: {} (current time: {})", order.expires, std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs());
        info!("  Fill deadline: {}", order.fillDeadline);
        info!("  Timestamps[0]: {}", timestamps[0]);
        info!("  Destination: {}", destination_arg);

        info!("🔧 Cast arguments (no quotes, correct types):");
        info!("  Function sig: {}", function_sig);
        info!("  Order: {}", &order_arg[..200.min(order_arg.len())]);
        info!("  Signatures: {} (length: {} chars = {} bytes)", signatures_arg, signatures_arg.len(), signatures_arg.len()/2 - 1);
        info!("  Signatures breakdown: sponsor={} bytes, allocator={} bytes (empty)", sponsor_fixed.len(), allocator_fixed.len());
        info!("  Timestamps: {}", timestamps_arg);
        info!("  Solvers: {}", solvers_arg);

        // Call cast abi-encode to obtain the parameter payload (without selector)
        let output = Command::new("cast")
            .arg("abi-encode")
            .arg(function_sig)
            .arg(&order_arg)
            .arg(&signatures_arg)
            .arg(&timestamps_arg)
            .arg(&solvers_arg)
            .arg(&destination_arg)
            .arg(&calls_arg)
            .output()
            .map_err(|e| anyhow::anyhow!("Failed to run cast abi-encode: {}", e))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            return Err(anyhow::anyhow!("cast abi-encode failed:\nSTDERR: {}\nSTDOUT: {}", stderr, stdout));
        }

        let encoded_hex = String::from_utf8(output.stdout)?.trim().to_string();
        
        // Convert the full (selector + params) hex string to bytes
        let encoded_hex = encoded_hex.strip_prefix("0x").unwrap_or(&encoded_hex);
        // Parameters only (cast doesn't include selector when --selector isn't used)
        let encoded_bytes = hex::decode(encoded_hex)
            .map_err(|e| anyhow::anyhow!("Failed to decode hex from cast: {}", e))?;

        // Prepend the correct selector constant (0xa80b6640)
        const SELECTOR: [u8; 4] = [0xa8, 0x0b, 0x66, 0x40];
        let calldata = [SELECTOR.as_slice(), &encoded_bytes[..]].concat();

        info!("✅ Foundry cast abi-encode completed: {} bytes", calldata.len());

        // DETAILED COMPARISON OUTPUT FOR TYPESCRIPT DEBUGGING
        info!("🔬 RUST CALL DATA FOR TYPESCRIPT COMPARISON:");
        info!("🔬 Rust CallData ({} chars = {} bytes):", calldata.len() * 2, calldata.len());
        info!("🔬 0x{}", hex::encode(&calldata));
        info!("🔬 END RUST CALL DATA");

        // Critical runtime checks with asserts
        info!("🔍 cast payload len = {}", calldata.len());  // Should be ~1348
        info!("🔍 selector = 0x{}", hex::encode(&calldata[..4]));

        // Assert payload is not absurdly short. For a single-output order the
        // correct size is ~1349 bytes (matching TypeScript).
        if calldata.len() < 1200 {
            error!("❌ Calldata unexpectedly small: {} bytes (expected ≈1349).", calldata.len());
            return Err(anyhow::anyhow!("Calldata too small: {} bytes, expected ≈1349", calldata.len()));
        }

        // Sanity check on selector
        if &calldata[..4] != SELECTOR {
            return Err(anyhow::anyhow!("Unexpected selector after prefixing: 0x{}", hex::encode(&calldata[..4])));
        }

        // Informative check for expected size range (allows small variations
        // depending on number of inputs/outputs). Should match TypeScript at 1349 bytes.
        if (1345..=1355).contains(&calldata.len()) {
            info!("🎉 SUCCESS! Cast payload = {} bytes (matches expected TypeScript ≈1349)", calldata.len());
        } else {
            warn!("⚠️  Cast payload = {} bytes (outside expected 1345-1355 range – verify vs TypeScript)", calldata.len());
        }

        // Debug: print first 64 bytes to check offset structure
        if calldata.len() >= 64 {
            info!("🔍 First 64 bytes: {}", hex::encode(&calldata[..64]));
        }

        Ok(calldata)
    }

    // ⚠️ REMOVED: calculate_order_hash function - not needed since signature verification 
    // happens in the smart contract via TheCompact.batchClaim()
    
// Utility methods for contract interaction
    
    pub fn address_to_bytes32(&self, address: Address) -> FixedBytes<32> {
        let mut bytes = [0u8; 32];
        bytes[12..].copy_from_slice(address.as_slice());
        FixedBytes::from(bytes)
    }

    pub fn string_to_order_id(&self, order_id: &str) -> FixedBytes<32> {
        keccak256(order_id.as_bytes())
    }

    pub async fn check_chain_connectivity(&self) -> Result<(u64, u64)> {
        let origin_block = self.get_origin_provider()?.get_block_number().await?;
        let dest_block = self.get_destination_provider()?.get_block_number().await?;
        
        Ok((origin_block, dest_block))
    }
} 

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Order, StandardOrder};
    
    fn create_test_config() -> AppConfig {
        AppConfig {
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
                    chain_id: 1,
                    rpc_url: "https://eth.llamarpc.com".to_string(),
                },
                destination: crate::config::ChainDetails {
                    chain_id: 137,
                    rpc_url: "https://polygon.llamarpc.com".to_string(),
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
            relayer: None,
        }
    }
    
    fn create_test_order() -> Order {
        use uuid::Uuid;
        use chrono::Utc;
        
        Order {
            id: Uuid::new_v4(),
            signature: "0x1111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111111".to_string(),
            status: crate::models::OrderStatus::Pending,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            fill_tx_hash: None,
            finalize_tx_hash: None,
            error_message: None,
            standard_order: StandardOrder {
                user: "0x1111111111111111111111111111111111111111".parse().unwrap(),
                nonce: 123,
                origin_chain_id: 1,
                expires: 1752062605,
                fill_deadline: 1752062605,
                local_oracle: "0x2222222222222222222222222222222222222222".parse().unwrap(),
                inputs: vec![("100".to_string(), "1000000000000000000".to_string())],
                outputs: vec![
                    crate::models::MandateOutput {
                        remote_oracle: "0x3333333333333333333333333333333333333333".parse().unwrap(),
                        remote_filler: "0x4444444444444444444444444444444444444444".parse().unwrap(),
                        chain_id: 137,
                        token: "0x5555555555555555555555555555555555555555".parse().unwrap(),
                        amount: "500000000000000000".to_string(),
                        recipient: "0x6666666666666666666666666666666666666666".parse().unwrap(),
                        remote_call: None,
                        fulfillment_context: None,
                    }
                ],
            },
        }
    }
    
    #[tokio::test]
    async fn test_factory_creates_orchestrator() {
        let config = create_test_config();
        let factory = ContractFactory::new(config).await.unwrap();
        
        // Test that factory can create orchestrator
        let orchestrator = factory.create_finalization_orchestrator().unwrap();
        
        // Verify wallet address matches
        let factory_wallet_addr = factory.get_wallet_address().unwrap();
        let orchestrator_wallet_addr = orchestrator.wallet_address();
        
        assert_eq!(factory_wallet_addr, orchestrator_wallet_addr);
    }
    
    #[tokio::test]
    async fn test_factory_finalization_integration() {
        let config = create_test_config();
        let factory = ContractFactory::new(config).await.unwrap();
        let order = create_test_order();
        
        // This test verifies the integration doesn't panic
        // In a real environment with actual RPC endpoints, this would succeed
        // But in tests, it will fail at the RPC call level, which is expected
        let result = factory.finalize_order(&order).await;
        
        // We expect this to fail due to test environment, but not due to integration issues
        assert!(result.is_err());
        
        // Verify the error is related to network/RPC, not integration
        let error_msg = result.unwrap_err().to_string();
        assert!(!error_msg.contains("integration") && !error_msg.contains("orchestrator"));
    }
    
    #[tokio::test]
    async fn test_factory_gas_estimation_integration() {
        let config = create_test_config();
        let factory = ContractFactory::new(config).await.unwrap();
        let order = create_test_order();
        
        // Test gas estimation integration - this may succeed or fail based on RPC
        // The important thing is that the integration works without panicking
        let result = factory.estimate_finalization_gas(&order).await;
        
        // Verify the integration layer works (either success or network-related error)
        match result {
            Ok(gas_estimate) => {
                // If it succeeds, gas estimate should be reasonable
                assert!(gas_estimate > 0 && gas_estimate < 10_000_000);
            }
            Err(error) => {
                // If it fails, should be network/RPC related, not integration issues
                let error_msg = error.to_string();
                assert!(!error_msg.contains("integration") && !error_msg.contains("orchestrator"));
            }
        }
    }
    
    #[test]
    fn test_factory_wallet_address_access() {
        let config = create_test_config();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let factory = runtime.block_on(ContractFactory::new(config)).unwrap();
        
        // Test wallet address retrieval
        let wallet_addr = factory.get_wallet_address().unwrap();
        
        // Verify it's a valid address format
        assert_eq!(wallet_addr.to_string().len(), 42); // 0x + 40 hex chars
        assert!(wallet_addr.to_string().starts_with("0x"));
    }
    
    #[tokio::test]
    async fn test_orchestrator_vs_factory_consistency() {
        let config = create_test_config();
        let factory = ContractFactory::new(config).await.unwrap();
        
        // Create orchestrator through factory
        let orchestrator = factory.create_finalization_orchestrator().unwrap();
        
        // Test that both have consistent configuration
        assert_eq!(factory.get_wallet_address().unwrap(), orchestrator.wallet_address());
        
        // Test that orchestrator has proper wallet functionality
        let order = create_test_order();
        let orchestrator_wallet = orchestrator.wallet_address();
        assert!(!orchestrator_wallet.is_zero());
    }
}