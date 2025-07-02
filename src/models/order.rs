use alloy::primitives::{Address, U256};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StandardOrder {
    pub nonce: U256,
    pub maker: Address,
    pub input_token: Address,
    pub input_amount: U256,
    pub output_token: Address,
    pub output_amount: U256,
    pub expiry: u64,
    pub origin_chain_id: u64,
    pub destination_chain_id: u64,
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

impl Default for OrderStatus {
    fn default() -> Self {
        OrderStatus::Pending
    }
} 