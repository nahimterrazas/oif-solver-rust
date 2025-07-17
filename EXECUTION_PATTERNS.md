# OIF Solver: Execution Patterns Architecture

## Overview

This document describes the modular execution architecture that enables seamless switching between **direct blockchain execution** (Alloy) and **relayer-based execution** (OpenZeppelin relayers).

## Design Patterns Implemented

### 1. **Strategy Pattern** 
The core pattern that allows interchangeable execution strategies:

```rust
// Different execution strategies
trait ExecutionEngine {
    async fn send_transaction(&self, ...) -> Result<ExecutionResponse>;
    fn transport_type(&self) -> TransportType;
    // ... other methods
}

// Implementations
struct AlloyExecutor { ... }      // Direct blockchain calls
struct OpenZeppelinExecutor { ... } // HTTP API calls to relayer
```

### 2. **Factory Pattern**
Easy creation and switching between execution engines:

```rust
// Simple factory
let factory = DefaultExecutionEngineFactory::new(config);
let executor = factory.create_engine(TransportType::Relayer)?;

// Smart factory with fallback
let smart_factory = SmartExecutionEngineFactory::new(config, true);
let executor = smart_factory.create_engine_with_fallback(preferred_transport)?;
```

### 3. **Builder Pattern**
Flexible configuration construction:

```rust
let config = ExecutionEngineConfigBuilder::new()
    .with_app_config(app_config)
    .with_relayer_config(relayer_config)
    .with_wallet_address(wallet_address)
    .with_default_transport(TransportType::Relayer)
    .build()?;
```

## Architecture Components

### Core Abstractions

#### `ExecutionEngine` Trait
The main strategy interface supporting both synchronous and asynchronous execution:

```rust
#[async_trait]
pub trait ExecutionEngine: Send + Sync {
    // Enhanced send_transaction with context support
    async fn send_transaction(
        &self,
        chain: ChainType,
        call_data: Vec<u8>,
        to: Address,
        gas: GasParams,
        context: Option<ExecutionContext>,
    ) -> Result<ExecutionResponse>;
    
    // Traditional blockchain operations
    async fn static_call(&self, ...) -> Result<Vec<u8>>;
    async fn estimate_gas(&self, ...) -> Result<u64>;
    
    // Metadata and async support
    fn transport_type(&self) -> TransportType;
    async fn check_async_status(&self, request_id: &str) -> Result<AsyncStatus>;
    async fn get_transaction_hash(&self, request_id: &str) -> Result<Option<String>>;
}
```

#### `TransportType` Enum
Defines available transport mechanisms:

```rust
pub enum TransportType {
    Direct,                    // Alloy, Web3, etc.
    Relayer,                   // OpenZeppelin, etc.
    Custom(String),            // Future extensions
}
```

#### `ExecutionResponse` Enum
Supports both immediate and asynchronous responses:

```rust
pub enum ExecutionResponse {
    Immediate(String),         // Direct execution returns tx hash
    Async {                    // Relayer execution returns tracking info
        request_id: String,
        status: AsyncStatus,
        estimated_completion: Option<u64>,
    },
}
```

### Execution Context

Fine-grained control over transaction execution:

```rust
pub struct ExecutionContext {
    pub priority: ExecutionPriority,     // Low, Normal, High, Critical
    pub timeout_seconds: Option<u64>,    // Max wait time for async
    pub metadata: HashMap<String, String>, // Custom data for relayers
    pub request_id: Option<String>,      // Optional tracking ID
}
```

## Implementation Details

### 1. Alloy Executor (Direct)

**Characteristics:**
- ✅ Direct blockchain connection
- ✅ Immediate transaction confirmation
- ✅ Full blockchain feature support (static calls, gas estimation)
- ❌ No built-in retry logic
- ❌ Single point of failure
- ❌ Manual gas management

