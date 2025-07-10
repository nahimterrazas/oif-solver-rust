# OIF-Solver Rust - Blockchain Implementation Plan

## 🎯 **Current Status**

✅ **Completed:**
- HTTP API endpoints (health, orders, queue, finalization)
- Order models matching actual payload structure
- Event-driven processing architecture
- In-memory storage with order lifecycle management
- Simulated blockchain transactions

❌ **Missing:**
- Real blockchain interactions using alloy
- Contract interface definitions
- Transaction signing and gas management
- Error handling for blockchain failures
- Contract deployment and validation

---

## 📋 **Phase 1: Alloy Integration & Contract Interfaces**

### **Step 1.1: Fix Alloy Dependencies & Setup**
**Priority**: High
**Estimated Time**: 4-6 hours

**Tasks:**
1. **Fix alloy imports and provider setup**
   ```rust
   // Update src/contracts/factory.rs
   use alloy::{
       providers::{ProviderBuilder, RootProvider},
       transports::http::{Client, Http},
       sol,
       primitives::{Address, U256, Bytes},
       contract::{Contract, Instance},
       signers::{Signer, local::PrivateKeySigner},
       network::EthereumWallet,
   };
   ```

2. **Create proper provider connections**
   ```rust
   pub async fn create_provider(rpc_url: &str) -> Result<RootProvider<Http<Client>>> {
       let provider = ProviderBuilder::new()
           .on_http(rpc_url.parse()?);
       Ok(provider)
   }
   ```

3. **Setup wallet management**
   ```rust
   pub async fn create_wallet(private_key: &str, provider: RootProvider<Http<Client>>) -> Result<EthereumWallet> {
       let signer = PrivateKeySigner::from_str(private_key)?;
       let wallet = EthereumWallet::from(signer);
       Ok(wallet)
   }
   ```

### **Step 1.2: Define Contract Interfaces**
**Priority**: High
**Estimated Time**: 3-4 hours

**Tasks:**
1. **CoinFiller Contract Interface**
   ```rust
   sol! {
       interface CoinFiller {
           function fill(
               uint32 fillDeadline,
               bytes32 orderId,
               MandateOutput memory output,
               bytes32 proposedSolver
           ) external returns (bool);
           
           struct MandateOutput {
               bytes32 remoteOracle;
               bytes32 remoteFiller;
               uint64 chainId;
               bytes32 token;
               uint256 amount;
               bytes32 recipient;
               bytes remoteCall;
               bytes fulfillmentContext;
           }
       }
   }
   ```

2. **SettlerCompact Contract Interface**
   ```rust
   sol! {
       interface SettlerCompact {
           function finalise(
               uint256 tokenId,
               address user,
               uint64 expires,
               uint64 originChainId,
               address[] memory inputTokens,
               uint256[] memory inputAmounts,
               MandateOutput[] memory outputs,
               bytes memory signature
           ) external returns (bool);
       }
   }
   ```

3. **TheCompact Contract Interface**
   ```rust
   sol! {
       interface TheCompact {
           function depositERC20(
               address token,
               uint256 amount,
               address user
           ) external returns (uint256 tokenId);
       }
   }
   ```

---

## 📋 **Phase 2: Real Fill Operations**

### **Step 2.1: Implement CoinFiller.fill() Execution**
**Priority**: High
**Estimated Time**: 6-8 hours

**Reference**: `CrossChainService.ts:executeDestinationFill()`

**Tasks:**
1. **Update CrossChainService::execute_fill()**
   ```rust
   async fn execute_fill(&self, order: &Order) -> Result<FillResult> {
       let destination_output = &order.standard_order.outputs[0];
       
       // Get destination chain provider
       let provider = self.create_provider(&self.config.chains.destination.rpc_url).await?;
       
       // Create wallet for destination chain
       let wallet = self.create_wallet(&self.config.solver.private_key, provider.clone()).await?;
       
       // Get CoinFiller contract
       let contract_address = Address::from_str(&self.config.contracts.coin_filler)?;
       let contract = CoinFiller::new(contract_address, &provider);
       let contract_with_signer = contract.with_signer(wallet);
       
       // Prepare MandateOutput struct
       let mandate_output = self.create_mandate_output(destination_output)?;
       
       // Prepare parameters
       let fill_deadline = 4294967295u32; // type(uint32).max
       let order_id = self.generate_order_id(&order.id)?;
       let solver_identifier = self.get_solver_identifier()?;
       
       // Execute fill transaction
       let tx = contract_with_signer
           .fill(fill_deadline, order_id, mandate_output, solver_identifier)
           .send()
           .await?;
       
       // Wait for confirmation
       let receipt = tx.get_receipt().await?;
       
       Ok(FillResult::success(
           format!("{:?}", tx.tx_hash()),
           Some(U256::from(receipt.gas_used))
       ))
   }
   ```

