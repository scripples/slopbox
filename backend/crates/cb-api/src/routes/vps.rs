use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::{Extension, Json};
use uuid::Uuid;

use cb_db::models::{Agent, Plan, User, Vps, VpsConfig, VpsState};
use cb_infra::ProviderName;
use cb_infra::types::{VpsId, VpsSpec};

use crate::auth::UserId;
use crate::dto::{ProvisionVpsRequest, VpsResponse};
use crate::error::ApiError;
use crate::state::AppState;

pub async fn provision_vps(
    State(state): State<AppState>,
    Extension(user_id): Extension<UserId>,
    Path(agent_id): Path<Uuid>,
    Json(req): Json<ProvisionVpsRequest>,
) -> Result<(StatusCode, Json<VpsResponse>), ApiError> {
    let agent = Agent::get_by_id(&state.db, agent_id)
        .await
        .map_err(|_| ApiError::NotFound)?;

    if agent.user_id != user_id.0 {
        return Err(ApiError::NotFound);
    }

    if agent.vps_id.is_some() {
        return Err(ApiError::Conflict("agent already has a VPS".into()));
    }

    // Check VPS count limit
    let user = User::get_by_id(&state.db, user_id.0).await?;
    let plan_id = user
        .plan_id
        .ok_or(ApiError::LimitExceeded("user has no plan".into()))?;
    let plan = Plan::get_by_id(&state.db, plan_id).await?;

    let vps_count = Vps::count_for_user(&state.db, user_id.0).await?;
    if vps_count >= plan.max_vpses as i64 {
        return Err(ApiError::LimitExceeded(format!(
            "VPS limit reached ({}/{})",
            vps_count, plan.max_vpses
        )));
    }

    // Validate vps_config belongs to the user's plan
    let allowed_configs = VpsConfig::list_for_plan(&state.db, plan_id).await?;
    if !allowed_configs.iter().any(|c| c.id == req.vps_config_id) {
        return Err(ApiError::BadRequest(
            "VPS config not available on your plan".into(),
        ));
    }

    let vps_config = VpsConfig::get_by_id(&state.db, req.vps_config_id).await?;

    // Derive provider from VpsConfig
    let provider_name: ProviderName = vps_config.provider.parse().map_err(|_| {
        ApiError::Internal(format!(
            "unknown provider in VPS config: {}",
            vps_config.provider
        ))
    })?;
    let provider = state
        .providers
        .get(provider_name)
        .ok_or_else(|| ApiError::BadRequest(format!("provider not available: {provider_name}")))?;

    // Insert VPS in Provisioning state
    let vps_name = format!("agent-{}", agent_id);
    let vps = Vps::insert(&state.db, user_id.0, req.vps_config_id, &vps_name).await?;

    // Assign VPS to agent
    Agent::assign_vps(&state.db, agent_id, Some(vps.id)).await?;

    // Create VM
    let mut env = HashMap::new();
    env.insert("OPENCLAW_GATEWAY_TOKEN".into(), agent.gateway_token.clone());

    // Proxy env vars — all outbound traffic flows through the control plane proxy
    let proxy_url = format!(
        "https://{}:{}@{}",
        agent.id, agent.gateway_token, state.config.proxy_external_addr
    );
    env.insert("HTTP_PROXY".into(), proxy_url.clone());
    env.insert("HTTPS_PROXY".into(), proxy_url.clone());
    env.insert("http_proxy".into(), proxy_url.clone());
    env.insert("https_proxy".into(), proxy_url);

    // OpenClaw config + workspace files
    let oc_config =
        crate::openclaw_config::build_openclaw_config(&crate::openclaw_config::ConfigParams {
            agent_id,
            model: None,
            tools_deny: None,
        });
    let config_json = crate::openclaw_config::render_openclaw_config(&oc_config);

    let mut files = vec![cb_infra::types::FileMount {
        guest_path: "/root/.openclaw/openclaw.json".into(),
        raw_value: config_json,
    }];
    files.extend(crate::openclaw_config::build_workspace_files(&agent.name));

    let vps_spec = VpsSpec {
        name: vps_name.clone(),
        image: vps_config.image.clone(),
        location: vps_config.location.clone(),
        cpu_millicores: vps_config.cpu_millicores,
        memory_mb: vps_config.memory_mb,
        env,
        files,
    };

    let vps_info = match provider.create_vps(&vps_spec).await {
        Ok(info) => info,
        Err(e) => {
            tracing::error!(error = %e, "failed to create VPS");
            return Err(ApiError::Infra(e));
        }
    };

    // Update provider refs and set state to Running
    Vps::update_provider_refs(
        &state.db,
        vps.id,
        Some(&vps_info.id.0),
        vps_info.address.as_deref(),
    )
    .await?;
    Vps::set_state(&state.db, vps.id, VpsState::Running).await?;

    let updated_vps = Vps::get_by_id(&state.db, vps.id).await?;
    Ok((
        StatusCode::CREATED,
        Json(VpsResponse::new(updated_vps, vps_config.provider.clone())),
    ))
}

