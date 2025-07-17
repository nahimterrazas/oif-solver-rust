use crate::contracts::execution::traits::{
    ExecutionEngine, ExecutionEngineFactory, TransportType, RelayerConfig
};
use crate::contracts::execution::{AlloyExecutor, OpenZeppelinExecutor};
use crate::config::AppConfig;
use alloy::primitives::Address;
use anyhow::Result;
use std::{collections::HashMap, sync::Arc};
use tracing::{info, warn};

/// Configuration for the execution engine factory
#[derive(Debug, Clone)]
pub struct ExecutionEngineConfig {
    /// Application configuration (for Alloy executor)
    pub app_config: Arc<AppConfig>,
    /// Relayer configuration (for OpenZeppelin executor)
    pub relayer_config: Option<RelayerConfig>,
    /// Wallet address for relayer executor
    pub wallet_address: Address,
    /// Default transport type to use
    pub default_transport: TransportType,
}

/// Factory for creating execution engines
pub struct DefaultExecutionEngineFactory {
    config: ExecutionEngineConfig,
}

impl DefaultExecutionEngineFactory {
    /// Create a new factory with the given configuration
    pub fn new(config: ExecutionEngineConfig) -> Self {
        info!("🏭 Creating ExecutionEngineFactory");
        info!("  Default transport: {:?}", config.default_transport);
        info!("  Relayer config available: {}", config.relayer_config.is_some());
        
        Self { config }
    }

    /// Create an Alloy-based executor
    fn create_alloy_executor(&self) -> Result<Box<dyn ExecutionEngine>> {
        info!("🔧 Creating AlloyExecutor");
        let executor = AlloyExecutor::new(self.config.app_config.clone())?;
        Ok(Box::new(executor))
    }

    /// Create an OpenZeppelin relayer-based executor
    fn create_openzeppelin_executor(&self) -> Result<Box<dyn ExecutionEngine>> {
        info!("🔧 Creating OpenZeppelinExecutor");
        
        let relayer_config = self.config.relayer_config.as_ref()
            .ok_or_else(|| anyhow::anyhow!("OpenZeppelin relayer configuration not provided"))?;
        
        let executor = OpenZeppelinExecutor::new(Arc::new(relayer_config.clone()), self.config.wallet_address)?;
        Ok(Box::new(executor))
    }
}

impl ExecutionEngineFactory for DefaultExecutionEngineFactory {
    fn create_engine(&self, transport: TransportType) -> Result<Box<dyn ExecutionEngine>> {
        match transport {
            TransportType::Direct => self.create_alloy_executor(),
            TransportType::Relayer => self.create_openzeppelin_executor(),
            TransportType::Custom(name) => {
                warn!("⚠️ Custom transport type '{}' not supported", name);
                Err(anyhow::anyhow!("Custom transport type '{}' not implemented", name))
            }
        }
    }

    fn available_transports(&self) -> Vec<TransportType> {
        let mut transports = vec![TransportType::Direct];
        
        if self.config.relayer_config.is_some() {
            transports.push(TransportType::Relayer);
        }
        
        transports
    }

    fn supports_transport(&self, transport: &TransportType) -> bool {
        match transport {
            TransportType::Direct => true,
            TransportType::Relayer => self.config.relayer_config.is_some(),
            TransportType::Custom(_) => false,
        }
    }
}

/// Builder for creating execution engine configurations
pub struct ExecutionEngineConfigBuilder {
    app_config: Option<Arc<AppConfig>>,
    relayer_config: Option<RelayerConfig>,
    wallet_address: Option<Address>,
    default_transport: TransportType,
}

impl ExecutionEngineConfigBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            app_config: None,
            relayer_config: None,
            wallet_address: None,
            default_transport: TransportType::Direct,
        }
    }

    /// Set the application configuration
    pub fn with_app_config(mut self, config: Arc<AppConfig>) -> Self {
        self.app_config = Some(config);
        self
    }

    /// Set the relayer configuration
    pub fn with_relayer_config(mut self, config: RelayerConfig) -> Self {
        self.relayer_config = Some(config);
        self
    }

    /// Set the wallet address
    pub fn with_wallet_address(mut self, address: Address) -> Self {
        self.wallet_address = Some(address);
        self
    }

    /// Set the default transport type
    pub fn with_default_transport(mut self, transport: TransportType) -> Self {
        self.default_transport = transport;
        self
    }

    /// Build the configuration
    pub fn build(self) -> Result<ExecutionEngineConfig> {
        let app_config = self.app_config
            .ok_or_else(|| anyhow::anyhow!("App configuration is required"))?;
        
        let wallet_address = self.wallet_address
            .ok_or_else(|| anyhow::anyhow!("Wallet address is required"))?;

        // Validate that relayer config is provided if relayer transport is default
        if matches!(self.default_transport, TransportType::Relayer) && self.relayer_config.is_none() {
            return Err(anyhow::anyhow!("Relayer configuration required when using relayer transport"));
        }

        Ok(ExecutionEngineConfig {
            app_config,
            relayer_config: self.relayer_config,
            wallet_address,
            default_transport: self.default_transport,
        })
    }
}

