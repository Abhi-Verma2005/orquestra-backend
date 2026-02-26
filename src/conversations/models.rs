use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Conversation {
    pub id: Uuid,
    pub user_id: Uuid,
    pub title: String,
    pub model: String,
    pub system_prompt: Option<String>,
    pub temperature: f64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
pub struct CreateConversationRequest {
    pub title: Option<String>,
    pub model: Option<String>,
    pub system_prompt: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateConversationSettingsRequest {
    pub model: Option<String>,
    pub system_prompt: Option<Option<String>>,
    pub temperature: Option<f64>,
}

#[derive(Debug, Deserialize)]
pub struct UpdateTitleRequest {
    pub title: String,
}

#[derive(Debug, Serialize)]
pub struct ConversationResponse {
    pub id: Uuid,
    pub title: String,
    pub model: String,
    pub system_prompt: Option<String>,
    pub temperature: f64,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Conversation> for ConversationResponse {
    fn from(value: Conversation) -> Self {
        Self {
            id: value.id,
            title: value.title,
            model: value.model,
            system_prompt: value.system_prompt,
            temperature: value.temperature,
            created_at: value.created_at,
            updated_at: value.updated_at,
        }
    }
}
