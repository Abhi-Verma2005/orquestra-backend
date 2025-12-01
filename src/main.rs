// src/main.rs
use crate::{
    config::Config,
    websocket::WebSocketServer,
    orchestrator::service::OrchestratorService,
};

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
    
    // Start only the WebSocket server (it handles both WebSocket and HTTP health checks)
    let result = websocket_server.run().await;
    println!("WebSocket server stopped: {:?}", result);

    Ok(())
}