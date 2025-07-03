# OIF Solver Rust POC

A Rust implementation of the OIF Protocol Solver

## 🚀 Quick Start

```bash
# Build the project
cargo build

# Run with default configuration
cargo run

# Run in development mode with auto-reload
cargo watch -x run
```

## 🏗️ Architecture

```
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   Origin Chain  │    │  Rust Solver    │    │ Destination Chain│
│   (TheCompact)  │◄──►│  (actix-web)    │◄──►│  (CoinFiller)   │
└─────────────────┘    └─────────────────┘    └─────────────────┘
```

### Technology Stack

- **HTTP Server**: `actix-web` - High-performance async web framework
- **Blockchain**: `alloy` - Modern Ethereum library (simplified for POC)
- **Async Runtime**: `tokio` - Industry-standard async runtime
- **Serialization**: `serde` - High-performance serialization
- **Configuration**: `config` - Flexible configuration management
- **Logging**: `tracing` - Structured, contextual logging

## 📡 API Endpoints

| Method | Path                           | Description                    |
|--------|--------------------------------|--------------------------------|
| GET    | `/`                           | API information                |
| GET    | `/api/v1/health`              | Health check                   |
| POST   | `/api/v1/orders`              | Submit new order               |
| GET    | `/api/v1/orders/{id}`         | Get order status               |
| POST   | `/api/v1/orders/{id}/finalize`| Manual finalization            |
| GET    | `/api/v1/queue`               | View processing queue          |

## 🔧 Configuration

### Configuration File (Recommended)

Edit `config/local.toml`:

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

### Environment Variables

```bash
export SOLVER_PRIVATE_KEY="0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a"
export ORIGIN_RPC_URL="http://localhost:8545"
export DESTINATION_RPC_URL="http://localhost:8546"
```

## 🔄 Order Lifecycle

```
pending → processing → filled → finalized
                    ↘  failed
```

### Event-Driven Processing

- **Order Submission**: Automatic processing of pending orders
- **Cross-Chain Fill**: Automatic execution on destination chain
- **Finalization**: Automatic settlement on origin chain
- **Manual Override**: Manual finalization via API endpoint

## 📊 Order Submission Example

```bash
curl -X POST http://localhost:3000/api/v1/orders \
  -H "Content-Type: application/json" \
  -d '{
    "order": {
      "nonce": "1",
      "maker": "0x...",
      "input_token": "0x...",
      "input_amount": "1000000000000000000",
      "output_token": "0x...",
      "output_amount": "1000000000000000000",
      "expiry": 1700000000,
      "origin_chain_id": 31337,
      "destination_chain_id": 31338
    },
    "signature": "0x..."
  }'
```

## 🔍 Monitoring

### Check Order Status

```bash
curl http://localhost:3000/api/v1/orders/{order_id}
```

### View Queue Status

```bash
curl http://localhost:3000/api/v1/queue
```

### Health Check

```bash
curl http://localhost:3000/api/v1/health
```

## 🛠️ Development

### Available Commands

```bash
cargo build           # Compile the project
cargo run              # Run the application
cargo test             # Run tests
cargo check            # Quick syntax check
cargo clippy           # Linting
cargo fmt              # Code formatting
```

### Development with Auto-Reload

```bash
# Install cargo-watch
cargo install cargo-watch

# Run with auto-reload
cargo watch -x run
```

## 📁 Project Structure

```
src/
├── main.rs                 # Application entry point
├── server.rs               # HTTP server (actix-web)
├── config.rs               # Configuration management
├── models/
│   ├── order.rs           # Order data structures
│   └── mandate.rs         # Mandate outputs
├── services/
│   ├── cross_chain.rs     # Cross-chain execution
│   ├── finalization.rs    # Order finalization
│   └── monitoring.rs      # Event-driven processing
├── contracts/
│   └── factory.rs         # Contract interactions (simplified)
├── storage/
│   └── memory.rs          # In-memory order storage
└── handlers/
    ├── health.rs          # Health endpoint
    ├── orders.rs          # Order endpoints
    └── queue.rs           # Queue status endpoint
```

## 🎯 Key Features

### ✅ Implemented (POC Complete)

- **HTTP API**:
- **Order Management**: Submit, track, and finalize orders
- **Event-Driven Processing**: Automatic order lifecycle management
- **In-Memory Storage**: Fast order storage and retrieval
- **Configuration**: Simple TOML-based configuration
- **Logging**: Structured logging with tracing
- **Error Handling**: Comprehensive error handling
- **CORS Support**: Cross-origin resource sharing

### 🎯 POC Simplifications

- **Contract Interactions**: Simulated blockchain transactions
- **No Retry Logic**: Simple error handling as requested
- **No Persistence**: In-memory storage only
- **Simplified Validation**: Basic order validation

## 🚦 Getting Started

1. **Prerequisites**
   ```bash
   # Install Rust
   curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
   
   # Verify installation
   cargo --version
   ```

2. **Clone and Build**
   ```bash
   git clone <repository>
   cd oif-solver-rust
   cargo build
   ```

3. **Configuration**
   - Edit `config/local.toml` with your settings
   - Or set environment variables

4. **Run**
   ```bash
   cargo run
   ```

5. **Test**
   ```bash
   curl http://localhost:3000/api/v1/health
   ```

## 🔮 Future Enhancements

For production deployment, consider:

- **Real Blockchain Integration**: Full alloy implementation
- **Persistent Storage**: Database integration
- **Retry Logic**: Robust error handling and retries
- **Monitoring**: Metrics and observability
- **Security**: Authentication and authorization
- **Performance**: Connection pooling and caching
