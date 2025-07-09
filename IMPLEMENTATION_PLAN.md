# OIF-Solver Rust POC - Implementation Plan

## 🎯 **Implementation Strategy**

This plan follows the principle of **incremental development** - build core functionality first, then add features step by step.

## 📋 **Phase 1: Project Setup & Foundation**

### **Step 1.1: Initialize Rust Project**
```bash
cargo init oif-solver-rust --bin
cd oif-solver-rust
```

### **Step 1.2: Setup Cargo.toml Dependencies**
Add core dependencies:
- `actix-web` - HTTP server
- `alloy` - Ethereum interactions
- `tokio` - Async runtime
- `serde` - JSON serialization
- `config` - Configuration management
- `tracing` - Logging
- `uuid` - Order ID generation

### **Step 1.3: Create Project Structure**
Set up the directory structure as defined in DESIGN.md

### **Step 1.4: Basic Configuration Setup**
- Create `src/config.rs` for configuration loading
- Create `config/local.toml` with development settings
- Implement simple configuration struct

---

## 📋 **Phase 2: Core Data Models**

### **Step 2.1: Define Order Models**
- `src/models/order.rs` - StandardOrder struct
- `src/models/mandate.rs` - MandateOutput struct  
- Order status enum and metadata structs

### **Step 2.2: Storage Layer**
- `src/storage/memory.rs` - In-memory order storage
- Simple HashMap-based storage with async access
- Order CRUD operations

---

## 📋 **Phase 3: Basic HTTP Server**

### **Step 3.1: Server Setup**
- `src/server.rs` - Actix-web server configuration
- Basic routing setup
- CORS and middleware configuration

### **Step 3.2: Health Check Endpoint**
- `src/handlers/health.rs` - Simple health check
- Test server is running correctly

### **Step 3.3: Main Entry Point**
- `src/main.rs` - Application startup
- Configuration loading
- Server initialization

---

## 📋 **Phase 4: Order Management Endpoints**

### **Step 4.1: Order Submission**
- `src/handlers/orders.rs` - POST `/api/v1/orders`
- JSON deserialization
- Basic validation
- Store order with "pending" status

### **Step 4.2: Order Status Query**
- GET `/api/v1/orders/:id` endpoint
- Retrieve order from storage
- Return order details and status

### **Step 4.3: Queue Status**
- `src/handlers/queue.rs` - GET `/api/v1/queue`
- Return list of pending/processing orders

---

## 📋 **Phase 5: Blockchain Integration**

### **Step 5.1: Contract Factory**
- `src/contracts/factory.rs` - Alloy provider setup
- Contract interface definitions
- Connection management

### **Step 5.2: Cross-Chain Service**
- `src/services/cross_chain.rs` - Fill execution
- CoinFiller contract interaction
- Transaction sending and confirmation

### **Step 5.3: Finalization Service**
- `src/services/finalization.rs` - Settlement
- SettlerCompact contract interaction
- Order completion logic

---

## 📋 **Phase 6: Event-Driven Processing**

### **Step 6.1: Event System**
- Internal event types (OrderReceived, OrderFilled, etc.)
- Tokio channels for event communication

### **Step 6.2: Order Monitoring Service**
- `src/services/monitoring.rs` - Background task
- Listen for order events
- Trigger appropriate actions

### **Step 6.3: Background Processing**
- Automatic order processing
- Event-driven state transitions
- Order lifecycle management

---

## 📋 **Phase 7: Manual Finalization**

### **Step 7.1: Finalization Endpoint**
- POST `/api/v1/orders/:id/finalize` endpoint
- Manual order finalization trigger
- Status updates

---

## 📋 **Phase 8: Integration & Testing**

### **Step 8.1: End-to-End Testing**
- Test complete order flow
- Verify API responses
- Validate blockchain interactions

### **Step 8.2: Configuration Validation**
- Test with different configurations
- Verify contract connections
- Validate RPC connectivity

---

## 🔧 **Detailed Implementation Steps**

### **Phase 1 Details**

#### **Step 1.2: Cargo.toml Setup**
```toml
[dependencies]
actix-web = "4.4"
alloy = { version = "0.3", features = ["full"] }
tokio = { version = "1.0", features = ["full"] }
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
config = "0.14"
tracing = "0.1"
tracing-subscriber = "0.3"
uuid = { version = "1.6", features = ["v4", "serde"] }
anyhow = "1.0"
```

#### **Step 1.4: Configuration Structure**
```rust
#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub solver: SolverConfig,
    pub chains: ChainConfig,
    pub contracts: ContractConfig,
}
```

### **Phase 2 Details**

#### **Step 2.1: Core Models**
```rust
// StandardOrder with alloy types
pub struct StandardOrder {
    pub nonce: U256,
    pub maker: Address,
    // ... other fields
}

// Order with metadata
pub struct Order {
    pub id: Uuid,
    pub standard_order: StandardOrder,
    pub signature: String,
    pub status: OrderStatus,
    pub created_at: SystemTime,
}
```

### **Phase 5 Details**

#### **Step 5.1: Alloy Integration**
```rust
// Provider setup
let provider = Provider::new(Http::new(rpc_url));

// Contract interface
sol! {
    interface CoinFiller {
        function fill(/* parameters */) external;
    }
}
```

## 🎯 **Success Criteria**

### **Minimum Viable Product (MVP)**
- ✅ HTTP server responds to all API endpoints
- ✅ Orders can be submitted and stored
- ✅ Cross-chain fill execution works
- ✅ Order finalization works
- ✅ Event-driven processing functional

### **POC Complete**
- ✅ Full order lifecycle working
- ✅ Same API as TypeScript version
- ✅ Event-driven architecture maintained
- ✅ Simple configuration system
- ✅ Basic error handling (no retry logic)

## ⏱️ **Estimated Timeline**

- **Phase 1-3**: 1-2 days (Foundation + HTTP server)
- **Phase 4**: 1 day (Order endpoints)
- **Phase 5**: 2-3 days (Blockchain integration)
- **Phase 6**: 1-2 days (Event processing)
- **Phase 7**: 0.5 days (Manual finalization)
- **Phase 8**: 1 day (Testing & validation)

**Total Estimated Time**: 6-9 days

## 🚀 **Quick Start Commands**

```bash
# Setup
cargo init oif-solver-rust --bin
cd oif-solver-rust

# Development
cargo check          # Quick syntax check
cargo build          # Compile
cargo run             # Run application
cargo test            # Run tests

# Production
cargo build --release
./target/release/oif-solver-rust
```

## 📝 **Implementation Notes**

1. **Start Simple**: Implement basic functionality first, add complexity gradually
2. **Test Early**: Test each phase before moving to the next
3. **Use Alloy Patterns**: Follow alloy documentation for blockchain interactions
4. **Actix-Web Patterns**: Use standard actix-web patterns for HTTP handling
5. **Error Handling**: Use `Result<T, E>` consistently, avoid panics
6. **Async First**: Design all I/O operations as async from the start

This plan ensures a working POC that maintains the same functionality as the TypeScript version while leveraging Rust's strengths. 