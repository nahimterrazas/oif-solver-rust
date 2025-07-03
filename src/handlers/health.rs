use actix_web::{web, HttpResponse, Result};
use serde_json::json;
use std::sync::Arc;

use crate::contracts::ContractFactory;

pub async fn health_check() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({
        "status": "healthy",
        "service": "oif-solver-rust",
        "version": "0.1.0",
        "timestamp": chrono::Utc::now().to_rfc3339()
    })))
}

pub async fn blockchain_health_check(
    contract_factory: web::Data<Arc<ContractFactory>>,
) -> Result<HttpResponse> {
    tracing::info!("Checking blockchain connectivity...");
    
    match contract_factory.check_chain_connectivity().await {
        Ok((origin_block, dest_block)) => {
            tracing::info!("Blockchain connectivity OK - Origin: {}, Destination: {}", origin_block, dest_block);
            Ok(HttpResponse::Ok().json(json!({
                "status": "healthy",
                "service": "oif-solver-rust",
                "version": "0.1.0",
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "blockchain": {
                    "origin_chain_block": origin_block,
                    "destination_chain_block": dest_block,
                    "connectivity": "ok"
                }
            })))
        }
        Err(e) => {
            tracing::warn!("Blockchain connectivity failed: {}", e);
            // Return 200 OK but with unhealthy status to provide more information
            Ok(HttpResponse::Ok().json(json!({
                "status": "degraded",
                "service": "oif-solver-rust", 
                "version": "0.1.0",
                "timestamp": chrono::Utc::now().to_rfc3339(),
                "blockchain": {
                    "connectivity": "failed",
                    "error": e.to_string(),
                    "note": "Service running but blockchain connectivity unavailable. Check RPC URLs in configuration."
                }
            })))
        }
    }
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/api/v1/health", web::get().to(health_check))
       .route("/api/v1/health/blockchain", web::get().to(blockchain_health_check));
} 