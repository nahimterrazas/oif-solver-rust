use actix_web::{web, HttpResponse, Result};
use serde_json::json;

use crate::storage::MemoryStorage;

pub async fn get_queue_status(
    storage: web::Data<MemoryStorage>,
) -> Result<HttpResponse> {
    match storage.get_queue_status().await {
        Ok(queue_status) => {
            Ok(HttpResponse::Ok().json(queue_status))
        }
        Err(e) => {
            tracing::error!("Failed to get queue status: {}", e);
            Ok(HttpResponse::InternalServerError().json(json!({
                "error": "Failed to get queue status",
                "details": e.to_string()
            })))
        }
    }
}

pub async fn get_all_orders(
    storage: web::Data<MemoryStorage>,
) -> Result<HttpResponse> {
    match storage.get_all_orders().await {
        Ok(orders) => {
            let order_responses: Vec<_> = orders.into_iter()
                .map(|order| order.to_response())
                .collect();
            
            Ok(HttpResponse::Ok().json(json!({
                "orders": order_responses,
                "count": order_responses.len()
            })))
        }
        Err(e) => {
            tracing::error!("Failed to get all orders: {}", e);
            Ok(HttpResponse::InternalServerError().json(json!({
                "error": "Failed to get all orders",
                "details": e.to_string()
            })))
        }
    }
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/api/v1/queue", web::get().to(get_queue_status))
       .route("/api/v1/orders", web::get().to(get_all_orders));
} 