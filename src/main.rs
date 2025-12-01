// src/main.rs
use crate::{
    config::Config,
    websocket::WebSocketServer,
    orchestrator::service::OrchestratorService,
};
use axum::{
    routing::get,
    Router,
    response::Json,
};
use std::collections::HashMap;

// Module declarations
pub mod config;
pub mod models;
pub mod orchestrator;
pub mod tools;
pub mod llm_provider;
pub mod context;
pub mod websocket;
pub mod persistence;
pub mod permissions;
pub mod utils;

async fn health_check() -> Json<HashMap<&'static str, &'static str>> {
    let mut response = HashMap::new();
    response.insert("status", "ok");
    response.insert("service", "websocket-server");
    Json(response)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 Starting AI Orchestrator Backend");
    
    // Load configuration
    let config = Config::from_env()?;
    
    // Use PORT for both HTTP and WebSocket (Render provides PORT automatically)
    // If PORT not set (local dev), use WS_PORT or default to 8080
    let port = std::env::var("PORT")
        .or_else(|_| std::env::var("WS_PORT"))
        .unwrap_or_else(|_| "8080".to_string())
        .parse::<u16>()?;
    
    let orchestrator = OrchestratorService::new(config);
    let websocket_server = WebSocketServer::new(port, orchestrator);
    
    // Create HTTP server for health checks (Render needs this)
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/", get(health_check));
    
    let http_addr = format!("0.0.0.0:{}", port);
    println!("🌐 HTTP health check server listening on: {}", http_addr);
    
    let listener = tokio::net::TcpListener::bind(&http_addr).await?;
    let http_server = axum::serve(listener, app);
    
    // Start both servers concurrently
    tokio::select! {
        result = websocket_server.run() => {
            println!("WebSocket server stopped: {:?}", result);
        }
        result = http_server => {
            println!("HTTP server stopped: {:?}", result);
        }
        _ = tokio::signal::ctrl_c() => {
            println!("Shutting down gracefully...");
        }
    }
    
    Ok(())
}