impl Default for ExecutionEngineConfigBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Smart factory that can automatically choose the best executor based on context
pub struct SmartExecutionEngineFactory {
    pub factory: DefaultExecutionEngineFactory,
    fallback_enabled: bool,
}

impl SmartExecutionEngineFactory {
    /// Create a new smart factory
    pub fn new(config: ExecutionEngineConfig, fallback_enabled: bool) -> Self {
        Self {
            factory: DefaultExecutionEngineFactory::new(config),
            fallback_enabled,
        }
    }

    /// Create an executor with automatic fallback logic
    pub fn create_engine_with_fallback(&self, preferred_transport: TransportType) -> Result<Box<dyn ExecutionEngine>> {
        // Try the preferred transport first
        if self.factory.supports_transport(&preferred_transport) {
            match self.factory.create_engine(preferred_transport.clone()) {
                Ok(engine) => {
                    info!("✅ Created executor with preferred transport: {:?}", preferred_transport);
                    return Ok(engine);
                }
                Err(e) => {
                    warn!("⚠️ Failed to create executor with preferred transport {:?}: {}", preferred_transport, e);
                    
                    if !self.fallback_enabled {
                        return Err(e);
                    }
                }
            }
        }

        // Fallback logic if enabled
        if self.fallback_enabled {
            info!("🔄 Attempting fallback to available transports");
            
            let available = self.factory.available_transports();
            for transport in available {
                if transport != preferred_transport {
                    match self.factory.create_engine(transport.clone()) {
                        Ok(engine) => {
                            warn!("🔄 Successfully created fallback executor with transport: {:?}", transport);
                            return Ok(engine);
                        }
                        Err(e) => {
                            warn!("⚠️ Fallback transport {:?} also failed: {}", transport, e);
                        }
                    }
                }
            }
        }

        Err(anyhow::anyhow!("Failed to create executor with any available transport"))
    }

    /// Get recommendations for transport type based on use case
    pub fn recommend_transport(&self, use_case: ExecutionUseCase) -> TransportType {
        match use_case {
            ExecutionUseCase::HighFrequency => {
                // For high frequency, prefer direct execution for speed
                TransportType::Direct
            }
            ExecutionUseCase::CrossChain => {
                // For cross-chain, relayers might be more suitable
                if self.factory.supports_transport(&TransportType::Relayer) {
                    TransportType::Relayer
                } else {
                    TransportType::Direct
                }
            }
            ExecutionUseCase::GasOptimized => {
                // For gas optimization, relayers can provide better gas management
                if self.factory.supports_transport(&TransportType::Relayer) {
                    TransportType::Relayer
                } else {
                    TransportType::Direct
                }
            }
            ExecutionUseCase::Development => {
                // For development, prefer direct execution for debugging
                TransportType::Direct
            }
            ExecutionUseCase::Production => {
                // For production, prefer relayers for robustness
                if self.factory.supports_transport(&TransportType::Relayer) {
                    TransportType::Relayer
                } else {
                    TransportType::Direct
                }
            }
        }
    }
}

/// Use cases for execution engine selection
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionUseCase {
    /// High frequency trading or operations
    HighFrequency,
    /// Cross-chain operations
    CrossChain,
    /// Gas cost optimization
    GasOptimized,
    /// Development and testing
    Development,
    /// Production deployment
    Production,
}

/// Utility functions for creating common configurations
pub mod presets {
    use super::*;
    use std::str::FromStr;

    /// Create a development configuration (Alloy only)
    pub fn development_config(app_config: Arc<AppConfig>) -> Result<ExecutionEngineConfig> {
        let wallet_address = Address::from_str("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266")?; // Common test address
        
        ExecutionEngineConfigBuilder::new()
            .with_app_config(app_config)
            .with_wallet_address(wallet_address)
            .with_default_transport(TransportType::Direct)
            .build()
    }

    /// Create a production configuration with OpenZeppelin relayer
    pub fn production_config(
        app_config: Arc<AppConfig>,
        relayer_config: RelayerConfig,
        wallet_address: Address,
    ) -> Result<ExecutionEngineConfig> {
        ExecutionEngineConfigBuilder::new()
            .with_app_config(app_config)
            .with_relayer_config(relayer_config)
            .with_wallet_address(wallet_address)
            .with_default_transport(TransportType::Relayer)
            .build()
    }

