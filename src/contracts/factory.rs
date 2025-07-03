use alloy::{
    primitives::{Address, U256, FixedBytes, Bytes},
    providers::{ProviderBuilder, RootProvider, Provider},
    transports::http::{Client, Http},
    rpc::types::{TransactionRequest, TransactionInput},
    sol,
    sol_types::{SolValue, SolCall},
    signers::local::PrivateKeySigner,
    network::EthereumWallet,
    network::ReceiptResponse,
    signers::Signature,
    primitives::keccak256,
    contract::SolCallBuilder
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

#[derive(Clone)]
pub struct ContractFactory {
    pub config: AppConfig,
    origin_provider: Option<RootProvider<Http<Client>>>,
    destination_provider: Option<RootProvider<Http<Client>>>,
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
        self.origin_provider = Some(origin_provider);

        // Create destination chain provider  
        let destination_provider = ProviderBuilder::new()
            .on_http(self.config.chains.destination.rpc_url.parse()
                .map_err(|e| anyhow::anyhow!("Invalid destination RPC URL '{}': {}", self.config.chains.destination.rpc_url, e))?);
        self.destination_provider = Some(destination_provider);

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
            .with_recommended_fillers()
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
            fillDeadline: fill_deadline,
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
        tx_request.gas = Some(360000u128.into());  // Match TypeScript gasLimit
        tx_request.gas_price = Some(50_000_000_000u128.into()); // 50 gwei

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
            .with_recommended_fillers()
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
                
                Ok(MandateOutput {
                    remoteOracle: self.address_to_bytes32(output.remote_oracle),
                    remoteFiller: self.address_to_bytes32(output.remote_filler),
                    chainId: U256::from(output.chain_id),
                    token: self.address_to_bytes32(output.token),
                    amount: amount_u256,
                    recipient: self.address_to_bytes32(output.recipient),
                    remoteCall: output.remote_call.as_ref()
                        .and_then(|s| hex::decode(s.strip_prefix("0x").unwrap_or(s)).ok())
                        .unwrap_or_default()
                        .into(),
                    fulfillmentContext: output.fulfillment_context.as_ref()
                        .and_then(|s| hex::decode(s.strip_prefix("0x").unwrap_or(s)).ok())
                        .unwrap_or_default()
                        .into(),
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

        // Encode signatures: [sponsorSig, allocatorSig] - MATCH TypeScript exactly
        let sponsor_sig = Bytes::from(hex::decode(order.signature.strip_prefix("0x").unwrap_or(&order.signature))
            .map_err(|e| anyhow::anyhow!("Invalid signature format: '{}' - {}", order.signature, e))?);
        let allocator_sig = Bytes::new(); // Empty for AlwaysOKAllocator
        
        // ABI encode the signatures array - match TypeScript: encode(['bytes', 'bytes'], [sponsorSig, allocatorSig])
        use alloy::sol_types::{SolValue, sol_data};
        let signatures_array = vec![sponsor_sig.clone(), allocator_sig.clone()];
        let signatures = signatures_array.abi_encode();

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

        // Empty calls
        let calls = Bytes::new();

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
        let debug_user = contract_order.user;
        let debug_nonce = contract_order.nonce;
        let debug_inputs_len = contract_order.inputs.len();
        let debug_outputs_len = contract_order.outputs.len();
        let debug_expires = contract_order.expires;
        let debug_fill_deadline = contract_order.fillDeadline;
        let debug_origin_chain_id = contract_order.originChainId;
        let debug_local_oracle = contract_order.localOracle;
        let debug_input_0 = contract_order.inputs[0];
        let debug_output_0_chain_id = contract_order.outputs[0].chainId;
        let debug_output_0_amount = contract_order.outputs[0].amount;
        let debug_signatures_len = signatures.len();
        let debug_timestamps = timestamps.clone();
        let debug_calls_len = calls.len();

        // Use the sol! macro to encode the function call
        let call_data = SettlerCompact::finaliseCall {
            order: contract_order,
            signatures: signatures.into(),
            timestamps,
            solvers,
            destination,
            calls,
        }.abi_encode();

        info!("Raw finalization transaction data:");
        info!("  Call data length: {} bytes", call_data.len());
        info!("  Call data (hex): 0x{}", hex::encode(&call_data));
        info!("  Function selector: 0x{}", hex::encode(&call_data[..4]));

        // Create transaction request with explicit from address
        let mut tx_request = TransactionRequest::default()
            .to(settler_compact_address)
            .from(wallet.default_signer().address())
            .input(TransactionInput::from(call_data.clone()));
        
        // Try with lower gas parameters first
        tx_request.gas = Some(500000u128.into());  // Lower gas limit
        tx_request.gas_price = Some(10_000_000_000u128.into()); // 10 gwei instead of 50

        info!("Transaction request details:");
        info!("  To: {:?}", tx_request.to);
        info!("  From: {:?}", tx_request.from);
        info!("  Value: {:?}", tx_request.value);
        info!("  Gas limit: {:?}", tx_request.gas);
        info!("  Gas price: {:?}", tx_request.gas_price);
        info!("  Input data: 0x{}", hex::encode(&call_data));

        info!("Sending SettlerCompact.finalise() transaction...");

        // Skip static call for now and try direct transaction
        info!("🚀 Attempting direct transaction (skipping static call)...");

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
    
    pub fn get_origin_provider(&self) -> Result<&RootProvider<Http<Client>>> {
        self.origin_provider.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Origin provider not initialized"))
    }

    pub fn get_destination_provider(&self) -> Result<&RootProvider<Http<Client>>> {
        self.destination_provider.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Destination provider not initialized"))
    }

    pub fn get_wallet(&self) -> Result<&EthereumWallet> {
        self.wallet.as_ref()
            .ok_or_else(|| anyhow::anyhow!("Wallet not initialized"))
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