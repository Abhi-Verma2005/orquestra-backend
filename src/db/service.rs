use std::sync::Arc;
use serde_json::Value;
use uuid::Uuid;
// use anyhow::{Ok, Result};
use sqlx::{query, query_as, PgPool};

pub struct SessionStateRecord {
    pub chat_id: String,
    pub user_id: String,
    pub topic: Option<String>,
    pub current_step: String
}

pub struct ChatDbService {
    pub pool: Arc<PgPool>
}

impl ChatDbService {
    pub fn new(pool: Arc<PgPool>) -> Self {
        Self {pool}
    }

    pub async fn save_chat(
        &self,
        id_str: &String,
        user_id: &String,
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
                messages = "Chat".messages || EXCLUDED."messages",
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

    // this function if state is found returns it if not found creates it and returns
    pub async fn read_or_update_session_state(
        &self,
        chat_id_str: &String,
        user_id: &String,
        default_topic: &str
    ) -> Result<SessionStateRecord, sqlx::Error> {
        let chat_id = self.parse_id_to_uuid(chat_id_str)
            .map_err(|e| sqlx::Error::Decode(Box::new(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to parse UUID: {}", e),
            ))))?;

        if let Some(existing) = query_as!(
            SessionStateRecord,
            r#"
            SELECT
                "chatId" AS "chat_id!",
                "userId" AS "user_id!",
                "currentStep" AS "current_step!",
                topic
            FROM "SessionState"
            WHERE "chatId" = $1 AND "userId" = $2
            LIMIT 1
            "#,
            chat_id,
            user_id
        )
        .fetch_optional(&*self.pool)
        .await? {
            return Ok(existing)
        };

        let inserted = query_as!(
            SessionStateRecord,
            r#"
            INSERT INTO "SessionState" ("chatId", "userId", "currentStep", topic)
            VALUES ($1, $2, $3, $4)
            RETURNING 
                "chatId" AS "chat_id!",
                "userId" AS "user_id!",
                "currentStep" AS "current_step!",
                topic
            "#,
            chat_id,
            user_id,
            "introduction",
            default_topic
        )
        .fetch_one(&*self.pool)
        .await?;

        Ok(inserted)

    }

    fn parse_id_to_uuid(&self, id_str: &str) -> anyhow::Result<Uuid> {
        Ok(id_str.parse::<Uuid>()?)
    }


}