2. **Implement helper methods**
   ```rust
   fn create_mandate_output(&self, output: &MandateOutput) -> Result<CoinFiller::MandateOutput> {
       // Convert MandateOutput to contract struct
   }
   
   fn generate_order_id(&self, uuid: &Uuid) -> Result<[u8; 32]> {
       // Convert UUID to bytes32
   }
   
   fn get_solver_identifier(&self) -> Result<[u8; 32]> {
       // Get solver address as bytes32
   }
   ```

### **Step 2.2: Gas Estimation & Management**
**Priority**: Medium
**Estimated Time**: 3-4 hours

**Tasks:**
1. **Implement gas estimation**
   ```rust
   async fn estimate_fill_gas(&self, order: &Order) -> Result<GasEstimate> {
       let provider = self.get_destination_provider().await?;
       
       // Estimate gas for fill operation
       let gas_estimate = contract.fill(/* params */).estimate_gas().await?;
       let gas_price = provider.get_gas_price().await?;
       
       Ok(GasEstimate {
           gas_limit: gas_estimate * 120 / 100, // 20% buffer
           gas_price,
           total_cost: gas_estimate * gas_price,
           is_affordable: gas_price <= self.config.max_gas_price,
       })
   }
   ```

2. **Add gas configuration**
   ```rust
   #[derive(Debug, Clone)]
   pub struct GasConfig {
       pub max_gas_price: U256,
       pub gas_multiplier: f64,
       pub priority_fee: Option<U256>,
   }
   ```

---

## 📋 **Phase 3: Real Finalization Operations**

### **Step 3.1: Implement SettlerCompact.finalise() Execution**
**Priority**: High
**Estimated Time**: 6-8 hours

**Reference**: `FinalizationService.ts:executeFinalization()`

**Tasks:**
1. **Update FinalizationService::execute_finalization()**
   ```rust
   async fn execute_finalization(&self, order: &Order) -> Result<FillResult> {
       let standard_order = &order.standard_order;
       
       // Get origin chain provider
       let provider = self.create_provider(&self.config.chains.origin.rpc_url).await?;
       
       // Create wallet for origin chain
       let wallet = self.create_wallet(&self.config.solver.private_key, provider.clone()).await?;
       
       // Get SettlerCompact contract
       let contract_address = Address::from_str(&self.config.contracts.settler_compact)?;
       let contract = SettlerCompact::new(contract_address, &provider);
       let contract_with_signer = contract.with_signer(wallet);
       
       // Parse inputs and prepare parameters
       let (token_id_str, _) = &standard_order.inputs[0];
       let token_id = U256::from_str(token_id_str)?;
       
       // Prepare signature
       let signature_bytes = self.parse_signature(&order.signature)?;
       
       // Execute finalization
       let tx = contract_with_signer
           .finalise(
               token_id,
               standard_order.user,
               standard_order.expires,
               standard_order.origin_chain_id,
               vec![], // inputTokens - need to extract from inputs
               vec![], // inputAmounts - need to extract from inputs  
               standard_order.outputs.iter().map(|o| self.convert_mandate_output(o)).collect(),
               signature_bytes.into(),
           )
           .send()
           .await?;
       
       // Wait for confirmation
       let receipt = tx.get_receipt().await?;
       
       Ok(FillResult::success(
           format!("{:?}", tx.tx_hash()),
           Some(U256::from(receipt.gas_used))
       ))
   }
   ```

2. **Implement signature parsing**
   ```rust
   fn parse_signature(&self, signature: &str) -> Result<Vec<u8>> {
       let sig_bytes = hex::decode(signature.strip_prefix("0x").unwrap_or(signature))?;
       if sig_bytes.len() != 65 {
           return Err(anyhow::anyhow!("Invalid signature length: {}", sig_bytes.len()));
       }
       Ok(sig_bytes)
   }
   ```

---

## 📋 **Phase 4: Error Handling & Resilience**

### **Step 4.1: Blockchain Error Handling**
**Priority**: Medium
**Estimated Time**: 4-5 hours

