use std::sync::Arc;

use sqlx::{PgPool, postgres::PgPoolOptions};

pub async fn init_db() -> PgPool {
    PgPoolOptions::new()
        .max_connections(10)
        .connect(&std::env::var("DATABASE_URL").unwrap())
        .await
        .unwrap()
}