use crate::contracts::execution::traits::{
    ExecutionEngine, GasParams, ChainType, TransportType, ExecutionContext, 
    ExecutionResponse, AsyncStatus, RelayerConfig, AsyncResponse, AsyncResponseHandler
};
use alloy::primitives::Address;
use anyhow::Result;
use async_trait::async_trait;
use reqwest::{Client, header::HeaderMap};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::RwLock;
use tracing::{info, error, warn, debug};
use uuid::Uuid;

/// OpenZeppelin relayer API request structures
#[derive(Debug, Clone, Serialize)]
struct RelayRequest {
    pub to: String,
    pub data: String,
    pub gas_limit: u64,
    pub gas_price: u64,
    pub speed: Option<String>,
    pub value: String,
    pub is_private: bool,
}

#[derive(Debug, Clone, Deserialize)]
struct RelayResponse {
    pub transaction_id: String,
    pub hash: Option<String>,
    pub status: String,
    pub gas_price: Option<String>,
    pub gas_limit: Option<String>,
    pub to: String,
}

#[derive(Debug, Clone, Deserialize)]
struct RelayStatusResponse {
    pub transaction_id: String,
    pub hash: Option<String>,
    pub status: String,
    pub block_number: Option<u64>,
    pub gas_used: Option<u64>,
    pub error: Option<String>,
}

/// Speed settings for OpenZeppelin relayer
#[derive(Debug, Clone)]
pub enum RelaySpeed {
    Safest,
    Average,
    Fast,
    Fastest,
}

impl RelaySpeed {
    fn as_str(&self) -> &str {
        match self {
            RelaySpeed::Safest => "safest",
            RelaySpeed::Average => "average", 
            RelaySpeed::Fast => "fast",
            RelaySpeed::Fastest => "fastest",
        }
    }
}

/// OpenZeppelin Relayer Executor implementation
pub struct OpenZeppelinExecutor {
    config: Arc<RelayerConfig>,
    client: Client,
    pending_requests: Arc<RwLock<HashMap<String, PendingRequest>>>,
    wallet_address: Address,
}

#[derive(Debug, Clone)]
struct PendingRequest {
    request_id: String,
    chain: ChainType,
    to: Address,
    call_data: Vec<u8>,
    status: AsyncStatus,
    transaction_hash: Option<String>,
    created_at: u64,
}

impl OpenZeppelinExecutor {
    /// Create a new OpenZeppelin relayer executor
    pub fn new(config: Arc<RelayerConfig>, wallet_address: Address) -> Result<Self> {
        info!("🔧 Initializing OpenZeppelinExecutor");
        info!("  API Base URL: {}", config.api_base_url);
        info!("  Supported chains: {:?}", config.chain_endpoints.keys().collect::<Vec<_>>());
        info!("  Async mode: {}", config.use_async);
        info!("  Wallet address: {}", wallet_address);

        // Create HTTP client with default headers
        let mut headers = HeaderMap::new();
        headers.insert("Authorization", format!("Bearer {}", config.api_key).parse()
            .map_err(|e| anyhow::anyhow!("Invalid API key format: {}", e))?);
        headers.insert("Content-Type", "application/json".parse()
            .map_err(|e| anyhow::anyhow!("Invalid content type: {}", e))?);

        let client = Client::builder()
            .default_headers(headers)
            .timeout(Duration::from_secs(config.timeout_seconds))
            .build()
            .map_err(|e| anyhow::anyhow!("Failed to create HTTP client: {}", e))?;

        info!("✅ OpenZeppelinExecutor initialized");

        Ok(Self {
            config,
            client,
            pending_requests: Arc::new(RwLock::new(HashMap::new())),
            wallet_address,
        })
    }

    /// Get the endpoint for a specific chain
    fn get_chain_endpoint(&self, chain: ChainType, chain_id: u64) -> Result<String> {
        let chain_endpoint = self.config.chain_endpoints.get(&chain_id)
            .ok_or_else(|| anyhow::anyhow!("No endpoint configured for chain {}", chain_id))?;
        
        Ok(format!("{}/{}/transactions", self.config.api_base_url.trim_end_matches('/'), chain_endpoint))
    }

    /// Map ChainType to chain ID (this should come from configuration)
    fn get_chain_id(&self, chain: ChainType) -> Result<u64> {
        // This should be configurable, but for now we'll use common chain IDs
        match chain {
            ChainType::Origin => Ok(31337), // Ethereum mainnet
            ChainType::Destination => Ok(31338), // Polygon
        }
    }

