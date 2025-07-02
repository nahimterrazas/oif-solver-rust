# OIF-Solver Rust POC - Design Document

## 🎯 **Overview**

This is a Rust implementation of the OIF Protocol Solver POC, maintaining the same functionality as the TypeScript version but leveraging Rust's performance and safety features.

## 🏗️ **Architecture**

### **Core Components**
```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   Origin Chain  │    │  Rust Solver    │    │ Destination Chain│
│   (TheCompact)  │◄──►│  (actix-web)    │◄──►│  (CoinFiller)   │
└─────────────────┘    └─────────────────┘    └─────────────────┘
```

### **Technology Stack**
- **HTTP Server**: `actix-web` - Fast, secure web framework
- **Blockchain**: `alloy` - Modern Ethereum library
- **Async Runtime**: `tokio` - Async runtime
- **Serialization**: `serde` - JSON handling
- **Configuration**: `config` - Simple configuration management
- **Logging**: `tracing` - Structured logging

## 📁 **Project Structure**

```
oif-solver-rust/
├── Cargo.toml                  # Dependencies and metadata
├── src/
│   ├── main.rs                 # Application entry point
│   ├── server.rs               # Actix-web HTTP server
│   ├── config.rs               # Configuration management
│   ├── models/
│   │   ├── mod.rs
│   │   ├── order.rs           # StandardOrder struct
│   │   └── mandate.rs         # MandateOutput struct
│   ├── services/
│   │   ├── mod.rs
│   │   ├── cross_chain.rs     # Cross-chain execution
│   │   ├── finalization.rs    # Order finalization
│   │   └── monitoring.rs      # Order monitoring/queue
│   ├── contracts/
│   │   ├── mod.rs
│   │   └── factory.rs         # Contract factory using alloy
│   ├── storage/
│   │   ├── mod.rs
│   │   └── memory.rs          # In-memory order storage
│   └── handlers/
│       ├── mod.rs
│       ├── health.rs          # Health check endpoint
│       ├── orders.rs          # Order CRUD endpoints
│       └── queue.rs           # Queue status endpoint
├── config/
│   └── local.toml             # Local configuration
└── README.md
```

## 🔌 **API Endpoints** (Same as TypeScript)

| Method | Path                       | Description                    |
|--------|----------------------------|--------------------------------|
| GET    | `/api/v1/health`           | Health check                   |
| POST   | `/api/v1/orders`           | Submit new order               |
| GET    | `/api/v1/orders/:id`       | Get order status               |
| POST   | `/api/v1/orders/:id/finalize` | Manual finalization         |
| GET    | `/api/v1/queue`            | View processing queue          |

## 📊 **Data Models**

### **StandardOrder**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandardOrder {
    pub nonce: U256,
    pub maker: Address,
    pub input_token: Address,
    pub input_amount: U256,
    pub output_token: Address,
    pub output_amount: U256,
    pub expiry: u64,
    pub origin_chain_id: u64,
    pub destination_chain_id: u64,
}
```

### **OrderStatus**
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum OrderStatus {
    Pending,
    Processing,
    Filled,
    Finalized,
    Failed,
}
```

## 🔄 **Order Lifecycle**

```
pending → processing → filled → finalized
                    ↘  failed
```

## ⚙️ **Configuration**

Simple TOML configuration following the user's requirement for simplicity:

```toml
[server]
host = "0.0.0.0"
port = 3000

[solver]
private_key = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"

[chains.origin]
rpc_url = "http://localhost:8545"
chain_id = 31337

[chains.destination]
rpc_url = "http://localhost:8546"
chain_id = 31338

[contracts]
the_compact = "0x..."
settler_compact = "0x..."
coin_filler = "0x..."
```

## 🔀 **Event-Driven Processing**

Using Rust's async ecosystem:
- **Tokio channels** for internal event communication
- **Event loops** for monitoring blockchain events
- **Background tasks** for order processing

## 🎯 **Key Principles** (Following User Requirements)

1. **Simplicity**: Minimal complexity, focus on core functionality
2. **No Retry Logic**: Keep error handling simple
3. **Event-Driven**: Maintain event-driven architecture
4. **Same API**: Identical endpoints to TypeScript version
5. **POC Focus**: Prioritize working functionality over optimization

## 🚀 **Processing Flow**

1. **Order Submission**: POST `/api/v1/orders`
   - Validate order and signature
   - Store in memory
   - Emit `OrderReceived` event

2. **Cross-Chain Fill**: Background task
   - Listen for `OrderReceived` events
   - Execute fill on destination chain using alloy
   - Update order status to `Filled`

3. **Finalization**: Background task or manual trigger
   - Execute finalize on origin chain
   - Update order status to `Finalized`

## 🛠️ **Development Considerations**

- **Error Handling**: Simple error propagation with `Result<T, E>`
- **Logging**: Structured logging with `tracing`
- **Testing**: Unit tests for core components
- **Documentation**: Inline documentation for public APIs
- **Memory Safety**: Leverage Rust's ownership system
- **Performance**: Async/await for non-blocking operations 