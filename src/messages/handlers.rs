use std::{convert::Infallible, pin::Pin, time::Duration};

use axum::{
    extract::{Path, Query, State},
    http::{header::AUTHORIZATION, HeaderMap, StatusCode},
    response::{sse::Event, IntoResponse, Sse},
    Json,
};
use futures::Stream;
use serde::Deserialize;
use serde_json::json;
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::{
    ai::agent::{
        build_chat_messages, build_context_window, build_system_prompt, AgentEvent, AgentConfig,
        AgentRunner,
    },
    auth::{middleware::AuthUser, models::User, verify_token},
    conversations::handlers::get_conversation_for_user,
    error::AppError,
    messages::models::{Message, MessageResponse, SendMessageRequest, SseEvent},
    AppState,
};

#[derive(Debug, Deserialize)]
pub struct StreamQueryParams {
    pub message: String,
    pub token: Option<String>,
}

async fn load_messages(db: &sqlx::PgPool, conversation_id: Uuid) -> Result<Vec<Message>, AppError> {
    let rows = sqlx::query_as::<_, Message>(
        r#"SELECT id, conversation_id, role, content, created_at
           FROM messages
           WHERE conversation_id = $1
           ORDER BY created_at ASC"#,
    )
    .bind(conversation_id)
    .fetch_all(db)
    .await?;

    Ok(rows)
}

async fn load_user(db: &sqlx::PgPool, user_id: Uuid) -> Result<User, AppError> {
    let user = sqlx::query_as::<_, User>(
        r#"SELECT id, first_name, last_name, email, password_hash, created_at, updated_at
           FROM users
           WHERE id = $1"#,
    )
    .bind(user_id)
    .fetch_optional(db)
    .await?
    .ok_or_else(|| AppError::Unauthorized("User not found".to_string()))?;

    Ok(user)
}

async fn save_assistant_message(
    db: &sqlx::PgPool,
    conversation_id: Uuid,
    content: &str,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO messages (conversation_id, role, content) VALUES ($1, 'assistant', $2)",
    )
    .bind(conversation_id)
    .bind(content)
    .execute(db)
    .await?;

    Ok(())
}

fn parse_auth_from_header(headers: &HeaderMap) -> Option<String> {
    headers
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|raw| raw.strip_prefix("Bearer "))
        .map(ToOwned::to_owned)
}

fn authenticate_stream_request(
    headers: &HeaderMap,
    params: &StreamQueryParams,
    jwt_secret: &str,
) -> Result<Uuid, AppError> {
    let token = parse_auth_from_header(headers)
        .or_else(|| params.token.clone())
        .ok_or_else(|| AppError::Unauthorized("Missing auth token".to_string()))?;

    let claims = verify_token(&token, jwt_secret)?;
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::Unauthorized("Invalid token subject".to_string()))?;

    Ok(user_id)
}

fn sse_json(event_type: &str, data: String) -> Event {
    let body = serde_json::to_string(&SseEvent {
        event_type: event_type.to_string(),
        data,
    })
    .unwrap_or_else(|_| "{\"type\":\"error\",\"data\":\"serialization failed\"}".to_string());

    Event::default().data(body)
}

type EventStream = Pin<Box<dyn Stream<Item = Result<Event, Infallible>> + Send>>;

