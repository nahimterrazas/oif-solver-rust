use anyhow::Result;
use serde::Deserialize;
use std::collections::HashMap;

#[derive(Debug, Deserialize, Clone)]
pub struct AppConfig {
    pub server: ServerConfig,
    pub solver: SolverConfig,
    pub chains: ChainConfig,
    pub contracts: ContractConfig,
    pub monitoring: MonitoringConfig,
    pub persistence: PersistenceConfig,
    pub relayer: Option<RelayerConfig>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct RelayerConfig {
    pub enabled: bool,
    pub api_base_url: String,
    pub api_key: String,
    pub webhook_url: Option<String>,
    pub timeout_seconds: u64,
    pub max_retries: u32,
    pub use_async: bool,
    pub chain_endpoints: HashMap<u64, String>,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Deserialize, Clone)]
pub struct SolverConfig {
    pub private_key: String,
    pub finalization_delay_seconds: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ChainConfig {
    pub origin: ChainDetails,
    pub destination: ChainDetails,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ChainDetails {
    pub rpc_url: String,
    pub chain_id: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct ContractConfig {
    pub the_compact: String,
    pub settler_compact: String,
    pub coin_filler: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct MonitoringConfig {
    pub enabled: bool,
    pub check_interval_seconds: u64,
}

#[derive(Debug, Deserialize, Clone)]
pub struct PersistenceConfig {
    pub enabled: bool,
    pub data_file: String,
}

impl AppConfig {
    pub async fn load() -> Result<Self> {
        tracing::info!("Loading configuration...");
        
        let settings = config::Config::builder()
            .add_source(config::File::with_name("config/local").required(false))
            .add_source(config::Environment::with_prefix("OIF_SOLVER"))
            .build()?;

        let mut config: AppConfig = match settings.try_deserialize() {
            Ok(config) => {
                tracing::info!("Configuration loaded from file/environment");
                config
            }
            Err(_) => {
                tracing::warn!("Could not load configuration from file/environment, using defaults");
                AppConfig::default()
            }
        };

        // Override with environment variables if present
        if let Ok(private_key) = std::env::var("SOLVER_PRIVATE_KEY") {
            config.solver.private_key = private_key;
        }

        if let Ok(origin_rpc) = std::env::var("ORIGIN_RPC_URL") {
            config.chains.origin.rpc_url = origin_rpc;
        }

        if let Ok(dest_rpc) = std::env::var("DESTINATION_RPC_URL") {
            config.chains.destination.rpc_url = dest_rpc;
        }

        // Override relayer config with environment variables if present
        if let Ok(relayer_enabled) = std::env::var("RELAYER_ENABLED") {
            if let Some(ref mut relayer) = config.relayer {
                relayer.enabled = relayer_enabled.parse().unwrap_or(false);
            }
        }

        if let Ok(relayer_url) = std::env::var("RELAYER_API_URL") {
            if let Some(ref mut relayer) = config.relayer {
                relayer.api_base_url = relayer_url;
            }
        }

        if let Ok(relayer_key) = std::env::var("RELAYER_API_KEY") {
            if let Some(ref mut relayer) = config.relayer {
                relayer.api_key = relayer_key;
            }
        }

        tracing::info!("Final configuration:");
        tracing::info!("  Server: {}:{}", config.server.host, config.server.port);
        tracing::info!("  Origin chain: {}", config.chains.origin.rpc_url);
        tracing::info!("  Destination chain: {}", config.chains.destination.rpc_url);
        if let Some(ref relayer) = config.relayer {
            tracing::info!("  Relayer enabled: {}", relayer.enabled);
            if relayer.enabled {
                tracing::info!("  Relayer URL: {}", relayer.api_base_url);
                tracing::info!("  Relayer API key: {}***", &relayer.api_key[..10.min(relayer.api_key.len())]);
            }
        }

        Ok(config)
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig {
                host: "0.0.0.0".to_string(),
                port: 3000,
            },
            solver: SolverConfig {
                private_key: "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80".to_string(),
                finalization_delay_seconds: 30,
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
            contracts: ContractConfig {
                the_compact: "0x0000000000000000000000000000000000000000".to_string(),
                settler_compact: "0x0000000000000000000000000000000000000000".to_string(),
                coin_filler: "0x0000000000000000000000000000000000000000".to_string(),
            },
            monitoring: MonitoringConfig {
                enabled: true,
                check_interval_seconds: 60,
            },
            persistence: PersistenceConfig {
                enabled: true,
                data_file: "data/orders.json".to_string(),
            },
            relayer: Some(RelayerConfig {
                enabled: false, // Disabled by default
                api_base_url: "http://localhost:8080/api/v1".to_string(),
                api_key: "example-#1234567890123456789012345678901234567890".to_string(),
                webhook_url: None,
                timeout_seconds: 300,
                max_retries: 3,
                use_async: false, // Use sync mode by default for local testing
                chain_endpoints: {
                    let mut endpoints: HashMap<u64, String> = HashMap::new();
                    endpoints.insert(31337, "anvil-origin-relayer".to_string());
                    endpoints.insert(31338, "anvil-destination-relayer".to_string());
                    endpoints
                },
            }),
        }
    }
} 