**Usage:**
```rust
let alloy_executor = AlloyExecutor::new(app_config)?;
let response = alloy_executor.send_transaction(
    ChainType::Origin,
    call_data,
    contract_address,
    gas_params,
    None, // No special context needed
).await?;

// Always returns ExecutionResponse::Immediate(tx_hash)
if let ExecutionResponse::Immediate(tx_hash) = response {
    println!("Transaction confirmed: {}", tx_hash);
}
```

### 2. OpenZeppelin Executor (Relayer)

**Characteristics:**
- ✅ Built-in retry and error handling
- ✅ Professional gas management
- ✅ Multi-chain support out of the box
- ✅ Async execution with webhooks
- ✅ MEV protection and privacy options
- ❌ API dependency
- ❌ No static calls support
- ❌ Additional latency

**Configuration:**
```rust
let relayer_config = RelayerConfig {
    api_base_url: "https://api.defender.openzeppelin.com/relay".to_string(),
    api_key: "your-api-key".to_string(),
    webhook_url: Some("https://your-app.com/webhook".to_string()),
    chain_endpoints: {
        let mut endpoints = HashMap::new();
        endpoints.insert(1, "ethereum".to_string());
        endpoints.insert(137, "polygon".to_string());
        endpoints
    },
    timeout_seconds: 300,
    max_retries: 3,
    use_async: true,
};

let oz_executor = OpenZeppelinExecutor::new(relayer_config, wallet_address)?;
```

**Usage:**
```rust
let context = ExecutionContext {
    priority: ExecutionPriority::High,
    timeout_seconds: Some(120),
    metadata: {
        let mut meta = HashMap::new();
        meta.insert("source".to_string(), "order_finalization".to_string());
        meta
    },
    ..Default::default()
};

let response = oz_executor.send_transaction(
    ChainType::Origin,
    call_data,
    contract_address,
    gas_params,
    Some(context),
).await?;

match response {
    ExecutionResponse::Immediate(tx_hash) => {
        println!("Sync execution completed: {}", tx_hash);
    }
    ExecutionResponse::Async { request_id, status, .. } => {
        println!("Async execution started: {} (status: {:?})", request_id, status);
        
        // Later, check status
        let current_status = oz_executor.check_async_status(&request_id).await?;
        if current_status == AsyncStatus::Confirmed {
            let tx_hash = oz_executor.get_transaction_hash(&request_id).await?;
            println!("Transaction confirmed: {:?}", tx_hash);
        }
    }
}
```

## Factory Usage Patterns

### 1. Development Setup (Alloy Only)

```rust
use crate::contracts::execution::presets;

let config = presets::development_config(app_config)?;
let factory = DefaultExecutionEngineFactory::new(config);
let executor = factory.create_engine(TransportType::Direct)?;
```

### 2. Production Setup (OpenZeppelin Relayer)

```rust
let config = presets::production_config(
    app_config,
    relayer_config,
    wallet_address,
)?;
let factory = DefaultExecutionEngineFactory::new(config);
let executor = factory.create_engine(TransportType::Relayer)?;
```

### 3. Hybrid Setup (Both Available)

```rust
let config = presets::hybrid_config(
    app_config,
    relayer_config,
    wallet_address,
    true, // prefer_relayer
)?;

let smart_factory = SmartExecutionEngineFactory::new(config, true);

// Get recommendations based on use case
let transport = smart_factory.recommend_transport(ExecutionUseCase::Production);
let executor = smart_factory.create_engine_with_fallback(transport)?;
```

### 4. Use Case-Based Selection

```rust
let use_case_examples = vec![
    (ExecutionUseCase::Development, TransportType::Direct),
    (ExecutionUseCase::HighFrequency, TransportType::Direct),
    (ExecutionUseCase::CrossChain, TransportType::Relayer),
    (ExecutionUseCase::GasOptimized, TransportType::Relayer),
    (ExecutionUseCase::Production, TransportType::Relayer),
];

for (use_case, expected) in use_case_examples {
    let recommended = smart_factory.recommend_transport(use_case);
    assert_eq!(recommended, expected);
}
```

