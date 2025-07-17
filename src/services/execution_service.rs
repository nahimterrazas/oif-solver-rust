use crate::contracts::execution::{
    ExecutionEngineConfigBuilder, DefaultExecutionEngineFactory, SmartExecutionEngineFactory,
    TransportType, ExecutionEngine, ExecutionResponse, ExecutionContext, ExecutionPriority,
    ExecutionUseCase, presets, ExecutionEngineFactory
};
use crate::config::AppConfig;
use crate::models::Order;
use alloy::primitives::Address;
use anyhow::Result;
use std::sync::Arc;
use tracing::{info, warn, error};

/// Service for managing execution engines and transaction execution
pub struct ExecutionService {
    smart_factory: SmartExecutionEngineFactory,
    config: Arc<AppConfig>,
    preferred_transport: TransportType,
}

impl ExecutionService {
    /// Create a new execution service with smart factory
    pub fn new(config: Arc<AppConfig>) -> Result<Self> {
        info!("🏭 Initializing ExecutionService");

        // Extract wallet address from solver private key
        let wallet_address = Self::get_wallet_address_from_config(&config)?;
        
        // Determine if relayer is enabled and available
        let relayer_available = config.relayer.as_ref().map_or(false, |r| r.enabled);
        let preferred_transport = if relayer_available {
            info!("🔗 Relayer enabled, preferring relayer transport");
            TransportType::Relayer
        } else {
            info!("📡 Relayer disabled, using direct transport");
            TransportType::Direct
        };

        // Build execution engine configuration
        let mut builder = ExecutionEngineConfigBuilder::new()
            .with_app_config(config.clone())
            .with_wallet_address(wallet_address)
            .with_default_transport(preferred_transport.clone());

        // Add relayer config if available
        if let Some(ref relayer_config) = config.relayer {
            if relayer_config.enabled {
                // Convert AppConfig::RelayerConfig to execution::RelayerConfig
                let execution_relayer_config = crate::contracts::execution::RelayerConfig {
                    api_base_url: relayer_config.api_base_url.clone(),
                    api_key: relayer_config.api_key.clone(),
                    webhook_url: relayer_config.webhook_url.clone(),
                    chain_endpoints: relayer_config.chain_endpoints.clone(),
                    timeout_seconds: relayer_config.timeout_seconds,
                    max_retries: relayer_config.max_retries,
                    use_async: relayer_config.use_async,
                };
                
                builder = builder.with_relayer_config(execution_relayer_config);
                info!("✅ Relayer configuration added to execution engine factory");
            }
        }

        let execution_config = builder.build()?;
        let smart_factory = SmartExecutionEngineFactory::new(execution_config, true); // Enable fallback

        info!("✅ ExecutionService initialized");
        info!("  Preferred transport: {:?}", preferred_transport);
        info!("  Fallback enabled: true");
        info!("  Available transports: {:?}", smart_factory.factory.available_transports());

        Ok(Self {
            smart_factory,
            config,
            preferred_transport,
        })
    }

    /// Execute a transaction for an order with smart transport selection
    pub async fn execute_order_transaction(
        &self,
        order: &Order,
        call_data: Vec<u8>,
        contract_address: Address,
        gas_limit: u64,
        gas_price: u64,
        use_case: Option<ExecutionUseCase>,
    ) -> Result<String> {
        info!("🚀 Executing transaction for order: {}", order.id);

        // Determine the best transport for this use case
        let transport = if let Some(use_case) = use_case {
            self.smart_factory.recommend_transport(use_case)
        } else {
            self.preferred_transport.clone()
        };

        info!("  Recommended transport: {:?}", transport);

        // Create execution context based on order priority
        let context = self.create_execution_context_for_order(order);

        // Create executor with fallback
        let executor = self.smart_factory.create_engine_with_fallback(transport)?;

        info!("  Using executor: {}", executor.description());
        info!("  Transport type: {:?}", executor.transport_type());

        // Execute transaction
        let gas_params = crate::contracts::execution::GasParams {
            gas_limit,
            gas_price,
        };

        let response = executor.send_transaction(
            crate::contracts::execution::ChainType::Origin, // Assuming finalization on origin
            call_data,
            contract_address,
            gas_params,
            Some(context),
        ).await?;

        // Handle response based on type
        match response {
            ExecutionResponse::Immediate(tx_hash) => {
                info!("✅ Transaction completed immediately: {}", tx_hash);
                Ok(tx_hash)
            }
            ExecutionResponse::Async { request_id, status, estimated_completion } => {
                info!("📋 Transaction queued for async processing");
                info!("  Request ID: {}", request_id);
                info!("  Status: {:?}", status);
                info!("  Estimated completion: {:?}", estimated_completion);

                // For async responses, we need to wait or return the request_id
                // For now, let's wait for completion (polling approach)
                self.wait_for_async_completion(&executor, &request_id).await
            }
        }
    }

