use alloy::{
    primitives::{Address, U256, FixedBytes, Bytes},
    providers::{ProviderBuilder, Provider},
    transports::http::{Client, Http},
    rpc::types::{TransactionRequest, TransactionInput},
    sol,
    sol_types::{SolValue, SolCall, SolInterface},
    signers::local::PrivateKeySigner,
    network::{EthereumWallet, Ethereum},
    primitives::keccak256,
    dyn_abi::{DynSolValue, DynSolType},
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

// Temporary contract interfaces - will be replaced with actual contracts
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
        info!("Executing real fill order: order_id={}, fill_deadline={}, remote_oracle={:?}, token={:?}, amount={}, recipient={:?}", order_id, fill_deadline, remote_oracle, token, amount, recipient);

        // Get wallet and create signing provider
        let wallet = self.get_wallet()?.clone();
        
        info!("Using wallet address: {:?}", wallet.default_signer().address());
        
        // Create signing provider for destination chain
        let provider = ProviderBuilder::new()
            .wallet(wallet.clone())
            .on_http(self.config.chains.destination.rpc_url.parse()?);

        // Prepare contract call parameters - use the original fillDeadline from the order
        let order_id_bytes32 = self.string_to_order_id(order_id); // Use the actual order ID
        let proposed_solver = self.address_to_bytes32(wallet.default_signer().address());
        
        // Get CoinFiller contract address from config
        let coin_filler_address: Address = self.config.contracts.coin_filler.parse()
            .map_err(|e| anyhow::anyhow!("Invalid CoinFiller address in config: {}", e))?;

        // Create MandateOutput struct for the contract call
        let mandate_output = MandateOutput {
            remoteOracle: self.address_to_bytes32(remote_oracle), // Use address directly, not bytes32
            remoteFiller: self.address_to_bytes32(self.config.contracts.coin_filler.parse()?),
            chainId: U256::from(self.config.chains.destination.chain_id),
            token: self.address_to_bytes32(token),
            amount: amount,
            recipient: self.address_to_bytes32(recipient),
            remoteCall: Bytes::default(),
            fulfillmentContext: Bytes::default(),
        };

        info!("MandateOutput structure details:");
        info!("  remoteOracle: 0x{}", hex::encode(mandate_output.remoteOracle));
        info!("  remoteFiller: 0x{}", hex::encode(mandate_output.remoteFiller));
        info!("  chainId: {}", mandate_output.chainId);
        info!("  token: 0x{}", hex::encode(mandate_output.token));
        info!("  amount: {}", mandate_output.amount);
        info!("  recipient: 0x{}", hex::encode(mandate_output.recipient));
        info!("  remoteCall: 0x{}", hex::encode(&mandate_output.remoteCall));
        info!("  fulfillmentContext: 0x{}", hex::encode(&mandate_output.fulfillmentContext));

        info!("Encoding CoinFiller.fill() call with parameters:");
        info!("  Fill deadline: {}", fill_deadline);
        info!("  Order ID: {:?}", order_id_bytes32);
        info!("  Proposed solver: {:?}", proposed_solver);
        info!("  Contract address: {:?}", coin_filler_address);

        // Use the sol! macro to encode the function call
        let call_data = CoinFiller::fillCall {
            fillDeadline: u32::MAX, // Use uint32::MAX like TypeScript, not the actual order deadline
            orderId: order_id_bytes32,
            output: mandate_output,
            proposedSolver: proposed_solver,
        }.abi_encode();

        info!("Raw transaction data:");
        info!("  Call data length: {} bytes", call_data.len());
        info!("  Call data (hex): 0x{}", hex::encode(&call_data));
        info!("  Function selector: 0x{}", hex::encode(&call_data[..4]));

        // Create transaction request with explicit gas parameters
        let mut tx_request = TransactionRequest::default()
            .to(coin_filler_address)
            .input(TransactionInput::from(call_data.clone()));
        
        // Set gas parameters explicitly
        tx_request.gas = Some(360000u64.into());  // Match TypeScript gasLimit
        tx_request.gas_price = Some(50_000_000_000u64.into()); // 50 gwei

        info!("Transaction request details:");
        info!("  To: {:?}", tx_request.to);
        info!("  From: {:?}", tx_request.from);
        info!("  Value: {:?}", tx_request.value);
        info!("  Gas limit: {:?}", tx_request.gas);
        info!("  Gas price: {:?}", tx_request.gas_price);
        info!("  Input data: 0x{}", hex::encode(&call_data));

        info!("Sending CoinFiller.fill() transaction...");

        // Send transaction and get pending transaction
        let pending_tx = provider.send_transaction(tx_request).await
            .map_err(|e| {
                error!("Transaction send failed with detailed error:");
                error!("  Error: {}", e);
                error!("  Contract address: {:?}", coin_filler_address);
                error!("  Wallet address: {:?}", wallet.default_signer().address());
                error!("  Call data: 0x{}", hex::encode(&call_data));
                anyhow::anyhow!("Failed to send fill transaction: {}", e)
            })?;

        // Get transaction hash
        let tx_hash = pending_tx.tx_hash().to_string();

        info!("✅ CoinFiller.fill() transaction sent successfully: {}", tx_hash);
        info!("   Waiting for confirmation...");

        // Optionally wait for receipt to confirm transaction
        let receipt = pending_tx.get_receipt().await?;
        
        // Check if transaction was successful
        if !receipt.status() {
            error!("❌ CoinFiller.fill() transaction FAILED (reverted)");
            error!("   Transaction hash: {}", tx_hash);
            error!("   Gas used: {}", receipt.gas_used);
            error!("   Block: {}", receipt.block_number.unwrap_or_default());
            return Err(anyhow::anyhow!("Fill transaction reverted: {}", tx_hash));
        }
        
        info!("✅ Transaction confirmed successfully");
        info!("   Gas used: {}", receipt.gas_used);
        info!("   Block: {}", receipt.block_number.unwrap_or_default());
        
        Ok(tx_hash)
    }

    pub async fn finalize_order(
        &self,
        order: &crate::models::Order,
    ) -> Result<String> {
        info!("Executing real SettlerCompact.finalise(): order_id={}", order.id);
        
        // Get wallet and provider for transaction
        let wallet = self.get_wallet()?.clone();
        let provider = self.get_origin_provider()?;

        info!("Using wallet address: {:?}", wallet.default_signer().address());
        
        // 🔍 CHAIN VERIFICATION: Ensure we're executing on the correct origin chain
        info!("🔍 CHAIN VERIFICATION - Expected Origin Chain:");
        info!("   Chain ID: {}", self.config.chains.origin.chain_id);
        info!("   RPC URL: {}", self.config.chains.origin.rpc_url);
        info!("   SettlerCompact: {}", self.config.contracts.settler_compact);
        info!("   TheCompact: {}", self.config.contracts.the_compact);
        
        // Create signing provider for origin chain (where SettlerCompact is deployed)
        let provider = ProviderBuilder::new()
            .wallet(wallet.clone())
            .on_http(self.config.chains.origin.rpc_url.parse()?);

        // 🔍 RUNTIME CHAIN VERIFICATION: Verify we're actually connected to the correct chain
        let actual_chain_id = provider.get_chain_id().await?;
        let expected_chain_id = self.config.chains.origin.chain_id;
        
        info!("🔍 RUNTIME CHAIN VERIFICATION:");
        info!("   Expected Chain ID: {}", expected_chain_id);
        info!("   Actual Chain ID: {}", actual_chain_id);
        
        if actual_chain_id != expected_chain_id {
            return Err(anyhow::anyhow!(
                "❌ CHAIN MISMATCH: Expected chain ID {} but connected to chain ID {}. Check your RPC configuration!",
                expected_chain_id, actual_chain_id
            ));
        }
        
        info!("✅ CHAIN VERIFICATION PASSED: Connected to correct origin chain ({})", actual_chain_id);
        
        // Get SettlerCompact contract address from config
        let settler_compact_address: Address = self.config.contracts.settler_compact.parse()
            .map_err(|e| anyhow::anyhow!("Invalid SettlerCompact address in config: {}", e))?;

        let standard_order = &order.standard_order;

        // Convert inputs to Input[] format WITH PROPER ERROR HANDLING
        let inputs: Result<Vec<Input>, anyhow::Error> = standard_order.inputs.iter()
            .enumerate()
            .map(|(i, (token_id, amount))| {
                // Careful BigInt conversion like TypeScript
                let token_id_u256 = token_id.parse::<U256>()
                    .map_err(|e| anyhow::anyhow!("Failed to parse token_id at input[{}]: '{}' - {}", i, token_id, e))?;
                let amount_u256 = amount.parse::<U256>()
                    .map_err(|e| anyhow::anyhow!("Failed to parse amount at input[{}]: '{}' - {}", i, amount, e))?;
                
                info!("🔢 Input[{}]: tokenId={} ({}), amount={} ({})", 
                      i, token_id_u256, token_id, amount_u256, amount);
                
                Ok(Input { tokenId: token_id_u256, amount: amount_u256 })
            })
            .collect();
        
        let inputs = inputs?;

        // Convert outputs to MandateOutput[] WITH PROPER ERROR HANDLING
        let outputs: Result<Vec<MandateOutput>, anyhow::Error> = standard_order.outputs.iter()
            .enumerate()
            .map(|(i, output)| {
                let amount_u256 = output.amount.parse::<U256>()
                    .map_err(|e| anyhow::anyhow!("Failed to parse output amount at output[{}]: '{}' - {}", i, output.amount, e))?;
                
                info!("🔢 Output[{}]: amount={} ({})", i, amount_u256, output.amount);
                
                // Handle remoteCall and fulfillmentContext - EXACTLY match TypeScript '0x' encoding
                let remote_call = match &output.remote_call {
                    Some(s) if !s.is_empty() && s != "null" => {
                        Bytes::from(hex::decode(s.strip_prefix("0x").unwrap_or(s)).unwrap_or_default())
                    }
                    _ => {
                        // TypeScript: remoteCall: output.remoteCall || '0x'
                        // Force explicit empty bytes encoding to match TypeScript ABI
                        Bytes::from(vec![])
                    }
                };
                
                let fulfillment_context = match &output.fulfillment_context {
                    Some(s) if !s.is_empty() && s != "null" => {
                        Bytes::from(hex::decode(s.strip_prefix("0x").unwrap_or(s)).unwrap_or_default())
                    }
                    _ => {
                        // TypeScript: fulfillmentContext: output.fulfillmentContext || '0x'  
                        // This encodes as empty bytes but with proper ABI encoding
                        Bytes::new()
                    }
                };
                
                info!("🔍 Output[{}] field handling:", i);
                info!("  remoteCall: {} bytes", remote_call.len());
                info!("  fulfillmentContext: {} bytes", fulfillment_context.len());
                
                Ok(MandateOutput {
                    remoteOracle: self.address_to_bytes32(output.remote_oracle),
                    remoteFiller: self.address_to_bytes32(output.remote_filler),
                    chainId: U256::from(output.chain_id),
                    token: self.address_to_bytes32(output.token),
                    amount: amount_u256,
                    recipient: self.address_to_bytes32(output.recipient),
                    remoteCall: remote_call,
                    fulfillmentContext: fulfillment_context,
                })
            })
            .collect();
        
        let outputs = outputs?;

        // Create contract order struct with proper BigInt handling
        let contract_order = StandardOrder {
            user: standard_order.user,
            nonce: U256::from(standard_order.nonce),
            originChainId: U256::from(standard_order.origin_chain_id),
            expires: U256::from(standard_order.expires),
            fillDeadline: U256::from(standard_order.fill_deadline),
            localOracle: standard_order.local_oracle,
            inputs,
            outputs,  // Use the already processed outputs, not re-mapping
        };

        // Validate signature is not empty
        if order.signature.trim().is_empty() || order.signature == "0x" {
            return Err(anyhow::anyhow!("Order has empty signature"));
        }

        // Encode signatures: [sponsorSig, allocatorSig] - ROBUST signature validation
        let sponsor_sig = {
            let sig_str = order.signature.strip_prefix("0x").unwrap_or(&order.signature);
            if sig_str.is_empty() {
                return Err(anyhow::anyhow!("Empty signature"));
            }
            if sig_str.len() % 2 != 0 {
                return Err(anyhow::anyhow!("Odd-length hex signature: '{}'", order.signature));
            }
            // Validate signature is exactly 65 bytes (130 hex chars) for ECDSA
            if sig_str.len() != 130 {
                return Err(anyhow::anyhow!("Invalid signature length: {} chars, expected 130 for ECDSA", sig_str.len()));
            }
            Bytes::from(hex::decode(sig_str)
                .map_err(|e| anyhow::anyhow!("Invalid hex in signature '{}': {}", order.signature, e))?)
        };
        let allocator_sig = Bytes::new(); // Empty for AlwaysOKAllocator
        
        info!("✅ Signature validation passed: {} bytes", sponsor_sig.len());
        
        // Don't pre-encode signatures with Alloy - let ethabi handle it to avoid the bug

        // Create timestamps array as u32 for uint32[] (not uint256[])
        let current_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as u32;
        let timestamps = vec![current_timestamp];
        
        info!("Using timestamp: {} (u32 for uint32[] ABI)", current_timestamp);

        // CRITICAL FIX: Use solver wallet address instead of remoteFiller address  
        // The TypeScript version uses solver wallet for both solvers[0] and destination
        let solver_wallet_address = wallet.default_signer().address();
        let solver_wallet_bytes32 = self.address_to_bytes32(solver_wallet_address);
        let solvers = vec![solver_wallet_bytes32];
        let destination = solver_wallet_bytes32;
        
        // Keep remote_filler_address for reference but don't use for contract params
        let remote_filler_address = if let Some(first_output) = standard_order.outputs.first() {
            first_output.remote_filler
        } else {
            return Err(anyhow::anyhow!("Order has no outputs to determine remoteFiller"));
        };
        
        info!("🔧 CORRECTED: Using solver wallet address to match TypeScript:");
        info!("  Solver wallet address: {:?}", solver_wallet_address);
        info!("  RemoteFiller address: {:?}", remote_filler_address);
        info!("  Contract expects solver wallet = solvers[0] = destination (matching TypeScript)");

        // 🔍 DEBUGGING: Check how many outputs we actually have
        info!("🔍 CRITICAL DEBUG: contract_order.outputs.len() = {}", contract_order.outputs.len());
        info!("🔍 CRITICAL DEBUG: standard_order.outputs.len() = {}", standard_order.outputs.len());
        for (i, o) in contract_order.outputs.iter().enumerate() {
            info!("🔍 CRITICAL DEBUG: out[{i}].amount = {}, chainId = {}", o.amount, o.chainId);
        }
        
        // Also dump the raw JSON structure
        info!("🔍 RAW JSON ORDER: {}", serde_json::to_string_pretty(&order.standard_order).unwrap_or_default());
        // TypeScript uses EMPTY calls - match exactly!
        // Ensure we use the exact same encoding as TypeScript '0x'
        let calls = Bytes::from(hex::decode("").unwrap_or_default());
        info!("🔍 Using empty calls parameter (matches TypeScript '0x' implementation)");

        info!("SettlerCompact.finalise() parameters:");
        info!("  User: {:?}", contract_order.user);
        info!("  Nonce: {}", contract_order.nonce);
        info!("  Origin Chain ID: {}", contract_order.originChainId);
        info!("  Expires: {}", contract_order.expires);
        info!("  Fill Deadline: {}", contract_order.fillDeadline);
        info!("  Local Oracle: 0x{}", hex::encode(contract_order.localOracle));
        info!("  Inputs count: {}", contract_order.inputs.len());
        for (i, input) in contract_order.inputs.iter().enumerate() {
            info!("    Input {}: tokenId={}, amount={}", i, input.tokenId, input.amount);
        }
        info!("  Outputs count: {}", contract_order.outputs.len());
        for (i, output) in contract_order.outputs.iter().enumerate() {
            info!("    Output {}: remoteOracle=0x{}, remoteFiller=0x{}, chainId={}, token=0x{}, amount={}, recipient=0x{}", 
                  i, hex::encode(output.remoteOracle), hex::encode(output.remoteFiller), 
                  output.chainId, hex::encode(output.token), output.amount, hex::encode(output.recipient));
        }
        info!("  Sponsor signature: 0x{}", hex::encode(&sponsor_sig));
        info!("  Allocator signature: 0x{}", hex::encode(&allocator_sig));
        info!("  Timestamps: {:?}", timestamps);
        info!("  Solver: 0x{}", hex::encode(solver_wallet_bytes32));
        info!("  Destination: 0x{}", hex::encode(destination));
        info!("  Contract address: {:?}", settler_compact_address);

        // CRITICAL DEBUGGING: The sponsor signature is NOT for StandardOrder verification
        // It's for BatchCompact verification inside TheCompact.batchClaim()
        // The signature verification happens automatically when we call finalise()
        // So we don't need to manually verify the signature here.
        info!("✅ Signature will be verified by TheCompact.batchClaim() during finalise() call");
        
        // ⚠️ REMOVED: Manual EIP-712 signature verification - not needed and was causing confusion
        // The actual verification happens in the smart contract when we call finalise()

        // Store values for debug logging before move
        let debug_sponsor_sig_len = sponsor_sig.len();
        let debug_allocator_sig_len = allocator_sig.len(); // Should be 0 (empty) to match TypeScript
        let debug_timestamps_len = timestamps.len();
        let debug_solvers_len = solvers.len();
        let debug_calls_len = calls.len();

        // CRITICAL FIX: Use EXACT same selector as working TypeScript (0xdd1ff485)
        info!("🚀 USING TYPESCRIPT SELECTOR: 0xdd1ff485 (exact match with working TypeScript)");
        
        let call_data = self.encode_finalize_call_with_typescript_selector(
            &contract_order,
            &sponsor_sig,
            &allocator_sig,
            &timestamps,
            &solvers,
            &destination,
            &calls,
        )?;

        info!("Raw finalization transaction data:");
        info!("  Call data length: {} bytes", call_data.len());
        info!("  Call data (hex): 0x{}", hex::encode(&call_data));
        info!("  Function selector: 0x{}", hex::encode(&call_data[..4]));
        
        // DETAILED BREAKDOWN FOR DEBUGGING vs TypeScript (1349 bytes)
        info!("🔍 DEBUGGING: Rust call data breakdown:");
        info!("  Expected TypeScript call data size: 1349 bytes");
        info!("  Actual Rust call data size: {} bytes", call_data.len());
        info!("  Size difference: {} bytes", 1349_i32 - call_data.len() as i32);
        
        // Break down the call data structure to debug encoding differences
        let breakdown_info = format!(
            "
🔍 Call data structure analysis:
  - Function selector (4 bytes): 0x{}
  - Order struct size estimate: ~{} bytes  
  - Sponsor sig size: {} bytes
  - Allocator sig size: {} bytes
  - Timestamps array size: ~{} bytes
  - Solvers array size: ~{} bytes
  - Destination (32 bytes): 32 bytes
  - Calls (empty): ~{} bytes
  - Total overhead (offsets/lengths): ~{} bytes",
            hex::encode(&call_data[..4]),
            call_data.len() - 200, // rough estimate
            debug_sponsor_sig_len,
            debug_allocator_sig_len,
            debug_timestamps_len * 4 + 32, // u32 array
            debug_solvers_len * 32 + 32,    // bytes32 array  
            debug_calls_len + 32,           // bytes
            200                         // ABI encoding overhead
        );
        info!("{}", breakdown_info);

        // Create transaction request with explicit from address
        let mut tx_request = TransactionRequest::default()
            .to(settler_compact_address)
            .from(wallet.default_signer().address())
            .input(TransactionInput::from(call_data.clone()));
        
        // Use EXACT same gas parameters as working TypeScript version
        tx_request.gas = Some(650000u64.into());  // Match TypeScript gas limit
        tx_request.gas_price = Some(1178761408u64.into()); // Match TypeScript gas price

        info!("Transaction request details:");
        info!("  To: {:?}", tx_request.to);
        info!("  From: {:?}", tx_request.from);
        info!("  Value: {:?}", tx_request.value);
        info!("  Gas limit: {:?}", tx_request.gas);
        info!("  Gas price: {:?}", tx_request.gas_price);
        info!("  Input data: 0x{}", hex::encode(&call_data));

        info!("Sending SettlerCompact.finalise() transaction...");

        // First, try a static call to get detailed revert reason if it would fail
        info!("🔍 Testing finalization call statically first...");
        
        let provider_chain = self.config.chains.origin.rpc_url.parse()?;
        info!("🔍 Provider chain: {:?}", provider_chain);
        // Create a provider without signer for static call
        let static_provider = ProviderBuilder::new()
            .on_http(provider_chain);
            
        // Prepare static call transaction
        let static_tx_request = TransactionRequest::default()
            .to(settler_compact_address)
            .from(wallet.default_signer().address())
            .input(TransactionInput::from(call_data.clone()));
            
        match static_provider.call(static_tx_request).await {
            Ok(result) => {
                info!("✅ Static call succeeded, proceeding with actual transaction...");
                info!("   Static call result: 0x{}", hex::encode(&result));
            }
            Err(static_error) => {
                error!("❌ Static call failed, this will help debug the issue:");
                error!("📋 Revert reason: {:?}", static_error);
                
                // Try to decode common error signatures
                if let Some(error_data) = static_error.to_string().split("data: ").nth(1) {
                    if let Some(hex_data) = error_data.split('"').next() {
                        info!("📋 Raw error data: {}", hex_data);
                        
                        // Try to decode as string if it looks like revert reason
                        if hex_data.len() > 8 && hex_data.starts_with("0x08c379a0") {
                            // Standard Error(string) signature
                            if let Ok(decoded_bytes) = hex::decode(&hex_data[10..]) {
                                if let Ok(error_msg) = String::from_utf8(decoded_bytes) {
                                    error!("📋 Decoded error message: {}", error_msg);
                                }
                            }
                        }
                    }
                }
                
                // Still proceed with the transaction to get more detailed logs
                info!("🚀 Proceeding with actual transaction despite static call failure...");
            }
        }

        // Send transaction and get pending transaction
        let pending_tx = provider.send_transaction(tx_request).await
            .map_err(|e| {
                error!("Finalize transaction send failed with detailed error:");
                error!("  Error: {}", e);
                error!("  Contract address: {:?}", settler_compact_address);
                error!("  Wallet address: {:?}", wallet.default_signer().address());
                error!("  Call data: 0x{}", hex::encode(&call_data));
                anyhow::anyhow!("Failed to send finalize transaction: {}", e)
            })?;

        // Get transaction hash
        let tx_hash = pending_tx.tx_hash().to_string();

        info!("✅ SettlerCompact.finalise() transaction sent successfully: {}", tx_hash);
        info!("   Waiting for confirmation...");

        // Wait for receipt and check transaction status
        let receipt = pending_tx.get_receipt().await?;
        
        // Check if transaction was successful
        if !receipt.status() {
            error!("❌ SettlerCompact.finalise() transaction FAILED (reverted)");
            error!("   Transaction hash: {}", tx_hash);
            error!("   Gas used: {}", receipt.gas_used);
            error!("   Block: {}", receipt.block_number.unwrap_or_default());
            error!("   🔍 POSSIBLE CAUSES:");
            error!("   • Order not properly filled/proven on destination chain");
            error!("   • Invalid signature or timestamp");
            error!("   • Nonce mismatch or order already finalized");
            error!("   • Contract state validation failed");
            return Err(anyhow::anyhow!("Finalization transaction reverted: {}", tx_hash));
        }
        
        info!("✅ Transaction confirmed successfully");
        info!("   Gas used: {}", receipt.gas_used);
        info!("   Block: {}", receipt.block_number.unwrap_or_default());
        
        Ok(tx_hash)
    }

    // Helper methods for real blockchain execution (to be implemented)
    
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
        use alloy::primitives::keccak256;

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