use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

use crate::models::{Order, OrderStatus};

#[derive(Debug, Clone)]
pub struct MemoryStorage {
    orders: Arc<RwLock<HashMap<Uuid, Order>>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self {
            orders: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn store_order(&self, order: Order) -> Result<()> {
        let mut orders = self.orders.write().await;
        orders.insert(order.id, order);
        Ok(())
    }

    pub async fn get_order(&self, id: Uuid) -> Result<Option<Order>> {
        let orders = self.orders.read().await;
        Ok(orders.get(&id).cloned())
    }

    pub async fn update_order(&self, order: Order) -> Result<()> {
        let mut orders = self.orders.write().await;
        orders.insert(order.id, order);
        Ok(())
    }

    pub async fn get_orders_by_status(&self, status: OrderStatus) -> Result<Vec<Order>> {
        let orders = self.orders.read().await;
        let filtered_orders = orders
            .values()
            .filter(|order| order.status == status)
            .cloned()
            .collect();
        Ok(filtered_orders)
    }

    pub async fn get_pending_orders(&self) -> Result<Vec<Order>> {
        self.get_orders_by_status(OrderStatus::Pending).await
    }

    pub async fn get_processing_orders(&self) -> Result<Vec<Order>> {
        self.get_orders_by_status(OrderStatus::Processing).await
    }

    pub async fn get_all_orders(&self) -> Result<Vec<Order>> {
        let orders = self.orders.read().await;
        Ok(orders.values().cloned().collect())
    }

    pub async fn get_queue_status(&self) -> Result<QueueStatus> {
        let orders = self.orders.read().await;
        let mut pending = 0;
        let mut processing = 0;
        let mut filled = 0;
        let mut finalized = 0;
        let mut failed = 0;

        for order in orders.values() {
            match order.status {
                OrderStatus::Pending => pending += 1,
                OrderStatus::Processing => processing += 1,
                OrderStatus::Filled => filled += 1,
                OrderStatus::Finalizing => processing += 1, // Treat finalizing as processing
                OrderStatus::Finalized => finalized += 1,
                OrderStatus::Failed => failed += 1,
            }
        }

        Ok(QueueStatus {
            total: orders.len(),
            pending,
            processing,
            filled,
            finalized,
            failed,
        })
    }
}

#[derive(Debug, serde::Serialize)]
pub struct QueueStatus {
    pub total: usize,
    pub pending: usize,
    pub processing: usize,
    pub filled: usize,
    pub finalized: usize,
    pub failed: usize,
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
} 