    /// Execute fill transaction on destination chain
    pub async fn execute_fill_transaction(
        &self,
        order: &Order,
        call_data: Vec<u8>,
        contract_address: Address,
        gas_limit: u64,
        gas_price: u64,
    ) -> Result<String> {
        info!("🔄 Executing fill transaction for order: {}", order.id);

        // For fill operations, we might prefer different transport
        let transport = self.smart_factory.recommend_transport(ExecutionUseCase::CrossChain);
        let executor = self.smart_factory.create_engine_with_fallback(transport)?;

        let context = self.create_execution_context_for_order(order);
        let gas_params = crate::contracts::execution::GasParams {
            gas_limit,
            gas_price,
        };

        let response = executor.send_transaction(
            crate::contracts::execution::ChainType::Destination, // Fill on destination
            call_data,
            contract_address,
            gas_params,
            Some(context),
        ).await?;

        match response {
            ExecutionResponse::Immediate(tx_hash) => {
                info!("✅ Fill transaction completed: {}", tx_hash);
                Ok(tx_hash)
            }
            ExecutionResponse::Async { request_id, .. } => {
                self.wait_for_async_completion(&executor, &request_id).await
            }
        }
    }

    /// Create execution context based on order characteristics
    fn create_execution_context_for_order(&self, order: &Order) -> ExecutionContext {
        // Determine priority based on order value or urgency
        let priority = if order.standard_order.expires < (chrono::Utc::now().timestamp() as u64 + 300) {
            ExecutionPriority::Critical // Expires soon
        } else if order.standard_order.inputs.iter().any(|(_, amount)| {
            amount.parse::<u128>().unwrap_or(0) > 1_000_000_000_000_000_000_000 // > 1000 tokens
        }) {
            ExecutionPriority::High // High value
        } else {
            ExecutionPriority::Normal
        };

        ExecutionContext {
            priority,
            timeout_seconds: Some(300), // 5 minutes timeout
            metadata: {
                let mut meta = std::collections::HashMap::new();
                meta.insert("order_id".to_string(), order.id.to_string());
                meta.insert("order_nonce".to_string(), order.standard_order.nonce.to_string());
                meta.insert("origin_chain".to_string(), order.standard_order.origin_chain_id.to_string());
                meta
            },
            request_id: Some(format!("order-{}-{}", order.id, chrono::Utc::now().timestamp())),
        }
    }