    /// Convert execution priority to relay speed
    fn priority_to_speed(priority: &crate::contracts::execution::traits::ExecutionPriority) -> RelaySpeed {
        match priority {
            crate::contracts::execution::traits::ExecutionPriority::Critical => RelaySpeed::Fastest,
            crate::contracts::execution::traits::ExecutionPriority::High => RelaySpeed::Fast,
            crate::contracts::execution::traits::ExecutionPriority::Normal => RelaySpeed::Average,
            crate::contracts::execution::traits::ExecutionPriority::Low => RelaySpeed::Safest,
        }
    }

    /// Create relay request payload
    fn create_relay_request(
        &self,
        call_data: Vec<u8>,
        to: Address,
        gas: GasParams,
        context: Option<&ExecutionContext>,
    ) -> RelayRequest {
        let speed = context
            .map(|ctx| Self::priority_to_speed(&ctx.priority))
            .unwrap_or(RelaySpeed::Average);

        RelayRequest {
            to: format!("0x{:x}", to),
            data: format!("0x{}", hex::encode(call_data)),
            gas_limit: gas.gas_limit,
            gas_price: gas.gas_price,
            speed: None,
            value: "0".to_string(), // Default to 0 ETH value for contract calls
            is_private: false, // Could be configurable
        }
    }

    /// Send relay request to OpenZeppelin API
    async fn send_relay_request(
        &self,
        endpoint: &str,
        request: &RelayRequest,
    ) -> Result<RelayResponse> {
        debug!("🚀 Sending relay request to: {}", endpoint);
        debug!("  Request: {:?}", request);

        let response = self.client
            .post(endpoint)
            .json(request)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to send relay request: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow::anyhow!("Relay request failed with status {}: {}", status, error_text));
        }

        let relay_response: RelayResponse = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse relay response: {}", e))?;

        debug!("✅ Relay request successful: {:?}", relay_response);
        Ok(relay_response)
    }

    /// Poll for transaction status
    async fn poll_transaction_status(
        &self,
        endpoint: &str,
        transaction_id: &str,
    ) -> Result<RelayStatusResponse> {
        // Remove /transactions from endpoint and add /{transaction_id} for status
        let base_endpoint = endpoint.strip_suffix("/transactions").unwrap_or(endpoint);
        let status_endpoint = format!("{}/transactions/{}", base_endpoint, transaction_id);
        
        debug!("🔍 Polling transaction status: {}", status_endpoint);

        let response = self.client
            .get(&status_endpoint)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to poll transaction status: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let error_text = response.text().await.unwrap_or_else(|_| "Unknown error".to_string());
            return Err(anyhow::anyhow!("Status polling failed with status {}: {}", status, error_text));
        }

        let status_response: RelayStatusResponse = response
            .json()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to parse status response: {}", e))?;

        debug!("📊 Transaction status: {:?}", status_response);
        Ok(status_response)
    }

    /// Convert OpenZeppelin status to our AsyncStatus
    fn map_status(oz_status: &str) -> AsyncStatus {
        match oz_status.to_lowercase().as_str() {
            "pending" | "queued" => AsyncStatus::Queued,
            "processing" | "submitted" => AsyncStatus::Processing,
            "mined" | "confirmed" => AsyncStatus::Confirmed,
            "failed" | "error" => AsyncStatus::Failed,
            _ => AsyncStatus::Queued, // Default to queued for unknown statuses
        }
    }

    /// Wait for transaction completion (polling-based)
    async fn wait_for_completion(
        &self,
        endpoint: &str,
        transaction_id: &str,
        timeout_seconds: u64,
    ) -> Result<RelayStatusResponse> {
        let start_time = std::time::Instant::now();
        let timeout_duration = Duration::from_secs(timeout_seconds);
        
        loop {
            if start_time.elapsed() > timeout_duration {
                return Err(anyhow::anyhow!("Transaction polling timed out after {} seconds", timeout_seconds));
            }

            let status = self.poll_transaction_status(endpoint, transaction_id).await?;
            
            match status.status.to_lowercase().as_str() {
                "mined" | "confirmed" => {
                    info!("✅ Transaction confirmed: {}", transaction_id);
                    return Ok(status);
                }
                "failed" | "error" => {
                    error!("❌ Transaction failed: {}", transaction_id);
                    return Err(anyhow::anyhow!("Transaction failed: {:?}", status.error));
                }
                _ => {
                    debug!("⏳ Transaction still processing: {} ({})", transaction_id, status.status);
                    tokio::time::sleep(Duration::from_secs(5)).await; // Poll every 5 seconds
                }
            }
        }
    }
}

