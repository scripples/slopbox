use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use uuid::Uuid;

use cb_db::models::{Agent, AgentChannel};

use crate::auth::UserId;
use crate::dto::{AddChannelRequest, ChannelResponse};
use crate::error::ApiError;
use crate::state::AppState;

const VALID_CHANNEL_KINDS: &[&str] = &["telegram", "whatsapp", "discord", "slack", "signal"];

/// POST /agents/{id}/channels
pub async fn add_channel(
    State(state): State<AppState>,
    Extension(user_id): Extension<UserId>,
    Path(agent_id): Path<Uuid>,
    Json(req): Json<AddChannelRequest>,
) -> Result<(StatusCode, Json<ChannelResponse>), ApiError> {
    let agent = Agent::get_by_id(&state.db, agent_id)
        .await
        .map_err(|_| ApiError::NotFound)?;

    if agent.user_id != user_id.0 {
        return Err(ApiError::NotFound);
    }

    if !VALID_CHANNEL_KINDS.contains(&req.channel_kind.as_str()) {
        return Err(ApiError::BadRequest(format!(
            "unknown channel kind: {}",
            req.channel_kind
        )));
    }

    // Check for duplicates
    if AgentChannel::get_by_agent_and_kind(&state.db, agent_id, &req.channel_kind)
        .await
        .is_ok()
    {
        return Err(ApiError::Conflict(format!(
            "agent already has a {} channel",
            req.channel_kind
        )));
    }

    let channel =
        AgentChannel::insert(&state.db, agent_id, &req.channel_kind, &req.credentials).await?;

    Ok((StatusCode::CREATED, Json(ChannelResponse::from(channel))))
}

/// GET /agents/{id}/channels
pub async fn list_channels(
    State(state): State<AppState>,
    Extension(user_id): Extension<UserId>,
    Path(agent_id): Path<Uuid>,
) -> Result<Json<Vec<ChannelResponse>>, ApiError> {
    let agent = Agent::get_by_id(&state.db, agent_id)
        .await
        .map_err(|_| ApiError::NotFound)?;

    if agent.user_id != user_id.0 {
        return Err(ApiError::NotFound);
    }

    let channels = AgentChannel::list_for_agent(&state.db, agent_id).await?;
    let responses: Vec<ChannelResponse> = channels.into_iter().map(ChannelResponse::from).collect();

    Ok(Json(responses))
}

/// DELETE /agents/{id}/channels/{kind}
pub async fn remove_channel(
    State(state): State<AppState>,
    Extension(user_id): Extension<UserId>,
    Path((agent_id, kind)): Path<(Uuid, String)>,
) -> Result<StatusCode, ApiError> {
    let agent = Agent::get_by_id(&state.db, agent_id)
        .await
        .map_err(|_| ApiError::NotFound)?;

    if agent.user_id != user_id.0 {
        return Err(ApiError::NotFound);
    }

    if !VALID_CHANNEL_KINDS.contains(&kind.as_str()) {
        return Err(ApiError::BadRequest(format!(
            "unknown channel kind: {kind}"
        )));
    }

    AgentChannel::delete_by_agent_and_kind(&state.db, agent_id, &kind).await?;

    Ok(StatusCode::NO_CONTENT)
}
