use alloy::primitives::U256;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub success: bool,
    pub tx_hash: Option<String>,
    pub error_message: Option<String>,
    pub gas_used: Option<U256>,
}

impl ExecutionResult {
    pub fn success(tx_hash: String, gas_used: Option<U256>) -> Self {
        Self {
            success: true,
            tx_hash: Some(tx_hash),
            error_message: None,
            gas_used,
        }
    }

    pub fn failure(error: String) -> Self {
        Self {
            success: false,
            tx_hash: None,
            error_message: Some(error),
            gas_used: None,
        }
    }
} 