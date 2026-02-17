use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use cb_db::models::{Agent, User, UserRole, UserStatus, Vps, VpsConfig, VpsState as DbVpsState};

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

impl AdminVpsResponse {
    pub fn new(v: Vps, provider: String) -> Self {
        Self {
            id: v.id,
            user_id: v.user_id,
            name: v.name,
            provider,
            state: v.state,
            address: v.address,
            created_at: v.created_at,
            updated_at: v.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AdminAgentResponse {
    pub id: Uuid,
    pub user_id: Uuid,
    pub name: String,
    pub vps_id: Option<Uuid>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<Agent> for AdminAgentResponse {
    fn from(a: Agent) -> Self {
        Self {
            id: a.id,
            user_id: a.user_id,
            name: a.name,
            vps_id: a.vps_id,
            created_at: a.created_at,
            updated_at: a.updated_at,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AdminVpsConfigResponse {
    pub id: Uuid,
    pub name: String,
    pub provider: String,
    pub image: Option<String>,
    pub location: Option<String>,
    pub cpu_millicores: i32,
    pub memory_mb: i32,
    pub disk_gb: i32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl From<VpsConfig> for AdminVpsConfigResponse {
    fn from(c: VpsConfig) -> Self {
        Self {
            id: c.id,
            name: c.name,
            provider: c.provider,
            image: c.image,
            location: c.location,
            cpu_millicores: c.cpu_millicores,
            memory_mb: c.memory_mb,
            disk_gb: c.disk_gb,
            created_at: c.created_at,
            updated_at: c.updated_at,
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

#[derive(Debug, Deserialize)]
pub struct CreateVpsConfigRequest {
    pub name: String,
    pub provider: String,
    pub image: Option<String>,
    pub location: Option<String>,
    pub cpu_millicores: i32,
    pub memory_mb: i32,
    pub disk_gb: i32,
}

#[derive(Debug, Deserialize)]
pub struct UpdateVpsConfigRequest {
    pub name: Option<String>,
    pub image: Option<Option<String>>,
    pub location: Option<Option<String>>,
    pub cpu_millicores: Option<i32>,
    pub memory_mb: Option<i32>,
    pub disk_gb: Option<i32>,
}

// ── Handlers ────────────────────────────────────────────────────────

pub async fn list_users(
    State(state): State<AppState>,
) -> Result<Json<Vec<AdminUserResponse>>, ApiError> {
    let users = User::list_all(&state.db).await?;
    Ok(Json(
        users.into_iter().map(AdminUserResponse::from).collect(),
    ))
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
    let mut responses = Vec::with_capacity(vpses.len());
    for vps in vpses {
        let provider = VpsConfig::get_by_id(&state.db, vps.vps_config_id)
            .await
            .map(|c| c.provider)
            .unwrap_or_default();
        responses.push(AdminVpsResponse::new(vps, provider));
    }
    Ok(Json(responses))
}

pub async fn stop_vps(
    State(state): State<AppState>,
    Path(vps_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let vps = Vps::get_by_id(&state.db, vps_id).await?;

    if vps.state != DbVpsState::Running {
        return Err(ApiError::Conflict("VPS is not running".into()));
    }

    let (provider, _config) = super::vps::provider_for_vps(&state, &vps).await?;

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

    if let Some(vm_id) = &vps.provider_vm_id
        && let Ok((provider, _config)) = super::vps::provider_for_vps(&state, &vps).await
    {
        let _ = provider
            .destroy_vps(&cb_infra::types::VpsId(vm_id.clone()))
            .await;
    }

    Vps::set_state(&state.db, vps_id, DbVpsState::Destroyed).await?;

    // Unassign from agent if linked
    let agents = Agent::list_for_user(&state.db, vps.user_id).await?;
    for agent in agents {
        if agent.vps_id == Some(vps_id) {
            Agent::assign_vps(&state.db, agent.id, None).await?;
        }
    }

    Ok(StatusCode::NO_CONTENT)
}

// ── Agent Admin ────────────────────────────────────────────────────

pub async fn list_all_agents(
    State(state): State<AppState>,
) -> Result<Json<Vec<AdminAgentResponse>>, ApiError> {
    let agents = Agent::list_all(&state.db).await?;
    Ok(Json(
        agents.into_iter().map(AdminAgentResponse::from).collect(),
    ))
}

pub async fn admin_delete_agent(
    State(state): State<AppState>,
    Path(agent_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let agent = Agent::get_by_id(&state.db, agent_id)
        .await
        .map_err(|_| ApiError::NotFound)?;

    // Destroy VPS if attached
    if let Some(vps_id) = agent.vps_id
        && let Ok(vps) = Vps::get_by_id(&state.db, vps_id).await
        && vps.state != DbVpsState::Destroyed
    {
        if let Some(ref vm_id) = vps.provider_vm_id
            && let Ok((provider, _config)) = super::vps::provider_for_vps(&state, &vps).await
        {
            let _ = provider
                .destroy_vps(&cb_infra::types::VpsId(vm_id.clone()))
                .await;
        }
        Vps::set_state(&state.db, vps.id, DbVpsState::Destroyed).await?;
    }

    Agent::delete(&state.db, agent_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── VpsConfig Admin ────────────────────────────────────────────────

pub async fn list_vps_configs(
    State(state): State<AppState>,
) -> Result<Json<Vec<AdminVpsConfigResponse>>, ApiError> {
    let configs = VpsConfig::list_all(&state.db).await?;
    Ok(Json(
        configs
            .into_iter()
            .map(AdminVpsConfigResponse::from)
            .collect(),
    ))
}

pub async fn create_vps_config(
    State(state): State<AppState>,
    Json(req): Json<CreateVpsConfigRequest>,
) -> Result<(StatusCode, Json<AdminVpsConfigResponse>), ApiError> {
    let config = VpsConfig::insert(
        &state.db,
        &req.name,
        &req.provider,
        req.image.as_deref(),
        req.location.as_deref(),
        req.cpu_millicores,
        req.memory_mb,
        req.disk_gb,
    )
    .await?;
    Ok((
        StatusCode::CREATED,
        Json(AdminVpsConfigResponse::from(config)),
    ))
}

pub async fn update_vps_config(
    State(state): State<AppState>,
    Path(config_id): Path<Uuid>,
    Json(req): Json<UpdateVpsConfigRequest>,
) -> Result<Json<AdminVpsConfigResponse>, ApiError> {
    // Verify exists
    VpsConfig::get_by_id(&state.db, config_id)
        .await
        .map_err(|_| ApiError::NotFound)?;

    let updated = VpsConfig::update(
        &state.db,
        config_id,
        req.name.as_deref(),
        req.image.as_ref().map(|o| o.as_deref()),
        req.location.as_ref().map(|o| o.as_deref()),
        req.cpu_millicores,
        req.memory_mb,
        req.disk_gb,
    )
    .await?;
    Ok(Json(AdminVpsConfigResponse::from(updated)))
}

pub async fn delete_vps_config(
    State(state): State<AppState>,
    Path(config_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    VpsConfig::get_by_id(&state.db, config_id)
        .await
        .map_err(|_| ApiError::NotFound)?;
    VpsConfig::delete(&state.db, config_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

// ── Cleanup ────────────────────────────────────────────────────────

// TODO: Add a time threshold (e.g. only destroy VPSes stuck in "provisioning"
// for more than 15 minutes) to avoid accidentally destroying VPSes that are
// legitimately still provisioning.
pub async fn cleanup_stuck(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let stuck = Vps::list_by_state(&state.db, DbVpsState::Provisioning).await?;
    let count = stuck.len();

    for vps in stuck {
        // Best-effort destroy at provider
        if let Some(ref vm_id) = vps.provider_vm_id
            && let Ok((provider, _config)) = super::vps::provider_for_vps(&state, &vps).await
        {
            let _ = provider
                .destroy_vps(&cb_infra::types::VpsId(vm_id.clone()))
                .await;
        }

        Vps::set_state(&state.db, vps.id, DbVpsState::Destroyed).await?;

        // Unassign from agent
        let agents = Agent::list_for_user(&state.db, vps.user_id).await?;
        for agent in agents {
            if agent.vps_id == Some(vps.id) {
                Agent::assign_vps(&state.db, agent.id, None).await?;
            }
        }
    }

    Ok(Json(serde_json::json!({ "cleaned_up": count })))
}