    /// Create a hybrid configuration supporting both direct and relayer execution
    pub fn hybrid_config(
        app_config: Arc<AppConfig>,
        relayer_config: RelayerConfig,
        wallet_address: Address,
        prefer_relayer: bool,
    ) -> Result<ExecutionEngineConfig> {
        let default_transport = if prefer_relayer {
            TransportType::Relayer
        } else {
            TransportType::Direct
        };

        ExecutionEngineConfigBuilder::new()
            .with_app_config(app_config)
            .with_relayer_config(relayer_config)
            .with_wallet_address(wallet_address)
            .with_default_transport(default_transport)
            .build()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AppConfig, ServerConfig, ChainConfig, ChainDetails, SolverConfig, ContractConfig, MonitoringConfig, PersistenceConfig};
    use std::{collections::HashMap, str::FromStr};

    fn create_test_app_config() -> Arc<AppConfig> {
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

    fn create_test_relayer_config() -> RelayerConfig {
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
    fn test_execution_engine_config_builder() {
        let app_config = create_test_app_config();
        let wallet_address = Address::from_str("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266").unwrap();

        let config = ExecutionEngineConfigBuilder::new()
            .with_app_config(app_config)
            .with_wallet_address(wallet_address)
            .with_default_transport(TransportType::Direct)
            .build()
            .expect("Config build should succeed");

        assert_eq!(config.default_transport, TransportType::Direct);
        assert_eq!(config.wallet_address, wallet_address);
        assert!(config.relayer_config.is_none());

        println!("✅ ExecutionEngineConfigBuilder works correctly");
    }

    #[test]
    fn test_factory_creation_direct() {
        let app_config = create_test_app_config();
        let wallet_address = Address::from_str("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266").unwrap();

        let config = ExecutionEngineConfigBuilder::new()
            .with_app_config(app_config)
            .with_wallet_address(wallet_address)
            .build()
            .expect("Config build should succeed");

        let factory = DefaultExecutionEngineFactory::new(config);
        
        assert!(factory.supports_transport(&TransportType::Direct));
        assert!(!factory.supports_transport(&TransportType::Relayer));

        let available = factory.available_transports();
        assert_eq!(available.len(), 1);
        assert_eq!(available[0], TransportType::Direct);

        println!("✅ Factory direct transport support works correctly");
    }

    #[test]
    fn test_factory_creation_hybrid() {
        let app_config = create_test_app_config();
        let relayer_config = create_test_relayer_config();
        let wallet_address = Address::from_str("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266").unwrap();

        let config = ExecutionEngineConfigBuilder::new()
            .with_app_config(app_config)
            .with_relayer_config(relayer_config)
            .with_wallet_address(wallet_address)
            .build()
            .expect("Config build should succeed");

        let factory = DefaultExecutionEngineFactory::new(config);
        
        assert!(factory.supports_transport(&TransportType::Direct));
        assert!(factory.supports_transport(&TransportType::Relayer));

        let available = factory.available_transports();
        assert_eq!(available.len(), 2);
        assert!(available.contains(&TransportType::Direct));
        assert!(available.contains(&TransportType::Relayer));

        println!("✅ Factory hybrid transport support works correctly");
    }

    #[test]
    fn test_smart_factory_recommendations() {
        let app_config = create_test_app_config();
        let relayer_config = create_test_relayer_config();
        let wallet_address = Address::from_str("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266").unwrap();

        let config = ExecutionEngineConfigBuilder::new()
            .with_app_config(app_config)
            .with_relayer_config(relayer_config)
            .with_wallet_address(wallet_address)
            .build()
            .expect("Config build should succeed");

        let smart_factory = SmartExecutionEngineFactory::new(config, true);

        // Test recommendations
        assert_eq!(smart_factory.recommend_transport(ExecutionUseCase::Development), TransportType::Direct);
        assert_eq!(smart_factory.recommend_transport(ExecutionUseCase::HighFrequency), TransportType::Direct);
        assert_eq!(smart_factory.recommend_transport(ExecutionUseCase::Production), TransportType::Relayer);
        assert_eq!(smart_factory.recommend_transport(ExecutionUseCase::GasOptimized), TransportType::Relayer);

        println!("✅ Smart factory recommendations work correctly");
    }

    #[test]
    fn test_preset_configurations() {
        let app_config = create_test_app_config();
        
        // Test development preset
        let dev_config = presets::development_config(app_config.clone())
            .expect("Development config should succeed");
        assert_eq!(dev_config.default_transport, TransportType::Direct);
        assert!(dev_config.relayer_config.is_none());

        // Test production preset
        let relayer_config = create_test_relayer_config();
        let wallet_address = Address::from_str("0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266").unwrap();
        
        let prod_config = presets::production_config(app_config.clone(), relayer_config.clone(), wallet_address)
            .expect("Production config should succeed");
        assert_eq!(prod_config.default_transport, TransportType::Relayer);
        assert!(prod_config.relayer_config.is_some());

        // Test hybrid preset
        let hybrid_config = presets::hybrid_config(app_config, relayer_config, wallet_address, false)
            .expect("Hybrid config should succeed");
        assert_eq!(hybrid_config.default_transport, TransportType::Direct);
        assert!(hybrid_config.relayer_config.is_some());

        println!("✅ Preset configurations work correctly");
    }
} 