**Tasks:**
1. **Define blockchain error types**
   ```rust
   #[derive(Debug, thiserror::Error)]
   pub enum BlockchainError {
       #[error("Transaction failed: {0}")]
       TransactionFailed(String),
       
       #[error("Gas estimation failed: {0}")]
       GasEstimationFailed(String),
       
       #[error("Insufficient funds: required {required}, available {available}")]
       InsufficientFunds { required: U256, available: U256 },
       
       #[error("Contract call reverted: {0}")]
       ContractReverted(String),
       
       #[error("Network error: {0}")]
       NetworkError(String),
   }
   ```

2. **Implement retry logic (optional - user said no retry)**
   ```rust
   // Simple error propagation without retries as requested
   async fn execute_with_error_handling<F, T>(&self, operation: F) -> Result<T>
   where
       F: Future<Output = Result<T>>,
   {
       match operation.await {
           Ok(result) => Ok(result),
           Err(e) => {
               tracing::error!("Blockchain operation failed: {}", e);
               Err(e)
           }
       }
   }
   ```

### **Step 4.2: Transaction Monitoring**
**Priority**: Low
**Estimated Time**: 3-4 hours

**Tasks:**
1. **Basic transaction confirmation**
   ```rust
   async fn wait_for_confirmation(&self, tx_hash: H256, provider: &Provider) -> Result<TransactionReceipt> {
       let receipt = provider.get_transaction_receipt(tx_hash).await?;
       match receipt {
           Some(receipt) if receipt.status == Some(1.into()) => Ok(receipt),
           Some(receipt) => Err(anyhow::anyhow!("Transaction failed: {:?}", receipt)),
           None => Err(anyhow::anyhow!("Transaction not found: {:?}", tx_hash)),
       }
   }
   ```

---

## 📋 **Phase 5: Configuration & Deployment**

### **Step 5.1: Enhanced Configuration**
**Priority**: Medium
**Estimated Time**: 2-3 hours

**Tasks:**
1. **Add blockchain-specific config**
   ```toml
   [contracts]
   the_compact = "0x1234..."
   settler_compact = "0x5678..."
   coin_filler = "0x9abc..."
   
   [gas]
   max_gas_price = "200000000000"  # 200 gwei
   gas_multiplier = 1.2
   priority_fee = "2000000000"     # 2 gwei
   
   [blockchain]
   confirmation_blocks = 1
   timeout_seconds = 300
   ```

2. **Contract address validation**
   ```rust
   impl AppConfig {
       pub fn validate_contracts(&self) -> Result<()> {
           // Validate all contract addresses are valid
           Address::from_str(&self.contracts.the_compact)?;
           Address::from_str(&self.contracts.settler_compact)?;
           Address::from_str(&self.contracts.coin_filler)?;
           Ok(())
       }
   }
   ```

### **Step 5.2: Health Checks & Monitoring**
**Priority**: Low
**Estimated Time**: 2-3 hours

**Tasks:**
1. **Blockchain connectivity health checks**
   ```rust
   pub async fn check_blockchain_health(&self) -> Result<HealthStatus> {
       // Check RPC connectivity
       let origin_provider = self.create_provider(&self.config.chains.origin.rpc_url).await?;
       let dest_provider = self.create_provider(&self.config.chains.destination.rpc_url).await?;
       
       // Check latest block numbers
       let origin_block = origin_provider.get_block_number().await?;
       let dest_block = dest_provider.get_block_number().await?;
       
       Ok(HealthStatus {
           origin_chain_block: origin_block,
           destination_chain_block: dest_block,
           contracts_accessible: self.check_contracts().await?,
       })
   }
   ```

---

## 📋 **Phase 6: Testing & Validation**

### **Step 6.1: Integration Testing**
**Priority**: High
**Estimated Time**: 4-6 hours

**Tasks:**
1. **Create test orders with real data**
   ```rust
   #[cfg(test)]
   mod tests {
       use super::*;
       
       #[tokio::test]
       async fn test_real_fill_execution() {
           // Test with actual contract deployments
       }
       
       #[tokio::test]  
       async fn test_real_finalization() {
           // Test complete order lifecycle
       }
   }
   ```

2. **Validate against TypeScript implementation**
   - Compare transaction parameters
   - Verify gas usage
   - Check final state consistency

---

## 🎯 **Implementation Priority**

