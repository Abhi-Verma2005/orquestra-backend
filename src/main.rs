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
    
    // Initialize services
    let ws_port = std::env::var("WS_PORT")
        .unwrap_or_else(|_| "8080".to_string())
        .parse::<u16>()?;
    
    let orchestrator = OrchestratorService::new(config);
    let websocket_server = WebSocketServer::new(ws_port, orchestrator);
    
    // Start WebSocket server
    tokio::select! {
        _ = websocket_server.run() => {
            println!("WebSocket server stopped");
        }
        _ = tokio::signal::ctrl_c() => {
            println!("Shutting down gracefully...");
        }
    }
    
    Ok(())
}