#[async_trait]
impl ExecutionEngine for OpenZeppelinExecutor {
    async fn send_transaction(
        &self,
        chain: ChainType,
        call_data: Vec<u8>,
        to: Address,
        gas: GasParams,
        context: Option<ExecutionContext>,
    ) -> Result<ExecutionResponse> {
        info!("🚀 OpenZeppelinExecutor: Sending transaction via relayer");
        info!("  Chain: {:?}", chain);
        info!("  To: {}", to);
        info!("  Call data: {} bytes", call_data.len());
        info!("  Gas limit: {}", gas.gas_limit);
        info!("  Gas price: {}", gas.gas_price);

        // Get chain configuration
        let chain_id = self.get_chain_id(chain)?;
        let endpoint = self.get_chain_endpoint(chain, chain_id)?;

        // Create relay request
        let relay_request = self.create_relay_request(call_data.clone(), to, gas, context.as_ref());

        // Send relay request
        let relay_response = self.send_relay_request(&endpoint, &relay_request).await?;

        let request_id = relay_response.transaction_id.clone();

        // Store pending request
        let pending_request = PendingRequest {
            request_id: request_id.clone(),
            chain,
            to,
            call_data,
            status: Self::map_status(&relay_response.status),
            transaction_hash: relay_response.hash.clone(),
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        {
            let mut pending = self.pending_requests.write().await;
            pending.insert(request_id.clone(), pending_request);
        }

        // Determine response type based on configuration and context
        let use_async = self.config.use_async && context.as_ref().map_or(true, |ctx| ctx.timeout_seconds.is_some());

        if use_async {
            info!("📋 Transaction queued for async processing: {}", request_id);
            Ok(ExecutionResponse::Async {
                request_id,
                status: Self::map_status(&relay_response.status),
                estimated_completion: context.as_ref().and_then(|ctx| ctx.timeout_seconds).map(|t| {
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs() + t
                }),
            })
        } else {
            // Synchronous mode - wait for completion
            info!("⏳ Waiting for transaction completion (sync mode): {}", request_id);
            let timeout = context.as_ref().and_then(|ctx| ctx.timeout_seconds).unwrap_or(300);
            let final_status = self.wait_for_completion(&endpoint, &request_id, timeout).await?;
            
            let tx_hash = final_status.hash.ok_or_else(|| {
                anyhow::anyhow!("Transaction completed but no hash returned")
            })?;

            info!("✅ Transaction completed synchronously: {}", tx_hash);
            Ok(ExecutionResponse::Immediate(tx_hash))
        }
    }

    async fn static_call(&self, chain: ChainType, call_data: Vec<u8>, to: Address, from: Address) -> Result<Vec<u8>> {
        warn!("🔍 OpenZeppelinExecutor: Static calls not supported via relayer - this requires direct blockchain access");
        Err(anyhow::anyhow!("Static calls are not supported by OpenZeppelin relayer - use a direct executor for read operations"))
    }

    async fn estimate_gas(&self, chain: ChainType, call_data: Vec<u8>, to: Address, from: Address) -> Result<u64> {
        warn!("⛽ OpenZeppelinExecutor: Gas estimation not supported via relayer - using default values");
        // For relayers, gas estimation would typically be handled by the relayer service
        // We return a reasonable default or could make an API call to the relayer's estimation endpoint
        Ok(200_000) // Default gas limit
    }

    fn wallet_address(&self) -> Address {
        self.wallet_address
    }

    fn description(&self) -> &str {
        "OpenZeppelinExecutor: Uses OpenZeppelin relayer HTTP API for blockchain transaction execution"
    }

    fn transport_type(&self) -> TransportType {
        TransportType::Relayer
    }

    async fn check_async_status(&self, request_id: &str) -> Result<AsyncStatus> {
        debug!("🔍 Checking async status for request: {}", request_id);

        // First check our local cache
        {
            let pending = self.pending_requests.read().await;
            if let Some(request) = pending.get(request_id) {
                if matches!(request.status, AsyncStatus::Confirmed | AsyncStatus::Failed) {
                    return Ok(request.status.clone());
                }
            }
        }

        // If not in cache or still pending, poll the relayer
        let pending_request = {
            let pending = self.pending_requests.read().await;
            pending.get(request_id).cloned()
        };

        if let Some(request) = pending_request {
            let chain_id = self.get_chain_id(request.chain)?;
            let endpoint = self.get_chain_endpoint(request.chain, chain_id)?;
            
            match self.poll_transaction_status(&endpoint, request_id).await {
                Ok(status_response) => {
                    let new_status = Self::map_status(&status_response.status);
                    
                    // Update our cache
                    {
                        let mut pending = self.pending_requests.write().await;
                        if let Some(cached_request) = pending.get_mut(request_id) {
                            cached_request.status = new_status.clone();
                            cached_request.transaction_hash = status_response.hash;
                        }
                    }
                    
                    Ok(new_status)
                }
                Err(e) => {
                    warn!("Failed to poll status for {}: {}", request_id, e);
                    Ok(AsyncStatus::Processing) // Assume still processing if we can't get status
                }
            }
        } else {
            Err(anyhow::anyhow!("Unknown request ID: {}", request_id))
        }
    }

    async fn get_transaction_hash(&self, request_id: &str) -> Result<Option<String>> {
        debug!("🔍 Getting transaction hash for request: {}", request_id);

        let pending = self.pending_requests.read().await;
        if let Some(request) = pending.get(request_id) {
            Ok(request.transaction_hash.clone())
        } else {
            Err(anyhow::anyhow!("Unknown request ID: {}", request_id))
        }
    }
}

/// Helper struct for webhook-based async response handling
pub struct OpenZeppelinWebhookHandler {
    pending_requests: Arc<RwLock<HashMap<String, PendingRequest>>>,
    webhook_port: u16,
}

#[async_trait]
impl AsyncResponseHandler for OpenZeppelinWebhookHandler {
    async fn handle_response(&self, request_id: String, response: AsyncResponse) -> Result<()> {
        info!("📨 Received webhook response for request: {}", request_id);
        
        let mut pending = self.pending_requests.write().await;
        if let Some(request) = pending.get_mut(&request_id) {
            request.status = response.status;
            request.transaction_hash = response.transaction_hash;
            
            info!("✅ Updated request {} status: {:?}", request_id, request.status);
        } else {
            warn!("⚠️ Received response for unknown request: {}", request_id);
        }
        
        Ok(())
    }