## Integration Guide

### Step 1: Add Dependencies

In your `Cargo.toml`:

```toml
# Already included in the project
reqwest = { version = "0.11", features = ["json", "rustls-tls"] }
async-trait = "0.1"
```

### Step 2: Configuration

```rust
// For Alloy only
let config = ExecutionEngineConfigBuilder::new()
    .with_app_config(app_config)
    .with_wallet_address(wallet_address)
    .build()?;

// For hybrid (Alloy + OpenZeppelin)
let config = ExecutionEngineConfigBuilder::new()
    .with_app_config(app_config)
    .with_relayer_config(relayer_config)
    .with_wallet_address(wallet_address)
    .with_default_transport(TransportType::Relayer)
    .build()?;
```

### Step 3: Factory Creation

```rust
let factory = DefaultExecutionEngineFactory::new(config);

// Check available transports
println!("Available: {:?}", factory.available_transports());
println!("Supports relayer: {}", factory.supports_transport(&TransportType::Relayer));
```

### Step 4: Executor Usage

```rust
// Create executor
let executor = factory.create_engine(TransportType::Relayer)?;

// Execute transaction
let response = executor.send_transaction(
    ChainType::Origin,
    encoded_call_data,
    target_contract,
    GasParams { gas_limit: 200_000, gas_price: 20_000_000_000 },
    Some(ExecutionContext {
        priority: ExecutionPriority::High,
        timeout_seconds: Some(300),
        ..Default::default()
    }),
).await?;

// Handle response
match response {
    ExecutionResponse::Immediate(hash) => {
        println!("✅ Transaction confirmed: {}", hash);
    }
    ExecutionResponse::Async { request_id, .. } => {
        println!("📋 Transaction queued: {}", request_id);
        // Implement status checking logic
    }
}
```

## Advanced Patterns

### 1. Dynamic Transport Selection

```rust
fn select_transport_for_order(order: &Order, factory: &SmartExecutionEngineFactory) -> TransportType {
    if order.is_urgent() {
        TransportType::Direct  // Faster confirmation
    } else if order.value > 1000.0 {
        TransportType::Relayer // Better for high-value transactions
    } else {
        factory.recommend_transport(ExecutionUseCase::Production)
    }
}
```

### 2. Fallback with Retry Logic

```rust
async fn execute_with_retry(
    factory: &SmartExecutionEngineFactory,
    preferred: TransportType,
    // ... transaction params
) -> Result<String> {
    match factory.create_engine_with_fallback(preferred) {
        Ok(executor) => {
            match executor.send_transaction(/* params */).await {
                Ok(ExecutionResponse::Immediate(hash)) => Ok(hash),
                Ok(ExecutionResponse::Async { request_id, .. }) => {
                    // Wait for async completion
                    wait_for_async_completion(&executor, &request_id).await
                }
                Err(e) => {
                    // Try fallback transport
                    let fallback = if preferred == TransportType::Direct {
                        TransportType::Relayer
                    } else {
                        TransportType::Direct
                    };
                    
                    let fallback_executor = factory.create_engine(fallback)?;
                    // Retry with fallback...
                    Err(e)
                }
            }
        }
        Err(e) => Err(e),
    }
}
```

### 3. Webhook Integration

```rust
// For async relayer execution
pub struct WebhookHandler {
    pending_transactions: Arc<RwLock<HashMap<String, PendingTx>>>,
}

#[async_trait]
impl AsyncResponseHandler for WebhookHandler {
    async fn handle_response(&self, request_id: String, response: AsyncResponse) -> Result<()> {
        let mut pending = self.pending_transactions.write().await;
        if let Some(tx) = pending.get_mut(&request_id) {
            tx.status = response.status;
            tx.transaction_hash = response.transaction_hash;
            
            // Notify waiting tasks
            tx.notify_completion();
        }
        Ok(())
    }
    
    async fn start_listener(&self) -> Result<()> {
        // Start HTTP server for webhook endpoint
        // Implementation would use actix-web or similar
        Ok(())
    }
}
```

