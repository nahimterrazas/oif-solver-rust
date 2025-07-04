use alloy::{
    primitives::{Address, U256, FixedBytes, Bytes},
    providers::{ProviderBuilder, Provider},
    transports::http::{Client, Http},
    rpc::types::{TransactionRequest, TransactionInput},
    sol,
    sol_types::{SolValue, SolCall},
    signers::local::PrivateKeySigner,
    network::{EthereumWallet, Ethereum},
    primitives::keccak256,
    dyn_abi::{DynSolValue, DynSolType},
};
use anyhow::Result;
use std::str::FromStr;
use tracing::{info, error};
use hex;

use crate::config::AppConfig;

// Temporary contract interfaces - will be replaced with actual contracts
sol! {
    struct MandateOutput {
        bytes32 remoteOracle;
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
        (uint256, uint256)[] inputs;
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
            uint256[] timestamps,
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
            remoteOracle: self.address_to_bytes32(remote_oracle), // Use the actual remote oracle
            remoteFiller: self.address_to_bytes32(coin_filler_address),
            chainId: U256::from(self.config.chains.destination.chain_id),
            token: self.address_to_bytes32(token),
            amount,
            recipient: self.address_to_bytes32(recipient),
            remoteCall: Bytes::new(),
            fulfillmentContext: Bytes::new(),
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

        // Get wallet and create signing provider
        let wallet = self.get_wallet()?.clone();
        
        info!("Using wallet address: {:?}", wallet.default_signer().address());
        
        // Create signing provider for origin chain (where SettlerCompact is deployed)
        let provider = ProviderBuilder::new()
            .wallet(wallet.clone())
            .on_http(self.config.chains.origin.rpc_url.parse()?);

        // Get SettlerCompact contract address from config
        let settler_compact_address: Address = self.config.contracts.settler_compact.parse()
            .map_err(|e| anyhow::anyhow!("Invalid SettlerCompact address in config: {}", e))?;

        let standard_order = &order.standard_order;

        // Convert inputs to (uint256, uint256)[] format WITH PROPER ERROR HANDLING
        let inputs: Result<Vec<(U256, U256)>, anyhow::Error> = standard_order.inputs.iter()
            .enumerate()
            .map(|(i, (token_id, amount))| {
                // Careful BigInt conversion like TypeScript
                let token_id_u256 = token_id.parse::<U256>()
                    .map_err(|e| anyhow::anyhow!("Failed to parse token_id at input[{}]: '{}' - {}", i, token_id, e))?;
                let amount_u256 = amount.parse::<U256>()
                    .map_err(|e| anyhow::anyhow!("Failed to parse amount at input[{}]: '{}' - {}", i, amount, e))?;
                
                info!("🔢 Input[{}]: tokenId={} ({}), amount={} ({})", 
                      i, token_id_u256, token_id, amount_u256, amount);
                
                Ok((token_id_u256, amount_u256))
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
            outputs,
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
        
        // ABI encode as TUPLE (bytes, bytes) - match TypeScript: encode(['bytes', 'bytes'], [sponsorSig, allocatorSig])
        // This is the KEY FIX: TypeScript ['bytes', 'bytes'] = tuple, not dynamic array!
        use alloy::sol_types::SolValue;
        let signatures = (sponsor_sig.clone(), allocator_sig.clone()).abi_encode();

        // Create timestamps array (current time) - EXACT TypeScript match: Math.floor(Date.now() / 1000)
        let current_timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let timestamps = vec![U256::from(current_timestamp)];
        
        info!("Using timestamp: {} (matches TypeScript Math.floor(Date.now() / 1000))", current_timestamp);

        // Create solvers array (solver identifier as bytes32) - PRESERVE ADDRESS CASE like TypeScript
        let solver_address = wallet.default_signer().address();
        let solver_identifier = self.address_to_bytes32(solver_address);
        let solvers = vec![solver_identifier];

        // Set destination (where tokens go - same as solver)
        let destination = solver_identifier;

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
            info!("    Input {}: tokenId={}, amount={}", i, input.0, input.1);
        }
        info!("  Outputs count: {}", contract_order.outputs.len());
        for (i, output) in contract_order.outputs.iter().enumerate() {
            info!("    Output {}: remoteOracle=0x{}, remoteFiller=0x{}, chainId={}, token=0x{}, amount={}, recipient=0x{}", 
                  i, hex::encode(output.remoteOracle), hex::encode(output.remoteFiller), 
                  output.chainId, hex::encode(output.token), output.amount, hex::encode(output.recipient));
        }
        info!("  Sponsor signature: 0x{}", hex::encode(&sponsor_sig));
        info!("  Allocator signature: 0x{}", hex::encode(&allocator_sig));
        info!("  Signatures encoded: 0x{}", hex::encode(&signatures));
        info!("  Timestamps: {:?}", timestamps);
        info!("  Solver: 0x{}", hex::encode(solver_identifier));
        info!("  Destination: 0x{}", hex::encode(destination));
        info!("  Contract address: {:?}", settler_compact_address);

        // Store values for debug logging before move
        let debug_signatures_len = signatures.len();
        let debug_timestamps_len = timestamps.len();
        let debug_solvers_len = solvers.len();
        let debug_calls_len = calls.len();

        // Manual ABI encoding to fix dynamic array bug
        let call_data = self.encode_finalize_call_manual(
            &contract_order,
            &signatures,
            &timestamps,
            &solvers,
            &destination,
            &calls,
        )?;

        info!("Raw finalization transaction data:");
        info!("  Call data length: {} bytes", call_data.len());
        info!("  Call data (hex): 0x{}", hex::encode(&call_data));
        info!("  Function selector: 0x{}", hex::encode(&call_data[..4]));
        
        // DETAILED BREAKDOWN FOR DEBUGGING vs TypeScript (2698 bytes)
        info!("🔍 DEBUGGING: Rust call data breakdown:");
        info!("  Expected TypeScript call data size: 2698 bytes");
        info!("  Actual Rust call data size: {} bytes", call_data.len());
        info!("  Size difference: {} bytes", 2698_i32 - call_data.len() as i32);
        
        // Break down the call data structure to debug encoding differences
        let breakdown_info = format!(
            "
🔍 Call data structure analysis:
  - Function selector (4 bytes): 0x{}
  - Order struct size estimate: ~{} bytes  
  - Signatures tuple size: {} bytes
  - Timestamps array size: ~{} bytes
  - Solvers array size: ~{} bytes
  - Destination (32 bytes): 32 bytes
  - Calls (empty): ~{} bytes
  - Total overhead (offsets/lengths): ~{} bytes",
            hex::encode(&call_data[..4]),
            call_data.len() - 200, // rough estimate
            debug_signatures_len,
            debug_timestamps_len * 32 + 32, // U256 array
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
        
        // Create a provider without signer for static call
        let static_provider = ProviderBuilder::new()
            .on_http(self.config.chains.origin.rpc_url.parse()?);
            
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

    // Manual ABI encoding to fix dynamic array length bug
    fn encode_finalize_call_manual(
        &self,
        order: &StandardOrder,
        signatures: &[u8],
        timestamps: &[U256],
        solvers: &[FixedBytes<32>],
        destination: &FixedBytes<32>,
        calls: &Bytes,
    ) -> Result<Vec<u8>> {
        use alloy::dyn_abi::{DynSolValue, DynSolType};

        // 1. Build MandateOutput as DynSolValue::Tuple
        let mandate_outputs: Vec<DynSolValue> = order.outputs.iter().map(|output| {
            DynSolValue::Tuple(vec![
                DynSolValue::FixedBytes(output.remoteOracle.into(), 32),
                DynSolValue::FixedBytes(output.remoteFiller.into(), 32),
                DynSolValue::Uint(output.chainId, 256),
                DynSolValue::FixedBytes(output.token.into(), 32),
                DynSolValue::Uint(output.amount, 256),
                DynSolValue::FixedBytes(output.recipient.into(), 32),
                DynSolValue::Bytes(output.remoteCall.to_vec()),
                DynSolValue::Bytes(output.fulfillmentContext.to_vec()),
            ])
        }).collect();

        // 2. Build StandardOrder as DynSolValue::Tuple
        let inputs: Vec<DynSolValue> = order.inputs.iter().map(|(token_id, amount)| {
            DynSolValue::Tuple(vec![
                DynSolValue::Uint(*token_id, 256),
                DynSolValue::Uint(*amount, 256),
            ])
        }).collect();

        let order_tuple = DynSolValue::Tuple(vec![
            DynSolValue::Address(order.user),
            DynSolValue::Uint(order.nonce, 256),
            DynSolValue::Uint(order.originChainId, 256),
            DynSolValue::Uint(order.expires, 256),
            DynSolValue::Uint(order.fillDeadline, 256),
            DynSolValue::Address(order.localOracle),
            DynSolValue::Array(inputs),
            DynSolValue::Array(mandate_outputs), // This should include proper array length!
        ]);

        // 3. Build other parameters
        let signatures_val = DynSolValue::Bytes(signatures.to_vec());
        
        let timestamps_val = DynSolValue::Array(
            timestamps.iter().map(|t| DynSolValue::Uint(*t, 256)).collect()
        );
        
        let solvers_val = DynSolValue::Array(
            solvers.iter().map(|s| DynSolValue::FixedBytes((*s).into(), 32)).collect()
        );
        
        let destination_val = DynSolValue::FixedBytes((*destination).into(), 32);
        let calls_val = DynSolValue::Bytes(calls.to_vec());

        // 4. Encode the function parameters
        let function_args = vec![
            order_tuple,
            signatures_val,
            timestamps_val,
            solvers_val,
            destination_val,
            calls_val,
        ];

        // Calculate function selector for finalise(...)
        let selector = keccak256("finalise((address,uint256,uint256,uint256,uint256,address,(uint256,uint256)[],(bytes32,bytes32,uint256,bytes32,uint256,bytes32,bytes,bytes)[]),bytes,uint256[],bytes32[],bytes32,bytes)".as_bytes());
        let function_selector = &selector[0..4];

        // Encode just the parameters 
        let params_value = DynSolValue::Tuple(function_args);
        let encoded_params = params_value.abi_encode();

        // Combine selector + encoded params
        let mut call_data = Vec::new();
        call_data.extend_from_slice(function_selector);
        call_data.extend_from_slice(&encoded_params);

        info!("✅ Manual ABI encoding completed: {} bytes", call_data.len());

        Ok(call_data)
    }

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