pub async fn start_vps(
    State(state): State<AppState>,
    Extension(user_id): Extension<UserId>,
    Path(agent_id): Path<Uuid>,
) -> Result<Json<VpsResponse>, ApiError> {
    let (_, vps) = get_agent_vps(&state, user_id.0, agent_id).await?;

    if vps.state != VpsState::Stopped {
        return Err(ApiError::Conflict(format!(
            "VPS is {}, expected stopped",
            serde_json::to_string(&vps.state)
                .unwrap_or_default()
                .trim_matches('"')
        )));
    }

    let vm_id = vps
        .provider_vm_id
        .as_ref()
        .ok_or(ApiError::Internal("VPS has no provider VM ID".into()))?;

    let (provider, config) = provider_for_vps(&state, &vps).await?;
    provider.start_vps(&VpsId(vm_id.clone())).await?;
    Vps::set_state(&state.db, vps.id, VpsState::Running).await?;

    let updated = Vps::get_by_id(&state.db, vps.id).await?;
    Ok(Json(VpsResponse::new(updated, config.provider)))
}

pub async fn stop_vps(
    State(state): State<AppState>,
    Extension(user_id): Extension<UserId>,
    Path(agent_id): Path<Uuid>,
) -> Result<Json<VpsResponse>, ApiError> {
    let (_, vps) = get_agent_vps(&state, user_id.0, agent_id).await?;

    if vps.state != VpsState::Running {
        return Err(ApiError::Conflict(format!(
            "VPS is {}, expected running",
            serde_json::to_string(&vps.state)
                .unwrap_or_default()
                .trim_matches('"')
        )));
    }

    let vm_id = vps
        .provider_vm_id
        .as_ref()
        .ok_or(ApiError::Internal("VPS has no provider VM ID".into()))?;

    let (provider, config) = provider_for_vps(&state, &vps).await?;
    provider.stop_vps(&VpsId(vm_id.clone())).await?;
    Vps::set_state(&state.db, vps.id, VpsState::Stopped).await?;

    let updated = Vps::get_by_id(&state.db, vps.id).await?;
    Ok(Json(VpsResponse::new(updated, config.provider)))
}

pub async fn destroy_vps(
    State(state): State<AppState>,
    Extension(user_id): Extension<UserId>,
    Path(agent_id): Path<Uuid>,
) -> Result<StatusCode, ApiError> {
    let (agent, vps) = get_agent_vps(&state, user_id.0, agent_id).await?;

    if vps.state == VpsState::Destroyed {
        return Err(ApiError::Conflict("VPS is already destroyed".into()));
    }

    // Best-effort destroy VM
    if let Some(ref vm_id) = vps.provider_vm_id
        && let Ok((provider, _config)) = provider_for_vps(&state, &vps).await
    {
        let _ = provider.destroy_vps(&VpsId(vm_id.clone())).await;
    }

    Vps::set_state(&state.db, vps.id, VpsState::Destroyed).await?;
    Agent::assign_vps(&state.db, agent.id, None).await?;

    Ok(StatusCode::NO_CONTENT)
}

/// Look up the configured provider for a VPS record by fetching its VpsConfig.
pub async fn provider_for_vps<'a>(
    state: &'a AppState,
    vps: &Vps,
) -> Result<(&'a Arc<dyn cb_infra::VpsProvider>, VpsConfig), ApiError> {
    let config = VpsConfig::get_by_id(&state.db, vps.vps_config_id).await?;
    let name: ProviderName = config.provider.parse().map_err(|_| {
        ApiError::Internal(format!(
            "unknown provider in VPS config: {}",
            config.provider
        ))
    })?;
    let provider = state
        .providers
        .get(name)
        .ok_or_else(|| ApiError::Internal(format!("provider not configured: {name}")))?;
    Ok((provider, config))
}

/// Helper: fetch agent + attached VPS, enforcing ownership.
async fn get_agent_vps(
    state: &AppState,
    user_id: Uuid,
    agent_id: Uuid,
) -> Result<(cb_db::models::Agent, Vps), ApiError> {
    let agent = Agent::get_by_id(&state.db, agent_id)
        .await
        .map_err(|_| ApiError::NotFound)?;

    if agent.user_id != user_id {
        return Err(ApiError::NotFound);
    }

    let vps_id = agent.vps_id.ok_or(ApiError::NotFound)?;

    let vps = Vps::get_by_id(&state.db, vps_id)
        .await
        .map_err(|_| ApiError::NotFound)?;

    Ok((agent, vps))
}
