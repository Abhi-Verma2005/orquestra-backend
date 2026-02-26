use std::sync::Arc;

use axum::{
    extract::FromRef,
    http::{header, HeaderValue, Method, StatusCode},
    middleware,
    routing::{get, patch, post},
    Json, Router,
};
use serde_json::{json, Value};
use sqlx::PgPool;
use tower_http::cors::CorsLayer;
use tracing_subscriber::EnvFilter;

mod ai;
mod auth;
mod config;
mod conversations;
mod database;
mod error;
mod logging;
mod messages;
mod models_list;

use ai::tools::ToolRegistry;
use config::Config;

#[derive(Clone, FromRef)]
pub struct AppState {
    pub db: PgPool,
    pub config: Arc<Config>,
    pub tools: Arc<ToolRegistry>,
}

async fn health_check() -> Json<Value> {
    Json(json!({ "status": "ok", "version": env!("CARGO_PKG_VERSION") }))
}

fn cors_layer() -> CorsLayer {
    let origin_predicate = tower_http::cors::AllowOrigin::predicate(
        |origin: &HeaderValue, _request_parts| {
            origin
                .to_str()
                .map(|o| {
                    o == "http://localhost:3000"
                        || (o.starts_with("http://localhost:")
                            && o["http://localhost:".len()..]
                                .chars()
                                .all(|ch| ch.is_ascii_digit()))
                })
                .unwrap_or(false)
        },
    );

    CorsLayer::new()
        .allow_origin(origin_predicate)
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::CONTENT_TYPE, header::AUTHORIZATION])
        .allow_credentials(true)
}

fn build_router(state: AppState) -> Router {
    let auth_routes = Router::new()
        .route("/register", post(auth::handlers::register))
        .route("/login", post(auth::handlers::login))
        .route("/me", get(auth::handlers::me));

    let conversation_routes = Router::new()
        .route("/", get(conversations::handlers::list).post(conversations::handlers::create))
        .route(
            "/:id",
            get(conversations::handlers::get)
                .delete(conversations::handlers::delete),
        )
        .route("/:id/settings", patch(conversations::handlers::update_settings))
        .route("/:id/title", patch(conversations::handlers::update_title))
        .route("/:id/messages", get(messages::handlers::list).post(messages::handlers::send))
        .route("/:id/stream", get(messages::handlers::stream_handler));

    Router::new()
        .route("/health", get(health_check))
        .route("/models", get(models_list::handlers::list_models))
        .nest("/auth", auth_routes)
        .nest("/conversations", conversation_routes)
        .layer(middleware::from_fn_with_state(
            state.clone(),
            logging::request_logging_middleware,
        ))
        .layer(cors_layer())
        .fallback(|| async {
            (
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Route not found" })),
            )
        })
        .with_state(state)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive("halo_backend=debug".parse()?),
        )
        .init();

    let config = Config::from_env();
    let port = config.port;

    let pool = database::create_pool(&config.database_url).await;
    sqlx::migrate!("./migrations").run(&pool).await?;
    tracing::info!("Migrations ran successfully");

    let state = AppState {
        db: pool,
        config: Arc::new(config),
        tools: Arc::new(ToolRegistry::new()),
    };

    let app = build_router(state);
    let addr = format!("0.0.0.0:{port}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    tracing::info!("Listening on {}", addr);
    axum::serve(listener, app).await?;

    Ok(())
}
