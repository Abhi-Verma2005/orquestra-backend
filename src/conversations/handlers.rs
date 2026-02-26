use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use uuid::Uuid;

use crate::{
    auth::middleware::AuthUser,
    conversations::models::{
        Conversation, ConversationResponse, CreateConversationRequest, UpdateConversationSettingsRequest,
        UpdateTitleRequest,
    },
    error::AppError,
    models_list::handlers::is_supported_model,
    AppState,
};

pub async fn get_conversation_for_user(
    db: &sqlx::PgPool,
    conversation_id: Uuid,
    user_id: Uuid,
) -> Result<Conversation, AppError> {
    let conversation = sqlx::query_as::<_, Conversation>(
        r#"SELECT id, user_id, title, model, system_prompt, temperature, created_at, updated_at
           FROM conversations
           WHERE id = $1"#,
    )
    .bind(conversation_id)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| AppError::NotFound("Conversation not found".to_string()))?;

    if conversation.user_id != user_id {
        return Err(AppError::Forbidden(
            "You are not allowed to access this conversation".to_string(),
        ));
    }

    Ok(conversation)
}

pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, AppError> {
    let rows = sqlx::query_as::<_, Conversation>(
        r#"SELECT id, user_id, title, model, system_prompt, temperature, created_at, updated_at
           FROM conversations
           WHERE user_id = $1
           ORDER BY updated_at DESC"#,
    )
    .bind(auth.user_id)
    .fetch_all(&state.db)
    .await?;

    let res: Vec<ConversationResponse> = rows.into_iter().map(ConversationResponse::from).collect();
    Ok((StatusCode::OK, Json(res)))
}

pub async fn get(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let conversation = get_conversation_for_user(&state.db, id, auth.user_id).await?;
    Ok((StatusCode::OK, Json(ConversationResponse::from(conversation))))
}

pub async fn create(
    State(state): State<AppState>,
    auth: AuthUser,
    Json(req): Json<CreateConversationRequest>,
) -> Result<impl IntoResponse, AppError> {
    let title = req
        .title
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("New Conversation")
        .to_string();

    if title.len() > 500 {
        return Err(AppError::BadRequest("Title must be <= 500 characters".to_string()));
    }

    let model = req.model.unwrap_or_else(|| "gpt-4o".to_string());
    if !is_supported_model(&model) {
        return Err(AppError::BadRequest("Unsupported model".to_string()));
    }

    let conversation = sqlx::query_as::<_, Conversation>(
        r#"INSERT INTO conversations (user_id, title, model, system_prompt)
           VALUES ($1, $2, $3, $4)
           RETURNING id, user_id, title, model, system_prompt, temperature, created_at, updated_at"#,
    )
    .bind(auth.user_id)
    .bind(title)
    .bind(model)
    .bind(req.system_prompt)
    .fetch_one(&state.db)
    .await?;

    Ok((
        StatusCode::CREATED,
        Json(ConversationResponse::from(conversation)),
    ))
}

pub async fn update_settings(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateConversationSettingsRequest>,
) -> Result<impl IntoResponse, AppError> {
    let current = get_conversation_for_user(&state.db, id, auth.user_id).await?;

    if let Some(temp) = req.temperature {
        if !(0.0..=2.0).contains(&temp) {
            return Err(AppError::BadRequest(
                "Temperature must be between 0.0 and 2.0".to_string(),
            ));
        }
    }

    if let Some(model) = req.model.as_deref() {
        if !is_supported_model(model) {
            return Err(AppError::BadRequest("Unsupported model".to_string()));
        }
    }

    let model = req.model.unwrap_or(current.model);
    let system_prompt = req.system_prompt.unwrap_or(current.system_prompt);
    let temperature = req.temperature.unwrap_or(current.temperature);

    let updated = sqlx::query_as::<_, Conversation>(
        r#"UPDATE conversations
           SET model = $1,
               system_prompt = $2,
               temperature = $3,
               updated_at = NOW()
           WHERE id = $4 AND user_id = $5
           RETURNING id, user_id, title, model, system_prompt, temperature, created_at, updated_at"#,
    )
    .bind(model)
    .bind(system_prompt)
    .bind(temperature)
    .bind(id)
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::OK, Json(ConversationResponse::from(updated))))
}

pub async fn update_title(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<UpdateTitleRequest>,
) -> Result<impl IntoResponse, AppError> {
    let title = req.title.trim();
    if title.is_empty() {
        return Err(AppError::BadRequest("Title is required".to_string()));
    }
    if title.len() > 500 {
        return Err(AppError::BadRequest("Title must be <= 500 characters".to_string()));
    }

    let _ = get_conversation_for_user(&state.db, id, auth.user_id).await?;

    let updated = sqlx::query_as::<_, Conversation>(
        r#"UPDATE conversations
           SET title = $1,
               updated_at = NOW()
           WHERE id = $2 AND user_id = $3
           RETURNING id, user_id, title, model, system_prompt, temperature, created_at, updated_at"#,
    )
    .bind(title)
    .bind(id)
    .bind(auth.user_id)
    .fetch_one(&state.db)
    .await?;

    Ok((StatusCode::OK, Json(ConversationResponse::from(updated))))
}

pub async fn delete(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let _ = get_conversation_for_user(&state.db, id, auth.user_id).await?;

    sqlx::query("DELETE FROM conversations WHERE id = $1 AND user_id = $2")
        .bind(id)
        .bind(auth.user_id)
        .execute(&state.db)
        .await?;

    Ok(StatusCode::NO_CONTENT)
}
