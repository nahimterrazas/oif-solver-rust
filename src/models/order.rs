use alloy::primitives::{Address, U256};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandardOrder {
    pub user: Address,
    pub nonce: u64,
    #[serde(rename = "originChainId")]
    pub origin_chain_id: u64,
    pub expires: u64,
    #[serde(rename = "fillDeadline")]
    pub fill_deadline: u64,
    #[serde(rename = "localOracle")]
    pub local_oracle: Address,
    pub inputs: Vec<(String, String)>, // [tokenId, amount] tuples
    pub outputs: Vec<MandateOutput>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MandateOutput {
    #[serde(rename = "remoteOracle")]
    pub remote_oracle: Address,
    #[serde(rename = "remoteFiller")]
    pub remote_filler: Address,
    #[serde(rename = "chainId")]
    pub chain_id: u64,
    pub token: Address,
    pub amount: String,
    pub recipient: Address,
    #[serde(rename = "remoteCall", default)]
    pub remote_call: Option<String>,
    #[serde(rename = "fulfillmentContext", default)]
    pub fulfillment_context: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum OrderStatus {
    Pending,
    Processing,
    Filled,
    Finalized,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Order {
    pub id: Uuid,
    pub standard_order: StandardOrder,
    pub signature: String,
    pub status: OrderStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub fill_tx_hash: Option<String>,
    pub finalize_tx_hash: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderSubmission {
    pub order: StandardOrder,
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderResponse {
    pub id: Uuid,
    pub status: OrderStatus,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub fill_tx_hash: Option<String>,
    pub finalize_tx_hash: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillResult {
    pub success: bool,
    pub tx_hash: Option<String>,
    pub gas_cost: Option<U256>,
    pub error: Option<String>,
}

impl Order {
    pub fn new(standard_order: StandardOrder, signature: String) -> Self {
        let now = Utc::now();
        Self {
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

    pub fn update_status(&mut self, status: OrderStatus) {
        self.status = status;
        self.updated_at = Utc::now();
    }

    pub fn set_fill_tx(&mut self, tx_hash: String) {
        self.fill_tx_hash = Some(tx_hash);
        self.updated_at = Utc::now();
    }

    pub fn set_finalize_tx(&mut self, tx_hash: String) {
        self.finalize_tx_hash = Some(tx_hash);
        self.updated_at = Utc::now();
    }

    pub fn set_error(&mut self, error: String) {
        self.error_message = Some(error);
        self.status = OrderStatus::Failed;
        self.updated_at = Utc::now();
    }

    pub fn to_response(&self) -> OrderResponse {
        OrderResponse {
            id: self.id,
            status: self.status.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            fill_tx_hash: self.fill_tx_hash.clone(),
            finalize_tx_hash: self.finalize_tx_hash.clone(),
            error_message: self.error_message.clone(),
        }
    }
}

impl MandateOutput {
    pub fn new(
        remote_oracle: Address,
        remote_filler: Address,
        chain_id: u64,
        token: Address,
        amount: String,
        recipient: Address,
    ) -> Self {
        Self {
            remote_oracle,
            remote_filler,
            chain_id,
            token,
            amount,
            recipient,
            remote_call: Some("0x".to_string()),
            fulfillment_context: Some("0x".to_string()),
        }
    }
}

impl FillResult {
    pub fn success(tx_hash: String, gas_cost: Option<U256>) -> Self {
        Self {
            success: true,
            tx_hash: Some(tx_hash),
            gas_cost,
            error: None,
        }
    }

    pub fn failure(error: String) -> Self {
        Self {
            success: false,
            tx_hash: None,
            gas_cost: None,
            error: Some(error),
        }
    }
}

impl Default for OrderStatus {
    fn default() -> Self {
        OrderStatus::Pending
    }
} 