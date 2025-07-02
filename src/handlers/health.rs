use actix_web::{web, HttpResponse, Result};
use serde_json::json;

pub async fn health_check() -> Result<HttpResponse> {
    Ok(HttpResponse::Ok().json(json!({
        "status": "healthy",
        "service": "oif-solver-rust",
        "version": "0.1.0",
        "timestamp": chrono::Utc::now().to_rfc3339()
    })))
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.route("/api/v1/health", web::get().to(health_check));
} 