    async fn start_listener(&self) -> Result<()> {
        info!("🎧 Starting webhook listener on port {}", self.webhook_port);
        // Implementation would start an HTTP server to receive webhooks
        // This is a simplified example - in production you'd use a proper web framework
        Ok(())
    }

    async fn stop_listener(&self) -> Result<()> {
        info!("🛑 Stopping webhook listener");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    fn create_test_config() -> RelayerConfig {
        let mut chain_endpoints = HashMap::new();
        chain_endpoints.insert(1, "ethereum".to_string());
        chain_endpoints.insert(137, "polygon".to_string());

        RelayerConfig {
            api_base_url: "https://api.defender.openzeppelin.com/relay".to_string(),
            api_key: "test-api-key".to_string(),
            webhook_url: Some("https://my-app.com/webhook".to_string()),
            chain_endpoints,
            timeout_seconds: 300,
            max_retries: 3,
            use_async: true,
        }
    }

    #[test]
    fn test_openzeppelin_executor_creation() {
        let config = create_test_config();
        let wallet_address = Address::from_str("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266")
            .expect("Valid test address");
        
        let result = OpenZeppelinExecutor::new(Arc::new(config), wallet_address);
        assert!(result.is_ok(), "OpenZeppelinExecutor creation should succeed: {:?}", result.err());
        
        let executor = result.unwrap();
        assert_eq!(executor.wallet_address(), wallet_address);
        assert_eq!(executor.transport_type(), TransportType::Relayer);
        
        println!("✅ OpenZeppelinExecutor created successfully");
    }

    #[test]
    fn test_priority_to_speed_mapping() {
        use crate::contracts::execution::traits::ExecutionPriority;
        
        assert_eq!(OpenZeppelinExecutor::priority_to_speed(&ExecutionPriority::Critical).as_str(), "fastest");
        assert_eq!(OpenZeppelinExecutor::priority_to_speed(&ExecutionPriority::High).as_str(), "fast");
        assert_eq!(OpenZeppelinExecutor::priority_to_speed(&ExecutionPriority::Normal).as_str(), "standard");
        assert_eq!(OpenZeppelinExecutor::priority_to_speed(&ExecutionPriority::Low).as_str(), "safest");
        
        println!("✅ Priority to speed mapping works correctly");
    }

    #[test]
    fn test_relay_request_creation() {
        let config = create_test_config();
        let wallet_address = Address::from_str("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266")
            .expect("Valid test address");
        let executor = OpenZeppelinExecutor::new(Arc::new(config), wallet_address).unwrap();
        
        let call_data = vec![0x01, 0x02, 0x03, 0x04];
        let to_address = Address::from_str("0x5FC8d32690cc91D4c39d9d3abcBD16989F875707")
            .expect("Valid address");
        let gas_params = GasParams {
            gas_limit: 650000,
            gas_price: 1178761408,
        };
        
        let context = ExecutionContext {
            priority: crate::contracts::execution::traits::ExecutionPriority::High,
            ..Default::default()
        };
        
        let request = executor.create_relay_request(call_data.clone(), to_address, gas_params, Some(&context));
        
        assert_eq!(request.to, "0x5fc8d32690cc91d4c39d9d3abcbd16989f875707");
        assert_eq!(request.data, "0x01020304");
        assert_eq!(request.gas_limit, 650000);
        assert_eq!(request.gas_price, 1178761408);
        assert_eq!(request.speed, None);
        assert_eq!(request.value, "0");
        
        println!("✅ Relay request creation works correctly");
    }
} 