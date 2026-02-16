use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use cb_db::models::{User, UserRole, UserStatus, Vps, VpsState as DbVpsState};
use cb_infra::ProviderName;

use crate::error::ApiError;
use crate::state::AppState;

// ── DTOs ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct AdminUserResponse {
    pub id: Uuid,
    pub email: String,
    pub name: Option<String>,
    pub role: UserRole,
    pub status: UserStatus,
    pub plan_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<User> for AdminUserResponse {
    fn from(u: User) -> Self {
        Self {
            id: u.id,
            email: u.email,
            name: u.name,
            role: u.role,
            status: u.status,
            plan_id: u.plan_id,
            created_at: u.created_at,
            updated_at: u.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AdminVpsResponse {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub provider: String,
    pub state: DbVpsState,
    pub address: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Vps> for AdminVpsResponse {
    fn from(v: Vps) -> Self {
        Self {
            id: v.id,
            user_id: v.user_id,
            name: v.name,
            provider: v.provider,
            state: v.state,
            address: v.address,
            created_at: v.created_at,
            updated_at: v.updated_at,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct SetStatusRequest {
    pub status: UserStatus,
}

#[derive(Debug, Deserialize)]
pub struct SetRoleRequest {
    pub role: UserRole,
}

// ── Handlers ────────────────────────────────────────────────────────

pub async fn list_users(
    State(state): State<AppState>,
) -> Result<Json<Vec<AdminUserResponse>>, ApiError> {
    let users = User::list_all(&state.db).await?;
    Ok(Json(users.into_iter().map(AdminUserResponse::from).collect()))
}

pub async fn set_user_status(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
    Json(req): Json<SetStatusRequest>,
) -> Result<StatusCode, ApiError> {
    // Verify user exists
    User::get_by_id(&state.db, user_id).await?;

    // If activating a user, auto-assign the demo plan if they don't have one
    if req.status == UserStatus::Active {
        let user = User::get_by_id(&state.db, user_id).await?;
        if user.plan_id.is_none() {
            let plans = cb_db::models::Plan::list(&state.db).await?;
            if let Some(demo_plan) = plans.iter().find(|p| p.name == "demo") {
                User::set_plan(&state.db, user_id, Some(demo_plan.id)).await?;
            }
        }
    }

    User::set_status(&state.db, user_id, req.status).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn set_user_role(
    State(state): State<AppState>,
    Path(user_id): Path<Uuid>,
    Json(req): Json<SetRoleRequest>,
) -> Result<StatusCode, ApiError> {
    User::get_by_id(&state.db, user_id).await?;
    User::set_role(&state.db, user_id, req.role).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn list_vpses(
    State(state): State<AppState>,
) -> Result<Json<Vec<AdminVpsResponse>>, ApiError> {
    let vpses = Vps::list_all(&state.db).await?;
    Ok(Json(vpses.into_iter().map(AdminVpsResponse::from).collect()))
}

pub async fn stop_vps(
    State(state): State<AppState>,
    Path(vps_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let vps = Vps::get_by_id(&state.db, vps_id).await?;

    if vps.state != DbVpsState::Running {
        return Err(ApiError::Conflict("VPS is not running".into()));
    }

    let provider_name: ProviderName = vps
        .provider
        .parse()
        .map_err(|_| ApiError::Internal(format!("unknown provider: {}", vps.provider)))?;

    let provider = state
        .providers
        .get(provider_name)
        .ok_or_else(|| ApiError::Internal(format!("provider {} not configured", vps.provider)))?;

    let vm_id = vps
        .provider_vm_id
        .as_deref()
        .ok_or_else(|| ApiError::Internal("VPS has no provider VM ID".into()))?;

    provider
        .stop_vps(&cb_infra::types::VpsId(vm_id.to_string()))
        .await?;

    Vps::set_state(&state.db, vps_id, DbVpsState::Stopped).await?;

    Ok(StatusCode::NO_CONTENT)
}

pub async fn destroy_vps(
    State(state): State<AppState>,
    Path(vps_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let vps = Vps::get_by_id(&state.db, vps_id).await?;

    if vps.state == DbVpsState::Destroyed {
        return Err(ApiError::Conflict("VPS is already destroyed".into()));
    }

    let provider_name: ProviderName = vps
        .provider
        .parse()
        .map_err(|_| ApiError::Internal(format!("unknown provider: {}", vps.provider)))?;

    let provider = state
        .providers
        .get(provider_name)
        .ok_or_else(|| ApiError::Internal(format!("provider {} not configured", vps.provider)))?;

    if let Some(vm_id) = &vps.provider_vm_id {
        let _ = provider
            .destroy_vps(&cb_infra::types::VpsId(vm_id.clone()))
            .await;
    }

    Vps::set_state(&state.db, vps_id, DbVpsState::Destroyed).await?;

    // Unassign from agent if linked
    let agents = cb_db::models::Agent::list_for_user(&state.db, vps.user_id).await?;
    for agent in agents {
        if agent.vps_id == Some(vps_id) {
            cb_db::models::Agent::assign_vps(&state.db, agent.id, None).await?;
        }
    }

    Ok(StatusCode::NO_CONTENT)
}
