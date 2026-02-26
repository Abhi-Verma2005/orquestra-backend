use axum::Json;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct ModelItem {
    pub id: String,
    pub name: String,
}

pub fn is_supported_model(model: &str) -> bool {
    matches!(
        model,
        "gpt-4o" | "gpt-4o-mini" | "gpt-4-turbo" | "gpt-3.5-turbo" | "o1-mini" | "o1"
    )
}

pub async fn list_models() -> Json<Vec<ModelItem>> {
    Json(vec![
        ModelItem {
            id: "gpt-4o".to_string(),
            name: "GPT-4o".to_string(),
        },
        ModelItem {
            id: "gpt-4o-mini".to_string(),
            name: "GPT-4o Mini".to_string(),
        },
        ModelItem {
            id: "gpt-4-turbo".to_string(),
            name: "GPT-4 Turbo".to_string(),
        },
        ModelItem {
            id: "gpt-3.5-turbo".to_string(),
            name: "GPT-3.5 Turbo".to_string(),
        },
        ModelItem {
            id: "o1-mini".to_string(),
            name: "o1 Mini".to_string(),
        },
        ModelItem {
            id: "o1".to_string(),
            name: "o1".to_string(),
        },
    ])
}
