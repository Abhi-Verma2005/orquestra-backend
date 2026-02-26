use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};

use crate::{
    auth::{create_token, middleware::AuthUser, models::*},
    error::AppError,
    AppState,
};

fn validate_registration(req: &RegisterRequest) -> Result<(), AppError> {
    if req.first_name.trim().is_empty() {
        return Err(AppError::BadRequest("First name is required".to_string()));
    }
    if req.last_name.trim().is_empty() {
        return Err(AppError::BadRequest("Last name is required".to_string()));
    }
    if req.email.trim().is_empty() || !req.email.contains('@') {
        return Err(AppError::BadRequest("Valid email is required".to_string()));
    }
    if req.password.len() < 8 {
        return Err(AppError::BadRequest(
            "Password must be at least 8 characters".to_string(),
        ));
    }
    Ok(())
}

pub async fn register(
    State(state): State<AppState>,
    Json(req): Json<RegisterRequest>,
) -> Result<impl IntoResponse, AppError> {
    validate_registration(&req)?;

    let email = req.email.trim().to_lowercase();
    let exists: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM users WHERE email = $1")
        .bind(&email)
        .fetch_optional(&state.db)
        .await?;

    if exists.is_some() {
        return Err(AppError::Conflict("Email already in use".to_string()));
    }

    let password_hash = bcrypt::hash(&req.password, 12)
        .map_err(|e| AppError::InternalError(anyhow::anyhow!("Password hashing failed: {e}")))?;

    let user: User = sqlx::query_as(
        r#"
        INSERT INTO users (first_name, last_name, email, password_hash)
        VALUES ($1, $2, $3, $4)
        RETURNING id, first_name, last_name, email, password_hash, created_at, updated_at
        "#,
    )
    .bind(req.first_name.trim())
    .bind(req.last_name.trim())
    .bind(&email)
    .bind(password_hash)
    .fetch_one(&state.db)
    .await?;

    let token = create_token(user.id, &state.config.jwt_secret)?;
    let resp = AuthResponse {
        token,
        user: user.into(),
    };

    Ok((StatusCode::CREATED, Json(resp)))
}

pub async fn login(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<impl IntoResponse, AppError> {
    if req.email.trim().is_empty() || req.password.is_empty() {
        return Err(AppError::BadRequest(
            "Email and password are required".to_string(),
        ));
    }

    let email = req.email.trim().to_lowercase();
    let user = sqlx::query_as::<_, User>(
        r#"SELECT id, first_name, last_name, email, password_hash, created_at, updated_at
           FROM users
           WHERE email = $1"#,
    )
    .bind(&email)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::Unauthorized("Invalid credentials".to_string()))?;

    let valid = bcrypt::verify(&req.password, &user.password_hash)
        .map_err(|_| AppError::Unauthorized("Invalid credentials".to_string()))?;

    if !valid {
        return Err(AppError::Unauthorized("Invalid credentials".to_string()));
    }

    let token = create_token(user.id, &state.config.jwt_secret)?;
    Ok((
        StatusCode::OK,
        Json(AuthResponse {
            token,
            user: user.into(),
        }),
    ))
}

pub async fn me(
    State(state): State<AppState>,
    auth: AuthUser,
) -> Result<impl IntoResponse, AppError> {
    let user = sqlx::query_as::<_, User>(
        r#"SELECT id, first_name, last_name, email, password_hash, created_at, updated_at
           FROM users
           WHERE id = $1"#,
    )
    .bind(auth.user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::Unauthorized("User not found".to_string()))?;

    Ok((StatusCode::OK, Json(UserResponse::from(user))))
}
