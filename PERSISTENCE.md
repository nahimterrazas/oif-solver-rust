# Data Persistence Feature

This document explains how the data persistence feature works in the OIF Solver Rust application.

## Overview

The persistence feature allows the server to save all orders/data to a JSON file before shutdown and restore the state when starting up again. This ensures that no data is lost when the server is restarted.

## Configuration

The persistence feature is configured in the `config/local.toml` file:

```toml
[persistence]
enabled = true
data_file = "data/orders.json"
```

- `enabled`: Whether persistence is enabled (true/false)
- `data_file`: Path to the JSON file where data will be saved/loaded

## How It Works

### On Startup

1. The server checks if persistence is enabled in the configuration
2. If enabled, it attempts to load data from the specified JSON file
3. If the file exists, all orders are loaded into memory storage
4. If the file doesn't exist, the server starts with empty storage
5. A log message shows how many orders were loaded

### During Operation

- All orders are stored in memory as usual
- The persistence file is not updated during operation for performance reasons
- Data is only written to disk during graceful shutdown

### On Shutdown

1. When the server receives a shutdown signal (SIGTERM, SIGINT, or Ctrl+C), it triggers graceful shutdown
2. If persistence is enabled, all orders are saved to the JSON file
3. The data directory is created automatically if it doesn't exist
4. A log message confirms how many orders were saved
5. The server then terminates

## File Format

The persistence file (`data/orders.json`) contains a JSON array of all orders:

```json
[
  {
    "id": "f47ac10b-58cc-4372-a567-0e02b2c3d479",
    "standard_order": {
      "user": "0x...",
      "nonce": 1,
      "originChainId": 31337,
      ...
    },
    "signature": "0x...",
    "status": "Pending",
    "created_at": "2024-01-01T00:00:00Z",
    "updated_at": "2024-01-01T00:00:00Z",
    "fill_tx_hash": null,
    "finalize_tx_hash": null,
    "error_message": null
  }
]
```

## Usage Examples

### Starting the Server

```bash
# Start the server - it will automatically load persisted data
cargo run
```

Expected log output:
```
INFO oif-solver-rust: Loading persisted data from: data/orders.json
INFO oif-solver-rust: Successfully loaded 5 orders from persistence file
```

### Stopping the Server

```bash
# Send shutdown signal (Ctrl+C or SIGTERM)
^C
```

Expected log output:
```
INFO oif-solver-rust: Received SIGINT
INFO oif-solver-rust: Shutting down server gracefully...
INFO oif-solver-rust: Saving data to file: data/orders.json
INFO oif-solver-rust: Successfully saved 5 orders to persistence file
INFO oif-solver-rust: Server shutdown complete
```

### Disabling Persistence

To disable persistence, set `enabled = false` in the configuration:

```toml
[persistence]
enabled = false
data_file = "data/orders.json"
```

When disabled:
- No data is loaded on startup
- No data is saved on shutdown
- The server operates with purely in-memory storage

## Error Handling

- If loading fails on startup, the server logs a warning and continues with empty storage
- If saving fails on shutdown, an error is logged but the server still terminates
- File permission errors, disk space issues, and JSON parsing errors are handled gracefully

## Implementation Details

### New Configuration

- Added `PersistenceConfig` struct to `src/config.rs`
- Configuration is loaded from TOML files and environment variables

### Storage Methods

Added to `MemoryStorage` in `src/storage/memory.rs`:
- `save_to_file()`: Saves all orders to JSON file
- `load_from_file()`: Loads orders from JSON file
- `count()`: Returns the number of stored orders

### Signal Handling

Modified `src/main.rs` to:
- Handle SIGTERM and SIGINT signals gracefully
- Save data before termination
- Use `tokio::select!` for concurrent operation

## Files Modified

- `src/config.rs`: Added persistence configuration
- `src/storage/memory.rs`: Added save/load methods
- `src/main.rs`: Added signal handling and startup/shutdown logic
- `config/local.toml`: Added persistence configuration section

## Testing

The persistence feature can be tested by:

1. Starting the server
2. Creating some orders via the API
3. Stopping the server with Ctrl+C
4. Checking that `data/orders.json` contains the orders
5. Restarting the server and verifying the orders are restored 