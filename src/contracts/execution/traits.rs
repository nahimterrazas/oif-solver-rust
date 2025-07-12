use alloy::primitives::Address;
use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Enum to specify which blockchain to execute transactions on
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainType {
    /// Origin chain (where SettlerCompact is deployed - for finalize operations)
    Origin,
    /// Destination chain (where CoinFiller is deployed - for fill operations)
    Destination,
}

#[derive(Debug, Clone)]
pub struct GasParams {
    pub gas_limit: u64,
    pub gas_price: u64,
}

/// Transport mechanism for transaction execution
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportType {
    /// Direct blockchain connection (Alloy, Web3, etc.)
    Direct,
    /// HTTP API relayer service (OpenZeppelin, etc.)
    Relayer,
    /// Future: Other transport mechanisms (gRPC, WebSocket, etc.)
    Custom(String),
}

/// Execution context containing additional metadata for relayers
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    /// Optional request ID for tracking async operations
    pub request_id: Option<String>,
    /// Additional metadata for relayers (API keys, endpoint configs, etc.)
    pub metadata: HashMap<String, String>,
    /// Priority level for transaction execution
    pub priority: ExecutionPriority,
    /// Maximum time to wait for async responses
    pub timeout_seconds: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecutionPriority {
    Low,
    Normal,
    High,
    Critical,
}

impl Default for ExecutionContext {
    fn default() -> Self {
        Self {
            request_id: None,
            metadata: HashMap::new(),
            priority: ExecutionPriority::Normal,
            timeout_seconds: Some(300), // 5 minutes default
        }
    }
}

/// Response from transaction execution - supports both sync and async patterns
#[derive(Debug, Clone)]
pub enum ExecutionResponse {
    /// Immediate response with transaction hash (Direct transport)
    Immediate(String),
    /// Async response with tracking information (Relayer transport)
    Async {
        request_id: String,
        status: AsyncStatus,
        estimated_completion: Option<u64>, // Unix timestamp
    },
}

impl std::fmt::Display for ExecutionResponse {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutionResponse::Immediate(hash) => write!(f, "Immediate({})", hash),
            ExecutionResponse::Async { request_id, status, estimated_completion } => {
                write!(f, "Async(request_id: {}, status: {:?}, estimated_completion: {:?})", 
                       request_id, status, estimated_completion)
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AsyncStatus {
    Queued,
    Processing,
    Submitted,
    Confirmed,
    Failed,
}

/// Configuration for relayer-based execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayerConfig {
    /// Base URL for the relayer API
    pub api_base_url: String,
    /// API key for authentication
    pub api_key: String,
    /// Optional webhook URL for async notifications
    pub webhook_url: Option<String>,
    /// Chain-specific endpoints
    pub chain_endpoints: HashMap<u64, String>, // chain_id -> endpoint
    /// Default timeout for API calls
    pub timeout_seconds: u64,
    /// Retry configuration
    pub max_retries: u32,
    /// Whether to use async execution (webhooks) or polling
    pub use_async: bool,
}

/// Enhanced execution engine trait supporting multiple transport types
#[async_trait]
pub trait ExecutionEngine: Send + Sync {
    /// Send a transaction to the specified blockchain
    /// 
    /// # Arguments
    /// * `chain` - Which blockchain to execute on (Origin or Destination)
    /// * `call_data` - The encoded function call data
    /// * `to` - The contract address to call
    /// * `gas` - Gas parameters for the transaction
    /// * `context` - Additional execution context for relayers
    async fn send_transaction(
        &self,
        chain: ChainType,
        call_data: Vec<u8>,
        to: Address,
        gas: GasParams,
        context: Option<ExecutionContext>,
    ) -> Result<ExecutionResponse>;
    
