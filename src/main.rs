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
    
    // Initialize services
    let ws_port = std::env::var("WS_PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse::<u16>()?;
    
    let orchestrator = OrchestratorService::new(config);
    let websocket_server = WebSocketServer::new(ws_port, orchestrator);
    
    // Create HTTP server for health checks (Render needs this)
    // Use PORT env var if set (Render provides this), otherwise use WS_PORT + 1 to avoid conflict
    let http_port = if std::env::var("PORT").is_ok() {
        std::env::var("PORT")?.parse::<u16>()?
    } else {
        // If PORT not set, use WS_PORT + 1 to avoid conflict
        ws_port + 1
    };
    
    // Only start HTTP server if it's on a different port
    if http_port != ws_port {
        let app = Router::new()
            .route("/health", get(health_check))
            .route("/", get(health_check));
        
        let http_addr = format!("0.0.0.0:{}", http_port);
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
    } else {
        // If same port, only run WebSocket server
        // Note: In this case, Render health checks to WS port will fail
        // but WebSocket connections will work
        println!("⚠️  HTTP and WebSocket using same port - health checks may fail");
        tokio::select! {
            result = websocket_server.run() => {
                println!("WebSocket server stopped: {:?}", result);
            }
            _ = tokio::signal::ctrl_c() => {
                println!("Shutting down gracefully...");
            }
        }
    }
    
    Ok(())
}