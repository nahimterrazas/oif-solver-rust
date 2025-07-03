use actix_web::{web, App, HttpServer, middleware::Logger, Result};
use actix_cors::Cors;
use std::sync::Arc;

use crate::config::AppConfig;
use crate::storage::MemoryStorage;
use crate::services::OrderMonitoringService;
use crate::contracts::ContractFactory;
use crate::handlers;

pub struct SolverServer {
    storage: MemoryStorage,
    monitoring_service: Arc<OrderMonitoringService>,
    contract_factory: Arc<ContractFactory>,
    config: AppConfig,
}

impl SolverServer {
    pub async fn new(storage: MemoryStorage, config: AppConfig) -> Result<Self, anyhow::Error> {
        // Create contract factory
        let contract_factory = ContractFactory::new(config.clone()).await?;
        let contract_factory = Arc::new(contract_factory);

        // Create monitoring service
        let monitoring_service = OrderMonitoringService::new(storage.clone(), config.clone()).await?;
        let monitoring_service = Arc::new(monitoring_service);

        Ok(Self {
            storage,
            monitoring_service,
            contract_factory,
            config,
        })
    }

    pub async fn run(self) -> std::io::Result<()> {
        let bind_address = format!("{}:{}", self.config.server.host, self.config.server.port);
        
        // Start background monitoring
        tracing::info!("Starting background monitoring service...");
        if let Err(e) = self.monitoring_service.start_background_monitoring().await {
            tracing::error!("Failed to start monitoring service: {}", e);
        }
        
        tracing::info!("Starting HTTP server on {}", bind_address);

        HttpServer::new(move || {
            let cors = Cors::default()
                .allow_any_origin()
                .allow_any_method()
                .allow_any_header()
                .max_age(3600);

            App::new()
                .app_data(web::Data::new(self.storage.clone()))
                .app_data(web::Data::new(self.monitoring_service.clone()))
                .app_data(web::Data::new(self.contract_factory.clone()))
                .wrap(cors)
                .wrap(Logger::default())
                .configure(handlers::health::config)
                .configure(handlers::orders::config)
                .configure(handlers::queue::config)
                .route("/", web::get().to(api_info))
        })
        .bind(&bind_address)?
        .run()
        .await
    }
}

async fn api_info() -> Result<actix_web::HttpResponse> {
    Ok(actix_web::HttpResponse::Ok().json(serde_json::json!({
        "name": "OIF Solver Rust POC",
        "version": "0.1.0",
        "description": "Rust implementation of the OIF Protocol Solver",
        "endpoints": {
            "health": "GET /api/v1/health",
            "blockchain_health": "GET /api/v1/health/blockchain",
            "submit_order": "POST /api/v1/orders",
            "get_order": "GET /api/v1/orders/{id}",
            "finalize_order": "POST /api/v1/orders/{id}/finalize",
            "queue_status": "GET /api/v1/queue"
        }
    })))
} 