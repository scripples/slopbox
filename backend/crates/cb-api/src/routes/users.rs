use axum::extract::State;
use axum::{Extension, Json};

use cb_db::models::{Plan, User};

use crate::auth::UserId;
use crate::dto::UserResponse;
use crate::error::ApiError;
use crate::state::AppState;

pub async fn get_me(
    State(state): State<AppState>,
    Extension(user_id): Extension<UserId>,
) -> Result<Json<UserResponse>, ApiError> {
    let user = User::get_by_id(&state.db, user_id.0).await?;

    let plan = match user.plan_id {
        Some(plan_id) => Plan::get_by_id(&state.db, plan_id).await.ok(),
        None => None,
    };

    Ok(Json(UserResponse::from_user(user, plan)))
}