fn single_event_stream(event_type: &str, data: String) -> Sse<EventStream> {
    let event = sse_json(event_type, data);
    let stream: EventStream = Box::pin(tokio_stream::once(Ok(event)));
    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

pub async fn list(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
) -> Result<impl IntoResponse, AppError> {
    let _ = get_conversation_for_user(&state.db, id, auth.user_id).await?;
    let rows = load_messages(&state.db, id).await?;
    let response: Vec<MessageResponse> = rows.into_iter().map(MessageResponse::from).collect();
    Ok((StatusCode::OK, Json(response)))
}

pub async fn send(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(id): Path<Uuid>,
    Json(req): Json<SendMessageRequest>,
) -> Result<impl IntoResponse, AppError> {
    let _ = get_conversation_for_user(&state.db, id, auth.user_id).await?;

    if req.content.trim().is_empty() {
        return Err(AppError::BadRequest(
            "Message content cannot be empty".to_string(),
        ));
    }
    if req.content.len() > 32000 {
        return Err(AppError::BadRequest(
            "Message content too long (max 32000 chars)".to_string(),
        ));
    }

    let message = sqlx::query_as::<_, Message>(
        r#"INSERT INTO messages (conversation_id, role, content)
           VALUES ($1, 'user', $2)
           RETURNING id, conversation_id, role, content, created_at"#,
    )
    .bind(id)
    .bind(req.content.trim())
    .fetch_one(&state.db)
    .await?;

    sqlx::query("UPDATE conversations SET updated_at = NOW() WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;

    Ok((StatusCode::CREATED, Json(MessageResponse::from(message))))
}

pub async fn stream_handler(
    Path(conversation_id): Path<Uuid>,
    Query(params): Query<StreamQueryParams>,
    headers: HeaderMap,
    State(state): State<AppState>,
) -> Result<Sse<EventStream>, AppError> {
    let decoded = match urlencoding::decode(&params.message) {
        Ok(v) => v.into_owned(),
        Err(_) => {
            return Ok(single_event_stream(
                "error",
                "Invalid message encoding".to_string(),
            ));
        }
    };

    if decoded.trim().is_empty() {
        return Ok(single_event_stream(
            "error",
            "Message content cannot be empty".to_string(),
        ));
    }

    if decoded.len() > 32000 {
        return Ok(single_event_stream(
            "error",
            "Message content too long (max 32000 chars)".to_string(),
        ));
    }

    let user_id = match authenticate_stream_request(&headers, &params, &state.config.jwt_secret) {
        Ok(id) => id,
        Err(_) => {
            return Ok(single_event_stream(
                "error",
                "401 Invalid or expired token".to_string(),
            ));
        }
    };

    let conversation = match get_conversation_for_user(&state.db, conversation_id, user_id).await {
        Ok(c) => c,
        Err(e) => return Ok(single_event_stream("error", format!("{e:?}"))),
    };

    let messages = match load_messages(&state.db, conversation_id).await {
        Ok(m) => m,
        Err(e) => return Ok(single_event_stream("error", format!("{e:?}"))),
    };

    let user = match load_user(&state.db, user_id).await {
        Ok(u) => u,
        Err(e) => return Ok(single_event_stream("error", format!("{e:?}"))),
    };

    let context_window = build_context_window(&messages);
    let system_prompt = build_system_prompt(
        conversation.system_prompt.as_deref(),
        &user.first_name,
        &user.last_name,
        &user.email,
    );

    let agent_config = AgentConfig {
        model: conversation.model.clone(),
        temperature: conversation.temperature,
        openai_api_key: state.config.openai_api_key.clone(),
    };

    let initial_messages = match build_chat_messages(&context_window, &decoded, &system_prompt) {
        Ok(m) => m,
        Err(e) => return Ok(single_event_stream("error", e.to_string())),
    };

    let (event_tx, mut event_rx) = mpsc::unbounded_channel::<AgentEvent>();
    let agent = AgentRunner::new(agent_config, state.tools.clone());

    let db = state.db.clone();
    let conv_id = conversation_id;
    tokio::spawn(async move {
        match agent.run(initial_messages, event_tx.clone()).await {
            Ok(response) => {
                if !response.trim().is_empty() {
                    let _ = save_assistant_message(&db, conv_id, &response).await;
                }

                let _ = sqlx::query("UPDATE conversations SET updated_at = NOW() WHERE id = $1")
                    .bind(conv_id)
                    .execute(&db)
                    .await;
            }
            Err(e) => {
                let _ = event_tx.send(AgentEvent::Error(e.to_string()));
            }
        }
    });

    let stream = async_stream::stream! {
        yield Ok(sse_json("start", String::new()));

        while let Some(event) = event_rx.recv().await {
            let payload = match event {
                AgentEvent::ToolCallsStarted { calls } => {
                    json!({ "type": "tool_calls", "data": calls })
                }
                AgentEvent::ToolExecuting { id, name, arguments } => {
                    json!({ "type": "tool_start", "id": id, "name": name, "arguments": arguments })
                }
                AgentEvent::ToolResult { id, name, result } => {
                    json!({ "type": "tool_result", "id": id, "name": name, "result": result })
                }
                AgentEvent::Token(token) => {
                    json!({ "type": "token", "data": token })
                }
                AgentEvent::Done => {
                    yield Ok(Event::default().data(json!({ "type": "done" }).to_string()));
                    break;
                }
                AgentEvent::Error(error) => {
                    yield Ok(Event::default().data(json!({ "type": "error", "data": error }).to_string()));
                    break;
                }
            };

            yield Ok(Event::default().data(payload.to_string()));
        }
    };

    let stream: EventStream = Box::pin(stream);
    Ok(Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    ))
}