## Benefits of This Architecture

### 1. **Flexibility**
- Easy switching between direct and relayer execution
- Support for multiple relayer providers
- Runtime transport selection based on conditions

### 2. **Maintainability**
- Clear separation of concerns
- Modular design allows independent updates
- Comprehensive test coverage for each component

### 3. **Production Readiness**
- Built-in error handling and retries (relayer)
- Professional gas management (relayer)
- Async execution support with status tracking

### 4. **Developer Experience**
- Consistent API across all transport types
- Rich configuration options
- Comprehensive logging and debugging support

### 5. **Extensibility**
- Easy to add new transport types
- Plugin-like architecture for custom implementations
- Future-proof design for emerging technologies

## Migration Guide

### From Current Alloy-Only Implementation

1. **Update imports:**
   ```rust
   use crate::contracts::execution::{
       ExecutionEngineConfigBuilder, DefaultExecutionEngineFactory,
       TransportType, ExecutionContext, ExecutionPriority
   };
   ```

2. **Replace direct executor creation:**
   ```rust
   // Old
   let executor = AlloyExecutor::new(config)?;
   
   // New
   let factory_config = ExecutionEngineConfigBuilder::new()
       .with_app_config(config)
       .with_wallet_address(wallet_address)
       .build()?;
   let factory = DefaultExecutionEngineFactory::new(factory_config);
   let executor = factory.create_engine(TransportType::Direct)?;
   ```

3. **Update transaction calls:**
   ```rust
   // Old
   let tx_hash = executor.send_transaction(chain, data, to, gas).await?;
   
   // New
   let response = executor.send_transaction(chain, data, to, gas, None).await?;
   let tx_hash = match response {
       ExecutionResponse::Immediate(hash) => hash,
       ExecutionResponse::Async { .. } => {
           // Handle async case
           return Err(anyhow::anyhow!("Unexpected async response"));
       }
   };
   ```

4. **Add relayer support (optional):**
   ```rust
   let factory_config = ExecutionEngineConfigBuilder::new()
       .with_app_config(config)
       .with_relayer_config(relayer_config)  // Add this
       .with_wallet_address(wallet_address)
       .with_default_transport(TransportType::Relayer)  // Change this
       .build()?;
   ```

## Testing Strategy

### Unit Tests
- Each executor implementation
- Factory creation logic
- Configuration builders

### Integration Tests
- End-to-end transaction flows
- Fallback mechanisms
- Async status tracking

### Performance Tests
- Direct vs relayer execution times
- Memory usage with concurrent executions
- Network resilience testing

## Configuration Examples

### Environment-Based Configuration

```rust
fn create_execution_config() -> Result<ExecutionEngineConfig> {
    let transport = match std::env::var("EXECUTION_TRANSPORT").as_deref() {
        Ok("relayer") => TransportType::Relayer,
        Ok("direct") => TransportType::Direct,
        _ => TransportType::Direct, // Default
    };
    
    let mut builder = ExecutionEngineConfigBuilder::new()
        .with_app_config(app_config)
        .with_wallet_address(wallet_address)
        .with_default_transport(transport);
    
    if transport == TransportType::Relayer {
        let relayer_config = RelayerConfig {
            api_base_url: std::env::var("OZ_RELAYER_URL")?,
            api_key: std::env::var("OZ_API_KEY")?,
            // ... other config
        };
        builder = builder.with_relayer_config(relayer_config);
    }
    
    builder.build()
}
```

### Multi-Environment Support

```rust
// config/development.toml
[execution]
transport = "direct"
enable_fallback = false

# config/production.toml
[execution]
transport = "relayer"
enable_fallback = true

[relayer]
api_base_url = "https://api.defender.openzeppelin.com/relay"
timeout_seconds = 300
use_async = true
```

This architecture provides a robust, flexible foundation for blockchain transaction execution that can adapt to different requirements, environments, and use cases while maintaining a consistent developer experience. 