### **Critical Path (Week 1)**
1. Phase 1: Alloy Integration & Contract Interfaces
2. Phase 2.1: Real Fill Operations  
3. Phase 3.1: Real Finalization Operations

### **Secondary (Week 2)**
4. Phase 2.2: Gas Management
5. Phase 4.1: Error Handling
6. Phase 5.1: Enhanced Configuration

### **Optional (Week 3)**
7. Phase 4.2: Transaction Monitoring
8. Phase 5.2: Health Checks
9. Phase 6: Testing & Validation

---

## 🚧 **Known Challenges**

### **Technical Challenges**
1. **Alloy API Changes**: Alloy is still evolving, may need version-specific adjustments
2. **Contract ABI Compatibility**: Ensuring Rust sol! macros match actual contracts
3. **Gas Estimation**: Real-world gas costs can vary significantly
4. **Transaction Timing**: Handling block confirmation delays

### **Integration Challenges**
1. **Type Conversions**: Converting between Rust types and blockchain types
2. **Error Mapping**: Translating blockchain errors to application errors
3. **Configuration Management**: Ensuring config matches deployed contracts

### **Testing Challenges**
1. **Local Testing**: Need local blockchain setup (Anvil/Hardhat)
2. **Gas Costs**: Testing with realistic gas prices
3. **Network Latency**: Simulating real network conditions

---

## 📝 **Completion Criteria**

### **Minimum Viable Implementation**
- ✅ Real CoinFiller.fill() execution
- ✅ Real SettlerCompact.finalise() execution  
- ✅ Basic error handling
- ✅ Gas estimation and management
- ✅ Transaction confirmation

### **Production Ready**
- ✅ Comprehensive error handling
- ✅ Health monitoring
- ✅ Gas optimization
- ✅ Integration tests
- ✅ Performance benchmarks

This plan provides a clear roadmap to implement real blockchain interactions while maintaining the existing API and architecture. 

# Implementing Real Blockchain Contract Calls

## Current Status ✅

The OIF Solver Rust implementation is **working** with:
- ✅ Real contract interfaces from Solidity
- ✅ Actual contract addresses loaded from config
- ✅ Blockchain connectivity validation
- ✅ Complete order processing pipeline
- ✅ Gas estimation framework
- 🔄 **Currently simulating contract calls** (marked with 🔄 in logs)

## Phase 5: Enable Real Contract Execution

### What's Currently Simulated

In `src/contracts/factory.rs`, these methods are simulating calls:
1. `fill_order()` - Should call `CoinFiller.fill()` on destination chain
2. `finalize_order()` - Should call `SettlerCompact.finalise()` on origin chain

### How to Enable Real Calls

#### Option 1: Use Raw Contract Calls (Recommended)

Replace the simulation in `fill_order()`:

```rust
// Current simulation (lines 275-285)
let tx_hash = format!("0x{:064x}", rand::random::<u64>());
info!("🔄 SIMULATED CoinFiller.fill() - Replace with real contract call");

// Replace with real call:
use alloy::providers::Provider;
use alloy::rpc::types::TransactionRequest;

let provider_with_wallet = provider.clone().with_signer(wallet.clone());

// Encode the function call data
let call_data = CoinFiller::fillCall {
    fillDeadline: fill_deadline,
    orderId: order_id,
    output: MandateOutput {
        remoteOracle: FixedBytes::<32>::ZERO,
        remoteFiller: self.address_to_bytes32(coin_filler_address),
        chainId: U256::from(self.config.chains.destination.chain_id),
        token: self.address_to_bytes32(token),
        amount,
        recipient: self.address_to_bytes32(recipient),
        remoteCall: Bytes::new(),
        fulfillmentContext: Bytes::new(),
    },
    proposedSolver: proposed_solver,
}.abi_encode();

// Create transaction request
let tx_request = TransactionRequest::default()
    .to(coin_filler_address)
    .data(call_data.into());

// Send transaction
let tx_hash = provider_with_wallet.send_transaction(tx_request).await?
    .get_receipt().await?
    .transaction_hash;

info!("✅ REAL CoinFiller.fill() successful: 0x{:x}", tx_hash);
```

#### Option 2: Use Foundry/Forge Generated Bindings

1. Generate contract bindings with forge:
```bash
forge bind --crate-name oif-contracts --module
```

2. Import the generated contracts:
```rust
use oif_contracts::{CoinFiller, SettlerCompact};
```

