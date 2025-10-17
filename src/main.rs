// AI Orchestrator Backend
// Main server setup and routing

use axum::{
    extract::Query,
    response::Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;
use tracing_subscriber;

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

#[derive(Serialize, Deserialize)]
struct HealthResponse {
    status: String,
    message: String,
}

#[derive(Serialize, Deserialize)]
struct ApiResponse<T> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
}

#[tokio::main]
async fn main() {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Build our application with routes
    let app = Router::new()
        .route("/", get(root))
        .route("/health", get(health_check))
        .route("/api/status", get(api_status))
        .route("/api/echo", post(echo))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http());

    // Run the server
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000")
        .await
        .unwrap();
    
    println!("🚀 Server running on http://0.0.0.0:3000");
    println!("📋 Available endpoints:");
    println!("  GET  /           - Root endpoint");
    println!("  GET  /health     - Health check");
    println!("  GET  /api/status - API status");
    println!("  POST /api/echo   - Echo endpoint");

    axum::serve(listener, app).await.unwrap();
}

async fn root() -> &'static str {
    "Welcome to the Rust Backend API!"
}

async fn health_check() -> Json<HealthResponse> {
    Json(HealthResponse {
        status: "healthy".to_string(),
        message: "Backend is running smoothly".to_string(),
    })
}

async fn api_status() -> Json<ApiResponse<HashMap<String, String>>> {
    let mut status = HashMap::new();
    status.insert("version".to_string(), "0.1.0".to_string());
    status.insert("environment".to_string(), "development".to_string());
    status.insert("framework".to_string(), "axum".to_string());

    Json(ApiResponse {
        success: true,
        data: Some(status),
        error: None,
    })
}

async fn echo(Query(params): Query<HashMap<String, String>>) -> Json<ApiResponse<HashMap<String, String>>> {
    Json(ApiResponse {
        success: true,
        data: Some(params),
        error: None,
    })
}
