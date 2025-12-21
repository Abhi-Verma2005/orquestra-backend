use std::sync::Arc;
use serde_json::Value;
use uuid::Uuid;
use anyhow::Result;
use sqlx::{PgPool};


pub struct ChatDbService {
    pub pool: Arc<PgPool>
}

impl ChatDbService {
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self {pool}
    }

    pub async fn save_chat(
        &self,
        id_str: &str,
        user_id: &str,
        messages: &Value,
        title: &str,
        summary: &str,
        is_group_chat: bool
    ) -> Result<(), sqlx::Error> {

        let now = chrono::Utc::now().naive_utc();

        let id = self.parse_id_to_uuid(id_str)
            .map_err(|e| sqlx::Error::Decode(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to parse UUID: {}", e),
            ))))?;

        sqlx::query!(
            r#"
            INSERT INTO "Chat" 
                (id, "userId", messages, title, summary, "createdAt", "updatedAt", "isGroupChat")
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (id) DO UPDATE SET
                messages = EXCLUDED.messages,
                title = COALESCE(EXCLUDED.title, "Chat".title),
                summary = COALESCE(EXCLUDED.summary, "Chat".summary),
                "updatedAt" = EXCLUDED."updatedAt",
                "isGroupChat" = EXCLUDED."isGroupChat"
            "#,
            id,
            user_id,
            messages,
            title,
            summary,
            now,
            now,
            is_group_chat
        )
        .execute(&*self.pool)
        .await?;

        Ok(())
    }

    fn parse_id_to_uuid(&self, id_str: &str) -> Result<Uuid> {
        Ok(id_str.parse::<Uuid>()?)
    }
}