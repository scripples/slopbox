use axum::extract::Request;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use jsonwebtoken::{Algorithm, DecodingKey, Validation};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use uuid::Uuid;

use cb_db::models::{User, UserRole, UserStatus};

use crate::error::ApiError;
use crate::state::AppState;

/// Extracted user ID from JWT `sub` claim, injected into extensions.
#[derive(Debug, Clone, Copy)]
pub struct UserId(pub Uuid);

#[derive(Debug, Serialize, Deserialize)]
pub struct JwtClaims {
    pub sub: String,
    pub email: Option<String>,
    pub exp: Option<u64>,
}

/// Validate a JWT token and extract the user ID from the `sub` claim.
pub fn validate_jwt(token: &str, secret: &str) -> Result<UserId, ApiError> {
    let key = DecodingKey::from_secret(secret.as_bytes());
    let mut validation = Validation::new(Algorithm::HS256);
    validation.required_spec_claims.clear();
    validation.validate_exp = false;

    let data = jsonwebtoken::decode::<JwtClaims>(token, &key, &validation)
        .map_err(|_| ApiError::Unauthorized)?;

    let user_id = Uuid::parse_str(&data.claims.sub)
        .map_err(|_| ApiError::Unauthorized)?;

    Ok(UserId(user_id))
}

/// Extract JWT from `Authorization: Bearer <token>` header.
fn extract_bearer(req: &Request) -> Option<&str> {
    req.headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
}

/// Middleware that validates JWT from `Authorization: Bearer <jwt>` and
/// extracts user_id from the `sub` claim.
pub async fn auth_middleware(
    axum::extract::State(state): axum::extract::State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    let token = match extract_bearer(&req) {
        Some(t) => t,
        None => return ApiError::Unauthorized.into_response(),
    };

    match validate_jwt(token, &state.config.jwt_secret) {
        Ok(user_id) => {
            req.extensions_mut().insert(user_id);
            next.run(req).await
        }
        Err(e) => e.into_response(),
    }
}

/// Middleware that checks the user's status is Active.
/// Must run after auth_middleware (requires UserId in extensions).
pub async fn status_middleware(
    axum::extract::State(state): axum::extract::State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let user_id = match req.extensions().get::<UserId>() {
        Some(id) => id.0,
        None => return ApiError::Unauthorized.into_response(),
    };

    let user = match User::get_by_id(&state.db, user_id).await {
        Ok(u) => u,
        Err(_) => return ApiError::Unauthorized.into_response(),
    };

    if user.status != UserStatus::Active {
        return ApiError::Forbidden(format!("account status: {:?}", user.status).to_lowercase())
            .into_response();
    }

    next.run(req).await
}

/// Middleware that checks the user has admin role.
/// Must run after auth_middleware (requires UserId in extensions).
pub async fn admin_middleware(
    axum::extract::State(state): axum::extract::State<AppState>,
    req: Request,
    next: Next,
) -> Response {
    let user_id = match req.extensions().get::<UserId>() {
        Some(id) => id.0,
        None => return ApiError::Unauthorized.into_response(),
    };

    let user = match User::get_by_id(&state.db, user_id).await {
        Ok(u) => u,
        Err(_) => return ApiError::Unauthorized.into_response(),
    };

    if user.role != UserRole::Admin {
        return ApiError::Forbidden("admin access required".into()).into_response();
    }

    if user.status != UserStatus::Active {
        return ApiError::Forbidden(format!("account status: {:?}", user.status).to_lowercase())
            .into_response();
    }

    next.run(req).await
}

/// Authenticate a gateway WebSocket or HTTP request via JWT.
///
/// For WebSocket: accepts JWT via `?token=<jwt>` query param
/// (browsers can't set headers on WS upgrade).
/// For HTTP: accepts JWT via `Authorization: Bearer` header.
pub fn authenticate_gateway_request(
    headers: &axum::http::HeaderMap,
    query: Option<&str>,
    jwt_secret: &str,
) -> Option<UserId> {
    // Try query param first (WebSocket)
    if let Some(query) = query {
        for param in query.split('&') {
            if let Some(token) = param.strip_prefix("token=")
                && let Ok(uid) = validate_jwt(token, jwt_secret)
            {
                return Some(uid);
            }
        }
    }

    // Try Authorization header (HTTP)
    let token = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))?;

    validate_jwt(token, jwt_secret).ok()
}

/// Authenticate a user via Auth.js session cookie (kept for backward compatibility).
#[allow(dead_code)]
pub async fn authenticate_session_cookie(
    headers: &axum::http::HeaderMap,
    db: &PgPool,
) -> Option<UserId> {
    let cookie_header = headers.get("cookie")?.to_str().ok()?;

    let token = cookie_header
        .split(';')
        .map(|s| s.trim())
        .find_map(|cookie| {
            cookie
                .strip_prefix("__Secure-authjs.session-token=")
                .or_else(|| cookie.strip_prefix("authjs.session-token="))
        })?;

    let session = cb_db::models::Session::get_valid_by_token(db, token)
        .await
        .ok()
        .flatten()?;

    Some(UserId(session.user_id))
}
