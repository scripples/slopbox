use axum::extract::Request;
use axum::http::HeaderMap;
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::ApiError;
use crate::state::AppState;

/// Extracted user ID from `X-User-Id` header, injected into extensions.
#[derive(Debug, Clone, Copy)]
pub struct UserId(pub Uuid);

/// Middleware that validates `Authorization: Bearer <key>` against the
/// configured `CONTROL_PLANE_API_KEY` and extracts `X-User-Id`.
pub async fn auth_middleware(
    axum::extract::State(state): axum::extract::State<AppState>,
    mut req: Request,
    next: Next,
) -> Response {
    match validate_request(&state, &req) {
        Ok(user_id) => {
            req.extensions_mut().insert(user_id);
            next.run(req).await
        }
        Err(e) => e.into_response(),
    }
}

/// Authenticate a user via Auth.js session cookie.
///
/// Looks for `authjs.session-token` or `__Secure-authjs.session-token`
/// in the Cookie header, then validates against the sessions table.
/// Returns `None` if no valid session is found.
pub async fn authenticate_session_cookie(
    headers: &HeaderMap,
    db: &PgPool,
) -> Option<UserId> {
    let cookie_header = headers.get("cookie")?.to_str().ok()?;

    // Parse cookies — look for both secure and non-secure variants
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

fn validate_request(state: &AppState, req: &Request) -> Result<UserId, ApiError> {
    let auth_header = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok());

    let token = auth_header
        .and_then(|h| h.strip_prefix("Bearer "))
        .ok_or(ApiError::Unauthorized)?;

    if token != state.config.control_plane_api_key {
        return Err(ApiError::Unauthorized);
    }

    let user_id = req
        .headers()
        .get("x-user-id")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| Uuid::parse_str(s).ok())
        .ok_or(ApiError::BadRequest(
            "missing or invalid X-User-Id header".into(),
        ))?;

    Ok(UserId(user_id))
}