    /// Perform a static call (read-only) on the specified blockchain
    /// 
    /// # Arguments
    /// * `chain` - Which blockchain to call (Origin or Destination)
    /// * `call_data` - The encoded function call data
    /// * `to` - The contract address to call
    /// * `from` - The address to call from
    async fn static_call(&self, chain: ChainType, call_data: Vec<u8>, to: Address, from: Address) -> Result<Vec<u8>>;
    
    /// Estimate gas for a transaction on the specified blockchain
    /// 
    /// # Arguments
    /// * `chain` - Which blockchain to estimate on (Origin or Destination)
    /// * `call_data` - The encoded function call data
    /// * `to` - The contract address to call
    /// * `from` - The address to call from
    async fn estimate_gas(&self, chain: ChainType, call_data: Vec<u8>, to: Address, from: Address) -> Result<u64>;
    
    /// Get the wallet address used by this executor
    fn wallet_address(&self) -> Address;
    
    /// Get a human-readable description of this executor
    fn description(&self) -> &str;
    
    /// Get the transport type used by this executor
    fn transport_type(&self) -> TransportType;
    
    /// Check status of an async transaction (for relayer-based executors)
    async fn check_async_status(&self, request_id: &str) -> Result<AsyncStatus>;
    
    /// Get transaction hash from async request (for relayer-based executors)
    async fn get_transaction_hash(&self, request_id: &str) -> Result<Option<String>>;
}

/// Trait for handling async execution responses (webhooks, polling, etc.)
#[async_trait]
pub trait AsyncResponseHandler: Send + Sync {
    /// Handle an async response from a relayer
    async fn handle_response(&self, request_id: String, response: AsyncResponse) -> Result<()>;
    
    /// Start listening for async responses (e.g., webhook server)
    async fn start_listener(&self) -> Result<()>;
    
    /// Stop listening for async responses
    async fn stop_listener(&self) -> Result<()>;
}

/// Async response structure for relayers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AsyncResponse {
    pub request_id: String,
    pub status: AsyncStatus,
    pub transaction_hash: Option<String>,
    pub error: Option<String>,
    pub gas_used: Option<u64>,
    pub block_number: Option<u64>,
    pub timestamp: u64,
}

/// High-level abstract trait for order execution
/// 
/// This trait provides a complete abstraction for order processing,
/// hiding the complexity of encoding and low-level transaction execution.
#[async_trait]
pub trait OrderExecutor: Send + Sync {
    /// Execute finalization for an order (high-level interface)
    /// 
    /// # Arguments
    /// * `order` - The order to finalize
    /// * `context` - Optional execution context for relayers
    /// 
    /// # Returns
    /// * `Result<ExecutionResponse>` - The finalization response (immediate or async)
    async fn execute_finalization(
        &self,
        order: &crate::models::Order,
        context: Option<ExecutionContext>,
    ) -> Result<ExecutionResponse>;
    
    /// Estimate gas for finalization
    /// 
    /// # Arguments
    /// * `order` - The order to estimate gas for
    /// 
    /// # Returns
    /// * `Result<u64>` - The estimated gas amount
    async fn estimate_finalization_gas(&self, order: &crate::models::Order) -> Result<u64>;
    
    /// Get the wallet address used by this executor
    /// 
    /// # Returns
    /// * `Address` - The wallet address
    fn wallet_address(&self) -> Address;
    
    /// Get a human-readable description of this executor
    /// 
    /// # Returns
    /// * `&str` - Description of the executor implementation  
    fn description(&self) -> &str;
    
    /// Get the transport type used by this executor
    /// 
    /// # Returns
    /// * `TransportType` - The transport mechanism
    fn transport_type(&self) -> TransportType;
}

/// Factory trait for creating execution engines based on configuration
pub trait ExecutionEngineFactory: Send + Sync {
    /// Create an execution engine of the specified type
    fn create_engine(&self, transport: TransportType) -> Result<Box<dyn ExecutionEngine>>;
    
    /// List available transport types
    fn available_transports(&self) -> Vec<TransportType>;
    
    /// Check if a transport type is available
    fn supports_transport(&self, transport: &TransportType) -> bool;
} 