3. Use the generated contract instances:
```rust
let contract = CoinFiller::new(coin_filler_address, provider_with_wallet);
let call = contract.fill(fill_deadline, order_id, mandate_output, proposed_solver);
let tx_hash = call.send().await?.get_receipt().await?.transaction_hash;
```

### Required Changes for Real Implementation

#### 1. Contract Address Configuration ✅
**Status: Complete** - Addresses loaded from `config/local.toml`:
- CoinFiller: `0x5FbDB2315678afecb367f032d93F642f64180aa3`
- SettlerCompact: `0x5FC8d32690cc91D4c39d9d3abcBD16989F875707`
- TheCompact: `0x5FbDB2315678afecb367f032d93F642f64180aa3`

#### 2. Fix Order Data Integration

Currently using placeholder values. Need to pass real order data:

```rust
// In fill_order(), replace placeholders:
let order_id = self.string_to_order_id(&order.id.to_string()); // Real order ID
let fill_deadline = order.standard_order.fill_deadline; // From actual order

// In finalize_order(), use real order:
let standard_order = StandardOrder {
    user: order.standard_order.user,
    nonce: U256::from(order.standard_order.nonce), 
    originChainId: U256::from(order.standard_order.origin_chain_id),
    expires: order.standard_order.expires,
    fillDeadline: order.standard_order.fill_deadline,
    localOracle: order.standard_order.local_oracle,
    inputs: order.standard_order.inputs, // Convert format
    outputs: order.standard_order.outputs, // Convert format
};
```

#### 3. Add Error Handling

```rust
match call.send().await {
    Ok(pending_tx) => {
        match pending_tx.get_receipt().await {
            Ok(receipt) => {
                if receipt.status() == Some(U64::from(1)) {
                    info!("✅ Transaction successful");
                    Ok(format!("0x{:x}", receipt.transaction_hash))
                } else {
                    Err(anyhow::anyhow!("Transaction reverted"))
                }
            }
            Err(e) => Err(anyhow::anyhow!("Failed to get receipt: {}", e))
        }
    }
    Err(e) => Err(anyhow::anyhow!("Failed to send transaction: {}", e))
}
```

#### 4. Event Monitoring (Future Enhancement)

Add event monitoring for transaction confirmations:

```rust
// Monitor for OutputFilled events
let filter = provider.filter::<CoinFiller::OutputFilled>()
    .address(coin_filler_address)
    .from_block(receipt.block_number.unwrap());

let events = filter.query().await?;
for event in events {
    info!("OutputFilled event: orderId={:?}, solver={:?}", 
          event.orderId, event.solver);
}
```

### Testing Real Implementation

#### 1. Local Anvil Setup
```bash
# Terminal 1: Start origin chain
anvil --port 8545 --chain-id 31337

# Terminal 2: Start destination chain  
anvil --port 8546 --chain-id 31338

# Terminal 3: Deploy contracts (if needed)
# Update config/local.toml with deployed addresses
```

#### 2. Test Order Flow
```bash
# Start solver
cargo run

# Submit test order
curl -X POST http://localhost:3000/api/v1/orders \
  -H "Content-Type: application/json" \
  -d '{"order": {...}, "signature": "0x..."}'

# Check logs for real contract calls
```

### Expected Log Output After Implementation

**Before (Current):**
```
🔄 SIMULATED CoinFiller.fill() - Replace with real contract call
Fill transaction hash: 0x1234...
```

**After (Real Calls):**
```
✅ REAL CoinFiller.fill() successful: 0x1234...
✅ REAL SettlerCompact.finalise() successful: 0x5678...
```

### Priority Implementation Order

1. **High Priority**: Replace `fill_order()` simulation with real CoinFiller calls
2. **High Priority**: Replace `finalize_order()` simulation with real SettlerCompact calls
3. **Medium Priority**: Add proper order data integration
4. **Medium Priority**: Add comprehensive error handling
5. **Low Priority**: Add event monitoring and confirmations

### Files to Modify

1. `src/contracts/factory.rs` - Replace simulation with real calls
2. `src/services/cross_chain.rs` - Pass real order data
3. `src/services/finalization.rs` - Pass real order data
4. `config/local.toml` - Update with deployed contract addresses

## Next Steps

1. **Deploy contracts to local anvil chains** (if not already done)
2. **Update contract addresses** in `config/local.toml` 
3. **Replace simulation code** with real contract calls
4. **Test end-to-end** order processing with real blockchain transactions

The infrastructure is ready - just need to enable the real contract calls! 