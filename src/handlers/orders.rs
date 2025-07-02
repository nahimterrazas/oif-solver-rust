use actix_web::{web, HttpResponse, Result, HttpRequest};
use serde_json::json;
use uuid::Uuid;
use std::str::FromStr;

use crate::models::{Order, OrderSubmission, OrderResponse};
use crate::storage::MemoryStorage;
use crate::services::OrderMonitoringService;

pub async fn submit_order(
    req_body: web::Json<OrderSubmission>,
    storage: web::Data<MemoryStorage>,
) -> Result<HttpResponse> {
    // Create new order from submission
    let order = Order::new(req_body.order.clone(), req_body.signature.clone());
    let order_id = order.id;

    // Store order
    match storage.store_order(order).await {
        Ok(_) => {
            tracing::info!("Order {} submitted successfully", order_id);
            
            Ok(HttpResponse::Created().json(json!({
                "id": order_id,
                "status": "pending",
                "message": "Order submitted successfully"
            })))
        }
        Err(e) => {
            tracing::error!("Failed to store order: {}", e);
            Ok(HttpResponse::InternalServerError().json(json!({
                "error": "Failed to store order",
                "details": e.to_string()
            })))
        }
    }
}

pub async fn get_order(
    path: web::Path<String>,
    storage: web::Data<MemoryStorage>,
) -> Result<HttpResponse> {
    let order_id_str = path.into_inner();
    
    // Parse UUID
    let order_id = match Uuid::from_str(&order_id_str) {
        Ok(id) => id,
        Err(_) => {
            return Ok(HttpResponse::BadRequest().json(json!({
                "error": "Invalid order ID format"
            })))
        }
    };

    // Get order from storage
    match storage.get_order(order_id).await {
        Ok(Some(order)) => {
            let response = order.to_response();
            Ok(HttpResponse::Ok().json(response))
        }
        Ok(None) => {
            Ok(HttpResponse::NotFound().json(json!({
                "error": "Order not found"
            })))
        }
        Err(e) => {
            tracing::error!("Failed to retrieve order: {}", e);
            Ok(HttpResponse::InternalServerError().json(json!({
                "error": "Failed to retrieve order",
                "details": e.to_string()
            })))
        }
    }
}

pub async fn finalize_order(
    path: web::Path<String>,
    storage: web::Data<MemoryStorage>,
    monitoring_service: web::Data<OrderMonitoringService>,
) -> Result<HttpResponse> {
    let order_id_str = path.into_inner();
    
    // Parse UUID
    let order_id = match Uuid::from_str(&order_id_str) {
        Ok(id) => id,
        Err(_) => {
            return Ok(HttpResponse::BadRequest().json(json!({
                "error": "Invalid order ID format"
            })))
        }
    };

    // Check if order exists
    match storage.get_order(order_id).await {
        Ok(Some(_)) => {
            // Trigger manual finalization
            match monitoring_service.trigger_finalization(order_id).await {
                Ok(true) => {
                    Ok(HttpResponse::Ok().json(json!({
                        "id": order_id,
                        "message": "Finalization triggered successfully"
                    })))
                }
                Ok(false) => {
                    Ok(HttpResponse::BadRequest().json(json!({
                        "error": "Finalization failed"
                    })))
                }
                Err(e) => {
                    tracing::error!("Failed to trigger finalization: {}", e);
                    Ok(HttpResponse::InternalServerError().json(json!({
                        "error": "Failed to trigger finalization",
                        "details": e.to_string()
                    })))
                }
            }
        }
        Ok(None) => {
            Ok(HttpResponse::NotFound().json(json!({
                "error": "Order not found"
            })))
        }
        Err(e) => {
            tracing::error!("Failed to retrieve order: {}", e);
            Ok(HttpResponse::InternalServerError().json(json!({
                "error": "Failed to retrieve order",
                "details": e.to_string()
            })))
        }
    }
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/api/v1/orders", web::post().to(submit_order))
       .route("/api/v1/orders/{id}", web::get().to(get_order))
       .route("/api/v1/orders/{id}/finalize", web::post().to(finalize_order));
} 