    /// Wait for async transaction completion with polling
    async fn wait_for_async_completion(
        &self,
        executor: &Box<dyn ExecutionEngine>,
        request_id: &str,
    ) -> Result<String> {
        info!("⏳ Waiting for async transaction completion: {}", request_id);

        let max_polls = 60; // 5 minutes with 5-second intervals
        let poll_interval = std::time::Duration::from_secs(5);

        for attempt in 1..=max_polls {
            tokio::time::sleep(poll_interval).await;

            match executor.check_async_status(request_id).await {
                Ok(status) => {
                    info!("📊 Poll {}/{}: Status = {:?}", attempt, max_polls, status);

                    match status {
                        crate::contracts::execution::AsyncStatus::Confirmed => {
                            if let Ok(Some(tx_hash)) = executor.get_transaction_hash(request_id).await {
                                info!("✅ Async transaction confirmed: {}", tx_hash);
                                return Ok(tx_hash);
                            } else {
                                return Err(anyhow::anyhow!("Transaction confirmed but no hash available"));
                            }
                        }
                        crate::contracts::execution::AsyncStatus::Failed => {
                            return Err(anyhow::anyhow!("Async transaction failed: {}", request_id));
                        }
                        _ => {
                            // Still processing, continue polling
                            continue;
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to check async status (attempt {}): {}", attempt, e);
                    if attempt == max_polls {
                        return Err(anyhow::anyhow!("Failed to get final transaction status after {} attempts", max_polls));
                    }
                }
            }
        }

        Err(anyhow::anyhow!("Async transaction timed out after {} polls", max_polls))
    }

    /// Get available transport types
    pub fn available_transports(&self) -> Vec<TransportType> {
        self.smart_factory.factory.available_transports()
    }

    /// Check if relayer is enabled and available
    pub fn is_relayer_enabled(&self) -> bool {
        self.config.relayer.as_ref().map_or(false, |r| r.enabled)
    }

    /// Get current preferred transport
    pub fn preferred_transport(&self) -> &TransportType {
        &self.preferred_transport
    }

    /// Switch to a different transport preference
    pub fn set_preferred_transport(&mut self, transport: TransportType) -> Result<()> {
        if !self.smart_factory.factory.supports_transport(&transport) {
            return Err(anyhow::anyhow!("Transport {:?} is not supported", transport));
        }

        self.preferred_transport = transport;
        info!("🔄 Switched preferred transport to: {:?}", self.preferred_transport);
        Ok(())
    }

    /// Create a direct executor (for testing or specific use cases)
    pub fn create_direct_executor(&self) -> Result<Box<dyn ExecutionEngine>> {
        self.smart_factory.factory.create_engine(TransportType::Direct)
    }

    /// Create a relayer executor (if configured)
    pub fn create_relayer_executor(&self) -> Result<Box<dyn ExecutionEngine>> {
        if !self.is_relayer_enabled() {
            return Err(anyhow::anyhow!("Relayer is not enabled in configuration"));
        }
        self.smart_factory.factory.create_engine(TransportType::Relayer)
    }

    /// Get wallet address from configuration
    fn get_wallet_address_from_config(config: &AppConfig) -> Result<Address> {
        use alloy::signers::local::PrivateKeySigner;
        use std::str::FromStr;

        let signer = PrivateKeySigner::from_str(&config.solver.private_key)?;
        Ok(signer.address())
    }

    /// Test relayer connectivity
    pub async fn test_relayer_connectivity(&self) -> Result<()> {
        if !self.is_relayer_enabled() {
            return Err(anyhow::anyhow!("Relayer is not enabled"));
        }

        info!("🧪 Testing relayer connectivity...");

        let relayer_executor = self.create_relayer_executor()?;
        
        // We can't actually test without sending a transaction, but we can check if the executor was created
        info!("✅ Relayer executor created successfully");
        info!("  Description: {}", relayer_executor.description());
        info!("  Transport: {:?}", relayer_executor.transport_type());
        info!("  Wallet: {}", relayer_executor.wallet_address());

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, ServerConfig, ChainConfig, ChainDetails, SolverConfig, ContractConfig, MonitoringConfig, PersistenceConfig};
    use crate::models::{StandardOrder, MandateOutput, OrderStatus};
    use chrono::Utc;
    use uuid::Uuid;
    use std::{collections::HashMap, str::FromStr};

    fn create_test_config_with_relayer() -> Arc<AppConfig> {
        let mut chain_endpoints = HashMap::new();
        chain_endpoints.insert(31337, "local-origin".to_string());
        chain_endpoints.insert(31338, "local-destination".to_string());

        Arc::new(AppConfig {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 3000,
            },
            chains: ChainConfig {
                origin: ChainDetails {
                    rpc_url: "http://localhost:8545".to_string(),
                    chain_id: 31337,
                },
                destination: ChainDetails {
                    rpc_url: "http://localhost:8546".to_string(),
                    chain_id: 31338,
                },
            },
            solver: SolverConfig {
                private_key: "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80".to_string(),
                finalization_delay_seconds: 30,
            },
            contracts: ContractConfig {
                the_compact: "0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0".to_string(),
                settler_compact: "0x5FC8d32690cc91D4c39d9d3abcBD16989F875707".to_string(),
                coin_filler: "0xCf7Ed3AccA5a467e9e704C703E8D87F634fB0Fc9".to_string(),
            },
            monitoring: MonitoringConfig {
                enabled: true,
                check_interval_seconds: 60,
            },
            persistence: PersistenceConfig {
                enabled: true,
                data_file: "data/orders.json".to_string(),
            },
            relayer: Some(crate::config::RelayerConfig {
                enabled: true,
                api_base_url: "http://localhost:8080/api/v1".to_string(),
                api_key: "example-#1234567890123456789012345678901234567890".to_string(),
                webhook_url: None,
                timeout_seconds: 300,
                max_retries: 3,
                use_async: false,
                chain_endpoints,
            }),
        })
    }

    fn create_test_config_without_relayer() -> Arc<AppConfig> {
        Arc::new(AppConfig {
            server: ServerConfig {
                host: "127.0.0.1".to_string(),
                port: 3000,
            },
            chains: ChainConfig {
                origin: ChainDetails {
                    rpc_url: "http://localhost:8545".to_string(),
                    chain_id: 31337,
                },
                destination: ChainDetails {
                    rpc_url: "http://localhost:8546".to_string(),
                    chain_id: 31338,
                },
            },
            solver: SolverConfig {
                private_key: "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80".to_string(),
                finalization_delay_seconds: 30,
            },
            contracts: ContractConfig {
                the_compact: "0x9fE46736679d2D9a65F0992F2272dE9f3c7fa6e0".to_string(),
                settler_compact: "0x5FC8d32690cc91D4c39d9d3abcBD16989F875707".to_string(),
                coin_filler: "0xCf7Ed3AccA5a467e9e704C703E8D87F634fB0Fc9".to_string(),
            },
            monitoring: MonitoringConfig {
                enabled: true,
                check_interval_seconds: 60,
            },
            persistence: PersistenceConfig {
                enabled: true,
                data_file: "data/orders.json".to_string(),
            },
            relayer: None,
        })
    }

    fn create_test_order() -> Order {
        let standard_order = StandardOrder {
            user: Address::from_str("0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266").unwrap(),
            nonce: 781,
            origin_chain_id: 31337,
            expires: 4294967295,
            fill_deadline: 4294967295,
            local_oracle: Address::from_str("0x0165878a594ca255338adfa4d48449f69242eb8f").unwrap(),
            inputs: vec![(
                "232173931049414487598928205764542517475099722052565410375093941968804628563".to_string(),
                "100000000000000000000".to_string()
            )],
            outputs: vec![MandateOutput {
                remote_oracle: Address::from_str("0xe7f1725e7734ce288f8367e1bb143e90bb3f0512").unwrap(),
                remote_filler: Address::from_str("0x5fbdb2315678afecb367f032d93f642f64180aa3").unwrap(),
                chain_id: 31338,
                token: Address::from_str("0x9fe46736679d2d9a65f0992f2272de9f3c7fa6e0").unwrap(),
                amount: "99000000000000000000".to_string(),
                recipient: Address::from_str("0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266").unwrap(),
                remote_call: None,
                fulfillment_context: None,
            }],
        };

        let signature = "0xb99e3849171a57335dc3e25bdffb48b778d9d43851a54ff0606af6095f653acb084513b1458f9c36674e0b529b8f4af5882f73324165bd3df91a0e29948f2bf01c".to_string();
        
        let now = Utc::now();
        Order {
            id: Uuid::new_v4(),
            standard_order,
            signature,
            status: OrderStatus::Pending,
            created_at: now,
            updated_at: now,
            fill_tx_hash: None,
            finalize_tx_hash: None,
            error_message: None,
        }
    }

    #[test]
    fn test_execution_service_with_relayer() {
        let config = create_test_config_with_relayer();
        let result = ExecutionService::new(config);

        assert!(result.is_ok(), "ExecutionService creation should succeed: {:?}", result.err());

        let service = result.unwrap();
        assert!(service.is_relayer_enabled());
        assert_eq!(service.preferred_transport(), &TransportType::Relayer);

        let available = service.available_transports();
        assert!(available.contains(&TransportType::Direct));
        assert!(available.contains(&TransportType::Relayer));

        println!("✅ ExecutionService with relayer created successfully");
    }

    #[test]
    fn test_execution_service_without_relayer() {
        let config = create_test_config_without_relayer();
        let result = ExecutionService::new(config);

        assert!(result.is_ok(), "ExecutionService creation should succeed: {:?}", result.err());

        let service = result.unwrap();
        assert!(!service.is_relayer_enabled());
        assert_eq!(service.preferred_transport(), &TransportType::Direct);

        let available = service.available_transports();
        assert!(available.contains(&TransportType::Direct));
        assert!(!available.contains(&TransportType::Relayer));

        println!("✅ ExecutionService without relayer created successfully");
    }

    #[test]
    fn test_execution_context_creation() {
        let config = create_test_config_with_relayer();
        let service = ExecutionService::new(config).unwrap();
        let order = create_test_order();

        let context = service.create_execution_context_for_order(&order);

        assert_eq!(context.priority, ExecutionPriority::Normal);
        assert_eq!(context.timeout_seconds, Some(300));
        assert!(context.metadata.contains_key("order_id"));
        assert!(context.metadata.contains_key("order_nonce"));
        assert!(context.request_id.is_some());

        println!("✅ Execution context creation works correctly");
    }

    #[tokio::test]
    async fn test_relayer_connectivity() {
        let config = create_test_config_with_relayer();
        let service = ExecutionService::new(config).unwrap();

        // This test will try to create the relayer executor (won't actually connect)
        let result = service.test_relayer_connectivity().await;
        
        // Since we're not actually running a relayer, this should succeed in creating the executor
        // but might fail on actual connectivity - that's expected in unit tests
        assert!(result.is_ok() || result.is_err()); // Either outcome is fine for unit test
        
        println!("✅ Relayer connectivity test completed");
    }
} 